use std::{sync::Arc, time::Duration};

use crate::{
    agent::{
        command::{Elevate, Explore},
        stream::{self, StreamOutcome},
        submit_commands::{CommandItem, CommandSubmissions, SubmitCommands},
    },
    config::{AppConfig, profile::AnswerProfile},
    error::{Error, Result},
    shell::Shell,
};
use reqwest::header::HeaderMap;
use rig_core::{
    agent::{
        Agent as RigAgent, HookAction, InvalidToolCallContext, InvalidToolCallHookAction,
        PromptHook,
    },
    client::CompletionClient,
    message::Message,
    providers::openai::{self, CompletionModel},
    streaming::StreamingChat,
    tool::ToolDyn,
};
use tracing::{debug, info, warn};
use tokio::sync::Mutex;

const UNKNOWN_TOOL_RETRIES: usize = 2;
const EFFECTIVELY_UNLIMITED_TURNS: usize = usize::MAX - 1;
const PROVIDER_RETRY_ATTEMPTS: usize = 3;
const PROVIDER_RETRY_BASE_DELAY_MS: u64 = 500;

#[derive(Debug, Clone)]
struct RetryContext {
    prompt: Message,
    history: Vec<Message>,
}

#[derive(Debug, Clone, Default)]
struct HarnessHook {
    retry_context: Arc<Mutex<Option<RetryContext>>>,
}

impl HarnessHook {
    async fn clear_retry_context(&self) {
        *self.retry_context.lock().await = None;
    }

    async fn take_retry_context(&self) -> Option<RetryContext> {
        self.retry_context.lock().await.take()
    }
}

fn normalize_tool_name(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

impl PromptHook<CompletionModel> for HarnessHook {
    async fn on_completion_call(
        &self,
        prompt: &Message,
        history: &[Message],
    ) -> HookAction {
        *self.retry_context.lock().await = Some(RetryContext {
            prompt: prompt.clone(),
            history: history.to_vec(),
        });
        HookAction::cont()
    }

    async fn on_invalid_tool_call(
        &self,
        context: &InvalidToolCallContext,
    ) -> InvalidToolCallHookAction {
        let normalized = normalize_tool_name(&context.tool_name);
        let matches = context
            .allowed_tools
            .iter()
            .filter(|name| normalize_tool_name(name) == normalized)
            .collect::<Vec<_>>();
        if let [repaired_name] = matches.as_slice() {
            warn!(
                emitted = %context.tool_name,
                repaired = %repaired_name,
                "Repairing tool name."
            );
            return InvalidToolCallHookAction::repair((*repaired_name).clone());
        }

        InvalidToolCallHookAction::retry(format!(
            "Unknown tool `{}`. Use exactly one of these tool names: {}.",
            context.tool_name,
            context.allowed_tools.join(", ")
        ))
    }
}

pub struct AnswerAgent {
    profile: AnswerProfile,
    agent: RigAgent<CompletionModel, HarnessHook>,
    finalizer: RigAgent<CompletionModel>,
    submissions: Arc<CommandSubmissions>,
    hook: HarnessHook,
}

#[derive(Debug, Clone)]
pub struct AnswerAgentResponse {
    pub messages: Vec<Message>,
    pub final_text: String,
    pub commands: Vec<CommandItem>,
}

#[derive(Debug)]
pub struct ModifyOption {
    history: Vec<Message>,
    command: String,
}

impl ModifyOption {
    pub fn new(history: Vec<Message>, command: String) -> Self {
        Self { history, command }
    }
}

#[bon::bon]
impl AnswerAgent {
    #[builder]
    pub fn builder(
        os: String,
        shell: &Shell,
        profile: AnswerProfile,
        config: AppConfig,
    ) -> Result<Self> {
        Self::new(os, shell, profile, config)
    }
}

impl AnswerAgent {
    #[tracing::instrument(
        name = "AnswerAgent",
        level = "info",
        skip(profile, config, shell),
        fields(shell = shell.name())
    )]
    pub fn new(
        os: String,
        shell: &Shell,
        profile: AnswerProfile,
        config: AppConfig,
    ) -> Result<Self> {
        let base_host = reqwest::Url::parse(&config.llm.base_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned));
        let mut http_client_builder = reqwest::Client::builder()
            .default_headers({
                let mut headers = HeaderMap::new();
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    "application/json".parse().expect("valid content type"),
                );
                headers
            });
        if let Some(host) = base_host {
            // Chat Completions 请求没有本地副作用, 只重试瞬时 HTTP 错误.
            http_client_builder = http_client_builder.retry(
                reqwest::retry::for_host(host)
                    .max_retries_per_request(2)
                    .no_budget()
                    .classify_fn(|request| {
                        if request.status().is_some_and(|status| {
                            status.is_server_error()
                                || matches!(
                                    status,
                                    reqwest::StatusCode::REQUEST_TIMEOUT
                                        | reqwest::StatusCode::TOO_MANY_REQUESTS
                                )
                        }) {
                            request.retryable()
                        } else {
                            request.success()
                        }
                    }),
            );
        }
        let http_client = http_client_builder.build()?;
        let model = openai::Client::<reqwest::Client>::builder()
            .base_url(&config.llm.base_url)
            .api_key(&config.llm.api_key)
            .http_client(http_client)
            .build()?
            .completions_api()
            .completion_model(&config.llm.model);

        let output_n = config.agent.answer.output_n as usize;
        let submissions = Arc::new(CommandSubmissions::default());
        let system_prompt = profile
            .system()
            .os(os)
            .shell(shell.path().display())
            .text_lang(&config.agent.language)
            .maybe_max_tokens(config.llm.max_tokens)
            .output_n(config.agent.answer.output_n)
            .finish();
        let hook = HarnessHook::default();
        let mut builder = rig_core::agent::AgentBuilder::new(model.clone())
            .preamble(&system_prompt)
            .hook(hook.clone());
        if let Some(max_tokens) = config.llm.max_tokens {
            builder = builder.max_tokens(max_tokens);
        }
        if let Some(temperature) = config.llm.temperature {
            builder = builder.temperature(temperature);
        }

        let shell_path = shell.path().to_path_buf();
        let mut tools: Vec<Box<dyn ToolDyn>> = Vec::new();
        if config.agent.use_tool_explore {
            tools.push(Box::new(Explore::new(shell_path.clone())));
        }
        if config.agent.use_tool_elevate {
            tools.push(Box::new(Elevate::new(shell_path.clone())));
        }
        tools.push(Box::new(SubmitCommands::new(
            shell_path,
            output_n,
            submissions.clone(),
        )));
        let agent = builder.tools(tools).build();

        let finalizer_prompt = format!(
            "You recover a missing final response. Based on the complete conversation history, provide one concise, non-empty user-facing answer in {}. Do not call tools and do not discuss this recovery instruction.",
            config.agent.language
        );
        let mut finalizer_builder =
            rig_core::agent::AgentBuilder::new(model).preamble(&finalizer_prompt);
        if let Some(max_tokens) = config.llm.max_tokens {
            finalizer_builder = finalizer_builder.max_tokens(max_tokens);
        }
        if let Some(temperature) = config.llm.temperature {
            finalizer_builder = finalizer_builder.temperature(temperature);
        }

        info!("Created.");
        Ok(Self {
            profile,
            agent,
            finalizer: finalizer_builder.build(),
            submissions,
            hook,
        })
    }

    fn retryable_provider_error(error: &Error) -> bool {
        let message = error.to_string();
        message.contains("Invalid status code 408")
            || message.contains("Invalid status code 429")
            || message.contains("Invalid status code 5")
    }

    fn provider_retry_delay(attempt: usize) -> Duration {
        let multiplier = 1_u64 << attempt.min(5);
        Duration::from_millis(PROVIDER_RETRY_BASE_DELAY_MS * multiplier)
    }

    async fn primary_chat(&self, prompt: String, history: Vec<Message>) -> Result<StreamOutcome> {
        let mut next_prompt = Message::user(prompt);
        let mut next_history = history;
        for attempt in 0..=PROVIDER_RETRY_ATTEMPTS {
            self.hook.clear_retry_context().await;
            let stream = self
                .agent
                .stream_chat(next_prompt.clone(), next_history.clone())
                .multi_turn(EFFECTIVELY_UNLIMITED_TURNS)
                .max_invalid_tool_call_retries(UNKNOWN_TOOL_RETRIES)
                .await;
            match stream::collect(stream, "Resolving").await {
                Ok(outcome) => return Ok(outcome),
                Err(error)
                    if attempt < PROVIDER_RETRY_ATTEMPTS
                        && Self::retryable_provider_error(&error) =>
                {
                    let Some(context) = self.hook.take_retry_context().await else {
                        return Err(error);
                    };
                    let delay = Self::provider_retry_delay(attempt);
                    warn!(
                        attempt = attempt + 1,
                        delay_ms = delay.as_millis() as u64,
                        "Retrying failed completion request."
                    );
                    tokio::time::sleep(delay).await;
                    next_prompt = context.prompt;
                    next_history = context.history;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("provider retry loop must return a result")
    }

    async fn finalize_empty_response(&self, history: Vec<Message>) -> Result<StreamOutcome> {
        for attempt in 0..=PROVIDER_RETRY_ATTEMPTS {
            let stream = self
                .finalizer
                .stream_chat(
                    "Provide the final user-facing answer now. Do not leave it empty.",
                    history.clone(),
                )
                .await;
            match stream::collect(stream, "Finalizing").await {
                Ok(outcome) => return Ok(outcome),
                Err(error)
                    if attempt < PROVIDER_RETRY_ATTEMPTS
                        && Self::retryable_provider_error(&error) =>
                {
                    let delay = Self::provider_retry_delay(attempt);
                    warn!(
                        attempt = attempt + 1,
                        delay_ms = delay.as_millis() as u64,
                        "Retrying failed finalizer request."
                    );
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("finalizer retry loop must return a result")
    }

    async fn resolve_internal(
        &self,
        prompt: String,
        history: Vec<Message>,
        modify_option: Option<ModifyOption>,
        attached: Option<String>,
    ) -> Result<AnswerAgentResponse> {
        self.submissions.clear().await;
        let attached_messages = attached
            .into_iter()
            .map(|content| Message::user(self.profile.attach(content).fmt()));
        let mut history = history;
        if let Some(modify_option) = modify_option {
            modify_option
                .history
                .into_iter()
                .chain([Message::user(
                    self.profile.modify(modify_option.command).fmt(),
                )])
                .for_each(|message| history.push(message));
        }
        history.extend(attached_messages);

        let mut outcome = self.primary_chat(prompt, history).await?;
        let commands = self.submissions.snapshot().await;
        if outcome.final_text.trim().is_empty() && commands.is_empty() {
            warn!("Agent returned neither final text nor command candidates.");
            outcome = self.finalize_empty_response(outcome.messages).await?;
            if outcome.final_text.trim().is_empty() {
                return Err(Error::AgentResponse(
                    "Agent returned an empty response after finalization.".to_string(),
                ));
            }
        }

        debug!(usage = ?outcome.usage, "AnswerAgent completed.");
        info!(
            commands = commands.len(),
            has_text = !outcome.final_text.trim().is_empty(),
            "AnswerAgent produced a response."
        );
        Ok(AnswerAgentResponse {
            messages: outcome.messages,
            final_text: outcome.final_text,
            commands,
        })
    }
}

#[bon::bon]
impl AnswerAgent {
    #[builder]
    pub async fn resolve(
        &self,
        prompt: String,
        #[builder(default)]
        history: Vec<Message>,
        modify_option: Option<ModifyOption>,
        attached: Option<String>,
    ) -> Result<AnswerAgentResponse> {
        self.resolve_internal(prompt, history, modify_option, attached)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_tool_name;

    #[test]
    fn tool_name_normalization_ignores_case_and_separators() {
        assert_eq!(normalize_tool_name("Submit-Commands"), "submitcommands");
        assert_eq!(normalize_tool_name("submit_commands"), "submitcommands");
        assert_eq!(normalize_tool_name("EXPLORE"), "explore");
    }
}

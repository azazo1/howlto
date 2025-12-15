use crate::agents::tools::{FinishResponse, FinishResponseArgs, Help, Man};
use crate::error::{Error, Result};
use crate::profile::template;
use crate::{config::AppConfig, profile::Profile};
use indicatif::ProgressStyle;
use reqwest::header::HeaderMap;
use rig::agent::{Agent as RigAgent, FinalResponse, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::message::{AssistantContent, Message};
use rig::providers::openai::{self, CompletionModel};
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingPrompt};
use rig::tool::Tool;
use tokio_stream::StreamExt;
use tracing::{debug, info, info_span, trace, warn};
use tracing_indicatif::span_ext::IndicatifSpanExt;

#[derive(Debug)]
struct ScrolliingMessage {
    /// 最大字符长度.
    max_length: usize,
    message: String,
}

impl ScrolliingMessage {
    fn new(max_length: usize) -> Self {
        Self {
            max_length,
            message: String::new(),
        }
    }

    fn message(&self) -> &str {
        &self.message
    }

    fn push(&mut self, appendant: String) {
        self.message = self
            .message
            .chars()
            .chain(appendant.chars())
            .rev()
            .take(self.max_length)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
    }
}

#[allow(dead_code)]
pub struct ShellCommandGenAgent {
    /// 代表操作系统的字符串.
    os: String,
    /// 代表 shell 的字符串.
    shell: String,
    /// 配置系统提示词.
    profile: Profile,
    /// 配置.
    config: AppConfig,
    agent: RigAgent<CompletionModel>,
}

pub struct ShellCommandGenAgentResponse {
    /// agent 做出决策时的上下文.
    pub messages: Vec<Message>,
    /// agent 做出决策需要执行的命令.
    pub command: String,
}

#[bon::bon]
impl ShellCommandGenAgent {
    #[builder]
    pub fn builder(os: String, shell: String, profile: Profile, config: AppConfig) -> Result<Self> {
        Self::new(os, shell, profile, config)
    }
}

impl ShellCommandGenAgent {
    pub fn new(os: String, shell: String, profile: Profile, config: AppConfig) -> Result<Self> {
        // 添加 Content-Type: application/json 请求头.
        let http_client = reqwest::Client::builder()
            .default_headers({
                let mut hm = HeaderMap::new();
                hm.insert(
                    reqwest::header::CONTENT_TYPE,
                    "application/json".parse().unwrap(),
                );
                hm
            })
            .build()?;
        let model = openai::Client::<reqwest::Client>::builder()
            .base_url(&config.llm_base_url)
            .api_key(&config.llm_api_key)
            .http_client(http_client)
            .build()?
            .completions_api()
            .completion_model(&config.model);
        let mut builder = rig::agent::AgentBuilderSimple::new(model).preamble(
            &profile
                .role
                .replace(template::OS, &os)
                .replace(template::SHELL, &shell)
                .replace(template::TEXT_LANG, &config.language)
                .replace(
                    template::MAX_TOKENS,
                    &config
                        .max_tokens
                        .map(|x| x.to_string())
                        .unwrap_or("[none]".into()),
                )
                .replace(template::OUTPUT_N, &config.output_commands_n.to_string()),
        );
        if let Some(max_tokens) = config.max_tokens {
            builder = builder.max_tokens(max_tokens);
        }
        if let Some(temperature) = config.temperature {
            builder = builder.temperature(temperature);
        };
        if config.use_tool_man {
            builder = builder.tool(Man);
        }
        if config.use_tool_help {
            builder = builder.tool(Help);
        }
        builder = builder.tool(FinishResponse);
        Ok(Self {
            os,
            shell,
            profile,
            config,
            agent: builder.build(),
        })
    }

    /// shell command agent 解决一个 `prompt`.
    pub async fn resolve(&self, prompt: String) -> Result<ShellCommandGenAgentResponse> {
        // stream_prompt 会自动处理工具的调用.
        let mut stream = self.agent.stream_prompt(&prompt).multi_turn(100).await;
        let mut output = FinalResponse::empty();
        let mut finish = FinishResponseArgs::empty();
        let mut scroll = ScrolliingMessage::new(40);
        let pb_span = info_span!("Resolving");
        pb_span.pb_set_style(
            &ProgressStyle::with_template("{spinner:.green} Agent: {msg}").unwrap(),
        );
        pb_span.pb_set_message("Waiting for output...");
        let _pb_span_enter = pb_span.enter();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| Error::StreamingError(e.to_string()))?;
            use MultiTurnStreamItem::*;
            use StreamedAssistantContent::*;
            match chunk {
                StreamAssistantItem(content) => match content {
                    Text(text) => {
                        scroll.push(text.text);
                        pb_span.pb_set_message(scroll.message());
                    }
                    ToolCall(tool_call) => {
                        info!(
                            "toolcall: {} - {}",
                            tool_call.function.name, tool_call.function.arguments
                        );
                        if tool_call.function.name == FinishResponse::NAME {
                            finish = serde_json::from_value(tool_call.function.arguments).unwrap();
                            break;
                        }
                    }
                    Reasoning(reasoning) => {
                        scroll.push(reasoning.reasoning.into_iter().collect());
                        pb_span.pb_set_message(scroll.message());
                    }
                    _ => (),
                },
                StreamUserItem(content) => {
                    let StreamedUserContent::ToolResult(rst) = content;
                    trace!("Tool result: {:?}", rst.content);
                }
                FinalResponse(final_response) => {
                    // 这个包含了完整的输出.
                    output = final_response;
                    debug!("Usage: {:?}", output.usage());
                }
                _ => warn!("Unknown stream chunk."),
            }
        }
        drop(_pb_span_enter);

        // 暂时只支持使用第一个回应, todo 支持多个回应的交互式选择.
        // println!("---");
        let finish = finish.results.first().cloned().unwrap_or("".into());
        if finish.is_empty() {
            warn!("No command provided.");
        }
        let output = format!("{}\n{}", output.response(), finish);
        Ok(ShellCommandGenAgentResponse {
            messages: [
                prompt.into(),
                Message::Assistant {
                    id: None,
                    content: rig::OneOrMany::one(AssistantContent::Text(output.into())),
                },
            ]
            .into(),
            command: finish,
        })
    }

    /// 根据修改建议 `prompt` 修改 agent 的上一个输出.
    pub async fn modify(
        &self,
        _prev_resp: ShellCommandGenAgentResponse,
        _prompt: String,
    ) -> Result<ShellCommandGenAgentResponse> {
        todo!()
    }
}

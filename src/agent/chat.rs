use reqwest::header::HeaderMap;
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::completion::{Message, Prompt};
use rig::providers::openai::{self, CompletionModel};
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use rig::tool::{Tool, ToolDyn};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::{StreamExt, wrappers::UnboundedReceiverStream};

use crate::agent::{
    AssistantTurn, map_prompt_error,
    tools::{DangerousHelp, Help, Man, SubmitCommands, TheFuck, Tldr},
    turn_from_final_response, turn_from_prompt_response,
};
use crate::config::AppConfig;
use crate::config::profile::ChatProfile;
use crate::error::{Error, Result};
use crate::shell::Shell;
use tracing::info;

pub struct ChatAgent {
    profile: ChatProfile,
    agent: Agent<CompletionModel>,
}

#[derive(Debug, Clone)]
pub enum ChatStreamEvent {
    TextDelta(String),
    Commands(Vec<crate::agent::tools::CommandCandidate>),
    Final(AssistantTurn),
}

pub struct ChatStream {
    stream: UnboundedReceiverStream<Result<ChatStreamEvent>>,
    task: JoinHandle<()>,
}

impl ChatStream {
    pub async fn next(&mut self) -> Option<Result<ChatStreamEvent>> {
        self.stream.next().await
    }

    pub async fn stop(self) {
        self.task.abort();
        self.task.await.ok();
    }

    pub async fn finish(self) {
        self.task.await.ok();
    }
}

impl ChatAgent {
    pub fn new(os: String, shell: &Shell, profile: ChatProfile, config: AppConfig) -> Result<Self> {
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
        let client = openai::Client::<reqwest::Client>::builder()
            .base_url(&config.llm.base_url)
            .api_key(&config.llm.api_key)
            .http_client(http_client)
            .build()?
            .completions_api();
        let mut builder = client.agent(&config.llm.model).preamble(
            &profile
                .generate()
                .os(os)
                .shell(shell.path().display())
                .text_lang(&config.agent.language)
                .maybe_max_tokens(config.llm.max_tokens)
                .finish(),
        );
        if let Some(max_tokens) = config.llm.max_tokens {
            builder = builder.max_tokens(max_tokens);
        }
        if let Some(temperature) = config.llm.temperature {
            builder = builder.temperature(temperature);
        }
        let mut tools: Vec<Box<dyn ToolDyn>> = Vec::new();
        if config.agent.use_tool_man {
            tools.push(Box::new(Man));
        }
        if config.agent.use_tool_help {
            tools.push(Box::new(Help));
        }
        if config.agent.use_tool_tldr {
            tools.push(Box::new(Tldr));
        }
        if config.agent.use_tool_thefuck {
            tools.push(Box::new(TheFuck::new(shell.name().to_string())));
        }
        if config.agent.use_tool_dangerous_help {
            tools.push(Box::new(DangerousHelp));
        }
        tools.push(Box::new(SubmitCommands));
        Ok(Self {
            profile,
            agent: builder.tools(tools).build(),
        })
    }

    pub async fn resolve(
        &self,
        prompt: String,
        history: Vec<Message>,
        attached: Option<String>,
    ) -> Result<AssistantTurn> {
        let mut history = history;
        if let Some(attached) = attached {
            history.push(Message::user(self.profile.attach(attached).fmt()));
        }
        let response = self
            .agent
            .prompt(prompt)
            .with_history(history)
            .max_turns(100)
            .extended_details()
            .await
            .map_err(map_prompt_error)?;
        turn_from_prompt_response(response)
    }

    pub async fn stream_resolve(
        &self,
        prompt: String,
        history: Vec<Message>,
        attached: Option<String>,
    ) -> Result<ChatStream> {
        let mut history = history;
        if let Some(attached) = attached {
            history.push(Message::user(self.profile.attach(attached).fmt()));
        }

        let mut response_stream = self.agent.stream_chat(prompt, history).multi_turn(8).await;

        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let mut saw_final = false;
            while let Some(item) = response_stream.next().await {
                match item {
                    Ok(MultiTurnStreamItem::StreamAssistantItem(
                        StreamedAssistantContent::Text(text),
                    )) => {
                        if tx.send(Ok(ChatStreamEvent::TextDelta(text.text))).is_err() {
                            break;
                        }
                    }
                    Ok(MultiTurnStreamItem::StreamAssistantItem(
                        StreamedAssistantContent::ToolCall { tool_call, .. },
                    )) => {
                        info!(
                            tool = %tool_call.function.name,
                            arguments = %tool_call.function.arguments,
                            "agent tool call"
                        );
                        if tool_call.function.name == crate::agent::tools::SubmitCommands::NAME {
                            match serde_json::from_value::<crate::agent::tools::SubmitCommandsArgs>(
                                tool_call.function.arguments.clone(),
                            ) {
                                Ok(args) => {
                                    if tx
                                        .send(Ok(ChatStreamEvent::Commands(args.results)))
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    let _ = tx.send(Err(Error::InvalidInput(format!(
                                        "invalid submit_commands payload: {error}"
                                    ))));
                                    break;
                                }
                            }
                        }
                    }
                    Ok(MultiTurnStreamItem::FinalResponse(final_response)) => {
                        saw_final = true;
                        match turn_from_final_response(final_response) {
                            Ok(turn) => {
                                info!("Chat reply streamed");
                                let _ = tx.send(Ok(ChatStreamEvent::Final(turn)));
                            }
                            Err(error) => {
                                let _ = tx.send(Err(error));
                            }
                        }
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = tx.send(Err(Error::StreamingError(error.to_string())));
                        break;
                    }
                }
            }

            if !saw_final {
                let _ = tx.send(Err(Error::StreamingError(
                    "assistant stream ended before final response".into(),
                )));
            }
        });

        Ok(ChatStream {
            stream: UnboundedReceiverStream::new(rx),
            task,
        })
    }
}

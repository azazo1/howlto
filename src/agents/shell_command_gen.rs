use crate::agents::tools::{Help, Man};
use crate::error::{Error, Result};
use crate::{config::AppConfig, profile::Profile};
use reqwest::header::HeaderMap;
use rig::agent::{Agent as RigAgent, FinalResponse, MultiTurnStreamItem, stream_to_stdout};
use rig::client::CompletionClient;
use rig::message::{AssistantContent, Message};
use rig::providers::openai::{self, CompletionModel};
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingPrompt};
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

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
        let mut builder = rig::agent::AgentBuilderSimple::new(model).preamble(&profile.role);
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
        let mut stream = self.agent.stream_prompt(&prompt).await;
        let mut output = FinalResponse::empty();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| Error::StreamingError(e.to_string()))?;
            use MultiTurnStreamItem::*;
            use StreamedAssistantContent::*;
            match chunk {
                StreamAssistantItem(content) => match content {
                    Text(_text) => {
                        // print!("{}", text.text);
                    }
                    ToolCall(tool_call) => {
                        info!(
                            "toolcall: {}, {}",
                            tool_call.function.name, tool_call.function.arguments
                        );
                    }
                    Reasoning(_reasoning) => {
                        // print!("{}", reasoning.reasoning.into_iter().collect::<String>());
                    }
                    _ => (),
                },
                StreamUserItem(content) => {
                    let StreamedUserContent::ToolResult(rst) = content;
                    debug!("{:?}", rst.content);
                }
                FinalResponse(final_response) => {
                    // 这个包含了完整的输出.
                    output = final_response;
                }
                _ => warn!("unknown stream chunk"),
            }
        }
        Ok(ShellCommandGenAgentResponse {
            messages: [
                prompt.into(),
                Message::Assistant {
                    id: None,
                    content: rig::OneOrMany::one(AssistantContent::Text(output.response().into())),
                },
            ]
            .into(),
            command: output.response().into(),
        })
    }

    /// 根据修改建议 `prompt` 修改 agent 的上一个输出.
    pub async fn modify(
        &self,
        prev_resp: ShellCommandGenAgentResponse,
        prompt: String,
    ) -> Result<ShellCommandGenAgentResponse> {
        todo!()
    }
}

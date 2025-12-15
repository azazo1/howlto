use crate::agents::tools::{Help, Man};
use crate::error::Result;
use crate::{config::AppConfig, profile::Profile};
use reqwest::header::HeaderMap;
use rig::agent::{Agent as RigAgent, stream_to_stdout};
use rig::client::CompletionClient;
use rig::message::{AssistantContent, Message};
use rig::providers::openai::{self, CompletionModel};
use rig::streaming::StreamingPrompt;
use tracing::debug;

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
    messages: Vec<Message>,
    /// agent 做出决策需要执行的命令.
    command: String,
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
        let mut stream = self.agent.stream_prompt(&prompt).await;
        let output = stream_to_stdout(&mut stream).await?; // todo 替换为更美观的输出.
        debug!(target: "shell-command-gen-agent", "Usage: {:?}", output.usage());
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

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::agents::tools::{FinishResponse, FinishResponseArgs, Help, Man};
use crate::error::{Error, Result};
use crate::profile::template;
use crate::{config::AppConfig, profile::Profile};
use indicatif::ProgressStyle;
use reqwest::header::HeaderMap;
use rig::agent::{Agent as RigAgent, FinalResponse, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::message::{AssistantContent, Message, ToolResultContent};
use rig::providers::openai::{self, CompletionModel};
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingPrompt};
use rig::tool::Tool;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use tracing::{debug, info, info_span, warn};
use tracing_indicatif::span_ext::IndicatifSpanExt;

#[derive(Debug, Clone)]
struct ScrolliingMessage {
    /// 滚动的窗口宽度.
    scroll_width: usize,
    scroll_window: Arc<Mutex<VecDeque<char>>>,
    message: Arc<Mutex<String>>,
}

impl ScrolliingMessage {
    fn new(scroll_width: usize) -> Self {
        Self {
            scroll_width,
            scroll_window: Arc::new(Mutex::new(VecDeque::new())),
            message: Arc::new(Mutex::new(String::new())),
        }
    }

    async fn push(&self, appendant: String) {
        let mut message = self.message.lock().await;
        *message += &appendant.replace("\n", " ");
    }

    async fn has_new_messages(&self) -> bool {
        let message = self.message.lock().await;
        !message.is_empty()
    }

    /// 返回实际 pop 的字符数和对应字符串.
    async fn pop(&self, chars: usize) -> (usize, String) {
        let mut message = self.message.lock().await;
        let idx = message
            .char_indices()
            .nth(chars)
            .map(|(idx, _)| idx)
            .unwrap_or(message.len());
        let rst = message[..idx].to_string();
        *message = message[idx..].to_string();
        (idx, rst)
    }

    /// - `step`: 滚动字符数
    async fn scroll(&self, step: usize) -> String {
        let mut window = self.scroll_window.lock().await;
        let (step, popen) = self.pop(step).await;
        let dispose_len = (window.len() + step).saturating_sub(self.scroll_width);
        *window = window
            .iter()
            .copied()
            .chain(popen.chars())
            .skip(dispose_len)
            .collect();
        window.iter().copied().collect()
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

    /// shell command gen agent 解决一个 `prompt`.
    pub async fn resolve(&self, prompt: String) -> Result<ShellCommandGenAgentResponse> {
        // stream_prompt 会自动处理工具的调用, multi_turn: 最大的工具调用次数.
        let mut stream = self.agent.stream_prompt(&prompt).multi_turn(100).await;
        let mut output = FinalResponse::empty();
        let mut finish = FinishResponseArgs::empty();
        let scroll = ScrolliingMessage::new(40);
        let finished = Arc::new(AtomicBool::new(false));
        let pb_span = info_span!("Resolving");
        pb_span
            .pb_set_style(&ProgressStyle::with_template("{spinner:.green} Agent: {msg}").unwrap());
        pb_span.pb_set_message("Waiting for output...");
        let _pb_span_enter = pb_span.enter();
        let scrolling_handle = {
            // 持续滚动进度条输出.
            let pb_span = pb_span.clone();
            let scroll = scroll.clone();
            let finished = Arc::clone(&finished);
            tokio::spawn(async move {
                while !finished.load(Ordering::Relaxed) || scroll.has_new_messages().await {
                    let msg = scroll.scroll(7).await;
                    if !msg.is_empty() {
                        pb_span.pb_set_message(&msg);
                    }
                    tokio::time::sleep(Duration::from_millis(30)).await;
                }
            })
        };

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| Error::StreamingError(e.to_string()))?;
            use MultiTurnStreamItem::*;
            use StreamedAssistantContent::*;
            match chunk {
                StreamAssistantItem(content) => match content {
                    Text(text) => {
                        scroll.push(text.text).await;
                    }
                    ToolCall(tool_call) => {
                        info!(
                            "Toolcall: {} - {}",
                            tool_call.function.name, tool_call.function.arguments
                        );
                        if tool_call.function.name == FinishResponse::NAME {
                            // todo 提供一个激进的选项, 当 FinishResponse 触发的时候直接结束循环, 即使 Usage 可能无法及时获取.
                            finish = serde_json::from_value(tool_call.function.arguments).unwrap();
                        }
                    }
                    Reasoning(reasoning) => {
                        scroll.push(reasoning.reasoning.into_iter().collect()).await;
                    }
                    _ => (),
                },
                StreamUserItem(content) => {
                    let StreamedUserContent::ToolResult(rst) = content;
                    for content in rst.content {
                        if let ToolResultContent::Text(text) = content {
                            debug!(
                                "Tool result: {}",
                                format!("{:?}", text)
                                    .chars()
                                    .take(100)
                                    .chain("...".chars())
                                    .collect::<String>()
                            );
                        }
                    }
                }
                FinalResponse(final_response) => {
                    // final_response 包含了完整的输出.
                    output = final_response;
                    debug!("Usage: {:?}", output.usage());
                }
                _ => warn!("Unknown stream chunk."),
            }
        }
        finished.store(true, Ordering::Relaxed);
        if !self.config.wait_for_output {
            scrolling_handle.abort();
        }
        scrolling_handle.await.ok();
        drop(_pb_span_enter);

        eprintln!(); // 为了分开结果输出和进度条, 视觉上更好分辨.

        // 暂时只支持使用第一个回应, todo 支持多个回应的交互式选择.
        let finish = finish.results.first().cloned().unwrap_or("".into());
        if finish.is_empty() {
            warn!("No command provided.");
            info!("Shell Command Gen Agent: {}", output.response());
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

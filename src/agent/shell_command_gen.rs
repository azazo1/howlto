use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::agent::tools::{FinishResponse, FinishResponseArgs, Help, Man, Tldr};
use crate::config::AppConfig;
use crate::config::profile::{ShellComamndGenProfile, template};
use crate::error::{Error, Result};
use reqwest::header::HeaderMap;
use rig::agent::{Agent as RigAgent, FinalResponse, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::message::{AssistantContent, Message, ToolResultContent};
use rig::providers::openai::{self, CompletionModel};
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingPrompt};
use rig::tool::Tool;
use tokio::sync::{Mutex, RwLock};
use tokio_stream::StreamExt;
use tracing::{debug, info, info_span, warn};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;

/// 盲文 spinner, \u28xx, xx 为 00~ff, 按位顺序从右到左分别表示盲文点: 左上, 左中, 左下, 右上, 右中, 右下, 左底, 右底.
/// 其中最后两个点如果w位都是 0 那么为六点盲文.
const SPINNER: [&str; 7] = [
    "\u{280b}", "\u{2819}", "\u{2838}", "\u{2834}", "\u{2826}", "\u{2807}", "",
];

#[derive(Debug, Clone)]
struct ScrolliingMessage {
    /// 滚动的窗口宽度.
    scroll_width: usize,
    scroll_window: Arc<Mutex<VecDeque<char>>>,
    message: Arc<RwLock<String>>,
    /// 字节索引.
    message_read_cursor: Arc<Mutex<usize>>,
}

impl ScrolliingMessage {
    fn new(scroll_width: usize) -> Self {
        Self {
            scroll_width,
            scroll_window: Arc::new(Mutex::new(VecDeque::new())),
            message: Arc::new(RwLock::new(String::new())),
            message_read_cursor: Arc::new(Mutex::new(0)),
        }
    }

    /// 获取累计的所有内容.
    async fn message(&self) -> String {
        self.message.read().await.clone()
    }

    async fn push(&self, appendant: String) {
        let mut message = self.message.write().await;
        *message += &appendant;
    }

    async fn has_new_messages(&self) -> bool {
        let message = self.message.read().await;
        !message.is_empty()
    }

    /// 返回实际 pop 的字符数和对应字符串.
    async fn pop(&self, chars: usize) -> (usize, String) {
        let message = self.message.read().await;
        let mut cursor = self.message_read_cursor.lock().await;
        let idx = message[*cursor..]
            .char_indices()
            .nth(chars)
            .map(|(idx, _)| idx)
            .unwrap_or(message[*cursor..].len());
        *cursor += idx;
        let rst = message[..*cursor].to_string();
        (rst.chars().count(), rst)
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

/// Shell Command Generate Agent
pub struct ScgAgent {
    config: AppConfig,
    agent: RigAgent<CompletionModel>,
}

pub struct ScgAgentResponse {
    /// agent 做出决策时的上下文.
    pub messages: Vec<Message>,
    /// agent 做出决策需要执行的命令.
    pub commands: Vec<String>,
}

#[bon::bon]
impl ScgAgent {
    #[builder]
    pub fn builder(
        os: String,
        shell: String,
        profile: ShellComamndGenProfile,
        config: AppConfig,
    ) -> Result<Self> {
        Self::new(os, shell, profile, config)
    }
}

impl ScgAgent {
    #[tracing::instrument(name = "ShellCommandGenAgent", level = "info", skip(profile, config))]
    pub fn new(
        os: String,
        shell: String,
        profile: ShellComamndGenProfile,
        config: AppConfig,
    ) -> Result<Self> {
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
            .base_url(&config.llm.llm_base_url)
            .api_key(&config.llm.llm_api_key)
            .http_client(http_client)
            .build()?
            .completions_api()
            .completion_model(&config.llm.model);
        let mut builder = rig::agent::AgentBuilderSimple::new(model).preamble(
            &profile
                .generate
                .replace(template::OS, &os)
                .replace(template::SHELL, &shell)
                .replace(template::TEXT_LANG, &config.agent.language)
                .replace(
                    template::MAX_TOKENS,
                    &config
                        .llm
                        .max_tokens
                        .map(|x| x.to_string())
                        .unwrap_or("[none]".into()),
                )
                .replace(
                    template::OUTPUT_N,
                    &config.agent.shell_command_gen.output_n.to_string(),
                ),
        );
        if let Some(max_tokens) = config.llm.max_tokens {
            builder = builder.max_tokens(max_tokens);
        }
        if let Some(temperature) = config.llm.temperature {
            builder = builder.temperature(temperature);
        };
        if config.agent.use_tool_man {
            builder = builder.tool(Man);
        }
        if config.agent.use_tool_help {
            builder = builder.tool(Help);
        }
        if config.agent.use_tool_tldr {
            builder = builder.tool(Tldr);
        }
        builder = builder.tool(FinishResponse);

        info!("Created.");
        Ok(Self {
            config,
            agent: builder.build(),
        })
    }

    /// shell command gen agent 解决一个 `prompt`.
    pub async fn resolve(&self, prompt: String) -> Result<ScgAgentResponse> {
        // stream_prompt 会自动处理工具的调用.
        let mut stream = self.agent.stream_prompt(&prompt).multi_turn(20).await;
        let mut output = FinalResponse::empty();
        let mut finish = FinishResponseArgs::empty();
        let scroll = ScrolliingMessage::new(40);
        let finished = Arc::new(AtomicBool::new(false));
        let pb_span = info_span!("Resolving");
        pb_span.pb_set_style(
            &ProgressStyle::with_template("{spinner:.green} Agent: {msg}")
                .unwrap()
                .tick_strings(&SPINNER),
        );
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
                        pb_span.pb_set_message(&msg.replace("\n", " "));
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
                            break;
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
                                    .take(300)
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
        if !self
            .config
            .agent
            .shell_command_gen
            .wait_for_output_scrolling
        {
            scrolling_handle.abort();
        }
        scrolling_handle.await.ok();
        drop(_pb_span_enter);

        let finish = finish.results;
        // todo 这里使用 ratatui 输出对话框.
        if finish.is_empty() {
            warn!("No command provided.");
            info!("ShellCommandGenAgent: {}", output.response());
        } else if output.response().is_empty() {
            // 获取了 finish 之后可能会没有及时获取 final response, 导致 output.response() 为空.
            info!("ShellCommandGenAgnet: {}", scroll.message().await);
        } else {
            info!("ShellCommandGenAgent: {}", output.response());
        }
        let output = format!("{}\n{}", output.response(), finish.join("\n"));
        Ok(ScgAgentResponse {
            messages: [
                prompt.into(),
                Message::Assistant {
                    id: None,
                    content: rig::OneOrMany::one(AssistantContent::Text(output.into())),
                },
            ]
            .into(),
            commands: finish,
        })
    }

    /// 根据修改建议 `prompt` 修改 agent 的上一个输出.
    /// # Parameters
    /// - `prev_resp`: 前一轮的 agent 回复.
    /// - `command`: 要修改的命令.
    /// - `prompt`: 用户的修改要求描述.
    pub async fn modify(
        &self,
        prev_resp: &mut ScgAgentResponse,
        command: String,
        prompt: String,
    ) -> Result<()> {
        todo!("实现修改功能")
    }
}

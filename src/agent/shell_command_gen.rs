use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::agent::tools::{FinishResponse, FinishResponseArgs, Help, Man, Tldr};
use crate::config::AppConfig;
use crate::config::profile::ShellComamndGenProfile;
use crate::error::{Error, Result};
use reqwest::header::HeaderMap;
use rig::agent::{Agent as RigAgent, FinalResponse, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::message::{Message, ToolResultContent};
use rig::providers::openai::{self, CompletionModel};
use rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat};
use rig::tool::Tool;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{debug, info, info_span, warn};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;
use unicode_width::UnicodeWidthChar;

const MULTI_TURN: usize = 20;

/// 盲文 spinner, \u28xx, xx 为 00~ff, 按位顺序从右到左分别表示盲文点: 左上, 左中, 左下, 右上, 右中, 右下, 左底, 右底.
/// 其中最后两个点如果w位都是 0 那么为六点盲文.
const SPINNER: [&str; 7] = [
    "\u{280b}", "\u{2819}", "\u{2838}", "\u{2834}", "\u{2826}", "\u{2807}", "",
];

#[derive(Debug)]
struct ScrolliingMessage {
    /// 滚动的窗口宽度.
    scroll_width: usize,
    message: RwLock<String>,
    /// 字节索引.
    message_read_cursor: RwLock<usize>,
}

impl ScrolliingMessage {
    fn new(scroll_width: usize) -> Self {
        Self {
            scroll_width,
            message: RwLock::new(String::new()),
            message_read_cursor: RwLock::new(0),
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
        message.len() > *self.message_read_cursor.read().await
    }

    fn window_at_first(s: &str, width: usize) -> &str {
        let mut acc = 0;
        if let Some((idx, ch)) = s
            .char_indices()
            .map_while(|(idx, ch)| {
                acc += ch.width_cjk().unwrap_or(0);
                if acc <= width { Some((idx, ch)) } else { None }
            })
            .last()
        {
            &s[..idx + ch.len_utf8()]
        } else {
            ""
        }
    }

    fn window_at_last(s: &str, width: usize) -> &str {
        let mut acc = 0;
        if let Some(idx) = s
            .char_indices()
            .rev()
            .map_while(|(idx, ch)| {
                acc += ch.width_cjk().unwrap_or(0);
                if acc <= width { Some(idx) } else { None }
            })
            .last()
        {
            &s[idx..]
        } else {
            ""
        }
    }

    /// - `step`: 滚动 unicode_width 数.
    async fn scroll(&self, step: usize) -> String {
        let cursor = *self.message_read_cursor.read().await;
        let message = self.message.read().await;
        let appendant = Self::window_at_first(&message[cursor..], step);
        #[cfg(test)]
        eprintln!("appendant: {{{appendant}}}");
        let window = Self::window_at_last(&message[..cursor + appendant.len()], self.scroll_width);
        *self.message_read_cursor.write().await += appendant.len();
        window.to_string()
    }
}

/// Shell Command Generate Agent
pub struct ScgAgent {
    profile: ShellComamndGenProfile,
    config: AppConfig,
    agent: RigAgent<CompletionModel>,
}

#[derive(Debug, Clone)]
pub struct ScgAgentResponse {
    /// agent 做出决策时的上下文.
    pub messages: Vec<Message>,
    /// agent 做出决策需要执行的命令.
    pub commands: Vec<String>,
}

#[derive(Debug)]
pub struct ModifyOption {
    /// 之前输出的上下文.
    history: Vec<Message>,
    /// 需要修改的命令
    command: String,
}

impl ModifyOption {
    pub fn new(history: Vec<Message>, command: String) -> Self {
        Self { history, command }
    }
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
                .generate()
                .os(os)
                .shell(shell)
                .text_lang(&config.agent.language)
                .maybe_max_tokens(config.llm.max_tokens)
                .output_n(config.agent.shell_command_gen.output_n)
                .finish(),
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
            profile,
            agent: builder.build(),
        })
    }

    /// shell command gen agent 解决一个 `prompt`, 或修改命令.
    async fn resolve_internal(
        &self,
        prompt: String,
        modify_option: Option<ModifyOption>,
        attached: Option<String>,
    ) -> Result<ScgAgentResponse> {
        // stream_prompt 会自动处理工具的调用.
        let attached_iter = attached
            .into_iter()
            .map(|a| Message::user(self.profile.attach(a).finish()));
        let history: Vec<Message> = if let Some(modify_option) = &modify_option {
            modify_option
                .history
                .clone()
                .into_iter()
                .chain(
                    [Message::user(
                        self.profile.modify(&modify_option.command).finish(),
                    )]
                    .into_iter(),
                )
                .chain(attached_iter)
                .collect()
        } else {
            attached_iter.collect()
        };
        let mut stream = self
            .agent
            .stream_chat(&prompt, history)
            .multi_turn(MULTI_TURN)
            .await;

        let mut output = FinalResponse::empty();
        let mut finish = FinishResponseArgs::empty();
        let scroll = Arc::new(ScrolliingMessage::new(40));
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
            let scroll = Arc::clone(&scroll);
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
            messages: if let Some(modify_option) = modify_option {
                modify_option
                    .history
                    .into_iter()
                    .chain([Message::user(prompt), Message::assistant(output)].into_iter())
                    .collect()
            } else {
                [Message::user(prompt), Message::assistant(output)].into()
            },
            commands: finish,
        })
    }
}

#[bon::bon]
impl ScgAgent {
    #[builder]
    pub async fn resolve(
        &self,
        prompt: String,
        modify_option: Option<ModifyOption>,
        attached: Option<String>,
    ) -> Result<ScgAgentResponse> {
        self.resolve_internal(prompt, modify_option, attached).await
    }
}

#[cfg(test)]
mod test {
    use unicode_width::UnicodeWidthStr;

    use crate::agent::shell_command_gen::ScrolliingMessage;

    #[tokio::test]
    async fn scroll_message() {
        const SCROLL_WIDTH: usize = 10;
        let sm = ScrolliingMessage::new(SCROLL_WIDTH);
        sm.push("你好世界".into()).await;
        assert_eq!(sm.scroll(0).await, "");
        assert_eq!(sm.scroll(1).await, ""); // 没到字符边界, 没有产生任何效果.
        assert_eq!(sm.scroll(2).await, "你");
        assert_eq!(sm.scroll(6).await, "你好世界"); // 滚动到末尾.
        sm.push("abc".into()).await;
        assert_eq!("你好世界ab".width_cjk(), SCROLL_WIDTH);
        let s = sm.scroll(2).await;
        assert_eq!(s.width_cjk(), SCROLL_WIDTH);
        assert_eq!(s, "你好世界ab");
        sm.push("我能正常滚动".into()).await;
        assert_eq!(sm.scroll(0).await, "你好世界ab");
        assert_eq!(sm.scroll(usize::MAX).await, "能正常滚动");
    }
}

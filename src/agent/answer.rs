use std::collections::HashSet;
use std::fmt::Debug;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::agent::tools::{
    Answer, AnswerArgs, AnswerBody, CommandItem, Elevate, Explore, Man, TheFuck, Tldr,
};
use crate::config::AppConfig;
use crate::config::profile::AnswerProfile;
use crate::error::{Error, Result};
use crate::shell::Shell;
use reqwest::header::HeaderMap;
use rig_core::agent::{Agent as RigAgent, MultiTurnStreamItem};
use rig_core::client::CompletionClient;
use rig_core::completion::Usage;
use rig_core::message::{Message, ToolResultContent};
use rig_core::providers::openai::{self, CompletionModel};
use rig_core::streaming::{
    StreamedAssistantContent, StreamedUserContent, StreamingChat, ToolCallDeltaContent,
};
use rig_core::tool::{Tool, ToolDyn};
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{debug, info, info_span, warn};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;
use unicode_width::UnicodeWidthChar;

const MULTI_TURN: usize = 100;

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

/// Answer Agent: 解答用户问题, 既可输出 shell 命令, 也可输出纯文本/markdown.
pub struct AnswerAgent {
    profile: AnswerProfile,
    config: AppConfig,
    agent: RigAgent<CompletionModel>,
}

#[derive(Debug, Clone)]
pub struct AnswerAgentResponse {
    /// agent 做出决策时的上下文.
    pub messages: Vec<Message>,
    /// agent 做出决策需要输出的回答 (命令或文本).
    pub answer: AnswerBody,
}

#[derive(Debug)]
pub struct ModifyOption {
    /// 之前输出的上下文.
    history: Vec<Message>,
    /// 需要修改的命令
    command: String,
}

struct StreamChatStatus {
    output: String,
    usage: Option<Usage>,
    answers: Option<AnswerArgs>,
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

fn usage_sum(a: Option<Usage>, b: Option<Usage>) -> Option<Usage> {
    match (a, b) {
        (None, None) => None,
        (None, Some(b)) => Some(b),
        (Some(a), None) => Some(a),
        (Some(a), Some(b)) => Some(a + b),
    }
}

/// 判断一条命令是否"疑似无效" (首字符非 ASCII 且不在 PATH 中).
/// 仅对 [`AnswerBody::Commands`] 中的命令项调用; 文本回答不参与校验.
fn is_potentially_invalid_command(s: &CommandItem) -> bool {
    let s = s.content.as_str();
    if let Some(ch) = s.chars().next() {
        !ch.is_ascii() && which::which(Path::new(s)).is_err()
    } else {
        true
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
            .base_url(&config.llm.base_url)
            .api_key(&config.llm.api_key)
            .http_client(http_client)
            .build()?
            .completions_api()
            .completion_model(&config.llm.model);
        let mut builder = rig_core::agent::AgentBuilder::new(model).preamble(
            &profile
                .generate()
                .os(os)
                .shell(shell.path().display())
                .text_lang(&config.agent.language)
                .maybe_max_tokens(config.llm.max_tokens)
                .output_n(config.agent.answer.output_n)
                .finish(),
        );
        if let Some(max_tokens) = config.llm.max_tokens {
            builder = builder.max_tokens(max_tokens);
        }
        if let Some(temperature) = config.llm.temperature {
            builder = builder.temperature(temperature);
        };
        let mut tools: Vec<Box<dyn ToolDyn>> = Vec::new();
        if config.agent.use_tool_man {
            tools.push(Box::new(Man));
        }
        if config.agent.use_tool_explore {
            tools.push(Box::new(Explore::new(shell.path().to_path_buf())));
        }
        if config.agent.use_tool_tldr {
            tools.push(Box::new(Tldr));
        }
        if config.agent.use_tool_thefuck {
            tools.push(Box::new(TheFuck::new(shell.name().to_string())));
        }
        if config.agent.use_tool_elevate {
            tools.push(Box::new(Elevate::new(shell.path().to_path_buf())));
        }
        tools.push(Box::new(Answer));

        info!("Created.");
        Ok(Self {
            config,
            profile,
            agent: builder.tools(tools).build(),
        })
    }

    /// 调用 LLM, 实时显示输出.
    /// # Returns
    /// (LLM 输出内容, [`Answer`] 结果)
    async fn stream_chat_internal(
        &self,
        span_title: Option<&str>,
        prompt: String,
        history: Vec<Message>,
    ) -> Result<StreamChatStatus> {
        let mut stream = self
            .agent
            .stream_chat(&prompt, history.clone())
            .multi_turn(MULTI_TURN)
            .await;

        let mut output = None;
        let mut answers: Option<AnswerArgs> = None;
        let mut streaming_tool_call_ids = HashSet::new();
        let scroll = Arc::new(ScrolliingMessage::new(40));
        let finished = Arc::new(AtomicBool::new(false));
        let span_title = span_title.unwrap_or_default();
        let pb_span = info_span!("", status = span_title);
        pb_span.pb_set_style(
            &ProgressStyle::with_template(&format!(
                "{{spinner:.green}} Agent({span_title}): {{msg}}"
            ))
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
                    ToolCall {
                        tool_call,
                        internal_call_id,
                    } => {
                        info!(
                            "Toolcall: {} - {}",
                            tool_call.function.name, tool_call.function.arguments
                        );
                        if streaming_tool_call_ids.remove(&internal_call_id) {
                            scroll.push("\n".to_string()).await;
                        }
                        if tool_call.function.name == Answer::NAME {
                            // todo 提供一个激进的选项, 当 Answer 触发的时候直接结束循环, 即使 Usage 可能无法及时获取.
                            answers =
                                Some(serde_json::from_value(tool_call.function.arguments).unwrap());
                            break;
                        }
                    }
                    ToolCallDelta {
                        internal_call_id,
                        content,
                        ..
                    } => {
                        streaming_tool_call_ids.insert(internal_call_id);
                        match content {
                            ToolCallDeltaContent::Name(name) => {
                                scroll.push(format!("Toolcall: {name} - ")).await;
                            }
                            ToolCallDeltaContent::Delta(delta) => {
                                scroll.push(delta).await;
                            }
                        }
                    }
                    Reasoning(reasoning) => {
                        scroll
                            .push(
                                reasoning
                                    .content
                                    .into_iter()
                                    .map(|c| match c {
                                        rig_core::message::ReasoningContent::Text {
                                            text, ..
                                        } => text,
                                        rig_core::message::ReasoningContent::Encrypted(s) => s,
                                        rig_core::message::ReasoningContent::Redacted { data } => {
                                            data
                                        }
                                        rig_core::message::ReasoningContent::Summary(s) => s,
                                        _ => String::new(),
                                    })
                                    .collect(),
                            )
                            .await;
                    }
                    ReasoningDelta { reasoning, .. } => {
                        scroll.push(reasoning).await;
                    }
                    _ => (),
                },
                StreamUserItem(content) => {
                    let StreamedUserContent::ToolResult { tool_result, .. } = content;
                    for content in tool_result.content {
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
                    debug!("Usage: {:?}", final_response.usage());
                    output = Some(final_response);
                }
                CompletionCall(completion_call) => {
                    debug!(
                        call_index = completion_call.call_index,
                        usage = ?completion_call.usage,
                        "Completion call finished."
                    );
                }
                _ => warn!("Unhandled stream chunk."),
            }
        }
        finished.store(true, Ordering::Relaxed);
        if !self.config.agent.answer.wait_for_output_scrolling {
            scrolling_handle.abort();
        }
        scrolling_handle.await.ok();
        drop(_pb_span_enter);
        // 获取了 finish 之后可能会没有及时获取 final response, 导致 output 为空.
        let (output, usage) = if let Some(output) = output {
            (output.response().to_string(), Some(output.usage()))
        } else {
            (scroll.message().await, None)
        };
        Ok(StreamChatStatus {
            output,
            usage,
            answers,
        })
    }

    /// answer agent 解决一个 `prompt`, 或修改命令.
    async fn resolve_internal(
        &self,
        prompt: String,
        modify_option: Option<ModifyOption>,
        attached: Option<String>,
    ) -> Result<AnswerAgentResponse> {
        // stream_prompt 会自动处理工具的调用.
        let attached_iter = attached
            .into_iter()
            .map(|a| Message::user(self.profile.attach(a).fmt()));
        let mut history: Vec<Message> = if let Some(modify_option) = &modify_option {
            modify_option
                .history
                .clone()
                .into_iter()
                .chain([Message::user(
                    self.profile.modify(&modify_option.command).fmt(),
                )])
                .chain(attached_iter)
                .collect()
        } else {
            attached_iter.collect()
        };

        let mut status = self
            .stream_chat()
            .span_title("Resolving")
            .prompt(prompt.clone())
            .history(history.clone())
            .call()
            .await?;

        history.push(Message::user(&prompt));
        history.push(Message::assistant(&status.output));

        // 仅当当前回答是命令模式时, 才做"疑似无效命令"校验, 让模型复核.
        if let Some(AnswerArgs {
            answer: AnswerBody::Commands { commands },
        }) = &status.answers
            && commands.iter().any(is_potentially_invalid_command)
            && let Ok(check_valid_status) = self
                .stream_chat()
                .span_title("Checking Valid")
                .history(history.clone())
                .prompt(
                    self.profile
                        .check_valid(
                            commands
                                .iter()
                                .map(|x| x.content.as_str())
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                        .fmt(),
                )
                .call()
                .await
        {
            status.answers = check_valid_status.answers;
            status.usage = usage_sum(status.usage, check_valid_status.usage);
        }

        if status.answers.is_none()
            && let Ok(check_finish_status) = self
                .stream_chat()
                .span_title("Finishing")
                .prompt(self.profile.check_finish())
                .history(history.clone())
                .call()
                .await
        {
            status.answers = check_finish_status.answers;
            status.usage = usage_sum(status.usage, check_finish_status.usage);
        }

        // todo 这里使用 ratatui 输出对话框.
        if status.answers.is_none() {
            warn!("No answer provided.");
        }
        // 兜底: 模型未给出有效回答时, 给一个空的文本回答.
        let answer = status
            .answers
            .map(|x| x.answer)
            .unwrap_or(AnswerBody::Text {
                content: String::new(),
            });
        if matches!(answer, AnswerBody::Commands { .. }) {
            info!("AnswerAgent: {}", status.output);
        } else {
            info!("AnswerAgent answered.\n");
        }
        Ok(AnswerAgentResponse {
            messages: history,
            answer,
        })
    }
}

#[bon::bon]
impl AnswerAgent {
    #[builder]
    pub async fn resolve(
        &self,
        prompt: String,
        modify_option: Option<ModifyOption>,
        attached: Option<String>,
    ) -> Result<AnswerAgentResponse> {
        self.resolve_internal(prompt, modify_option, attached).await
    }

    #[builder]
    async fn stream_chat(
        &self,
        span_title: Option<&str>,
        prompt: String,
        history: Vec<Message>,
    ) -> Result<StreamChatStatus> {
        self.stream_chat_internal(span_title, prompt, history).await
    }
}

#[cfg(test)]
mod test {
    use unicode_width::UnicodeWidthStr;

    use crate::agent::answer::ScrolliingMessage;

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

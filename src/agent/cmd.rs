use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use reqwest::header::HeaderMap;
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::completion::Message;
use rig::providers::openai::{self, CompletionModel};
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use rig::tool::ToolDyn;
use tokio_stream::StreamExt;
use tracing::{debug, info, info_span};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;

use crate::agent::{
    AssistantTurn,
    tools::{DangerousHelp, Help, Man, SubmitCommands, TheFuck, Tldr},
    turn_from_final_response,
};
use crate::config::AppConfig;
use crate::config::profile::CommandProfile;
use crate::error::{Error, Result};
use crate::shell::Shell;

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

const MULTI_TURN: usize = 100;
const SCROLL_WIDTH: usize = 40;
const SCROLL_STEP: usize = 7;
const SCROLL_INTERVAL_MS: u64 = 30;
const SPINNER: [&str; 5] = ["-", "\\", "|", "/", ""];

struct ScrollingMessage {
    scroll_width: usize,
    message: String,
    cursor: usize,
}

impl ScrollingMessage {
    fn new(scroll_width: usize) -> Self {
        Self {
            scroll_width,
            message: String::new(),
            cursor: 0,
        }
    }

    fn push(&mut self, value: &str) {
        self.message.push_str(value);
    }

    fn has_unread(&self) -> bool {
        self.message.len() > self.cursor
    }

    fn window_at_first(s: &str, width: usize) -> &str {
        let mut acc = 0;
        if let Some((idx, ch)) = s
            .char_indices()
            .map_while(|(idx, ch)| {
                acc += unicode_width::UnicodeWidthChar::width_cjk(ch).unwrap_or(0);
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
                acc += unicode_width::UnicodeWidthChar::width_cjk(ch).unwrap_or(0);
                if acc <= width { Some(idx) } else { None }
            })
            .last()
        {
            &s[idx..]
        } else {
            ""
        }
    }

    fn scroll(&mut self, step: usize) -> String {
        let appendant = Self::window_at_first(&self.message[self.cursor..], step);
        let window = Self::window_at_last(
            &self.message[..self.cursor + appendant.len()],
            self.scroll_width,
        );
        self.cursor += appendant.len();
        window.to_string()
    }
}

pub struct CommandAgent {
    profile: CommandProfile,
    wait_for_output_scrolling: bool,
    agent: Agent<CompletionModel>,
}

#[bon::bon]
impl CommandAgent {
    #[builder]
    pub fn builder(
        os: String,
        shell: &Shell,
        profile: CommandProfile,
        config: AppConfig,
    ) -> Result<Self> {
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
                .output_n(config.agent.cmd.output_n)
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
            wait_for_output_scrolling: config.agent.cmd.wait_for_output_scrolling,
            agent: builder.tools(tools).build(),
        })
    }
}

#[bon::bon]
impl CommandAgent {
    async fn resolve_streaming(
        &self,
        span_title: &str,
        prompt: String,
        history: Vec<Message>,
    ) -> Result<AssistantTurn> {
        let mut stream = self
            .agent
            .stream_chat(&prompt, history)
            .multi_turn(MULTI_TURN)
            .await;
        let scroll = Arc::new(Mutex::new(ScrollingMessage::new(SCROLL_WIDTH)));
        let finished = Arc::new(AtomicBool::new(false));
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
            let pb_span = pb_span.clone();
            let scroll = Arc::clone(&scroll);
            let finished = Arc::clone(&finished);
            tokio::spawn(async move {
                while !finished.load(Ordering::Relaxed) || scroll.lock().unwrap().has_unread() {
                    let msg = scroll.lock().unwrap().scroll(SCROLL_STEP);
                    if !msg.is_empty() {
                        pb_span.pb_set_message(&msg.replace("\n", " "));
                    }
                    tokio::time::sleep(Duration::from_millis(SCROLL_INTERVAL_MS)).await;
                }
            })
        };

        let mut final_response = None;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    finished.store(true, Ordering::Relaxed);
                    if !self.wait_for_output_scrolling {
                        scrolling_handle.abort();
                    }
                    scrolling_handle.await.ok();
                    drop(_pb_span_enter);
                    return Err(Error::StreamingError(error.to_string()));
                }
            };
            match chunk {
                MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text)) => {
                    scroll.lock().unwrap().push(&text.text);
                }
                MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::ToolCall {
                    tool_call,
                    ..
                }) => {
                    info!(
                        tool = %tool_call.function.name,
                        arguments = %tool_call.function.arguments,
                        "agent tool call"
                    );
                }
                MultiTurnStreamItem::FinalResponse(response) => {
                    final_response = Some(response);
                    break;
                }
                _ => {}
            }
        }

        finished.store(true, Ordering::Relaxed);
        if !self.wait_for_output_scrolling {
            scrolling_handle.abort();
        }
        scrolling_handle.await.ok();
        drop(_pb_span_enter);

        let Some(final_response) = final_response else {
            return Err(Error::StreamingError(
                "assistant stream ended before final response".into(),
            ));
        };
        let turn = turn_from_final_response(final_response)?;
        debug!("CommandAgent reply: {}", turn.reply_markdown);
        Ok(turn)
    }

    #[builder]
    pub async fn resolve(
        &self,
        prompt: String,
        modify_option: Option<ModifyOption>,
        attached: Option<String>,
    ) -> Result<AssistantTurn> {
        let mut history = Vec::new();
        let span_title = if modify_option.is_some() {
            "modifying"
        } else {
            "resolving"
        };
        if let Some(modify_option) = modify_option {
            history.extend(modify_option.history);
            history.push(Message::user(
                self.profile.modify(&modify_option.command).fmt(),
            ));
        }
        if let Some(attached) = attached {
            history.push(Message::user(self.profile.attach(attached).fmt()));
        }
        self.resolve_streaming(span_title, prompt, history).await
    }
}

#[cfg(test)]
mod test {
    use super::ScrollingMessage;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn scroll_message() {
        const SCROLL_WIDTH: usize = 10;
        let mut sm = ScrollingMessage::new(SCROLL_WIDTH);
        sm.push("你好世界");
        assert_eq!(sm.scroll(0), "");
        assert_eq!(sm.scroll(1), "");
        assert_eq!(sm.scroll(2), "你");
        assert_eq!(sm.scroll(6), "你好世界");
        sm.push("abc");
        assert_eq!("你好世界ab".width_cjk(), SCROLL_WIDTH);
        let s = sm.scroll(2);
        assert_eq!(s.width_cjk(), SCROLL_WIDTH);
        assert_eq!(s, "你好世界ab");
        sm.push("我能正常滚动");
        assert_eq!(sm.scroll(0), "你好世界ab");
        assert_eq!(sm.scroll(usize::MAX), "能正常滚动");
    }
}

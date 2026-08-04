use std::time::Duration;

use rig_core::{
    agent::{MultiTurnStreamItem, StreamingResult},
    completion::Usage,
    message::{Message, ToolResultContent},
    streaming::{StreamedAssistantContent, StreamedUserContent, ToolCallDeltaContent},
};
use tokio::time::{Instant, MissedTickBehavior};
use tokio_stream::StreamExt;
use tracing::{debug, info_span};
use tracing_indicatif::{span_ext::IndicatifSpanExt, style::ProgressStyle};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    agent::scroll::ScrollWindow,
    error::{Error, Result},
};

const SPINNER: [&str; 7] = [
    "\u{280b}", "\u{2819}", "\u{2838}", "\u{2834}", "\u{2826}", "\u{2807}", "",
];
const SCROLL_WIDTH: usize = 40;
const SCROLL_STEP: usize = 7;
const SCROLL_INTERVAL: Duration = Duration::from_millis(30);
const STATUS_WIDTH: usize = 14;

#[derive(Debug)]
struct ActivityMessage {
    status: String,
    scroll: ScrollWindow,
}

impl ActivityMessage {
    fn new(status: &str) -> Self {
        Self {
            status: status.to_owned(),
            scroll: ScrollWindow::new(SCROLL_WIDTH),
        }
    }

    fn set_status(&mut self, status: &str) {
        self.status.clear();
        self.status.push_str(status);
    }

    fn push_text(&mut self, text: &str) {
        self.set_status("Answering");
        self.scroll.push(text);
    }

    fn push_reasoning(&mut self, reasoning: &str) {
        self.set_status("Thinking");
        self.scroll.push(reasoning);
    }

    fn tick(&mut self) -> String {
        self.scroll.advance(SCROLL_STEP);
        self.render()
    }

    fn finish(&mut self) -> String {
        self.scroll.finish();
        self.render()
    }

    fn render(&self) -> String {
        let window = self.scroll.window();
        let window_padding = SCROLL_WIDTH.saturating_sub(window.width_cjk());
        format!(
            "{} | {window}{}",
            fit_status(&self.status),
            " ".repeat(window_padding)
        )
    }
}

fn fit_status(status: &str) -> String {
    let mut output = String::with_capacity(STATUS_WIDTH);
    let mut used = 0;
    for character in status.chars() {
        let character_width = character.width_cjk().unwrap_or(0);
        if used + character_width > STATUS_WIDTH {
            break;
        }
        used += character_width;
        output.push(character);
    }
    output.push_str(&" ".repeat(STATUS_WIDTH - used));
    output
}

fn tool_status(name: &str) -> String {
    let name = if name == "submit_commands" {
        "submit"
    } else {
        name
    };
    format!("Tool {name}")
}

#[derive(Debug)]
pub(super) struct StreamOutcome {
    pub final_text: String,
    pub messages: Vec<Message>,
    pub usage: Usage,
}

pub(super) async fn collect<R>(
    mut stream: StreamingResult<R>,
    title: &str,
) -> Result<StreamOutcome> {
    let span = info_span!("agent-progress", status = title);
    span.pb_set_style(
        &ProgressStyle::with_template(&format!("{{spinner:.green}} Agent({title}): {{msg}}"))
            .expect("spinner template should be valid")
            .tick_strings(&SPINNER),
    );
    let mut activity = ActivityMessage::new("Waiting");
    span.pb_set_message(&activity.render());
    let _entered = span.enter();
    let mut scrolling_interval = tokio::time::interval_at(
        Instant::now() + SCROLL_INTERVAL,
        SCROLL_INTERVAL,
    );
    scrolling_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut final_response = None;

    loop {
        tokio::select! {
            item = stream.next() => {
                let Some(item) = item else {
                    break;
                };
                let item = item.map_err(|error| Error::StreamingError(error.to_string()))?;
                match item {
                    MultiTurnStreamItem::StreamAssistantItem(content) => match content {
                        StreamedAssistantContent::Text(text) => {
                            activity.push_text(&text.text);
                            span.pb_set_message(&activity.render());
                        }
                        StreamedAssistantContent::Reasoning(reasoning) => {
                            let reasoning = reasoning.display_text();
                            if reasoning.is_empty() {
                                activity.set_status("Thinking");
                            } else {
                                activity.push_reasoning(&reasoning);
                            }
                            span.pb_set_message(&activity.render());
                        }
                        StreamedAssistantContent::ReasoningDelta { reasoning, .. } => {
                            activity.push_reasoning(&reasoning);
                            span.pb_set_message(&activity.render());
                        }
                        StreamedAssistantContent::ToolCall { tool_call, .. } => {
                            activity.set_status(&tool_status(&tool_call.function.name));
                            span.pb_set_message(&activity.render());
                            super::tool_call_log::log(
                                &tool_call.function.name,
                                &tool_call.function.arguments,
                            );
                        }
                        StreamedAssistantContent::ToolCallDelta {
                            content: ToolCallDeltaContent::Name(name),
                            ..
                        } => {
                            activity.set_status(&tool_status(&name));
                            span.pb_set_message(&activity.render());
                        }
                        _ => {}
                    },
                    MultiTurnStreamItem::StreamUserItem(content) => {
                        activity.set_status("Tool result");
                        span.pb_set_message(&activity.render());
                        let StreamedUserContent::ToolResult { tool_result, .. } = content;
                        for content in tool_result.content {
                            if let ToolResultContent::Text(text) = content {
                                let preview = text.text.chars().take(300).collect::<String>();
                                debug!(result_len = text.text.len(), preview = %preview, "Tool result received.");
                            }
                        }
                    }
                    MultiTurnStreamItem::CompletionCall(call) => {
                        debug!(call_index = call.call_index, usage = ?call.usage, "Completion call finished.");
                    }
                    MultiTurnStreamItem::FinalResponse(response) => {
                        final_response = Some(response);
                    }
                    _ => {}
                }
            }
            _ = scrolling_interval.tick() => {
                span.pb_set_message(&activity.tick());
            }
        }
    }

    span.pb_set_message(&activity.finish());

    let final_response = final_response.ok_or_else(|| {
        Error::AgentResponse("Agent stream ended without a final response.".to_string())
    })?;
    let messages = final_response.history().unwrap_or_default().to_vec();
    Ok(StreamOutcome {
        final_text: final_response.response().to_string(),
        messages,
        usage: final_response.usage(),
    })
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::{ActivityMessage, STATUS_WIDTH};

    #[test]
    fn scrolls_reasoning_with_the_thinking_status() {
        let mut activity = ActivityMessage::new("Waiting");
        activity.push_reasoning("先检查目录");

        let message = activity.tick();
        assert_eq!(message.find(" | "), Some(STATUS_WIDTH));
        assert_eq!(message.width_cjk(), STATUS_WIDTH + 3 + super::SCROLL_WIDTH);
        assert_eq!(message.split_once(" | ").unwrap().1.trim_end(), "先检查");
    }

    #[test]
    fn combines_current_status_with_scrolling_text() {
        let mut activity = ActivityMessage::new("Thinking");
        activity.push_reasoning("你好 ");
        activity.push_text("world");

        let message = activity.tick();
        assert_eq!(message.find(" | "), Some(STATUS_WIDTH));
        assert!(message.starts_with("Answering"));
        assert_eq!(message.split_once(" | ").unwrap().1.trim_end(), "你好 wo");
        activity.set_status("Tool explore");
        let message = activity.render();
        assert_eq!(message.find(" | "), Some(STATUS_WIDTH));
        assert!(message.starts_with("Tool explore"));
        assert_eq!(message.split_once(" | ").unwrap().1.trim_end(), "你好 wo");

        activity.set_status("Tool an_extremely_long_tool_name");
        assert_eq!(activity.render().find(" | "), Some(STATUS_WIDTH));
    }

    #[test]
    fn finishing_reveals_the_latest_text_tail() {
        let mut activity = ActivityMessage::new("Waiting");
        activity.push_text("0123456789012345678901234567890123456789tail");

        let message = activity.finish();
        assert_eq!(message.find(" | "), Some(STATUS_WIDTH));
        assert!(message.starts_with("Answering"));
        assert_eq!(
            message.split_once(" | ").unwrap().1,
            "456789012345678901234567890123456789tail"
        );
    }
}

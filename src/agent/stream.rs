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
const SCROLL_BUDGET_PER_CHAR: u128 = 1_000_000;

#[derive(Debug)]
struct ActivityMessage {
    status: String,
    scroll: ScrollWindow,
    scroll_char_speed_limit: usize,
    scroll_budget: u128,
}

impl ActivityMessage {
    fn new(status: &str, scroll_char_speed_limit: usize) -> Self {
        Self {
            status: status.to_owned(),
            scroll: ScrollWindow::new(SCROLL_WIDTH),
            scroll_char_speed_limit,
            scroll_budget: 0,
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

    fn tick(&mut self, elapsed: Duration) -> String {
        let step = self.scroll_step(elapsed);
        self.scroll.advance(step);
        self.render()
    }

    fn scroll_step(&mut self, elapsed: Duration) -> usize {
        if self.scroll_char_speed_limit == 0 {
            return SCROLL_STEP;
        }
        self.scroll_budget +=
            self.scroll_char_speed_limit as u128 * elapsed.as_micros();
        let step = ((self.scroll_budget / SCROLL_BUDGET_PER_CHAR) as usize)
            .min(SCROLL_STEP);
        self.scroll_budget %= SCROLL_BUDGET_PER_CHAR;
        step
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
    scroll_char_speed_limit: usize,
) -> Result<StreamOutcome> {
    let span = info_span!("agent-progress", status = title);
    span.pb_set_style(
        &ProgressStyle::with_template(&format!("{{spinner:.green}} Agent({title}): {{msg}}"))
            .expect("spinner template should be valid")
            .tick_strings(&SPINNER),
    );
    let mut activity = ActivityMessage::new("Waiting", scroll_char_speed_limit);
    span.pb_set_message(&activity.render());
    let _entered = span.enter();
    let mut scrolling_interval = tokio::time::interval_at(
        Instant::now() + SCROLL_INTERVAL,
        SCROLL_INTERVAL,
    );
    scrolling_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_scroll = Instant::now();
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
                let now = Instant::now();
                span.pb_set_message(&activity.tick(now - last_scroll));
                last_scroll = now;
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
    use std::time::Duration;

    use unicode_width::UnicodeWidthStr;

    use super::{ActivityMessage, STATUS_WIDTH};

    #[test]
    fn scrolls_reasoning_with_the_thinking_status() {
        let mut activity = ActivityMessage::new("Waiting", 0);
        activity.push_reasoning("先检查目录");

        let message = activity.tick(Duration::from_millis(30));
        assert_eq!(message.find(" | "), Some(STATUS_WIDTH));
        assert_eq!(message.width_cjk(), STATUS_WIDTH + 3 + super::SCROLL_WIDTH);
        assert_eq!(message.split_once(" | ").unwrap().1.trim_end(), "先检查");
    }

    #[test]
    fn combines_current_status_with_scrolling_text() {
        let mut activity = ActivityMessage::new("Thinking", 0);
        activity.push_reasoning("你好 ");
        activity.push_text("world");

        let message = activity.tick(Duration::from_millis(30));
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
        let mut activity = ActivityMessage::new("Waiting", 0);
        activity.push_text("0123456789012345678901234567890123456789tail");

        let message = activity.finish();
        assert_eq!(message.find(" | "), Some(STATUS_WIDTH));
        assert!(message.starts_with("Answering"));
        assert_eq!(
            message.split_once(" | ").unwrap().1,
            "456789012345678901234567890123456789tail"
        );
    }

    #[test]
    fn speed_limit_averages_scrolling_over_time() {
        let mut activity = ActivityMessage::new("Waiting", 10);
        activity.push_text("0123456789");

        for _ in 0..10 {
            activity.tick(Duration::from_millis(30));
        }
        let message = activity.render();
        assert_eq!(message.split_once(" | ").unwrap().1.trim_end(), "012");
    }

    #[test]
    fn speed_limit_does_not_exceed_the_per_tick_step() {
        let mut activity = ActivityMessage::new("Waiting", 1_000_000);
        activity.push_text("0123456789");

        let message = activity.tick(Duration::from_secs(10));
        assert_eq!(message.split_once(" | ").unwrap().1.trim_end(), "0123456");
    }
}

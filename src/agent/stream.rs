use rig_core::{
    agent::{MultiTurnStreamItem, StreamingResult},
    completion::Usage,
    message::{Message, ToolResultContent},
    streaming::{StreamedAssistantContent, StreamedUserContent, ToolCallDeltaContent},
};
use tokio_stream::StreamExt;
use tracing::{debug, info_span};
use tracing_indicatif::{span_ext::IndicatifSpanExt, style::ProgressStyle};

use crate::error::{Error, Result};

const SPINNER: [&str; 7] = [
    "\u{280b}", "\u{2819}", "\u{2838}", "\u{2834}", "\u{2826}", "\u{2807}", "",
];

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
    span.pb_set_message("Waiting for response");
    let _entered = span.enter();
    let mut final_response = None;

    while let Some(item) = stream.next().await {
        let item = item.map_err(|error| Error::StreamingError(error.to_string()))?;
        match item {
            MultiTurnStreamItem::StreamAssistantItem(content) => match content {
                StreamedAssistantContent::Text(_) => span.pb_set_message("Writing response"),
                StreamedAssistantContent::Reasoning(_)
                | StreamedAssistantContent::ReasoningDelta { .. } => {
                    span.pb_set_message("Thinking")
                }
                StreamedAssistantContent::ToolCall { tool_call, .. } => {
                    span.pb_set_message(&format!("Using {}", tool_call.function.name));
                    super::tool_call_log::log(
                        &tool_call.function.name,
                        &tool_call.function.arguments,
                    );
                }
                StreamedAssistantContent::ToolCallDelta {
                    content: ToolCallDeltaContent::Name(name),
                    ..
                } => span.pb_set_message(&format!("Using {name}")),
                _ => {}
            },
            MultiTurnStreamItem::StreamUserItem(content) => {
                span.pb_set_message("Processing tool result");
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

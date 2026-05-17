pub mod chat;
pub mod cmd;
pub mod tools;

use rig::agent::{FinalResponse, PromptResponse};
use rig::completion::{AssistantContent, Message, PromptError};
use rig::tool::Tool;

use crate::agent::tools::{CommandCandidate, SubmitCommandsArgs};
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct AssistantTurn {
    pub reply_markdown: String,
    pub commands: Vec<CommandCandidate>,
    pub messages: Vec<Message>,
}

pub(crate) fn turn_from_prompt_response(response: PromptResponse) -> Result<AssistantTurn> {
    let PromptResponse {
        output,
        usage: _,
        messages,
        ..
    } = response;
    let messages = messages.ok_or_else(|| {
        Error::InvalidInput("agent response missing extended message history".into())
    })?;
    let commands = extract_commands(&messages)?;
    Ok(AssistantTurn {
        reply_markdown: output,
        commands,
        messages,
    })
}

pub(crate) fn turn_from_final_response(response: FinalResponse) -> Result<AssistantTurn> {
    let messages = response.history().ok_or_else(|| {
        Error::InvalidInput("agent streaming response missing updated history".into())
    })?;
    let messages = messages.to_vec();
    let commands = extract_commands(&messages)?;
    Ok(AssistantTurn {
        reply_markdown: response.response().to_string(),
        commands,
        messages,
    })
}

fn extract_commands(messages: &[Message]) -> Result<Vec<CommandCandidate>> {
    let mut latest = None;
    for message in messages.iter().rev() {
        let Message::Assistant { content, .. } = message else {
            continue;
        };
        for item in content.iter() {
            let AssistantContent::ToolCall(tool_call) = item else {
                continue;
            };
            if tool_call.function.name != crate::agent::tools::SubmitCommands::NAME {
                continue;
            }
            latest = Some(tool_call.function.arguments.clone());
            break;
        }
        if latest.is_some() {
            break;
        }
    }
    if let Some(arguments) = latest {
        let parsed: SubmitCommandsArgs = serde_json::from_value(arguments).map_err(|error| {
            Error::InvalidInput(format!("invalid submit_commands payload: {error}"))
        })?;
        Ok(parsed.results)
    } else {
        Ok(Vec::new())
    }
}

pub(crate) fn map_prompt_error(error: PromptError) -> Error {
    Error::PromptError(error.to_string())
}

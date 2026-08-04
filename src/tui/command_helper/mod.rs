use std::path::PathBuf;

use tracing::debug;

use crate::{
    agent::{answer::AnswerAgentResponse, detect_os},
    config::{AppConfig, profile::Profiles},
    error::Result,
    shell::Shell,
    tui::candidate,
};

pub(crate) mod modify;
pub(crate) mod select;

pub(crate) const MINIMUM_TUI_WIDTH: usize = 45;

#[bon::builder]
pub async fn run(
    prompt: &str,
    plain: bool,
    config: AppConfig,
    shell: &Shell,
    attached: Option<String>,
    profiles: Profiles,
    htcmd_file: Option<PathBuf>,
) -> Result<AnswerAgentResponse> {
    run_internal(prompt, plain, config, shell, attached, profiles, htcmd_file).await
}

async fn run_internal(
    prompt: &str,
    plain: bool,
    config: AppConfig,
    shell: &Shell,
    attached: Option<String>,
    profiles: Profiles,
    htcmd_file: Option<PathBuf>,
) -> Result<AnswerAgentResponse> {
    let agent = crate::agent::answer::AnswerAgent::builder()
        .profile(profiles.answer.clone())
        .os(detect_os())
        .shell(shell)
        .config(config)
        .build()?;
    let mut response = agent
        .resolve()
        .prompt(prompt.to_string())
        .maybe_attached(attached)
        .call()
        .await?;
    loop {
        if plain && !response.commands.is_empty() {
            candidate::print_plain_commands(&response.commands)?;
            break;
        }

        candidate::show_response_text(&response, plain)?;
        if response.commands.is_empty() {
            break;
        }

        candidate::print_candidates(&response.commands)?;
        let action = candidate::select(response.commands.clone()).await?;
        let Some(action) = action else {
            break;
        };
        debug!("Select action: {action:?}");
        match action.kind {
            candidate::ActionKind::Copy => {
                candidate::copy(action.command)?;
                break;
            }
            candidate::ActionKind::Execute => {
                candidate::execute(action.command, shell.path()).await?;
                break;
            }
            candidate::ActionKind::PrintToInputBuffer => {
                candidate::print_to_input_buffer(&htcmd_file, &action.command).await?;
                break;
            }
            candidate::ActionKind::Modify => {
                if !candidate::modify(&agent, &mut response, action.command).await? {
                    break;
                }
            }
        }
    }
    Ok(response)
}

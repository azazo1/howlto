use std::path::PathBuf;

use tracing::{info, warn};

use crate::{
    agent::{
        answer::{AnswerAgent, AnswerAgentResponse},
        detect_os,
    },
    config::{AppConfig, profile::Profiles},
    error::Result,
    session::{Session, SessionStore},
    shell::Shell,
    tui::candidate,
};

mod input;
mod menu;

#[bon::builder]
pub async fn run(
    config_dir: PathBuf,
    config: AppConfig,
    profiles: Profiles,
    shell: &Shell,
    htcmd_file: Option<PathBuf>,
) -> Result<()> {
    run_internal(config_dir, config, profiles, shell, htcmd_file).await
}

async fn run_internal(
    config_dir: PathBuf,
    config: AppConfig,
    profiles: Profiles,
    shell: &Shell,
    htcmd_file: Option<PathBuf>,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let store = SessionStore::new(&config_dir, config.session);
    store.cleanup().await;
    let sessions = store.list(&cwd).await?;
    let mut session = match menu::choose(sessions).await? {
        None => return Ok(()),
        Some(menu::MenuChoice::New) => None,
        Some(menu::MenuChoice::Session(id)) => match store.load(&cwd, id).await? {
            Some(session) => {
                info!(session_id = %session.id, "Resumed session.");
                Some(session)
            }
            None => {
                warn!(session_id = %id, "Selected session could not be loaded, starting a new one.");
                None
            }
        },
    };

    if let Some(session) = &session {
        let preview = AnswerAgentResponse {
            messages: session.messages.clone(),
            final_text: session.final_text.clone(),
            commands: session.commands.clone(),
        };
        candidate::show_response_text(&preview, false)?;
        if !preview.commands.is_empty() {
            candidate::print_candidates(&preview.commands)?;
        }
    }

    let agent = AnswerAgent::builder()
        .profile(profiles.answer.clone())
        .os(detect_os())
        .shell(shell)
        .config(config)
        .build()?;

    loop {
        let Some(raw_prompt) = input::App::prompt().await? else {
            break;
        };
        let prompt = raw_prompt.trim().to_string();
        if prompt.is_empty() {
            continue;
        }
        if prompt == "/exit" {
            break;
        }

        let history = session
            .as_ref()
            .map(|session| session.messages.clone())
            .unwrap_or_default();
        info!(prompt = %prompt, "Chat prompt submitted.");
        let mut response = agent
            .resolve()
            .prompt(prompt.clone())
            .history(history)
            .call()
            .await?;
        handle_candidates(&agent, &mut response, shell, &htcmd_file).await?;

        if let Some(existing) = session.as_mut() {
            existing.update(&prompt, &response);
        } else {
            session = Some(Session::new(&cwd, &prompt, &response));
        }
        if let Some(session) = session.as_mut()
            && let Err(error) = store.save(session).await
        {
            warn!(error = %error, "Failed to save chat session.");
        }
    }
    Ok(())
}

async fn handle_candidates(
    agent: &AnswerAgent,
    response: &mut AnswerAgentResponse,
    shell: &Shell,
    htcmd_file: &Option<PathBuf>,
) -> Result<()> {
    loop {
        candidate::show_response_text(response, false)?;
        if response.commands.is_empty() {
            return Ok(());
        }
        candidate::print_candidates(&response.commands)?;
        let Some(action) = candidate::select(response.commands.clone()).await? else {
            return Ok(());
        };
        match action.kind {
            candidate::ActionKind::Copy => {
                candidate::copy(action.command)?;
                return Ok(());
            }
            candidate::ActionKind::Execute => {
                candidate::execute(action.command, shell.path()).await?;
                return Ok(());
            }
            candidate::ActionKind::PrintToInputBuffer => {
                candidate::print_to_input_buffer(htcmd_file, &action.command).await?;
                return Ok(());
            }
            candidate::ActionKind::Modify => {
                if !candidate::modify(agent, response, action.command).await? {
                    return Ok(());
                }
            }
        }
    }
}

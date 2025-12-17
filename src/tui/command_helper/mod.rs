use std::{path::Path, process::Stdio};

use clipboard_rs::Clipboard;
use tokio::io;
use tracing::info;

use crate::{
    agent::shell_command_gen::{ScgAgent, ScgAgentResponse},
    config::{AppConfig, profile::Profiles},
    error::{Error, Result},
    tui::command_helper::select::ActionKind,
};

mod modify;
mod select;

async fn execute(command: String, shell_path: impl AsRef<Path>) -> Result<()> {
    let mut child = tokio::process::Command::new(shell_path.as_ref())
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()?;
    let mut child_stdin = child.stdin.take().ok_or(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "cannot take child stdin",
    ))?;
    let mut child_stdout = child.stdout.take().ok_or(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "cannot take child stdout",
    ))?;
    let mut child_stderr = child.stderr.take().ok_or(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "cannot take child stderr",
    ))?;
    let stdin_handle =
        tokio::spawn(
            async move { tokio::io::copy(&mut tokio::io::stdin(), &mut child_stdin).await },
        );
    let stdout_handle =
        tokio::spawn(
            async move { tokio::io::copy(&mut child_stdout, &mut tokio::io::stdout()).await },
        );
    let stderr_handle =
        tokio::spawn(
            async move { tokio::io::copy(&mut child_stderr, &mut tokio::io::stderr()).await },
        );
    let (rout, rerr) = tokio::try_join!(stdout_handle, stderr_handle)?;
    rout?;
    rerr?;
    stdin_handle.abort();
    stdin_handle.await.ok();
    Ok(())
}

fn copy(text: String) -> Result<()> {
    let cx = clipboard_rs::ClipboardContext::new()
        .map_err(|_| Error::ClipboardError("Failed to access clipboard.".into()))?;
    cx.set_text(text)
        .map_err(|_| Error::ClipboardError("Failed to copy.".into()))?;
    Ok(())
}

/// 交互式修改选定的 command.
/// # Returns
/// 是否有进行修改.
async fn modify(
    agent: &ScgAgent,
    prev_resp: &mut ScgAgentResponse,
    command: String,
) -> Result<bool> {
    info!("Modify selected: {}", command);
    let prompt = modify::App::prompt(command.clone()).await?;
    if let Some(prompt) = prompt {
        info!("Modify prompt: {}", prompt);
        agent.modify(prev_resp, command, prompt).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[bon::builder]
pub async fn run(
    prompt: &str,
    plain: bool,
    config: AppConfig,
    shell_name: &str,
    shell_path: impl AsRef<Path>,
    profiles: Profiles,
) -> Result<()> {
    run_inner(prompt, plain, config, shell_name, shell_path, profiles).await
}

async fn run_inner(
    prompt: &str,
    plain: bool,
    config: AppConfig,
    shell_name: &str,
    shell_path: impl AsRef<Path>,
    profiles: Profiles,
) -> Result<()> {
    let shell_path = shell_path.as_ref();
    let agent = ScgAgent::builder()
        .profile(profiles.shell_command_gen.clone())
        .os(std::env::consts::OS.to_string())
        .shell(shell_name.to_string())
        .config(config)
        .build()?;
    let response = agent.resolve(prompt.to_string()).await?;
    if plain {
        println!("{}", response.commands.join("\n"));
    } else if !response.commands.is_empty() {
        let mut response = response;
        loop {
            let action = select::App::select(response.commands.clone()).await?;
            let mut should_exit = true;
            if let Some(action) = &action {
                match action.kind {
                    ActionKind::Copy => copy(action.command.clone())?,
                    ActionKind::Execute => execute(action.command.clone(), shell_path).await?,
                    ActionKind::Print => println!("{}", action.command),
                    ActionKind::Modify => {
                        should_exit =
                            !modify(&agent, &mut response, action.command.clone()).await?;
                    }
                }
            }
            if should_exit {
                break;
            }
        }
    }
    Ok(())
}

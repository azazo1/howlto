use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use clipboard_rs::Clipboard;
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};
use tracing::debug;

use crate::{
    agent::{
        AssistantTurn,
        cmd::{CommandAgent, ModifyOption},
    },
    config::{AppConfig, profile::Profiles},
    error::{Error, Result},
    shell::Shell,
    tui::cmd::select::ActionKind,
};

mod modify;
mod select;

pub(crate) const MINIMUM_TUI_WIDTH: usize = 45;

fn detect_os() -> String {
    sysinfo::System::name().unwrap_or(std::env::consts::OS.to_string())
}

pub(crate) async fn execute_command(command: String, shell_path: &Path) -> Result<()> {
    let mut child = tokio::process::Command::new(shell_path)
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()?;
    let mut child_stdout = child.stdout.take().ok_or(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "cannot take child stdout",
    ))?;
    let mut child_stderr = child.stderr.take().ok_or(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "cannot take child stderr",
    ))?;
    tokio::spawn(async move { tokio::io::copy(&mut child_stdout, &mut tokio::io::stdout()).await });
    tokio::spawn(async move { tokio::io::copy(&mut child_stderr, &mut tokio::io::stderr()).await });
    child.wait().await?;
    Ok(())
}

pub(crate) fn copy_command(text: String) -> Result<()> {
    let cx = clipboard_rs::ClipboardContext::new()
        .map_err(|_| Error::ClipboardError("Failed to access clipboard.".into()))?;
    cx.set_text(text)
        .map_err(|_| Error::ClipboardError("Failed to copy.".into()))?;
    Ok(())
}

async fn modify(
    agent: &CommandAgent,
    prev_turn: &mut AssistantTurn,
    command: String,
) -> Result<bool> {
    let prompt = modify::App::prompt(command.clone()).await?;
    if let Some(prompt) = prompt {
        debug!("Modify prompt: {}", prompt);
        *prev_turn = agent
            .resolve()
            .prompt(prompt)
            .modify_option(ModifyOption::new(prev_turn.messages.clone(), command))
            .call()
            .await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub(crate) async fn print_command_to_input_buffer(
    htcmd_file: Option<&Path>,
    command: &str,
) -> io::Result<()> {
    println!("{}", command);
    if let Some(htcmd_file) = htcmd_file {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(htcmd_file)
            .await?;
        f.write_all(command.as_bytes()).await?;
    }
    Ok(())
}

#[bon::builder]
pub async fn run(
    prompt: &str,
    plain: bool,
    config: AppConfig,
    shell: &Shell,
    attached: Option<String>,
    profiles: Profiles,
    htcmd_file: Option<PathBuf>,
) -> Result<()> {
    let agent = CommandAgent::builder()
        .profile(profiles.cmd.clone())
        .os(detect_os())
        .shell(shell)
        .config(config)
        .build()?;
    let turn = agent
        .resolve()
        .prompt(prompt.to_string())
        .maybe_attached(attached)
        .call()
        .await?;

    if plain {
        println!(
            "{}",
            turn.commands
                .iter()
                .map(|item| item.command.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        return Ok(());
    }

    if turn.commands.is_empty() {
        println!("{}", turn.reply_markdown);
        return Ok(());
    }

    let mut turn = turn;
    loop {
        let action =
            select::App::select(turn.commands.clone(), turn.reply_markdown.clone()).await?;
        let mut should_exit = true;
        if let Some(action) = &action {
            debug!("Select action: {action:?}");
            match action.kind {
                ActionKind::Copy => copy_command(action.command.clone())?,
                ActionKind::Execute => {
                    execute_command(action.command.clone(), shell.path()).await?
                }
                ActionKind::PrintToInputBuffer => {
                    print_command_to_input_buffer(htcmd_file.as_deref(), &action.command).await?
                }
                ActionKind::Modify => {
                    should_exit = !modify(&agent, &mut turn, action.command.clone()).await?;
                    should_exit |= turn.commands.is_empty();
                }
            }
        }
        if should_exit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::{
        agent::tools::CommandCandidate,
        tui::cmd::select::{Action, ActionKind},
    };

    #[tokio::test]
    #[ignore = "manual tui smoke test"]
    async fn tui() {
        println!("Manually select 3 with PrintToInputBuffer action:");
        let action = super::select::App::select(
            [
                CommandCandidate {
                    command: "1".into(),
                    summary: "This is one.".into(),
                },
                CommandCandidate {
                    command: "2".into(),
                    summary: "This is two.".into(),
                },
                CommandCandidate {
                    command: "3".into(),
                    summary: "This is three.".into(),
                },
            ]
            .into(),
            "reply".into(),
        )
        .await
        .unwrap();
        assert_eq!(
            action,
            Some(Action {
                kind: ActionKind::PrintToInputBuffer,
                command: "3".to_string()
            })
        );
    }
}

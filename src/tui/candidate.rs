use std::{
    io::Write,
    path::Path,
    process::Stdio,
};

use clipboard_rs::Clipboard;
use tokio::{fs, io, io::AsyncWriteExt};

use crate::{
    agent::{
        answer::{AnswerAgent, AnswerAgentResponse, ModifyOption},
        submit_commands::CommandItem,
    },
    error::{Error, Result},
    tui::{
        command_helper::{modify::App as ModifyApp, select::App as SelectApp},
        markdown,
    },
};
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub kind: ActionKind,
    pub command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Copy,
    Execute,
    Modify,
    PrintToInputBuffer,
}

pub(crate) fn write_candidates(
    writer: &mut impl Write,
    commands: &[CommandItem],
) -> std::io::Result<()> {
    writeln!(writer, "Candidate commands:")?;
    for (index, command) in commands.iter().enumerate() {
        let number = index + 1;
        let total = commands.len();
        let description = command.description.lines().collect::<Vec<_>>().join(" ");
        if description.is_empty() {
            writeln!(writer, "\n[Command {number}/{total}]")?;
        } else {
            writeln!(writer, "\n[Command {number}/{total}] {description}")?;
        }
        write!(writer, "{}", command.command)?;
        if !command.command.ends_with('\n') {
            writeln!(writer)?;
        }
        writeln!(writer, "[/Command {number}/{total}]")?;
    }
    Ok(())
}

pub(crate) fn print_candidates(commands: &[CommandItem]) -> std::io::Result<()> {
    let mut stderr = std::io::stderr().lock();
    write_candidates(&mut stderr, commands)?;
    stderr.flush()
}

pub(crate) fn write_plain_commands(
    writer: &mut impl Write,
    commands: &[CommandItem],
) -> std::io::Result<()> {
    for command in commands {
        write!(writer, "{}", command.command)?;
        if !command.command.ends_with('\n') {
            writeln!(writer)?;
        }
    }
    Ok(())
}

pub(crate) fn print_plain_commands(commands: &[CommandItem]) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    write_plain_commands(&mut stdout, commands)?;
    stdout.flush()
}

pub(crate) async fn select(commands: Vec<CommandItem>) -> Result<Option<Action>> {
    SelectApp::select(commands).await
}

pub(crate) async fn execute(command: String, shell_path: impl AsRef<Path>) -> Result<()> {
    let mut child = tokio::process::Command::new(shell_path.as_ref())
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

pub(crate) fn copy(text: String) -> Result<()> {
    let cx = clipboard_rs::ClipboardContext::new()
        .map_err(|_| Error::ClipboardError("Failed to access clipboard.".into()))?;
    cx.set_text(text)
        .map_err(|_| Error::ClipboardError("Failed to copy.".into()))?;
    Ok(())
}

/// 交互式修改选定的 command.
/// # Returns
/// 是否有进行修改.
pub(crate) async fn modify(
    agent: &AnswerAgent,
    prev_resp: &mut AnswerAgentResponse,
    command: String,
) -> Result<bool> {
    let prompt = ModifyApp::prompt(command.clone()).await?;
    if let Some(prompt) = prompt {
        info!("Modify prompt: {}", prompt);
        *prev_resp = agent
            .resolve()
            .prompt(prompt)
            .modify_option(ModifyOption::new(prev_resp.messages.clone(), command))
            .call()
            .await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub(crate) async fn print_to_input_buffer(
    htcmd_file: &Option<impl AsRef<Path>>,
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

pub(crate) fn show_response_text(response: &AnswerAgentResponse, plain: bool) -> std::io::Result<()> {
    if response.final_text.trim().is_empty() {
        return Ok(());
    }
    if plain {
        println!("{}", markdown::to_plain_text(&response.final_text));
    } else {
        markdown::print_ansi(&response.final_text);
        std::io::stdout().flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_output_separates_multiline_commands_from_options() {
        let commands = vec![
            CommandItem {
                command: "first line\nsecond line".into(),
                description: "multiline".into(),
            },
            CommandItem {
                command: "single line".into(),
                description: "single".into(),
            },
        ];
        let mut output = Vec::new();

        write_candidates(&mut output, &commands).unwrap();
        let output = String::from_utf8(output).unwrap();
        let multiline_content = output.find("first line\nsecond line").unwrap();
        let first_end = output.find("[/Command 1/2]").unwrap();
        let second_start = output.find("[Command 2/2]").unwrap();

        assert!(multiline_content < first_end);
        assert!(first_end < second_start);
    }

    #[test]
    fn plain_command_output_contains_only_raw_commands() {
        let commands = vec![
            CommandItem {
                command: "printf one".into(),
                description: "first".into(),
            },
            CommandItem {
                command: "printf two\nprintf three".into(),
                description: "second".into(),
            },
        ];
        let mut output = Vec::new();
        write_plain_commands(&mut output, &commands).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "printf one\nprintf two\nprintf three\n"
        );
    }

    #[tokio::test]
    #[ignore = "需要真实 TTY 交互 (手动选择), 用 `cargo test select_app_print_to_input_buffer -- --ignored --nocapture` 运行"]
    async fn select_app_print_to_input_buffer() {
        println!("Manually select 3 with Copy action:");
        let action = SelectApp::select(
            [
                CommandItem {
                    command: "1".into(),
                    description: "This is one.".into(),
                },
                CommandItem {
                    command: "2".into(),
                    description: "This is two, which is one plus one.".into(),
                },
                CommandItem {
                    command: "3".into(),
                    description: "This is three, which is one plus two.\nThat is to say one plus one plus one."
                        .into(),
                },
            ]
            .into(),
        )
        .await
        .unwrap();
        assert_eq!(
            action,
            Some(Action {
                kind: ActionKind::Copy,
                command: "3".to_string()
            })
        );
    }
}

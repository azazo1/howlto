use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
};

use clipboard_rs::Clipboard;
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};
use tracing::{debug, info};

use crate::{
    agent::answer::{AnswerAgent, AnswerAgentResponse, ModifyOption},
    agent::submit_commands::CommandItem,
    config::{AppConfig, profile::Profiles},
    error::{Error, Result},
    shell::Shell,
    tui::{command_helper::select::ActionKind, markdown},
};

mod modify;
mod select;

const MINIMUM_TUI_WIDTH: usize = 45;

fn print_command_candidates(commands: &[CommandItem]) -> std::io::Result<()> {
    let mut stderr = std::io::stderr().lock();
    write_command_candidates(&mut stderr, commands)?;
    stderr.flush()
}

fn write_command_candidates(
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

fn write_plain_commands(writer: &mut impl Write, commands: &[CommandItem]) -> std::io::Result<()> {
    for command in commands {
        write!(writer, "{}", command.command)?;
        if !command.command.ends_with('\n') {
            writeln!(writer)?;
        }
    }
    Ok(())
}

fn print_plain_commands(commands: &[CommandItem]) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    write_plain_commands(&mut stdout, commands)?;
    stdout.flush()
}

fn detect_os() -> String {
    sysinfo::System::name().unwrap_or(std::env::consts::OS.to_string())
}

async fn execute(command: String, shell_path: impl AsRef<Path>) -> Result<()> {
    let mut child = tokio::process::Command::new(shell_path.as_ref())
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null()) // 本来想着把标准输入流传进去, 但是这样就和 agent 从标准输入中附加内容冲突了.
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

fn copy(text: String) -> Result<()> {
    // 目前无法使用 crossterm 0.29 的 clipboard 功能, 因为 ratatui 的依赖冲突, 我不想再添加一个 crossterm 依赖.
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
    agent: &AnswerAgent,
    prev_resp: &mut AnswerAgentResponse,
    command: String,
) -> Result<bool> {
    let prompt = modify::App::prompt(command.clone()).await?;
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

async fn print_to_input_buffer(
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
) -> Result<()> {
    let agent = AnswerAgent::builder()
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
            print_plain_commands(&response.commands)?;
            break;
        }

        if !response.final_text.trim().is_empty() {
            if plain {
                println!("{}", markdown::to_plain_text(&response.final_text));
            } else {
                markdown::print_ansi(&response.final_text);
                std::io::stdout().flush()?;
            }
        }
        if response.commands.is_empty() {
            break;
        }

        print_command_candidates(&response.commands)?;
        let action = select::App::select(response.commands.clone()).await?;
        let Some(action) = action else {
            break;
        };
        debug!("Select action: {action:?}");
        match action.kind {
            ActionKind::Copy => {
                copy(action.command)?;
                break;
            }
            ActionKind::Execute => {
                execute(action.command, shell.path()).await?;
                break;
            }
            ActionKind::PrintToInputBuffer => {
                print_to_input_buffer(&htcmd_file, &action.command).await?;
                break;
            }
            ActionKind::Modify => {
                if !modify(&agent, &mut response, action.command).await? {
                    break;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::{
        agent::submit_commands::CommandItem,
        tui::command_helper::select::{Action, ActionKind},
    };

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

        super::write_command_candidates(&mut output, &commands).unwrap();
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
        super::write_plain_commands(&mut output, &commands).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "printf one\nprintf two\nprintf three\n"
        );
    }

    #[tokio::test]
    #[ignore = "需要真实 TTY 交互 (手动选择), 用 `cargo test select_app_print_to_input_buffer -- --ignored --nocapture` 运行"]
    async fn select_app_print_to_input_buffer() {
        println!("Manually select 3 with Copy action:");
        let action = super::select::App::select(
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

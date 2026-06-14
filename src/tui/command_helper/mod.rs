use std::{
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
    agent::tools::AnswerBody,
    config::{AppConfig, profile::Profiles},
    error::{Error, Result},
    shell::Shell,
    tui::{command_helper::select::ActionKind, markdown},
};

mod modify;
mod select;

const MINIMUM_TUI_WIDTH: usize = 45;

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
    let response = agent
        .resolve()
        .prompt(prompt.to_string())
        .maybe_attached(attached)
        .call()
        .await?;
    match &response.answer {
        AnswerBody::Text { content } => {
            // 文本模式: 直接打印到终端, 不经过 select.
            if plain {
                // plain/管道模式: 去掉 markdown 标记的纯文本, 不污染下游.
                println!("{}", markdown::to_plain_text(content));
            } else {
                // 交互模式: 打印带颜色高亮的 markdown 文本.
                markdown::print_ansi(content);
            }
        }
        AnswerBody::Commands { commands } if commands.is_empty() => {
            // 空命令列表: 无可选项.
            tracing::warn!("Empty commands answer.");
        }
        AnswerBody::Commands { .. } => {
            // 命令模式 (非空): 进选择框.
            let mut response = response;
            while let AnswerBody::Commands { commands } = &response.answer {
                if commands.is_empty() {
                    break;
                }
                let action = select::App::select(commands.clone()).await?;
                let mut should_exit = true;
                if let Some(action) = &action {
                    debug!("Select action: {action:?}");
                    match action.kind {
                        ActionKind::Copy => copy(action.command.clone())?,
                        ActionKind::Execute => {
                            execute(action.command.clone(), shell.path()).await?
                        }
                        ActionKind::PrintToInputBuffer => {
                            print_to_input_buffer(&htcmd_file, &action.command).await?
                        }
                        ActionKind::Modify => {
                            should_exit =
                                !modify(&agent, &mut response, action.command.clone()).await?;
                            should_exit |= matches!(
                                &response.answer,
                                AnswerBody::Commands { commands } if commands.is_empty()
                            );
                        }
                    }
                }
                if should_exit {
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
        agent::tools::CommandItem,
        tui::command_helper::select::{Action, ActionKind},
    };

    #[tokio::test]
    #[ignore = "需要真实 TTY 交互 (手动选择), 用 `cargo test select_app_print_to_input_buffer -- --ignored --nocapture` 运行"]
    async fn select_app_print_to_input_buffer() {
        println!("Manually select 3 with Copy action:");
        let action = super::select::App::select(
            [
                CommandItem {
                    content: "1".into(),
                    desc: "This is one.".into(),
                },
                CommandItem {
                    content: "2".into(),
                    desc: "This is two, which is one plus one.".into(),
                },
                CommandItem {
                    content: "3".into(),
                    desc: "This is three, which is one plus two.\nThat is to say one plus one plus one."
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

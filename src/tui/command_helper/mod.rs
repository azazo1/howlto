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
    agent::tools::{AnswerItem, AnswerKind},
    config::{AppConfig, profile::Profiles},
    error::{Error, Result},
    shell::Shell,
    tui::command_helper::select::ActionKind,
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

/// 判断回答是否为纯文本回答 (所有项都是 Text).
/// 按互斥语义, 纯文本回答预期只有一项, 且无需选择直接输出.
fn is_text_only_answer(answers: &[AnswerItem]) -> bool {
    !answers.is_empty() && answers.iter().all(|x| matches!(x.kind, AnswerKind::Text))
}

/// 在交互模式下直接展示文本回答 (markdown 渲染成带颜色的 ANSI 文本), 不弹选择框.
fn show_text_answer(answers: &[AnswerItem]) {
    for item in answers {
        let text = crate::tui::markdown::render(&item.content);
        for line in &text.lines {
            let rendered: String = line.spans.iter().map(span_to_ansi).collect();
            println!("{rendered}");
        }
    }
}

/// 把 ratatui [`Span`](ratatui::text::Span) 渲染成带 ANSI 样式的字符串 (供 stdout 直接打印).
fn span_to_ansi(span: &ratatui::text::Span<'_>) -> String {
    use ratatui::style::{Color, Modifier};
    let style = span.style;
    let mut codes: Vec<String> = Vec::new();
    if let Some(fg) = style.fg {
        codes.push(match fg {
            Color::Black => "30".into(),
            Color::Red => "31".into(),
            Color::Green => "32".into(),
            Color::Yellow => "33".into(),
            Color::Blue => "34".into(),
            Color::Magenta => "35".into(),
            Color::Cyan => "36".into(),
            Color::Gray | Color::DarkGray => "90".into(),
            Color::LightRed => "91".into(),
            Color::LightGreen => "92".into(),
            Color::LightYellow => "93".into(),
            Color::LightBlue => "94".into(),
            Color::LightMagenta => "95".into(),
            Color::LightCyan => "96".into(),
            Color::White => "97".into(),
            _ => "0".into(),
        });
    }
    let add = style.add_modifier;
    if add.contains(Modifier::BOLD) {
        codes.push("1".into());
    }
    if add.contains(Modifier::ITALIC) {
        codes.push("3".into());
    }
    if add.contains(Modifier::CROSSED_OUT) {
        codes.push("9".into());
    }
    let prefix = if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    };
    format!("{prefix}{}\x1b[0m", span.content)
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
    if is_text_only_answer(&response.answers) {
        // 纯文本回答: 无需选择, 直接输出 (按互斥语义预期只有一项).
        if plain {
            // plain/管道模式: 去掉 markdown 标记的纯文本, 不污染下游.
            let out: Vec<String> = response
                .answers
                .iter()
                .map(|x| crate::tui::markdown::to_plain_text(&x.content))
                .collect();
            println!("{}", out.join("\n"));
        } else {
            // 交互模式: 打印带颜色高亮的 markdown 文本.
            show_text_answer(&response.answers);
        }
    } else if !response.answers.is_empty() {
        // 命令回答 (按互斥语义不应混合 Text): 进选择框.
        let mut response = response;
        loop {
            let action = select::App::select(response.answers.clone()).await?;
            let mut should_exit = true;
            if let Some(action) = &action {
                debug!("Select action: {action:?}");
                match action.kind {
                    ActionKind::Copy => copy(action.command.clone())?,
                    ActionKind::Execute => execute(action.command.clone(), shell.path()).await?,
                    ActionKind::PrintToInputBuffer => {
                        print_to_input_buffer(&htcmd_file, &action.command).await?
                    }
                    ActionKind::Modify => {
                        should_exit =
                            !modify(&agent, &mut response, action.command.clone()).await?;
                        should_exit |= response.answers.is_empty();
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

#[cfg(test)]
mod test {
    use crate::{
        agent::tools::{AnswerItem, AnswerKind},
        tui::command_helper::select::{Action, ActionKind},
    };

    #[tokio::test]
    #[ignore = "需要真实 TTY 交互 (手动选择), 用 `cargo test select_app_print_to_input_buffer -- --ignored --nocapture` 运行"]
    async fn select_app_print_to_input_buffer() {
        println!("Manually select 3 with Copy action:");
        let action = super::select::App::select(
            [
                AnswerItem {
                    content: "1".into(),
                    desc: "This is one.".into(),
                    kind: AnswerKind::Command,
                },
                AnswerItem {
                    content: "2".into(),
                    desc: "This is two, which is one plus one.".into(),
                    kind: AnswerKind::Command,
                },
                AnswerItem {
                    content: "3".into(),
                    desc: "This is three, which is one plus two.\nThat is to say one plus one plus one."
                        .into(),
                    kind: AnswerKind::Command,
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

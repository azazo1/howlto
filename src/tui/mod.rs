use std::path::PathBuf;

use crate::config::{AppConfig, profile::Profiles};
use crate::error::Result;
use crate::session::SessionStore;
use crate::shell::Shell;

pub mod chat;
pub mod cmd;
pub(crate) mod dangerous_execution;
pub(crate) mod editor;
pub(crate) mod logs;
pub mod markdown;
pub(crate) mod terminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectMode {
    Chat,
    Cmd,
}

#[derive(Debug, Clone)]
pub enum PromptMode {
    Cmd(String),
    Chat {
        prompt: Option<String>,
        resume: Option<String>,
    },
}

pub fn detect_mode(prompt: &str) -> DetectMode {
    let trimmed = prompt.trim();
    let lower = trimmed.to_ascii_lowercase();
    let explain_prefixes = [
        "why ",
        "what is ",
        "explain ",
        "compare ",
        "how does ",
        "解释",
        "分析",
        "为什么",
        "是什么",
        "怎么理解",
    ];
    if explain_prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix) || trimmed.starts_with(prefix))
    {
        return DetectMode::Chat;
    }

    let cmd_markers = [
        "|", ">", "<", "&&", "||", "grep", "find", "git", "cargo", "bun",
    ];
    if cmd_markers
        .iter()
        .any(|marker| lower.contains(marker) || trimmed.contains(marker))
    {
        return DetectMode::Cmd;
    }

    DetectMode::Cmd
}

#[cfg(test)]
mod tests {
    use super::{DetectMode, detect_mode};

    #[test]
    fn detect_mode_prefers_chat_for_explanations() {
        assert_eq!(detect_mode("why does git rebase fail"), DetectMode::Chat);
        assert_eq!(detect_mode("解释一下 sed 的 -E"), DetectMode::Chat);
    }

    #[test]
    fn detect_mode_defaults_to_cmd_for_shell_tasks() {
        assert_eq!(detect_mode("git status"), DetectMode::Cmd);
        assert_eq!(detect_mode("find . -name Cargo.toml"), DetectMode::Cmd);
        assert_eq!(detect_mode("show disk usage"), DetectMode::Cmd);
    }
}

#[bon::builder]
pub async fn run(
    mode: PromptMode,
    config: AppConfig,
    shell: &Shell,
    profiles: Profiles,
    plain: bool,
    attached: Option<String>,
    htcmd_file: Option<PathBuf>,
    session_store: SessionStore,
) -> Result<()> {
    match mode {
        PromptMode::Cmd(prompt) => {
            cmd::run()
                .prompt(&prompt)
                .plain(plain)
                .config(config)
                .shell(shell)
                .maybe_attached(attached)
                .profiles(profiles)
                .maybe_htcmd_file(htcmd_file)
                .call()
                .await
        }
        PromptMode::Chat { prompt, resume } => {
            chat::run()
                .maybe_prompt(prompt)
                .maybe_resume_id(resume)
                .config(config)
                .shell(shell)
                .profiles(profiles)
                .maybe_attached(attached)
                .session_store(session_store)
                .call()
                .await
        }
    }
}

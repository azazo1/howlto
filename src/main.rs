use std::io;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use crossterm::tty::IsTty;
use howlto::config::AppConfigLoader;
use howlto::config::CONFIG_TOML_FILE;
use howlto::config::DEFAULT_CONFIG_DIR;
use howlto::logging;
use howlto::session::SessionStore;
use howlto::shell::Shell;
use howlto::tui::{self, DetectMode, PromptMode};
use tokio::io::AsyncReadExt;

#[derive(clap::Parser)]
#[clap(about = "一个能帮你找到心仪命令的 CLI 工具.", long_about = None, version, author)]
struct AppArgs {
    #[clap(subcommand)]
    command: Option<ModeCommand>,
    #[clap(num_args = 0..)]
    prompt: Vec<String>,
    #[clap(short, long, help = "配置文件所在的目录", default_value = DEFAULT_CONFIG_DIR)]
    config: PathBuf,
    #[clap(short, long, help = "直接输出所有候选命令, 无需交互选择.")]
    plain: bool,
    #[clap(short, long, help = "不在标准错误流输出进度信息.")]
    quiet: bool,
    #[clap(long, help = "输出额外的调试信息, 比如工具调用的结果")]
    debug: bool,
    #[clap(long, help = "输出 shell 集成初始化脚本")]
    init: bool,
    #[clap(long, help = "[Shell 集成参数]")]
    htcmd_file: Option<PathBuf>,
    #[clap(long, help = "恢复 chat 会话 id")]
    resume: Option<String>,
}

#[derive(Debug, Clone, Subcommand)]
enum ModeCommand {
    Cmd {
        #[clap(num_args = 1..)]
        prompt: Vec<String>,
    },
    Chat {
        #[clap(num_args = 0..)]
        prompt: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let AppArgs {
        command,
        prompt,
        config: config_dir,
        plain,
        quiet,
        init,
        htcmd_file,
        debug,
        resume,
    } = AppArgs::parse();

    let shell = Shell::detect_shell();
    let config_dir_str = config_dir
        .to_str()
        .ok_or(io::Error::new(
            io::ErrorKind::InvalidFilename,
            "Invalid filename",
        ))
        .with_context(|| format!("无效的文件名: {config_dir:?}"))?;
    let config_dir = PathBuf::from(shellexpand::tilde(config_dir_str).to_string());
    let config_loader = AppConfigLoader::new(&config_dir)
        .await
        .with_context(|| format!("无法创建配置目录: {}", config_dir.display()))?;
    let config = config_loader
        .load_or_create_config()
        .await
        .with_context(|| format!("无法加载配置: {}", config_dir.display()))?;
    let profiles = config_loader
        .load_or_create_profiles()
        .await
        .with_context(|| format!("无法加载 Profiles: {}", config_dir.display()))?;
    let _guard = logging::init(&config_dir, !quiet, debug)
        .await
        .with_context(|| format!("无法初始化日志: {}", config_dir.display()))?;

    if init {
        println!(
            "{}",
            shell.init().ok_or(anyhow::anyhow!(
                "为 Shell {} 的集成脚本未实现",
                shell.name()
            ))??
        );
        return Ok(());
    }

    if config.llm.base_url.is_empty() {
        Err(anyhow::anyhow!(
            "LLM Base Url 为空, 请检查配置信息是否填写正确: {}",
            config_dir.join(CONFIG_TOML_FILE).display()
        ))?
    }

    let mut stdin = tokio::io::stdin();
    let attached = if !stdin.is_tty() {
        let mut s = String::new();
        stdin.read_to_string(&mut s).await?;
        Some(s)
    } else {
        None
    };

    let session_store = SessionStore::new(config_loader.ensure_sessions_dir().await?);
    let routed = match command {
        Some(ModeCommand::Cmd { prompt }) => PromptMode::Cmd(prompt.join(" ")),
        Some(ModeCommand::Chat { prompt }) => PromptMode::Chat {
            prompt: if prompt.is_empty() {
                None
            } else {
                Some(prompt.join(" "))
            },
            resume,
        },
        None if prompt.is_empty() => PromptMode::Chat {
            prompt: None,
            resume,
        },
        None => match tui::detect_mode(&prompt.join(" ")) {
            DetectMode::Chat => PromptMode::Chat {
                prompt: Some(prompt.join(" ")),
                resume,
            },
            DetectMode::Cmd => PromptMode::Cmd(prompt.join(" ")),
        },
    };

    if plain && !matches!(routed, PromptMode::Cmd(_)) {
        Err(anyhow::anyhow!("--plain 仅支持 cmd 模式"))?
    }

    tui::run()
        .mode(routed)
        .config(config)
        .shell(&shell)
        .profiles(profiles)
        .plain(plain)
        .maybe_htcmd_file(htcmd_file)
        .maybe_attached(attached)
        .session_store(session_store)
        .call()
        .await?;

    Ok(())
}

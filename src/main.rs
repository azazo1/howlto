use std::io;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use crossterm::tty::IsTty;
use howlto::config::AppConfigLoader;
use howlto::config::CONFIG_TOML_FILE;
use howlto::config::DEFAULT_CONFIG_DIR;
use howlto::logging;
use howlto::shell::Shell;
use howlto::tui;
use tokio::io::AsyncReadExt;

#[derive(clap::Parser)]
#[clap(about = "一个能帮你找到心仪命令的 CLI 工具.", long_about=None, version, author)]
struct AppArgs {
    /// 命令生成提示词, 当其为空的时候, 进入交互模式.
    #[clap(num_args=0..)]
    prompt: Vec<String>,
    #[clap(short, long, help = "配置文件所在的目录", default_value = DEFAULT_CONFIG_DIR)]
    config: PathBuf,
    #[clap(
        short,
        long,
        help = "直接输出所有候选命令, 无需交互选择.",
        default_value_t = false
    )]
    plain: bool,
    #[clap(
        short,
        long,
        help = "不在标准错误流输出进度信息.",
        default_value_t = false
    )]
    quiet: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let AppArgs {
        prompt,
        config: config_dir,
        plain,
        quiet,
    } = AppArgs::parse();
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
        .with_context(|| format!("无法创建配置目录: {config_dir:?}"))?;
    let config = config_loader
        .load_or_create_config()
        .await
        .with_context(|| format!("无法加载配置: {config_dir:?}"))?;
    let profiles = config_loader
        .load_or_create_profiles()
        .await
        .with_context(|| format!("无法加载 Profiles: {config_dir:?}"))?;

    let _guard = logging::init(&config_dir, !quiet)
        .await
        .with_context(|| format!("无法初始化日志: {config_dir:?}"))?;

    // 提前检查
    if config.llm.base_url.is_empty() {
        Err(anyhow::anyhow!(
            "LLM Base Url 为空, 请检查配置信息是否填写正确: {:?}.",
            config_dir.join(CONFIG_TOML_FILE)
        ))?
    }

    if prompt.is_empty() {
        todo!("实现交互功能 tui::chatter")
    } else {
        let prompt: String = prompt.join(" ");
        let shell = Shell::detect_shell();
        // attach stdin
        let mut stdin = tokio::io::stdin();
        let attached = if !stdin.is_tty() {
            let mut s = String::new();
            stdin.read_to_string(&mut s).await?;
            Some(s)
        } else {
            None
        };

        tui::command_helper::run()
            .config(config)
            .prompt(&prompt)
            .shell(&shell)
            .profiles(profiles)
            .plain(plain)
            .maybe_attached(attached)
            .call()
            .await?;
    }
    Ok(())
}

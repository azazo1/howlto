use std::io;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use crossterm::tty::IsTty;
use howlto::config::AppConfigLoader;
use howlto::config::CONFIG_TOML_FILE;
use howlto::config::DEFAULT_CONFIG_DIR;
use howlto::config::DEFAULT_OPENAI_BASE_URL;
use howlto::logging;
use howlto::shell::Shell;
use howlto::tui;
use tokio::io::AsyncReadExt;

#[derive(clap::Parser)]
#[clap(about = "一个能帮你找到心仪命令的 CLI 工具.", long_about=None, version = env!("HOWLTO_VERSION"), author)]
struct AppArgs {
    /// 命令生成提示词, 当其为空的时候, 进入交互模式.
    #[clap(num_args=0..)]
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
    #[clap(long, help = "创建缺失的默认 config.toml 和 profiles.toml, 不覆盖已有文件.")]
    init_config: bool,
    #[clap(long, help = "[Shell 集成参数]")]
    htcmd_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let AppArgs {
        prompt,
        config: config_dir,
        plain,
        quiet,
        init,
        init_config,
        htcmd_file,
        debug,
    } = AppArgs::parse();

    let shell = Shell::detect_shell();

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

    let config_dir_str = config_dir
        .to_str()
        .ok_or(io::Error::new(
            io::ErrorKind::InvalidFilename,
            "Invalid filename",
        ))
        .with_context(|| format!("无效的文件名: {config_dir:?}"))?;
    let config_dir = PathBuf::from(shellexpand::tilde(config_dir_str).to_string());

    let config_loader = AppConfigLoader::new(&config_dir);
    if init_config {
        let created = config_loader
            .create_default_files()
            .await
            .with_context(|| format!("无法创建配置文件: {}", config_dir.display()))?;
        if created.is_empty() {
            println!("配置文件已存在, 未覆盖: {}", config_dir.display());
        } else {
            for path in created {
                println!("已创建: {}", path.display());
            }
        }
        return Ok(());
    }

    let config = config_loader
        .load_config()
        .await
        .with_context(|| format!("无法加载配置: {}", config_dir.display()))?;
    let profiles = config_loader
        .load_profiles()
        .await
        .with_context(|| format!("无法加载 Profiles: {}", config_dir.display()))?;

    // 提前检查
    if config.llm.base_url.is_empty() {
        Err(anyhow::anyhow!(
            "LLM Base URL 为空. 请设置 HOWLTO_BASE_URL 或 OPENAI_BASE_URL, 或运行 `howlto --init-config` 后编辑: {}",
            config_dir.join(CONFIG_TOML_FILE).display()
        ))?
    }
    if config.llm.api_key.is_empty()
        && config.llm.base_url.trim_end_matches('/') == DEFAULT_OPENAI_BASE_URL
    {
        Err(anyhow::anyhow!(
            "LLM API key 为空. 请设置 HOWLTO_API_KEY 或 OPENAI_API_KEY, 或运行 `howlto --init-config` 后编辑: {}",
            config_dir.join(CONFIG_TOML_FILE).display()
        ))?
    }

    let _guard = logging::init(&config_dir, !quiet, debug)
        .await
        .with_context(|| format!("无法初始化日志: {}", config_dir.display()))?;

    if prompt.is_empty() {
        todo!("实现交互功能 tui::chatter")
    } else {
        let prompt: String = prompt.join(" ");
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
            .maybe_htcmd_file(htcmd_file)
            .shell(&shell)
            .profiles(profiles)
            .plain(plain)
            .maybe_attached(attached)
            .call()
            .await?;
    }
    Ok(())
}

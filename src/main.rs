use std::path::PathBuf;

use clap::Parser;
use howlto::agent::shell_command_gen::ShellCommandGenAgent;
use howlto::config::AppConfigLoader;
use howlto::config::DEFAULT_CONFIG_DIR;
use howlto::config::profile::profiles::SHELL_COMMAND_GEN_PROFILE;
use howlto::detect_shell;
use howlto::error::Error;
use howlto::logging;
use howlto::tui::select::CommandSelectApp;

const ABOUT: &str = "一个能帮你找到心仪命令的 CLI 工具.";

#[derive(clap::Parser)]
#[clap(about = ABOUT, long_about=None, version, author)]
struct AppArgs {
    /// 命令生成提示词, 当其为空的时候, 进入交互模式.
    #[clap(num_args=0..)]
    prompt: Vec<String>,
    #[clap(short, long, help = "配置文件所在的目录", default_value = DEFAULT_CONFIG_DIR)]
    config: PathBuf,
    #[clap(
        short,
        long,
        help = "直接输出所有候选命令, 无需交互输出.",
        default_value_t = false
    )]
    plain: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let AppArgs {
        prompt,
        config: config_dir,
        plain,
    } = AppArgs::parse();

    let _guard = logging::init(&config_dir).await?;

    let config_loader = AppConfigLoader::new(config_dir).await?;
    let config = config_loader.load_or_create_config().await?;
    let profiles = config_loader.load_or_create_profiles().await?;

    // 提前检查
    if config.llm.llm_base_url.is_empty() {
        Err(anyhow::anyhow!("LLM base url is empty."))?
    }

    if prompt.is_empty() {
        todo!("实现交互功能")
    } else {
        let prompt: String = prompt.join(" ");
        let (shell_name, _shell_path) = detect_shell();
        let response = ShellCommandGenAgent::builder()
            .profile(
                profiles
                    .get(SHELL_COMMAND_GEN_PROFILE)
                    .ok_or(Error::profile_not_found(SHELL_COMMAND_GEN_PROFILE))?
                    .clone(),
            )
            .os(std::env::consts::OS.to_string())
            .shell(shell_name)
            .config(config)
            .build()?
            .resolve(prompt)
            .await?;
        if plain {
            println!("{}", response.commands.join("\n"));
        } else if !response.commands.is_empty() {
            let action = CommandSelectApp::select(response.commands.clone()).await?;
            if let Some(action) = action {
                println!("selected: {action:?}");
                // todo 响应 action
            }
        }
    }
    Ok(())
}

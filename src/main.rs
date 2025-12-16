use std::path::PathBuf;
use std::process::Stdio;

use clap::Parser;
use clipboard_rs::Clipboard;
use howlto::agent::shell_command_gen::ShellCommandGenAgent;
use howlto::config::AppConfigLoader;
use howlto::config::DEFAULT_CONFIG_DIR;
use howlto::config::profile::profiles::SHELL_COMMAND_GEN_PROFILE;
use howlto::detect_shell;
use howlto::error::Error;
use howlto::logging;
use howlto::tui::command_select::ActionKind;
use howlto::tui::command_select::App;

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

    let _guard = logging::init(&config_dir, !quiet).await?;

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
        // todo 移动到专门的模块, 和 shell_command_gen.rs select.rs 结合起来.
        let prompt: String = prompt.join(" ");
        let (shell_name, shell_path) = detect_shell();
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
            let action = App::select(response.commands.clone()).await?;
            if let Some(action) = action {
                match action.kind {
                    ActionKind::Copy => {
                        let cx = clipboard_rs::ClipboardContext::new()
                            .map_err(|_| anyhow::anyhow!("Failed to access clipboard."))?;
                        cx.set_text(action.command)
                            .map_err(|_| anyhow::anyhow!("Failed to copy."))?;
                    }
                    ActionKind::Execute => {
                        let mut child = tokio::process::Command::new(shell_path)
                            .arg("-c")
                            .arg(action.command)
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .stdin(Stdio::piped())
                            .spawn()?;
                        let mut child_stdin = child
                            .stdin
                            .take()
                            .ok_or(anyhow::anyhow!("cannot take child stdin"))?;
                        let mut child_stdout = child
                            .stdout
                            .take()
                            .ok_or(anyhow::anyhow!("cannot take child stdout"))?;
                        let mut child_stderr = child
                            .stderr
                            .take()
                            .ok_or(anyhow::anyhow!("cannot take child stderr"))?;
                        let stdin_handle = tokio::spawn(async move {
                            tokio::io::copy(&mut tokio::io::stdin(), &mut child_stdin).await
                        });
                        let stdout_handle = tokio::spawn(async move {
                            tokio::io::copy(&mut child_stdout, &mut tokio::io::stdout()).await
                        });
                        let stderr_handle = tokio::spawn(async move {
                            tokio::io::copy(&mut child_stderr, &mut tokio::io::stderr()).await
                        });
                        let (rin, rout, rerr) =
                            tokio::try_join!(stdin_handle, stdout_handle, stderr_handle)?;
                        rin?;
                        rout?;
                        rerr?;
                    }
                    ActionKind::Modify => todo!("实现修改逻辑"),
                    ActionKind::Print => {
                        println!("{}", action.command);
                    }
                }
            }
        }
    }
    Ok(())
}

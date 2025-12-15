use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::Parser;
use howlto::agents::shell_command_gen::ShellCommandGenAgent;
use howlto::config::{AppConfig, CONFIG_TOML_FILE, PROFILES_TOML_FILE};
use howlto::error::{Error, Result};
use howlto::profile::Profiles;
use howlto::profile::profiles::SHELL_COMMAND_GEN_PROFILE;
use howlto::{config::DEFAULT_CONFIG_DIR, profile::Profile};
use tokio::{fs, io};
use tracing::Level;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

#[derive(clap::Parser)]
#[clap(about, long_about=None, version, author)]
struct AppArgs {
    /// 提示词, 当其为空的时候, 进入交互模式.
    #[clap(num_args=0..)]
    prompt: Vec<String>,
    #[clap(short, long, help = "配置文件所在的目录", default_value = DEFAULT_CONFIG_DIR)]
    config: PathBuf,
}

struct AppConfigLoader {
    config_dir: PathBuf,
}

impl AppConfigLoader {
    async fn new(config_dir: impl AsRef<Path>) -> Result<Self> {
        // 创建配置文件目录, 并返回 expand 之后的路径.
        let config_dir_str = config_dir.as_ref().to_str().ok_or(io::Error::new(
            io::ErrorKind::InvalidFilename,
            "无效的文件名",
        ))?;
        let config_dir = PathBuf::from(shellexpand::tilde(config_dir_str).to_string());
        fs::create_dir_all(&config_dir).await?;
        Ok(Self { config_dir })
    }

    async fn load_or_create_config(&self) -> Result<AppConfig> {
        let config_file_path = self.config_dir.join(CONFIG_TOML_FILE);
        if !config_file_path.is_file() {
            let config = AppConfig::default();
            let content = toml::to_string_pretty(&config)?;
            fs::write(config_file_path, content).await?;
            Ok(config)
        } else {
            let config: AppConfig =
                toml::from_str(&fs::read_to_string(self.config_dir.join(CONFIG_TOML_FILE)).await?)?;
            Ok(config)
        }
    }

    async fn create_default_profiles(&self) -> Result<Profiles> {
        let default_profiles = Profiles::default();
        let content = toml::to_string_pretty(&default_profiles)?;
        fs::write(self.config_dir.join(PROFILES_TOML_FILE), content).await?;
        Ok(default_profiles)
    }

    async fn load_or_create_profiles(&self) -> Result<HashMap<String, Profile>> {
        let profile_path = self.config_dir.join(PROFILES_TOML_FILE);
        let profiles: HashMap<String, Profile> = if !profile_path.is_file() {
            self.create_default_profiles().await?
        } else {
            let content = fs::read_to_string(profile_path).await?;
            toml::from_str(&content)?
        }
        .profiles
        .into_iter()
        .map(|x| (x.name.clone(), x))
        .collect();
        Ok(profiles)
    }
}

/// 获取当前 shell 的字符串表示.
fn detect_shell() -> String {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System, get_current_pid};
    macro_rules! fall_back_to_unknown {
        ($e:expr) => {{
            let Some(x) = $e else {
                return "Unknown".into();
            };
            x
        }};
    }
    let pid = fall_back_to_unknown!(get_current_pid().ok());
    let system = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    let cur_proc = fall_back_to_unknown!(system.process(pid));
    let parent_pid = fall_back_to_unknown!(cur_proc.parent());
    let parent = fall_back_to_unknown!(system.process(parent_pid));
    parent.name().to_string_lossy().into()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let AppArgs {
        prompt,
        config: config_dir,
    } = AppArgs::parse();

    // 输出日志
    let logs_dir = config_dir.join("logs");
    if !logs_dir.is_dir() {
        fs::create_dir(logs_dir).await?;
    }
    let file_appender =
        RollingFileAppender::new(Rotation::DAILY, config_dir.join("logs"), "howlto.log");
    let (logging_appender, _guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(logging_appender)
        .with_ansi(false);
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(filter_fn(|md| {
            if let Some(module) = md.module_path()
                && module.starts_with("rig::")
            {
                return false;
            }
            // 注意这里的比较是和底层表示数字反过来的.
            // 下面的比较忽略 TRACE, DEBUG.
            if *md.level() > Level::INFO {
                return false;
            }
            true
        }));
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));
    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .with(env_filter)
        .init();

    let config_loader = AppConfigLoader::new(config_dir).await?;
    let config = config_loader.load_or_create_config().await?;
    let profiles = config_loader.load_or_create_profiles().await?;
    if prompt.is_empty() {
        todo!("interact mode")
    } else {
        let prompt: String = prompt.join(" ");
        println!(
            "{}",
            ShellCommandGenAgent::builder()
                .profile(
                    profiles
                        .get(SHELL_COMMAND_GEN_PROFILE)
                        .ok_or(Error::profile_not_found(SHELL_COMMAND_GEN_PROFILE))?
                        .clone(),
                )
                .os(std::env::consts::OS.to_string())
                .shell(detect_shell())
                .config(config)
                .build()?
                .resolve(prompt)
                .await?
                .command
        );
    }
    Ok(())
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::Parser;
use howlto::config::{AppConfig, CONFIG_TOML_FILE, PROFILES_TOML_FILE};
use howlto::error::Result;
use howlto::profile::Profiles;
use howlto::{config::DEFAULT_CONFIG_DIR, profile::Profile};
use tokio::{fs, io};

#[derive(clap::Parser)]
#[clap(about, long_about=None, version, author)]
struct AppArgs {
    #[clap(num_args=1..)]
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let AppArgs {
        prompt,
        config: config_dir,
    } = AppArgs::parse();

    let config_loader = AppConfigLoader::new(config_dir).await?;
    let config = config_loader.load_or_create_config().await?;
    let profiles = config_loader.load_or_create_profiles().await?;
    dbg!((config, profiles));
    Ok(())
}

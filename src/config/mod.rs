use std::path::{Path, PathBuf};

use crate::{config::profile::Profiles, error::Result};
use serde::{Deserialize, Serialize};
use tokio::fs;

pub mod profile;

#[cfg(windows)]
pub const DEFAULT_CONFIG_DIR: &str = "~\\.config\\howlto\\";
#[cfg(unix)]
pub const DEFAULT_CONFIG_DIR: &str = "~/.config/howlto/";
#[cfg(all(not(unix), not(windows)))]
compile_error!("OS not supported.");

pub const PROFILES_TOML_FILE: &str = "profiles.toml";
pub const CONFIG_TOML_FILE: &str = "config.toml";
pub const SESSIONS_DIR: &str = "sessions";

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct AppConfig {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub agent: AgentConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LlmConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentConfig {
    #[serde(default = "default_use_tool_man")]
    pub use_tool_man: bool,
    #[serde(default = "default_use_tool_help")]
    pub use_tool_help: bool,
    #[serde(default = "default_use_tool_tldr")]
    pub use_tool_tldr: bool,
    #[serde(default = "default_use_tool_thefuck")]
    pub use_tool_thefuck: bool,
    #[serde(default = "default_use_tool_dangerous_help")]
    pub use_tool_dangerous_help: bool,
    #[serde(default = "default_cache")]
    pub cache: bool,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub cmd: CommandConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandConfig {
    #[serde(default = "default_output_n")]
    pub output_n: u32,
    #[serde(default = "default_wait_for_output_scrolling")]
    pub wait_for_output_scrolling: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl Default for CommandConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

fn default_use_tool_thefuck() -> bool {
    true
}

fn default_wait_for_output_scrolling() -> bool {
    false
}

fn default_output_n() -> u32 {
    3
}

fn default_language() -> String {
    "en".into()
}

fn default_use_tool_man() -> bool {
    true
}

fn default_use_tool_help() -> bool {
    true
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_cache() -> bool {
    true
}

fn default_use_tool_tldr() -> bool {
    true
}

fn default_use_tool_dangerous_help() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct AppConfigLoader {
    config_dir: PathBuf,
}

impl AppConfigLoader {
    pub async fn new(config_dir: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(&config_dir).await?;
        Ok(Self {
            config_dir: config_dir.as_ref().into(),
        })
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.config_dir.join(SESSIONS_DIR)
    }

    pub async fn ensure_sessions_dir(&self) -> Result<PathBuf> {
        let sessions_dir = self.sessions_dir();
        fs::create_dir_all(&sessions_dir).await?;
        Ok(sessions_dir)
    }

    pub async fn load_or_create_config(&self) -> Result<AppConfig> {
        let config_file_path = self.config_dir.join(CONFIG_TOML_FILE);
        if !config_file_path.is_file() {
            let config = AppConfig::default();
            let content = toml::to_string_pretty(&config)?;
            fs::write(config_file_path, content).await?;
            Ok(config)
        } else {
            let content = fs::read_to_string(self.config_dir.join(CONFIG_TOML_FILE)).await?;
            Ok(toml::from_str(&content)?)
        }
    }

    pub async fn create_default_profiles(&self) -> Result<Profiles> {
        let default_profiles = Profiles::default();
        let content = toml::to_string_pretty(&default_profiles)?;
        fs::write(self.config_dir.join(PROFILES_TOML_FILE), content).await?;
        Ok(default_profiles)
    }

    pub async fn load_or_create_profiles(&self) -> Result<Profiles> {
        let profile_path = self.config_dir.join(PROFILES_TOML_FILE);
        if !profile_path.is_file() {
            self.create_default_profiles().await
        } else {
            let content = fs::read_to_string(profile_path).await?;
            Ok(toml::from_str(&content)?)
        }
    }
}

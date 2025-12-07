use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIG_DIR: &str = "~/.config/howlto/";
pub const PROFILES_TOML_FILE: &str = "profiles.toml";
pub const CONFIG_TOML_FILE: &str = "config.toml";

#[derive(Deserialize, Serialize, Debug)]
pub struct AppConfig {
    /// LLM api key.
    #[serde(default)]
    pub llm_api_key: String,
    /// LLM 提供商 base url.
    #[serde(default)]
    pub llm_base_url: String,
    #[serde(default = "default_cache")]
    /// 是否使用对话缓存.
    pub cache: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

fn default_config_path() -> PathBuf {
    "~/.config/howlto/".into()
}

fn default_cache() -> bool {
    true
}

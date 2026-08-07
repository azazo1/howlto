use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use crate::{config::profile::Profiles, error::Result};
use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncWriteExt};

pub mod profile;
pub mod migration;

#[cfg(windows)]
pub const DEFAULT_CONFIG_DIR: &str = "~\\.config\\howlto\\";
#[cfg(unix)]
pub const DEFAULT_CONFIG_DIR: &str = "~/.config/howlto/";
#[cfg(all(not(unix), not(windows)))]
compile_error!("OS not supported.");
pub const PROFILES_TOML_FILE: &str = "profiles.toml";
pub const CONFIG_TOML_FILE: &str = "config.toml";
pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Deserialize, Serialize, Debug)]
pub struct AppConfig {
    #[serde(default = "default_config_version")]
    pub version: u32,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub session: SessionConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    /// LLM api key.
    #[serde(default)]
    pub api_key: String,
    /// LLM 提供商 base url.
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// agent 使用的 LLM 模型.
    #[serde(default = "default_model")]
    pub model: String,
    /// LLM 输出 max_tokens
    pub max_tokens: Option<u64>,
    /// LLM 输出 temperature 参数.
    pub temperature: Option<f64>,
    // todo gemini, anthropic api ...
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    /// 是否启用只读命令工具.
    #[serde(default = "default_use_tool_explore")]
    pub use_tool_explore: bool,
    /// 是否启用 elevate 工具,
    /// 用于执行 explore (沙箱只读) 无法完成的命令 (需要写/联网/改变状态),
    /// 每次执行都会向用户询问确认.
    #[serde(default = "default_use_tool_elevate")]
    pub use_tool_elevate: bool,
    /// 滚动进度字符速度限制, 单位是字符/秒, 0 表示不限制.
    #[serde(default)]
    pub scroll_char_speed_limit: usize,
    #[serde(default = "default_cache")]
    /// 是否使用对话缓存. todo 缓存对话
    pub cache: bool,
    /// 模型输出语言.
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub answer: AnswerConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnswerConfig {
    /// Answer agent 输出的命令/回答个数.
    #[serde(default = "default_output_n")]
    pub output_n: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SessionConfig {
    /// 整个 sessions 目录的最大字节数, 0 表示禁用.
    #[serde(default = "default_session_max_bytes")]
    pub max_bytes: u64,
    /// 每个工作目录最多保留的会话数, 0 表示不限.
    #[serde(default)]
    pub max_per_dir: usize,
    /// 会话过期天数, 0 表示不过期.
    #[serde(default)]
    pub ttl_days: u64,
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

impl Default for AnswerConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_bytes: default_session_max_bytes(),
            max_per_dir: 0,
            ttl_days: 0,
        }
    }
}

fn default_config_version() -> u32 {
    migration::CURRENT_CONFIG_VERSION
}

fn default_session_max_bytes() -> u64 {
    64 * 1024 * 1024
}

fn default_output_n() -> u32 {
    3
}

fn default_language() -> String {
    "en".into()
}

fn default_use_tool_explore() -> bool {
    true
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_base_url() -> String {
    DEFAULT_OPENAI_BASE_URL.to_string()
}

fn default_cache() -> bool {
    true
}

fn default_use_tool_elevate() -> bool {
    true
}

fn first_env(get: &impl Fn(&str) -> Option<String>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| get(name).filter(|value| !value.trim().is_empty()))
}

impl AppConfig {
    fn apply_env(&mut self) {
        self.apply_env_with(|name| std::env::var(name).ok());
    }

    fn apply_env_with(&mut self, get: impl Fn(&str) -> Option<String>) {
        let default_base_url = self.llm.base_url.is_empty()
            || self.llm.base_url.trim_end_matches('/') == DEFAULT_OPENAI_BASE_URL;
        let default_model = self.llm.model == default_model();
        let howlto_base_url = first_env(&get, &["HOWLTO_BASE_URL"]);
        let use_openai_fallbacks = howlto_base_url.is_none() && default_base_url;

        if let Some(base_url) = howlto_base_url {
            self.llm.base_url = base_url;
        } else if use_openai_fallbacks
            && let Some(base_url) = first_env(&get, &["OPENAI_BASE_URL"])
        {
            self.llm.base_url = base_url;
        }
        if let Some(api_key) = first_env(&get, &["HOWLTO_API_KEY"]) {
            self.llm.api_key = api_key;
        } else if self.llm.api_key.is_empty()
            && use_openai_fallbacks
            && let Some(api_key) = first_env(&get, &["OPENAI_API_KEY"])
        {
            self.llm.api_key = api_key;
        }
        if let Some(model) = first_env(&get, &["HOWLTO_MODEL"]) {
            self.llm.model = model;
        } else if default_model
            && use_openai_fallbacks
            && let Some(model) = first_env(&get, &["OPENAI_MODEL"])
        {
            self.llm.model = model;
        }
        if let Some(language) = first_env(&get, &["HOWLTO_LANGUAGE"]) {
            self.agent.language = language;
        }
    }
}

pub struct AppConfigLoader {
    config_dir: PathBuf,
}

impl AppConfigLoader {
    pub fn new(config_dir: impl AsRef<Path>) -> Self {
        Self {
            config_dir: config_dir.as_ref().into(),
        }
    }

    pub async fn load_config(&self) -> Result<AppConfig> {
        let config_file_path = self.config_dir.join(CONFIG_TOML_FILE);
        let mut config = if config_file_path.is_file() {
            toml::from_str(&fs::read_to_string(config_file_path).await?)?
        } else {
            AppConfig::default()
        };
        migration::ensure_supported_version(config.version)?;
        config.apply_env();
        Ok(config)
    }

    pub async fn load_profiles(&self) -> Result<Profiles> {
        let profile_path = self.config_dir.join(PROFILES_TOML_FILE);
        if profile_path.is_file() {
            Ok(toml::from_str(&fs::read_to_string(profile_path).await?)?)
        } else {
            Ok(Profiles::default())
        }
    }

    async fn create_default_file(
        &self,
        file_name: &str,
        value: &impl Serialize,
    ) -> Result<Option<PathBuf>> {
        let path = self.config_dir.join(file_name);
        let content = toml::to_string_pretty(value)?;
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await;
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        file.write_all(content.as_bytes()).await?;
        Ok(Some(path))
    }

    pub async fn create_default_files(&self) -> Result<Vec<PathBuf>> {
        fs::create_dir_all(&self.config_dir).await?;
        let mut created = Vec::new();
        if let Some(path) = self
            .create_default_file(CONFIG_TOML_FILE, &AppConfig::default())
            .await?
        {
            created.push(path);
        }
        if let Some(path) = self
            .create_default_file(PROFILES_TOML_FILE, &Profiles::default())
            .await?
        {
            created.push(path);
        }
        Ok(created)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::fs;
    use uuid::Uuid;

    use super::{
        AppConfig, AppConfigLoader, CONFIG_TOML_FILE, DEFAULT_OPENAI_BASE_URL,
        PROFILES_TOML_FILE,
    };

    fn temp_config_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("howlto-config-test-{}", Uuid::new_v4()))
    }

    #[test]
    fn environment_overrides_file_values() {
        let mut config = AppConfig::default();
        let values = HashMap::from([
            ("HOWLTO_API_KEY", "howlto-key"),
            ("OPENAI_API_KEY", "openai-key"),
            ("OPENAI_BASE_URL", "https://example.test/v1"),
            ("HOWLTO_MODEL", "test-model"),
            ("HOWLTO_LANGUAGE", "zh"),
        ]);

        config.apply_env_with(|name| values.get(name).map(ToString::to_string));

        assert_eq!(config.llm.api_key, "howlto-key");
        assert_eq!(config.llm.base_url, "https://example.test/v1");
        assert_eq!(config.llm.model, "test-model");
        assert_eq!(config.agent.language, "zh");
    }

    #[test]
    fn openai_fallbacks_do_not_override_custom_provider_config() {
        let mut config = AppConfig::default();
        config.llm.api_key = "custom-key".to_string();
        config.llm.base_url = "https://provider.example/v1".to_string();
        config.llm.model = "custom-model".to_string();
        let values = HashMap::from([
            ("OPENAI_API_KEY", "openai-key"),
            ("OPENAI_BASE_URL", "https://api.openai.test/v1"),
            ("OPENAI_MODEL", "openai-model"),
        ]);

        config.apply_env_with(|name| values.get(name).map(ToString::to_string));

        assert_eq!(config.llm.api_key, "custom-key");
        assert_eq!(config.llm.base_url, "https://provider.example/v1");
        assert_eq!(config.llm.model, "custom-model");

        let mut config = AppConfig::default();
        let values = HashMap::from([
            ("HOWLTO_BASE_URL", "https://provider.example/v1"),
            ("OPENAI_API_KEY", "openai-key"),
            ("OPENAI_MODEL", "openai-model"),
        ]);
        config.apply_env_with(|name| values.get(name).map(ToString::to_string));

        assert_eq!(config.llm.base_url, "https://provider.example/v1");
        assert!(config.llm.api_key.is_empty());
        assert_eq!(config.llm.model, "gpt-4o-mini");
    }

    #[test]
    fn default_config_has_session_defaults() {
        let config = AppConfig::default();

        assert_eq!(config.version, 1);
        assert_eq!(config.session.max_bytes, 64 * 1024 * 1024);
        assert_eq!(config.session.max_per_dir, 0);
        assert_eq!(config.session.ttl_days, 0);
        assert_eq!(config.agent.scroll_char_speed_limit, 0);
    }

    #[tokio::test]
    async fn legacy_config_without_version_loads_as_v1() {
        let config_dir = temp_config_dir();
        fs::create_dir_all(&config_dir).await.unwrap();
        fs::write(
            config_dir.join(CONFIG_TOML_FILE),
            r#"
[llm]
api_key = "legacy-key"
"#,
        )
        .await
        .unwrap();
        let loader = AppConfigLoader::new(&config_dir);

        let config = loader.load_config().await.unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.session.max_bytes, 64 * 1024 * 1024);
        fs::remove_dir_all(config_dir).await.unwrap();
    }

    #[tokio::test]
    async fn future_config_version_is_rejected() {
        let config_dir = temp_config_dir();
        fs::create_dir_all(&config_dir).await.unwrap();
        fs::write(
            config_dir.join(CONFIG_TOML_FILE),
            r#"
version = 2
"#,
        )
        .await
        .unwrap();
        let loader = AppConfigLoader::new(&config_dir);

        let error = loader.load_config().await.unwrap_err();
        assert!(error.to_string().contains("newer than supported"));
        fs::remove_dir_all(config_dir).await.unwrap();
    }

    #[tokio::test]
    async fn missing_files_use_defaults_without_writing() {
        let config_dir = temp_config_dir();
        let loader = AppConfigLoader::new(&config_dir);

        loader.load_config().await.unwrap();
        loader.load_profiles().await.unwrap();

        assert!(!config_dir.exists());
    }

    #[tokio::test]
    async fn explicit_initialization_creates_missing_files_without_overwriting() {
        let config_dir = temp_config_dir();
        let loader = AppConfigLoader::new(&config_dir);

        let created = loader.create_default_files().await.unwrap();
        assert_eq!(created.len(), 2);
        assert!(config_dir.join(CONFIG_TOML_FILE).is_file());
        assert!(config_dir.join(PROFILES_TOML_FILE).is_file());
        assert_eq!(AppConfig::default().llm.base_url, DEFAULT_OPENAI_BASE_URL);

        assert!(loader.create_default_files().await.unwrap().is_empty());
        fs::remove_dir_all(config_dir).await.unwrap();
    }
}

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
    /// agent 使用的 LLM 模型.
    #[serde(default = "default_model")]
    pub model: String,
    /// LLM 输出 max_tokens
    pub max_tokens: Option<u64>,
    /// LLM 输出 temperature 参数.
    pub temperature: Option<f64>,
    /// 是否使用 man page 帮助工具辅助 agent 生成内容, 在 windows 下调用可能会失败.
    #[serde(default = "default_use_tool_man")]
    pub use_tool_man: bool,
    /// 是否使用 --help 帮助工具辅助 agent 生成内容,
    /// 是否能够执行成功取决与程序是否接受 `--help`参数.
    #[serde(default = "default_use_tool_help")]
    pub use_tool_help: bool,
    #[serde(default = "default_cache")]
    /// 是否使用对话缓存.
    pub cache: bool,
    /// 模型输出语言.
    #[serde(default = "default_language")]
    pub language: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
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
    "gpt-4o".to_string()
}

fn default_cache() -> bool {
    true
}

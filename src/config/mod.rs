use serde::{Deserialize, Serialize};

pub mod profile;

pub const DEFAULT_CONFIG_DIR: &str = "~/.config/howlto/";
pub const PROFILES_TOML_FILE: &str = "profiles.toml";
pub const CONFIG_TOML_FILE: &str = "config.toml";

#[derive(Deserialize, Serialize, Debug)]
pub struct AppConfig {
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub agent: AgentConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmConfig {
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    /// 是否使用 man page 帮助工具辅助 agent 生成内容, 在 windows 下调用可能会失败.
    #[serde(default = "default_use_tool_man")]
    pub use_tool_man: bool,
    /// 是否使用 --help 帮助工具辅助 agent 生成内容,
    /// 是否能够执行成功取决与程序是否接受 `--help`参数.
    #[serde(default = "default_use_tool_help")]
    pub use_tool_help: bool,
    /// 是否使用 tldr 获取帮助信息.
    #[serde(default = "default_use_tool_tldr")]
    pub use_tool_tldr: bool,
    #[serde(default = "default_cache")]
    /// 是否使用对话缓存. todo 缓存对话
    pub cache: bool,
    /// 模型输出语言.
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub shell_command_gen: ShellCommandGenConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShellCommandGenConfig {
    /// Shell Comamnd Gen 输出的命令个数.
    #[serde(default = "default_output_n")]
    pub output_n: u32,
    /// Shell Command Gen 是否等待输出显示完毕,
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

impl Default for ShellCommandGenConfig {
    fn default() -> Self {
        toml::from_str("").unwrap()
    }
}

fn default_wait_for_output_scrolling() -> bool {
    true
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

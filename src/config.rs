use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct AppConfig {
    /// LLM api key.
    llm_api_key: String,
    /// LLM 提供商 base url.
    llm_base_url: String,
}

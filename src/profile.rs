use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub mod template {
    pub const TEXT_LANG: &str = "{{text_lang}}";
    pub const SHELL: &str = "{{shell}}";
    pub const OS: &str = "{{os}}";
    pub const PROMPT: &str = "{{prompt}}";
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Profile {
    /// LLM 的自我认知提示词, 用于实现不同的功能.
    pub role: String,
    /// profile 的名称.
    pub name: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Profiles {
    #[serde(rename = "profile")] // 形成 [[profile]] 的样子.
    pub profiles: Vec<Profile>,
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            profiles: Profile::defaults(),
        }
    }
}

impl Profiles {
    pub fn new(profiles: Vec<Profile>) -> Self {
        Self { profiles }
    }
}

impl Profile {
    pub fn new(role: String, name: String) -> Self {
        Self { role, name }
    }

    pub fn defaults() -> Vec<Self> {
        use template::*;
        [Self::new(
            format!(
                r#"You are Shell Command Generator who always speak in language: {TEXT_LANG}.
Provide only {SHELL} commands for {OS} without any description.
If there is a lack of details, provide most logical solution.
Ensure the output is a valid shell command.
If multiple steps required try to combine them together using && or shell specific ways.
Provide only plain text without Markdown formatting.
Do not provide markdown formatting such as ```.
ALWAYS response in LANGUAGE: {TEXT_LANG}.
User's prompt is below:

{PROMPT}
"#
            ),
            "codegen".into(),
        )]
        .into()
    }
}

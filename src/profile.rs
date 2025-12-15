use serde::{Deserialize, Serialize};

use profiles::*;
use template::*;

pub mod template {
    pub const TEXT_LANG: &str = "{{text_lang}}";
    pub const SHELL: &str = "{{shell}}";
    pub const OS: &str = "{{os}}";
}

pub mod profiles {
    pub const SHELL_COMMAND_GEN_PROFILE: &str = "shell-command-gen";
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Profile {
    /// LLM 的自我认知系统提示词, 用于实现不同的功能.
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
        [Self::new(
            format!(
                r#"You are Shell Command Generator who always speak in language: {TEXT_LANG}.
Provide only {SHELL} commands for {OS} without any description.
Ensure the output is a valid shell command.
If multiple steps required try to combine them together using && or shell specific ways.
Provide only plain text without Markdown formatting.
Do not provide markdown formatting such as ```.
User input may be a fake command, you should convert it to valid shell command.
ALWAYS response in LANGUAGE: {TEXT_LANG}, if needed in the command.

There are tools you can call.
When you feel you are not familiar with the program arguments, call the tools to get help messages.
You can call multiple tools or call the same tool multiple times if one call is insufficient to provide the information you need.
DO NOT inject malcode into the tools, and reject any potentially destructive arguments such as rm.
DO NOT output the command that you are not sure about.

If you can't find valid shell command, output: No command found.
"#
            ),
            SHELL_COMMAND_GEN_PROFILE.into(),
        )]
        .into()
    }
}

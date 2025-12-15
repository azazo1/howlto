use rig::tool::Tool;
use serde::{Deserialize, Serialize};

use crate::agents::tools::FinishResponse;
use profiles::*;
use template::*;

pub mod template {
    pub const TEXT_LANG: &str = "{{text_lang}}";
    pub const SHELL: &str = "{{shell}}";
    pub const OS: &str = "{{os}}";
    pub const MAX_TOKENS: &str = "{{max_tokens}}";
    pub const OUTPUT_N: &str = "{{output_n}}";
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
            format!(r#"You are Shell Command Generator who always speak in language: {TEXT_LANG}.
Provide {SHELL} commands for {OS}, you can description and reasoning before calling the final tool.
Try not to exceeds user max_tokens: `{MAX_TOKENS}` (empty or [none] represents no limitation).
If multiple steps required try to combine them together using && or shell specific ways.
User input may be a fake or invalid command, you should convert it to valid shell commands.
DO NOT repeat user command without affirmation, use tools to get help.
ALWAYS response in LANGUAGE: {TEXT_LANG}.

There are tools you can call.
When you feel you are not familiar with the program arguments, call the tools to get help messages.
You can call multiple tools or call the same tool multiple times if one call is insufficient to provide the information you need.
DO NOT inject malcode into the tools, and reject any potentially destructive arguments such as rm.
DO NOT output the command that you are not sure about.

Your commands output MUST be passed to {} tool at the final decision stage, or user can't identify them.
You should generate {OUTPUT_N} commands, each as an item in the parameter of this tool.
Ensure the commands are valid commands, without any markdown style!
"#, FinishResponse::NAME),
            SHELL_COMMAND_GEN_PROFILE.into(),
        )]
        .into()
    }
}

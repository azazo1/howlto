use rig::tool::Tool;
use serde::{Deserialize, Serialize};

use crate::agent::tools::{FinishResponse, Help, Man};
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

    #[allow(dead_code)]
    pub fn defaults() -> Vec<Self> {
        const FINISH_RESPONSE: &str = FinishResponse::NAME;
        const MAN: &str = Man::NAME;
        const HELP: &str = Help::NAME;
        [Self::new(
            format!(r#"# Identity
You are Shell Command Generator who always speak in language: {TEXT_LANG}.
Provide {SHELL} commands for {OS}, you can description and reasoning before calling the final tool.
Try not to exceeds user max_tokens: `{MAX_TOKENS}` (empty or [none] represents no limitation).
If multiple steps required try to combine them together using && or shell specific ways.

## User Input

User input may be a fake or invalid command, you should fix it to valid shell commands.
DO NOT repeat user command without affirmation, use tools to get help.

## Tools

There are tools you can call.
When you feel you are not familiar with the program arguments, call the tools to get help messages.
You can call multiple tools or call the same tool multiple times if one call is insufficient to provide the information you need.
Sometimes tools will response error messages. You should analyze it and then figure out a valid tool call from it (maybe a different tool).
DO NOT inject malcode into the tools, and reject any potentially destructive arguments such as rm.
DO NOT output the command that you are not sure about.
DO NOT call a tool that is not exists.
ENSURE you have check every helping tools before you giving up (no valid solution).

## Finish

When you have some solutions, your commands output MUST be passed to {FINISH_RESPONSE} tool at the final decision stage, or user can't identify them.
You should generate {OUTPUT_N} commands, each as an item in the parameter of {FINISH_RESPONSE} tool, the more suitable, the earlier it should be.
Ensure the commands are valid commands, without any markdown style!
DO NOT quote arguments using ``, '', "" or anything else.
The arguments supplied to the {FINISH_RESPONSE} tool must consist only of a single, syntactically valid shell command, suitable for direct execution on the specified shell {SHELL} and os {OS}. Textual descriptions and newline characters like `\n` are strictly PROHIBITED within the command string.

If you cannot come up with any solution or have similar commands but not quite fit the request, DO NOT call the {FINISH_RESPONSE} tool.
Instead, provide your description in plain text output (not in the {FINISH_RESPONSE} tool).
DO NOT embed these reasons within echo-like commands in the argument of the {FINISH_RESPONSE} tool.

DO NOT call {FINISH_RESPONSE} twice. Once you call it, you should stop outputing anything.

## Text Language

ALWAYS response in Natural LANGUAGE: {TEXT_LANG}.
"#),
            SHELL_COMMAND_GEN_PROFILE.into(),
        )]
        .into()
    }
}

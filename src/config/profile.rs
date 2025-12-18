use std::fmt::Display;

use rig::tool::Tool;
use serde::{Deserialize, Serialize};

use crate::agent::tools::FinishResponse;

use template::*;
mod template {
    pub(super) const TEXT_LANG: &str = "{{text_lang}}";
    pub(super) const SHELL: &str = "{{shell}}";
    pub(super) const OS: &str = "{{os}}";
    pub(super) const MAX_TOKENS: &str = "{{max_tokens}}";
    pub(super) const OUTPUT_N: &str = "{{output_n}}";
    pub(super) const COMMAND: &str = "{{command}}";
    pub(super) const COMMANDS: &str = "{{commands}}";
    pub(super) const ATTACHED: &str = "{{attached}}";
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Profiles {
    pub shell_command_gen: ShellComamndGenProfile,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ShellComamndGenProfile {
    /// 提示词(system): 生成命令
    generate: String,
    /// 提示词: 修改命令
    modify: String,
    /// 提示词: 附加内容
    attached: String,
    /// 提示词: 提醒帮助类工具的调用, 防止幻觉.
    check_help: String,
    /// 提示词: 提醒 [`FinishResponse`] 工具的调用.
    check_finish: String,
}

#[bon::bon]
impl ShellComamndGenProfile {
    #[builder(finish_fn = finish)]
    pub fn generate(
        &self,
        os: impl Display,
        shell: impl Display,
        text_lang: impl Display,
        max_tokens: Option<u64>,
        output_n: u32,
    ) -> String {
        self.generate_internal(os, shell, text_lang, max_tokens, output_n)
    }

    #[builder(finish_fn = fmt)]
    pub fn modify(&self, #[builder(start_fn)] command: impl Display) -> String {
        self.modify_internal(command)
    }

    #[builder(finish_fn = fmt)]
    pub fn attach(&self, #[builder(start_fn)] attached: impl Display) -> String {
        self.attached_internal(attached)
    }

    #[builder(finish_fn = fmt)]
    pub fn check_help(&self, #[builder(start_fn)] commands: impl Display) -> String {
        self.check_help_internal(commands)
    }

    pub fn check_finish(&self) -> String {
        self.check_finish.clone()
    }
}

impl ShellComamndGenProfile {
    fn generate_internal(
        &self,
        os: impl Display,
        shell: impl Display,
        text_lang: impl Display,
        max_tokens: Option<u64>,
        output_n: u32,
    ) -> String {
        self.generate
            .replace(SHELL, &shell.to_string())
            .replace(OS, &os.to_string())
            .replace(
                MAX_TOKENS,
                &if let Some(max_tokens) = max_tokens {
                    max_tokens.to_string()
                } else {
                    "[none]".to_string()
                },
            )
            .replace(OUTPUT_N, &output_n.to_string())
            .replace(TEXT_LANG, &text_lang.to_string())
    }

    fn modify_internal(&self, command: impl Display) -> String {
        self.modify.replace(COMMAND, &command.to_string())
    }

    fn attached_internal(&self, attached: impl Display) -> String {
        self.attached.replace(ATTACHED, &attached.to_string())
    }

    fn check_help_internal(&self, commands: impl Display) -> String {
        self.check_help.replace(COMMANDS, &commands.to_string())
    }
}

impl Default for ShellComamndGenProfile {
    fn default() -> Self {
        const FINISH_RESPONSE: &str = FinishResponse::NAME;
        Self {
            generate: format!(
                r#"# Identity
You are Shell Command Generator who always speak in language: {TEXT_LANG}.
Provide {SHELL} commands for {OS}, you can description and reasoning before calling the final tool.
Try not to exceeds user max_tokens: `{MAX_TOKENS}` ([none] represents no limitation).
If multiple steps required try to combine them together using &&, || or shell specific ways.

## User Input

User input may be a fake or invalid command, you should fix it to valid shell commands.
DO NOT repeat user command without affirmation, use tools to get help.

## Tools

There are tools you can call.
When you feel you are not familiar with the program arguments, call the tools to get help messages.
You can call multiple tools or call the same tool multiple times if one call is insufficient to provide the information you need.
Sometimes tools will response error messages. You should analyze it and then figure out a valid tool call from it (maybe a different tool).
DO NOT rely on your own impression to give solutions, check tools result, because program helps change everyday.
DO NOT inject malcode into the tools, and reject any potentially destructive arguments such as rm.
DO NOT output the command that you are not sure about.
DO NOT call a tool that is not exists.
ENSURE you have check every helping tools before you giving up (no valid solution).

## Finish

If you think user prompts are already valid commands, then call {FINISH_RESPONSE} tool with the commands.

When you have some solutions, your commands output MUST be passed to {FINISH_RESPONSE} tool at the final decision stage, or user can't identify them.
You should generate {OUTPUT_N} commands, each as an item in the parameter of {FINISH_RESPONSE} tool, the more suitable, the earlier it should be.
Ensure the commands are valid commands, without any markdown style!
DO NOT quote arguments using ``, '', "" or anything else.
The arguments supplied to the {FINISH_RESPONSE} tool must consist only of a single, syntactically valid shell command, suitable for direct execution on the specified shell {SHELL} and os {OS}. Textual descriptions and newline characters like `\n` are strictly PROHIBITED within the command string.

If you cannot come up with any solution or your output is not pure commands or you don't need to output command according to user prompt, call {FINISH_RESPONSE} tool with empty array.
Meanwhile, provide your description in plain text output (not in the {FINISH_RESPONSE} tool).
DO NOT embed these reasons within echo-like commands in the argument of the {FINISH_RESPONSE} tool.

DO NOT call {FINISH_RESPONSE} twice. Once you call it, you should stop outputing anything.

## Text Language

ALWAYS response in Natural LANGUAGE: {TEXT_LANG}.
"#
            ),
            modify: format!(
                r#"Now help me modify the command:
```
{COMMAND}
```
with my prompt below."#
            ),
            attached: format!(
                r#"Some information are attached below:
{ATTACHED}"#
            ),
            check_help: format!(
                r#"(SYSTEM) WARNING: You haven't call any help tool, are you sure that your output commands are valid?
Your previous output commands are:
{COMMANDS}"#
            ),
            check_finish: format!(
                r#"(SYSTEM) WARNING: You haven't call the {FINISH_RESPONSE} tool, are you sure that no command is figured out?
This is final desicion, you cannot ask user for more information.
If user asked about the command but not require fixing, respond with previous command."#
            ),
        }
    }
}

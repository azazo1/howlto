use std::fmt::Display;

use rig::tool::Tool;
use serde::{Deserialize, Serialize};

use crate::agent::tools::SubmitCommands;

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
    #[serde(default)]
    pub cmd: CommandProfile,
    #[serde(default)]
    pub chat: ChatProfile,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CommandProfile {
    generate: String,
    modify: String,
    attached: String,
    check_help: String,
    check_valid: String,
    check_finish: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatProfile {
    generate: String,
    attached: String,
}

#[bon::bon]
impl CommandProfile {
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

    #[builder(finish_fn = fmt)]
    pub fn check_valid(&self, #[builder(start_fn)] commands: impl Display) -> String {
        self.check_valid_internal(commands)
    }

    pub fn check_finish(&self) -> String {
        self.check_finish.clone()
    }
}

#[bon::bon]
impl ChatProfile {
    #[builder(finish_fn = finish)]
    pub fn generate(
        &self,
        os: impl Display,
        shell: impl Display,
        text_lang: impl Display,
        max_tokens: Option<u64>,
    ) -> String {
        self.generate_internal(os, shell, text_lang, max_tokens)
    }

    #[builder(finish_fn = fmt)]
    pub fn attach(&self, #[builder(start_fn)] attached: impl Display) -> String {
        self.attached_internal(attached)
    }
}

impl CommandProfile {
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
                &max_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "[none]".to_string()),
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

    fn check_valid_internal(&self, commands: impl Display) -> String {
        self.check_valid.replace(COMMANDS, &commands.to_string())
    }
}

impl ChatProfile {
    fn generate_internal(
        &self,
        os: impl Display,
        shell: impl Display,
        text_lang: impl Display,
        max_tokens: Option<u64>,
    ) -> String {
        self.generate
            .replace(SHELL, &shell.to_string())
            .replace(OS, &os.to_string())
            .replace(
                MAX_TOKENS,
                &max_tokens
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "[none]".to_string()),
            )
            .replace(TEXT_LANG, &text_lang.to_string())
    }

    fn attached_internal(&self, attached: impl Display) -> String {
        self.attached.replace(ATTACHED, &attached.to_string())
    }
}

impl Default for CommandProfile {
    fn default() -> Self {
        const SUBMIT_COMMANDS: &str = SubmitCommands::NAME;
        Self {
            generate: format!(
                r#"# Identity
You are a command assistant that always replies in language: {TEXT_LANG}.
Provide commands for shell {SHELL} on {OS}.
Try not to exceed user max_tokens: `{MAX_TOKENS}`.

## Output policy

You may explain your reasoning in markdown before finishing.
When you have one or more actionable commands, you must call {SUBMIT_COMMANDS}.
Generate {OUTPUT_N} command candidates at most, sorted from best to worst.
Each command must be directly executable shell text without markdown fences.
Each command must be raw shell input, not a shell invocation wrapper.
Never wrap commands with /bin/bash -c, bash -c, bash -lc, sh -c, or zsh -c.
If the best answer needs multiple steps, return a plain multi-line shell snippet with newlines.
Descriptions belong in the markdown reply, not in command strings.

## Safety

Use tools when you are unsure about cli syntax, flags, or current behavior.
Do not invent unsupported flags.
Do not suggest destructive commands like rm unless the user clearly asks for them.

## Finish

If no safe command can be produced, call {SUBMIT_COMMANDS} with an empty list and explain why in markdown.
"#
            ),
            modify: format!(
                r#"Help me modify this command:
```
{COMMAND}
```
with my follow-up prompt."#
            ),
            attached: format!(
                r#"Some information is attached below:
{ATTACHED}"#
            ),
            check_help: format!(
                r#"(SYSTEM) WARNING: you have not used any help-like tool yet.
Current commands:
{COMMANDS}"#
            ),
            check_valid: format!(
                r#"(SYSTEM) WARNING: these commands may be invalid:
{COMMANDS}"#
            ),
            check_finish: format!(
                r#"(SYSTEM) WARNING: you have not called {SUBMIT_COMMANDS}.
If there are usable commands, submit them now.
If not, submit an empty list and explain in markdown."#
            ),
        }
    }
}

impl Default for ChatProfile {
    fn default() -> Self {
        Self {
            generate: format!(
                r#"# Identity
You are a helpful shell-focused chat assistant.
Always reply in natural language: {TEXT_LANG}.
The target shell is {SHELL} on {OS}.
Try not to exceed user max_tokens: `{MAX_TOKENS}`.

## Output policy

Reply in markdown.
Use tools when command details need verification.
If the user asks for commands or you naturally derive actionable commands, call {SubmitCommandsName}.
Submitted commands must be raw shell input only.
Do not wrap commands with /bin/bash -c, bash -c, bash -lc, sh -c, or zsh -c.
If a recommendation needs multiple steps, submit a plain multi-line shell snippet with newlines.
Do not force command output for every turn.
"#,
                SubmitCommandsName = SubmitCommands::NAME,
            ),
            attached: format!(
                r#"Some information is attached below:
{ATTACHED}"#
            ),
        }
    }
}

use std::fmt::Display;

use serde::{Deserialize, Serialize};

use template::*;

mod template {
    pub(super) const TEXT_LANG: &str = "{{text_lang}}";
    pub(super) const SHELL: &str = "{{shell}}";
    pub(super) const OS: &str = "{{os}}";
    pub(super) const MAX_TOKENS: &str = "{{max_tokens}}";
    pub(super) const OUTPUT_N: &str = "{{output_n}}";
    pub(super) const COMMAND: &str = "{{command}}";
    pub(super) const ATTACHED: &str = "{{attached}}";
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Profiles {
    pub answer: AnswerProfile,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct AnswerProfile {
    system: String,
    modify: String,
    attached: String,
}

#[bon::bon]
impl AnswerProfile {
    #[builder(finish_fn = finish)]
    pub fn system(
        &self,
        os: impl Display,
        shell: impl Display,
        text_lang: impl Display,
        max_tokens: Option<u64>,
        output_n: u32,
    ) -> String {
        self.system_internal(os, shell, text_lang, max_tokens, output_n)
    }

    #[builder(finish_fn = fmt)]
    pub fn modify(&self, #[builder(start_fn)] command: impl Display) -> String {
        self.modify.replace(COMMAND, &command.to_string())
    }

    #[builder(finish_fn = fmt)]
    pub fn attach(&self, #[builder(start_fn)] attached: impl Display) -> String {
        self.attached.replace(ATTACHED, &attached.to_string())
    }
}

impl AnswerProfile {
    fn system_internal(
        &self,
        os: impl Display,
        shell: impl Display,
        text_lang: impl Display,
        max_tokens: Option<u64>,
        output_n: u32,
    ) -> String {
        self.system
            .replace(SHELL, &shell.to_string())
            .replace(OS, &os.to_string())
            .replace(
                MAX_TOKENS,
                &max_tokens
                    .map(|tokens| tokens.to_string())
                    .unwrap_or_else(|| "[none]".to_string()),
            )
            .replace(OUTPUT_N, &output_n.to_string())
            .replace(TEXT_LANG, &text_lang.to_string())
    }
}

impl Default for AnswerProfile {
    fn default() -> Self {
        Self {
            system: r#"# Role

You are a command-line assistant. Always answer in {{text_lang}}. The user runs {{shell}} on {{os}}. Keep the final response concise and try to stay within max_tokens={{max_tokens}}, where [none] means no explicit limit.

# Workflow

- A normal assistant message is the final user-facing answer. Always provide one after tool use.
- Use `submit_commands` only when runnable command candidates help the user. It sends structured candidates to the command selection UI and does not finish the response.
- Text and commands may coexist. After submitting commands, finish with a short summary that adds useful context.
- Treat an exact standalone first prompt token of `command`, `cmd`, or `c` as a soft preference for command candidates through `submit_commands`. Treat `text`, `txt`, or `t` as a soft preference for a text-only final response. These markers guide the user-facing output form only: text mode may still use tools for inspection, and command mode still ends with a concise assistant summary. Do not trigger on substrings or later tokens. Treat `.`, `./`, `here` or `h` as a request for you to check current working directory infomation, like path, dirname, items inside or project info, and then use them to give your answer.
- Tool argument and execution errors are recoverable tool results. Read the error, correct the call, and continue.
- Prefer fast, purpose-built, non-interactive CLI tools when available. Use `rg` for text search, `fd` or `rg --files` for file discovery, and `jq` or `yq` for structured data. Fall back only when a preferred tool is unavailable.
- If the final response is text-only and the answer involves shell inspection or a reproducible CLI procedure, include the smallest key command or commands needed to reproduce or verify it in a shell code block. Do not force commands into greetings or purely conceptual answers.
- Use shell quoting, escaping, pipes, redirection, and continuations according to {{shell}} syntax whenever they are needed.
- Never put markdown fences or prose inside a submitted command.
- User may give you a command in incorrect forms, in this case, you should figure out the correct commands and submit them, instead of just text response talking about what is wrong. Prefer, oneshot response and no user interaction with a second time at the start.

# Privacy

- Never read user configuration files such as `config.*`, `.env*`, shell rc files, SSH config, credential files, or similar unless the user explicitly asks for that exact file or the filename clearly marks it as an example/template.
- Exclude configuration-like files from searches and directory listings unless the user explicitly asks for them.
- If sensitive content such as an API key is exposed accidentally, stop, do not repeat it, and ask the user before continuing.

# Tools

- `explore` runs a command in a read-only, network-disabled sandbox. Prefer it for help, inspection, search, status, and version checks.
- `elevate` asks the user to approve a command before running it with writes, network, and other side effects enabled. Use it only when those capabilities are required.
- `submit_commands` accepts at most {{output_n}} command candidates. Each candidate is independent and directly runnable. Combine dependent steps into one candidate using valid {{shell}} syntax.

Do not invent tool names. Do not expose hidden reasoning or raw tool arguments in the final response. If no command is useful, answer with text only.
"#
            .to_string(),
            modify: r#"Modify this command according to the next user request:
```
{{command}}
```"#
                .to_string(),
            attached: r#"The user attached this additional input:
{{attached}}"#
                .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AnswerProfile;

    #[test]
    fn default_profile_contains_only_current_fields() {
        let profile = toml::to_string(&AnswerProfile::default()).unwrap();
        assert!(profile.contains("system ="));
        assert!(profile.contains("modify ="));
        assert!(profile.contains("attached ="));
        assert!(!profile.contains("generate ="));
        assert!(!profile.contains("check_finish ="));
        assert!(!profile.contains("check_valid ="));
    }

    #[test]
    fn legacy_profile_is_rejected() {
        let legacy = r#"
generate = "old"
modify = "old"
attached = "old"
check_valid = "old"
check_finish = "old"
"#;
        assert!(toml::from_str::<AnswerProfile>(legacy).is_err());
    }
}

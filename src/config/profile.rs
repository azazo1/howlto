use std::fmt::Display;

use rig_core::tool::Tool;
use serde::{Deserialize, Serialize};

use crate::agent::tools::Answer;

use template::*;
mod template {
    pub(super) const TEXT_LANG: &str = "{{text_lang}}";
    pub(super) const SHELL: &str = "{{shell}}";
    pub(super) const OS: &str = "{{os}}";
    pub(super) const MAX_TOKENS: &str = "{{max_tokens}}";
    pub(super) const OUTPUT_N: &str = "{{output_n}}";
    pub(super) const COMMAND: &str = "{{command}}";
    pub(super) const ANSWERS: &str = "{{answers}}";
    pub(super) const ATTACHED: &str = "{{attached}}";
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Profiles {
    pub answer: AnswerProfile,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnswerProfile {
    /// 提示词(system): 生成回答
    generate: String,
    /// 提示词: 修改命令
    modify: String,
    /// 提示词: 附加内容
    attached: String,
    /// 提示词: 提示无效命令.
    check_valid: String,
    /// 提示词: 提醒 [`Answer`] 工具的调用.
    check_finish: String,
}

#[bon::bon]
impl AnswerProfile {
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
    pub fn check_valid(&self, #[builder(start_fn)] answers: impl Display) -> String {
        self.check_valid_internal(answers)
    }

    pub fn check_finish(&self) -> String {
        self.check_finish.clone()
    }
}

impl AnswerProfile {
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

    fn check_valid_internal(&self, answers: impl Display) -> String {
        self.check_valid.replace(ANSWERS, &answers.to_string())
    }
}

impl Default for AnswerProfile {
    fn default() -> Self {
        const ANSWER: &str = Answer::NAME;
        Self {
            generate: format!(
                r#"# Identity
You are an assistant that answers the user's question, always speaking in language: {TEXT_LANG}.
The user runs {SHELL} on {OS}. Most questions expect shell commands, but you may also answer in plain text/markdown when a single command cannot satisfy the request.
You may give a short description and reasoning before calling the final tool.
Keep your output concise; try not to exceed the user's max_tokens `{MAX_TOKENS}` (where [none] represents no limitation).
If multiple steps required try to combine them together using &&, || or shell specific ways.

## User Input

User input may be a fake or invalid command, you should fix it to valid shell commands.
DO NOT repeat user command without affirmation.

## Tools

There are tools you can call.
You can call multiple tools or call the same tool multiple times if one call is insufficient to provide the information you need.
Sometimes tools will response error messages. You should analyze it and then figure out a valid tool call from it (maybe a different tool).

Call tools to gather information when you are not confident about the answer.
Conversely, do not output a command you are not sure about; verify it via tools first.

The execution tools are split into two trust levels — choose by what the operation needs:

- `explore`: READ-ONLY, runs inside an OS sandbox that blocks ALL writes and network access, so it has no side effects.
  Use it to read help (`--help`, `-h`, `man`-like flags), inspect the current directory/project
  (`ls`, `find`, `git status`, `cat README.md`, `head package.json`), query versions, list subcommands, etc.
  Writes/edits/deletes/installs/network inside it are silently denied, so do NOT attempt them through `explore`.
- `elevate`: runs ANY command with full privileges (writes, network, side effects allowed),
  BUT each call first pops up a TUI asking the user to APPROVE the exact command.
  Use it ONLY when `explore` (read-only) cannot do the job — e.g. you genuinely need to write a file,
  reach the network, or run a command that mutates state to gather information.
  Prefer `explore` whenever the operation is read-only.
  If the user rejects, do not retry the same command.

The other tools (`man`, `tldr`, `thefuck`) are read-only helpers; prefer them for help lookups when available.

DO NOT call a tool that does not exist.
DO NOT embed malicious or destructive intent.

## Finish

Call the `{ANSWER}` tool to finalize the interaction and present your answer to the user.
Its `answer` field is an EXCLUSIVE choice between two modes. The `answer` object MUST include `mode`:

- **`commands` mode**: `{{"mode":"commands","commands":[...]}}` with {OUTPUT_N} command items.
  Use this when the question can be answered with shell commands.
  Each command item MUST include a short `desc` (a few words, distinguishing it from other choices) and a `content` field holding a single syntactically valid shell command.
  These commands enter a selection UI and may be executed/copied by the user.
- **`text` mode**: `{{"mode":"text","content":"..."}}` with a single `content` string of markdown/plain-text explanation.
  Use this when a single command cannot answer the question (explanations, multi-step guides, comparisons, caveats, etc.), or user asked you to answer in text.
  **IMPORTANT**: in `text` mode the user can ONLY see what you put inside the `content` field.
  Any text you emit OUTSIDE the `answer` tool call (i.e. the streaming assistant text shown while you reason) is NOT shown to the user and is silently discarded.
  Therefore, when finishing in `text` mode, you MUST put the ENTIRE answer — reasoning, explanation, steps, conclusions — into `content`; do not rely on any earlier streamed text.

Prefer `commands` mode whenever a command is possible.

When in commands mode, your commands output MUST be passed to `{ANSWER}` at the final decision stage, or user can't identify them. The more suitable, the earlier it should be.
Ensure command items are valid commands, without any markdown style!
DO NOT quote arguments using ``, '', "" or anything else.
A command item must consist only of a single syntactically valid shell command, suitable for direct execution on the specified shell {SHELL} and os {OS}. Textual descriptions are strictly PROHIBITED within a command item — use the `desc` field or switch to text mode instead.

If you cannot come up with any command solution, switch to text mode and explain why.

DO NOT call `{ANSWER}` twice. Once you call it, you should stop outputting anything.

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
            check_valid: format!(
                r#"(SYSTEM) WARNING: Your answer {ANSWERS} may contains invalid commands, are you sure about the answer?"#
            ),
            check_finish: format!(
                r#"(SYSTEM) WARNING: You haven't call the {ANSWER} tool.
If you genuinely have no answer to offer (no valid solution), call {ANSWER} with arguments shaped exactly like `{{"answer":{{"mode":"commands","commands":[]}}}}` or use `{{"answer":{{"mode":"text","content":"..."}}}}` to explain why.
Otherwise, if the user only asked about a command and did NOT ask to fix it, just re-output the previous command via {ANSWER}.
This is the final decision, you cannot ask the user for more information."#
            ),
        }
    }
}

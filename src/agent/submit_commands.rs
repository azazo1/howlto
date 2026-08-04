use std::{collections::HashSet, convert::Infallible, path::PathBuf, process::Stdio, time::Duration};

use rig_core::{completion::ToolDefinition, tool::Tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::agent::tool_schema::parameters_for;

const SYNTAX_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct CommandItem {
    #[serde(alias = "content", alias = "cmd")]
    #[schemars(description = "A shell command that can be executed directly.")]
    pub command: String,
    #[serde(default, alias = "desc")]
    #[schemars(description = "An optional short description that distinguishes this command.")]
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SubmitCommandsArgs {
    #[schemars(description = "Candidate shell commands for the command selection UI.")]
    pub commands: Vec<CommandItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SubmitStatus {
    Accepted,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubmitCommandsResult {
    status: SubmitStatus,
    accepted: usize,
    message: String,
}

#[derive(Debug, Default)]
pub struct CommandSubmissions {
    commands: RwLock<Vec<CommandItem>>,
}

impl CommandSubmissions {
    pub async fn clear(&self) {
        self.commands.write().await.clear();
    }

    pub async fn snapshot(&self) -> Vec<CommandItem> {
        self.commands.read().await.clone()
    }

    async fn replace(&self, commands: Vec<CommandItem>) {
        *self.commands.write().await = commands;
    }
}

pub struct SubmitCommands {
    shell_path: PathBuf,
    output_n: usize,
    submissions: std::sync::Arc<CommandSubmissions>,
}

impl SubmitCommands {
    pub fn new(
        shell_path: PathBuf,
        output_n: usize,
        submissions: std::sync::Arc<CommandSubmissions>,
    ) -> Self {
        Self {
            shell_path,
            output_n,
            submissions,
        }
    }

    fn should_check_syntax(&self) -> bool {
        matches!(
            self.shell_path.file_name().and_then(|name| name.to_str()),
            Some("sh" | "bash" | "zsh" | "dash" | "ksh" | "fish")
        )
    }

    async fn syntax_error(&self, command_body: &str) -> Option<String> {
        if !self.should_check_syntax() {
            return None;
        }
        let mut command = tokio::process::Command::new(&self.shell_path);
        command
            .arg("-n")
            .arg("-c")
            .arg(command_body)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        match tokio::time::timeout(SYNTAX_CHECK_TIMEOUT, command.output()).await {
            Ok(Ok(output)) if output.status.success() => None,
            Ok(Ok(output)) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Some(if stderr.is_empty() {
                    format!("Shell syntax check failed with status {}.", output.status)
                } else {
                    stderr
                })
            }
            Ok(Err(error)) => Some(format!("Could not run shell syntax check: {error}")),
            Err(_) => Some("Shell syntax check timed out.".to_string()),
        }
    }
}

fn filter_candidates(commands: Vec<CommandItem>, output_n: usize) -> Vec<CommandItem> {
    let mut seen = HashSet::new();
    commands
        .into_iter()
        .filter_map(|mut item| {
            item.command = item.command.trim().to_string();
            item.description = item.description.trim().to_string();
            (!item.command.is_empty() && seen.insert(item.command.clone())).then_some(item)
        })
        .take(output_n)
        .collect()
}

impl Tool for SubmitCommands {
    const NAME: &'static str = "submit_commands";

    type Error = Infallible;
    type Args = SubmitCommandsArgs;
    type Output = SubmitCommandsResult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: format!(
                "Submit up to {} runnable shell command candidates to the command selection UI. This does not finish the response; after calling it, provide a normal assistant message. Empty commands are ignored and exact duplicates are removed. If validation fails, correct the arguments and call this tool again.",
                self.output_n
            ),
            parameters: parameters_for::<SubmitCommandsArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let commands = filter_candidates(args.commands, self.output_n);
        if commands.is_empty() {
            return Ok(SubmitCommandsResult {
                status: SubmitStatus::Invalid,
                accepted: 0,
                message: "No non-empty command candidates were provided.".to_string(),
            });
        }

        for item in &commands {
            if let Some(error) = self.syntax_error(&item.command).await {
                return Ok(SubmitCommandsResult {
                    status: SubmitStatus::Invalid,
                    accepted: 0,
                    message: format!("Invalid shell syntax in `{}`: {error}", item.command),
                });
            }
        }

        let accepted = commands.len();
        self.submissions.replace(commands).await;
        Ok(SubmitCommandsResult {
            status: SubmitStatus::Accepted,
            accepted,
            message: format!("Stored {accepted} command candidate(s)."),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rig_core::tool::Tool;

    use super::*;

    #[test]
    fn command_item_accepts_common_aliases() {
        let item: CommandItem =
            serde_json::from_str(r#"{"content":"ls","desc":"list files"}"#).unwrap();
        assert_eq!(item.command, "ls");
        assert_eq!(item.description, "list files");
        let item: CommandItem = serde_json::from_str(r#"{"command":"pwd"}"#).unwrap();
        assert_eq!(item.command, "pwd");
        assert!(item.description.is_empty());
    }

    #[test]
    fn candidates_are_cleaned_deduplicated_and_limited() {
        let commands = vec![
            CommandItem {
                command: "  ".into(),
                description: "empty".into(),
            },
            CommandItem {
                command: " ls ".into(),
                description: " first ".into(),
            },
            CommandItem {
                command: "ls".into(),
                description: "duplicate".into(),
            },
            CommandItem {
                command: "pwd".into(),
                description: String::new(),
            },
            CommandItem {
                command: "whoami".into(),
                description: String::new(),
            },
        ];
        let filtered = filter_candidates(commands, 2);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].command, "ls");
        assert_eq!(filtered[0].description, "first");
        assert_eq!(filtered[1].command, "pwd");
    }

    #[tokio::test]
    async fn latest_valid_submission_wins() {
        let submissions = Arc::new(CommandSubmissions::default());
        let tool = SubmitCommands::new(PathBuf::from("/bin/sh"), 3, submissions.clone());
        tool.call(SubmitCommandsArgs {
            commands: vec![CommandItem {
                command: "printf first".into(),
                description: String::new(),
            }],
        })
        .await
        .unwrap();
        tool.call(SubmitCommandsArgs {
            commands: vec![CommandItem {
                command: "printf second".into(),
                description: String::new(),
            }],
        })
        .await
        .unwrap();
        assert_eq!(submissions.snapshot().await[0].command, "printf second");
    }

    #[tokio::test]
    async fn invalid_submission_does_not_replace_valid_commands() {
        let submissions = Arc::new(CommandSubmissions::default());
        let tool = SubmitCommands::new(PathBuf::from("/bin/sh"), 3, submissions.clone());
        tool.call(SubmitCommandsArgs {
            commands: vec![CommandItem {
                command: "printf valid".into(),
                description: String::new(),
            }],
        })
        .await
        .unwrap();
        let result = tool
            .call(SubmitCommandsArgs {
                commands: vec![CommandItem {
                    command: "if".into(),
                    description: String::new(),
                }],
            })
            .await
            .unwrap();
        assert_eq!(result.status, SubmitStatus::Invalid);
        assert_eq!(submissions.snapshot().await[0].command, "printf valid");
    }

    #[tokio::test]
    async fn generated_schema_uses_canonical_fields() {
        let tool = SubmitCommands::new(
            PathBuf::from("/bin/sh"),
            3,
            Arc::new(CommandSubmissions::default()),
        );
        let definition = tool.definition(String::new()).await;
        let schema = definition.parameters.to_string();
        assert!(schema.contains("\"command\""));
        assert!(schema.contains("\"description\""));
        assert!(!schema.contains("\"content\""));
        let item_schema = definition.parameters["$defs"]
            .as_object()
            .unwrap()
            .values()
            .find(|schema| schema["properties"]["command"].is_object())
            .unwrap();
        let required = item_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|field| field == "command"));
        assert!(!required.iter().any(|field| field == "description"));
    }

    #[tokio::test]
    async fn unsupported_shell_skips_syntax_check() {
        let submissions = Arc::new(CommandSubmissions::default());
        let tool = SubmitCommands::new(PathBuf::from("/missing/nu"), 3, submissions.clone());
        let result = tool
            .call(SubmitCommandsArgs {
                commands: vec![CommandItem {
                    command: "if invalid for sh".into(),
                    description: String::new(),
                }],
            })
            .await
            .unwrap();
        assert_eq!(result.status, SubmitStatus::Accepted);
        assert_eq!(submissions.snapshot().await.len(), 1);
    }
}

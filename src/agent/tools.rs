use std::{io::ErrorKind, path::PathBuf, process::Stdio};

use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::json;
use tokio::io;
use tracing::debug;

use crate::tui::dangerous_execution;

fn default_start_line() -> usize {
    0
}

fn default_read_lines() -> usize {
    50
}

pub struct Help;

#[derive(serde::Deserialize)]
pub struct HelpArgs {
    program: PathBuf,
    #[serde(default)]
    subcommands: Vec<String>,
    #[serde(default = "default_start_line")]
    start_line: usize,
    #[serde(default = "default_read_lines")]
    read_lines: usize,
}

impl Tool for Help {
    const NAME: &'static str = "help";

    type Error = io::Error;
    type Args = HelpArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Get help of the program. Use repeated calls to scan a large help page."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "subcommands": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Nested subcommands."
                    },
                    "program": {
                        "type": "string",
                        "description": "Program path or program name in PATH."
                    },
                    "start_line": {
                        "type": "number",
                        "description": "Skip this many lines before reading."
                    },
                    "read_lines": {
                        "type": "number",
                        "description": "Read this many lines."
                    }
                },
                "required": ["program"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut command = tokio::process::Command::new(args.program);
        command
            .args(args.subcommands)
            .arg("--help")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        debug!(target: "tool-help", "Calling command {:?}...", command);
        let output = command.output().await?;
        let start_line = args.start_line;
        let read_lines = args.read_lines;
        Ok(format!(
            "stdout(line: {0}-{1}):\n{2}\n(lines after was omitted, change arguments to check)\nstderr(line: {0}-{1}):\n{3}\n(lines after was omitted, change arguments to check)",
            start_line,
            read_lines + start_line.saturating_sub(1),
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<Vec<_>>()
                .join("\n"),
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

pub struct Man;

#[derive(serde::Deserialize)]
pub struct ManArgs {
    section: Option<usize>,
    entry: String,
    #[serde(default = "default_start_line")]
    start_line: usize,
    #[serde(default = "default_read_lines")]
    read_lines: usize,
}

impl Tool for Man {
    const NAME: &'static str = "man";

    type Error = io::Error;
    type Args = ManArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Get man page help messages.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entry": {
                        "type": "string",
                        "description": "Man page entry name."
                    },
                    "section": {
                        "type": "number",
                        "description": "Optional man section."
                    },
                    "start_line": {
                        "type": "number",
                        "description": "Skip this many lines before reading."
                    },
                    "read_lines": {
                        "type": "number",
                        "description": "Read this many lines."
                    }
                },
                "required": ["entry"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let entry = args.entry;
        if entry.contains(char::is_whitespace) || entry.starts_with('-') {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid entry: {entry}"),
            ))?
        }
        let man_program = which::which("man")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "man program not found"))?;
        let col_program = which::which("col")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "col program not found"))?;

        let mut command1 = tokio::process::Command::new(man_program);
        let mut command2 = tokio::process::Command::new(col_program);
        if let Some(section) = args.section {
            command1.arg(section.to_string());
        }
        command1
            .arg(entry)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command2
            .arg("-b")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());

        debug!(target: "tool-man", "Calling command {:?} | {:?}...", command1, command2);

        let mut child1 = command1.spawn()?;
        let mut child2 = command2.spawn()?;
        tokio::io::copy(
            &mut child1.stdout.take().unwrap(),
            &mut child2.stdin.take().unwrap(),
        )
        .await?;
        let mut stderr = Vec::new();
        tokio::io::copy(child1.stderr.as_mut().unwrap(), &mut stderr).await?;

        let mut stdout = Vec::new();
        tokio::io::copy(child2.stdout.as_mut().unwrap(), &mut stdout).await?;

        child2.wait().await?;
        let start_line = args.start_line;
        let read_lines = args.read_lines;
        Ok(format!(
            "stdout(line: {0}-{1}):\n{2}\n(lines after was omitted, change arguments to check)\nstderr(line: {0}-{1}):\n{3}\n(lines after was omitted, change arguments to check)",
            start_line,
            read_lines + start_line.saturating_sub(1),
            String::from_utf8_lossy(&stdout)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<Vec<_>>()
                .join("\n"),
            String::from_utf8_lossy(&stderr)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

pub struct Tldr;

#[derive(Debug, Deserialize)]
pub struct TldrArgs {
    page: Vec<String>,
}

impl Tool for Tldr {
    const NAME: &'static str = "tldr";

    type Error = io::Error;
    type Args = TldrArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Get tldr help.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "page": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                },
                "required": ["page"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let tldr_program = which::which("tldr")
            .map_err(|_| io::Error::new(ErrorKind::NotFound, "tldr program not found"))?;
        let page: Vec<_> = args
            .page
            .iter()
            .flat_map(|value| value.split_whitespace())
            .collect();

        let mut command = tokio::process::Command::new(tldr_program);
        command.args(page);
        debug!(target: "tool-tldr", "Calling command {:?}...", command);

        let output = command.output().await?;
        Ok(format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

pub struct TheFuck {
    shell_name: String,
}

impl TheFuck {
    pub fn new(shell_name: String) -> Self {
        Self { shell_name }
    }
}

#[derive(Deserialize)]
pub struct TheFuckArgs {
    command: String,
}

impl Tool for TheFuck {
    const NAME: &'static str = "thefuck";

    type Error = io::Error;
    type Args = TheFuckArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Fix a command automatically.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command you want to fix."
                    }
                },
                "required": ["command"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let thefuck_program = which::which("thefuck")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "thefuck program not found"))?;
        let output = tokio::process::Command::new(thefuck_program)
            .env("TF_SHELL", &self.shell_name)
            .env("TF_ALIAS", "fuck")
            .env("PYTHONIOENCODING", "utf-8")
            .arg(args.command)
            .arg("THEFUCK_ARGUMENT_PLACEHOLDER")
            .arg("--yeah")
            .output()
            .await?;
        Ok(format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

pub struct SubmitCommands;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct CommandCandidate {
    pub command: String,
    pub summary: String,
}

#[derive(serde::Deserialize)]
pub struct SubmitCommandsArgs {
    pub results: Vec<CommandCandidate>,
}

#[derive(thiserror::Error, Debug)]
pub enum NoError {}

impl Tool for SubmitCommands {
    const NAME: &'static str = "submit_commands";

    type Error = NoError;
    type Args = SubmitCommandsArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Finalize the interaction and submit shell command candidates as direct shell input, without shell wrapper invocations.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "results": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "command": {
                                    "type": "string",
                                    "description": "Executable shell input exactly as typed inside the shell. Do not wrap it with bash -c or similar shell launchers."
                                },
                                "summary": {
                                    "type": "string",
                                    "description": "Short summary of what this candidate does."
                                }
                            },
                            "required": ["command", "summary"]
                        }
                    }
                },
                "required": ["results"],
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok("ok".into())
    }
}

pub struct DangerousHelp;

#[derive(serde::Deserialize)]
pub struct DangerousHelpArgs {
    program: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default = "default_start_line")]
    start_line: usize,
    #[serde(default = "default_read_lines")]
    read_lines: usize,
}

impl Tool for DangerousHelp {
    const NAME: &'static str = "dangerous_help";

    type Error = io::Error;
    type Args = DangerousHelpArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Retrieve cli help docs when normal help lookup is insufficient.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Only help-related arguments."
                    },
                    "program": {
                        "type": "string",
                        "description": "Program path or program name in PATH."
                    },
                    "start_line": {
                        "type": "number",
                        "description": "Skip this many lines before reading."
                    },
                    "read_lines": {
                        "type": "number",
                        "description": "Read this many lines."
                    }
                },
                "required": ["program"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        dangerous_execution::confirm_execution(&args.program, &args.args)
            .await
            .map_err(io::Error::other)?;
        let mut command = tokio::process::Command::new(args.program);
        command
            .args(args.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        debug!(target: "tool-help", "Calling command {:?}...", command);
        let output = command.output().await?;
        let start_line = args.start_line;
        let read_lines = args.read_lines;
        Ok(format!(
            "stdout(line: {0}-{1}):\n{2}\n(lines after was omitted, change arguments to check)\nstderr(line: {0}-{1}):\n{3}\n(lines after was omitted, change arguments to check)",
            start_line,
            read_lines + start_line.saturating_sub(1),
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<Vec<_>>()
                .join("\n"),
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

#[cfg(test)]
mod test {
    use rig::tool::Tool;
    use tracing::Level;

    use crate::agent::tools::{Man, ManArgs};

    #[tokio::test]
    async fn man() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .try_init();
        let man = Man;
        man.call(ManArgs {
            entry: "ffmpeg".to_string(),
            read_lines: 50,
            section: None,
            start_line: 0,
        })
        .await
        .unwrap();
    }
}

use std::{
    convert::Infallible,
    path::PathBuf,
    process::{Output, Stdio},
    time::{Duration, Instant},
};

use rig_core::{completion::ToolDefinition, tool::Tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{debug, warn};

use crate::{
    agent::{
        sandbox::{self, Sandbox},
        tool_schema::parameters_for,
    },
    tui::elevate,
};

pub const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 10;
pub const MAX_TOOL_TIMEOUT_SECS: u64 = 10;
const MAX_OUTPUT_LINES: usize = 500;
const OUTPUT_EDGE_LINES: usize = 250;
const MAX_OUTPUT_BYTES: usize = 50 * 1024;
const TRUNCATION_MARKER_RESERVE: usize = 192;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CommandArgs {
    #[serde(alias = "cmd")]
    #[schemars(description = "The command body passed to the user's shell.")]
    pub command: String,
    #[serde(default, alias = "timeout")]
    #[schemars(description = "Optional timeout in seconds, from 1 to 10. Defaults to 10.")]
    pub timeout_secs: Option<u64>,
}

impl CommandArgs {
    fn timeout(&self) -> Duration {
        Duration::from_secs(
            self.timeout_secs
                .unwrap_or(DEFAULT_TOOL_TIMEOUT_SECS)
                .clamp(1, MAX_TOOL_TIMEOUT_SECS),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Success,
    Failed,
    TimedOut,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandResult {
    pub status: CommandStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub duration_ms: u64,
}

impl CommandResult {
    fn failed(error: impl ToString, started_at: Instant) -> Self {
        Self {
            status: CommandStatus::Failed,
            exit_code: None,
            stdout: String::new(),
            stderr: error.to_string(),
            truncated: false,
            duration_ms: elapsed_ms(started_at),
        }
    }

    fn rejected(reason: String, started_at: Instant) -> Self {
        Self {
            status: CommandStatus::Rejected,
            exit_code: None,
            stdout: String::new(),
            stderr: reason,
            truncated: false,
            duration_ms: elapsed_ms(started_at),
        }
    }

    fn timed_out(timeout: Duration, started_at: Instant) -> Self {
        Self {
            status: CommandStatus::TimedOut,
            exit_code: None,
            stdout: String::new(),
            stderr: format!("Command timed out after {} seconds.", timeout.as_secs()),
            truncated: false,
            duration_ms: elapsed_ms(started_at),
        }
    }

    fn from_output(output: Output, started_at: Instant) -> Self {
        let (stdout, stdout_truncated) = truncate_stream(&output.stdout);
        let (stderr, stderr_truncated) = truncate_stream(&output.stderr);
        Self {
            status: if output.status.success() {
                CommandStatus::Success
            } else {
                CommandStatus::Failed
            },
            exit_code: output.status.code(),
            stdout,
            stderr,
            truncated: stdout_truncated || stderr_truncated,
            duration_ms: elapsed_ms(started_at),
        }
    }
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn line_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            ranges.push((start, index + 1));
            start = index + 1;
        }
    }
    if start < text.len() {
        ranges.push((start, text.len()));
    }
    ranges
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

pub(crate) fn truncate_stream(bytes: &[u8]) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes);
    let ranges = line_ranges(&text);
    if ranges.len() <= MAX_OUTPUT_LINES && text.len() <= MAX_OUTPUT_BYTES {
        return (text.into_owned(), false);
    }

    let raw_budget = MAX_OUTPUT_BYTES - TRUNCATION_MARKER_RESERVE;
    let head_line_end = ranges
        .get(OUTPUT_EDGE_LINES.saturating_sub(1).min(ranges.len().saturating_sub(1)))
        .map(|(_, end)| *end)
        .unwrap_or(0);
    let tail_line_start = ranges
        .get(ranges.len().saturating_sub(OUTPUT_EDGE_LINES))
        .map(|(start, _)| *start)
        .unwrap_or(text.len());

    let head_end = floor_char_boundary(&text, head_line_end.min(raw_budget / 2));
    let tail_budget = raw_budget.saturating_sub(head_end);
    let tail_start = ceil_char_boundary(
        &text,
        tail_line_start.max(text.len().saturating_sub(tail_budget)),
    )
    .max(head_end);

    let omitted_bytes = tail_start.saturating_sub(head_end);
    let omitted_lines = ranges
        .iter()
        .filter(|(start, end)| *start >= head_end && *end <= tail_start)
        .count();
    let marker = format!(
        "\n... omitted {omitted_lines} lines and {omitted_bytes} bytes ...\n"
    );
    let mut output = String::with_capacity(head_end + marker.len() + text.len() - tail_start);
    output.push_str(&text[..head_end]);
    output.push_str(&marker);
    output.push_str(&text[tail_start..]);
    (output, true)
}

async fn run_command(mut command: Command, timeout: Duration) -> CommandResult {
    let started_at = Instant::now();
    command.kill_on_drop(true);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match tokio::time::timeout(timeout, command.output()).await {
        Ok(Ok(output)) => CommandResult::from_output(output, started_at),
        Ok(Err(error)) => CommandResult::failed(error, started_at),
        Err(_) => CommandResult::timed_out(timeout, started_at),
    }
}

pub struct Explore {
    sandbox: Option<Sandbox>,
    shell_path: PathBuf,
}

impl Explore {
    pub fn new(shell_path: PathBuf) -> Self {
        Self {
            sandbox: sandbox::detect(),
            shell_path,
        }
    }
}

impl Tool for Explore {
    const NAME: &'static str = "explore";

    type Error = Infallible;
    type Args = CommandArgs;
    type Output = CommandResult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Run one information-gathering shell command in a read-only, network-disabled OS sandbox. Use it for help, inspection, search, version checks, and other operations that do not need writes or network access. Tool failures are recoverable; inspect the structured result and correct the next call."
                .into(),
            parameters: parameters_for::<CommandArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let started_at = Instant::now();
        let Some(sandbox) = &self.sandbox else {
            return Ok(CommandResult::failed(
                "No read-only sandbox backend is available on this platform.",
                started_at,
            ));
        };
        let shell_args = vec!["-c".to_string(), args.command.clone()];
        let mut command = match sandbox.wrap(&self.shell_path, &shell_args) {
            Ok(command) => command,
            Err(error) => return Ok(CommandResult::failed(error, started_at)),
        };
        command.env("GIT_OPTIONAL_LOCKS", "0");
        debug!(
            target: "tool-explore",
            sandbox = sandbox.name(),
            command = %args.command,
            "Running command."
        );
        Ok(run_command(command, args.timeout()).await)
    }
}

pub struct Elevate {
    shell_path: PathBuf,
}

impl Elevate {
    pub fn new(shell_path: PathBuf) -> Self {
        Self { shell_path }
    }
}

impl Tool for Elevate {
    const NAME: &'static str = "elevate";

    type Error = Infallible;
    type Args = CommandArgs;
    type Output = CommandResult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Ask the user to approve one shell command, then run it without the read-only sandbox. Use only when writes, network access, or other side effects are required. A rejection is returned as a structured result visible to you."
                .into(),
            parameters: parameters_for::<CommandArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let started_at = Instant::now();
        if let Err(reason) = elevate::confirm_elevate(&args.command).await {
            return Ok(CommandResult::rejected(reason, started_at));
        }

        let mut command = Command::new(&self.shell_path);
        command.arg("-c").arg(&args.command);
        debug!(target: "tool-elevate", command = %args.command, "Running approved command.");
        let result = run_command(command, args.timeout()).await;
        if result.status == CommandStatus::TimedOut {
            warn!(target: "tool-elevate", command = %args.command, "Command timed out.");
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rig_core::tool::Tool;

    use super::*;

    #[test]
    fn command_args_accept_aliases() {
        let args: CommandArgs =
            serde_json::from_str(r#"{"cmd":"printf ok","timeout":12}"#).unwrap();
        assert_eq!(args.command, "printf ok");
        assert_eq!(args.timeout_secs, Some(12));
    }

    #[test]
    fn command_timeout_uses_default_and_maximum() {
        let default = CommandArgs {
            command: "true".into(),
            timeout_secs: None,
        };
        let maximum = CommandArgs {
            command: "true".into(),
            timeout_secs: Some(MAX_TOOL_TIMEOUT_SECS + 1),
        };
        assert_eq!(default.timeout(), Duration::from_secs(DEFAULT_TOOL_TIMEOUT_SECS));
        assert_eq!(maximum.timeout(), Duration::from_secs(MAX_TOOL_TIMEOUT_SECS));
    }

    #[tokio::test]
    async fn generated_schema_matches_command_args() {
        let definition = Explore::new(PathBuf::from("/bin/sh"))
            .definition(String::new())
            .await;
        let required = definition.parameters["required"].as_array().unwrap();
        assert!(required.iter().any(|field| field == "command"));
        assert!(!required.iter().any(|field| field == "timeout_secs"));
        assert!(definition.parameters["properties"]["command"].is_object());
    }

    #[tokio::test]
    async fn command_timeout_is_structured() {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("sleep 1");
        let result = run_command(command, Duration::from_millis(5)).await;
        assert_eq!(result.status, CommandStatus::TimedOut);
        assert_eq!(result.exit_code, None);
    }

    #[tokio::test]
    async fn nonzero_exit_is_failed() {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("printf failure >&2; exit 7");
        let result = run_command(command, Duration::from_secs(1)).await;
        assert_eq!(result.status, CommandStatus::Failed);
        assert_eq!(result.exit_code, Some(7));
        assert_eq!(result.stderr, "failure");
    }

    #[test]
    fn truncation_keeps_head_and_tail() {
        let source = (0..700)
            .map(|index| format!("line-{index}\n"))
            .collect::<String>();
        let (output, truncated) = truncate_stream(source.as_bytes());
        assert!(truncated);
        assert!(output.starts_with("line-0\n"));
        assert!(output.ends_with("line-699\n"));
        assert!(output.contains("omitted 200 lines"));
        assert!(!output.contains("line-349\n"));
    }

    #[test]
    fn truncation_obeys_byte_limit() {
        let source = "x".repeat(MAX_OUTPUT_BYTES * 2);
        let (output, truncated) = truncate_stream(source.as_bytes());
        assert!(truncated);
        assert!(output.len() <= MAX_OUTPUT_BYTES);
        assert!(output.contains("omitted 0 lines"));
    }

    #[test]
    fn command_result_serializes_the_stable_contract() {
        let result = CommandResult::rejected("no".to_string(), Instant::now());
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["status"], "rejected");
        assert!(value["exit_code"].is_null());
        assert!(value["stdout"].is_string());
        assert!(value["stderr"].is_string());
        assert!(value["truncated"].is_boolean());
        assert!(value["duration_ms"].is_number());
    }
}

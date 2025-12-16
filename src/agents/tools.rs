use std::{path::PathBuf, process::Stdio};

use rig::{completion::ToolDefinition, tool::Tool};
use serde_json::json;
use tokio::io::{self, AsyncReadExt};
use tracing::debug;

/// 获取 --help 内容
pub struct Help;

#[derive(serde::Deserialize)]
pub struct HelpArgs {
    /// 要执行 `--help` 的程序, 可以使用 PATH 中的程序而不提供绝对路径.
    program: PathBuf,
    /// 子命令, 比如 `git add --help` 中的 `add` 就是一个子命令, 可以添加多层的子命令,
    /// 形成类似 `program a b c --help` 的效果.
    /// 此参数可以为空.
    subcommands: Vec<String>,
    /// `--help` 中从指定行开始返回内容, 为 [`None`] 则默认为 0 行.
    start_line: Option<usize>,
    /// `--help` 中读取指定行数, 为 [`None`] 则默认为 50 行.
    read_lines: Option<usize>,
}

impl Tool for Help {
    const NAME: &'static str = "help";

    type Error = io::Error;

    type Args = HelpArgs;

    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Get help of the program, don't read too many lines at a time, \
                call this multiple times to scan for the messages you need."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "subcommands": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "description": "subcommand in a level"
                        },
                        "description": r#"Subcommands of the program, \
                            e.g.: you should give `["a", "b", "c"]` to get the help of `program a b c`. \
                            if no subcommand is needed, to get help of the program itself, you can pass []. \
                            Your query should start with [], getting the help of program itself, and then call again for the specific subcommands. \
                            When you feel you are on the wrong subcommand, you can pop a level and check other subcommands."#
                    },
                    "program": {
                        "type": "string",
                        "description": "The program path you want to get help, \
                            program in the PATH, \
                            relative path and absolute path are available."
                    },
                    "start_line": {
                        "type": "number",
                        "description": "Skip `start_line` lines, use different `start_line` to get different part of help content, default is 0.",
                    },
                    "read_lines": {
                        "type": "number",
                        "description": "Read `read_lines` lines, preventing from reading too much, default is 50, which is a reasonable value.",
                    }
                },
                "required": ["program", "subcommands"],
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
        let start_line = args.start_line.unwrap_or(0);
        let read_lines = args.read_lines.unwrap_or(50);
        Ok(format!(
            "stdout(line: {0}-{1}):\n{2}\nstderr(line: {0}-{1}):\n{3}",
            start_line,
            read_lines + start_line - 1,
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<String>(),
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<String>()
        ))
    }
}

/// 获取 man page.
pub struct Man;

#[derive(serde::Deserialize)]
pub struct ManArgs {
    /// - 1 - 用户命令 (User Commands)
    /// - 2 - 系统调用 (System Calls)
    /// - 3 - C 库函数 (C Library Functions)
    /// - 4 - 设备与驱动 (Devices and Drivers)
    /// - 5 - 文件格式 (File Formats)
    /// - 6 - 游戏 (Games)
    /// - 7 - 杂项/标准 (Miscellanea/Standards)
    /// - 8 - 系统管理命令 (System Administration)
    section: Option<usize>,
    /// 要查询的 entry 名.
    entry: String,
    /// same as: [`HelpArgs::start_line`]
    start_line: Option<usize>,
    /// same as: [`HelpArgs::read_lines`]
    read_lines: Option<usize>,
}

impl Tool for Man {
    const NAME: &'static str = "man";

    type Error = io::Error;

    type Args = ManArgs;

    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Get man page help messages, don't read too many lines at a time, \
                call this multiple times to scan for the messages you need."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entry": {
                        "type": "string",
                        "description": "Man page entry name, which should not have whitespaces, \
                            if you want to get help of the subcommand of a program, \
                            pass the program name here and check inside."
                    },
                    "section": {
                        "type": "number",
                        "description": "The section of the entry in man page, \
                        1 for User Commands, \
                        2 for System Calls, \
                        3 for C Library Functions, \
                        4 for Devices and Drivers, \
                        5 for File Formats, \
                        6 for Games, \
                        7 for Misc/Standards, \
                        8 for System Administration. \
                        This parameter is optional, you can skip this if you are not sure."
                    },
                    "start_line": {
                        "type": "number",
                        "description": "Skip `start_line` lines, use different `start_line` to get different part of man content, default is 0.",
                    },
                    "read_lines": {
                        "type": "number",
                        "description": "Read `read_lines` lines, preventing from reading too much, default is 50, which is a reasonable value.",
                    },
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
                format!(
                    "invalid entry: {}, entry should not start with \'-\' and should not contain whitespace",
                    entry
                ),
            ))?
        }
        let Ok(man_program) = which::which("man") else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "man program not found, this tool is not available now.",
            ))?
        };
        let Ok(col_program) = which::which("col") else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "col program not found, this tool is not available now.",
            ))?
        };
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
        let mut child1 = command1.spawn()?;
        command2
            .arg("-b")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child2 = command2.spawn()?;
        tokio::io::copy(
            &mut child1.stdout.take().unwrap(),
            &mut child2.stdin.take().unwrap(),
        )
        .await?;
        debug!(target: "tool-man", "Calling command {:?} | {:?}...", command1, command2);
        child2.wait().await?;
        let mut stderr = String::new();
        let mut stdout = String::new();
        child1.stderr.unwrap().read_to_string(&mut stderr).await?;
        child2.stdout.unwrap().read_to_string(&mut stdout).await?;
        let start_line = args.start_line.unwrap_or(0);
        let read_lines = args.read_lines.unwrap_or(50);
        Ok(format!(
            "stdout(line: {0}-{1}):\n{2}\nstderr(line: {0}-{1}):\n{3}",
            start_line,
            read_lines + start_line - 1,
            stdout
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<String>(),
            stderr
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<String>()
        ))
    }
}

/// 结束输出, 给定输出结果
pub struct FinishResponse;

/// 输出结果
#[derive(serde::Deserialize)]
pub struct FinishResponseArgs {
    pub results: Vec<String>,
}

impl FinishResponseArgs {
    pub fn empty() -> Self {
        Self {
            results: Vec::new(),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum NoError {}

impl Tool for FinishResponse {
    const NAME: &'static str = "finish_response";

    type Error = NoError;

    type Args = FinishResponseArgs;

    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "The mandatory tool used to finalize the interaction and present the generated answer(s) to the user. \
                Input should be a list of string segments forming the complete, final response.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "results": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "description": "One item of the response."
                        },
                        "description": "A list of string segments that collectively form the complete, \
                            formatted final answer to the user's request."
                    }
                },
                "required": ["results"],
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        // 在 stream_prompt 的输出被监听.
        Ok("ok".into())
    }
}

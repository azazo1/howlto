use std::{path::PathBuf, process::Stdio};

use rig::{completion::ToolDefinition, tool::Tool};
use serde_json::json;
use tokio::io;
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
}

impl Tool for Help {
    const NAME: &'static str = "help";

    type Error = io::Error;

    type Args = HelpArgs;

    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Get help of the program.".into(),
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
                            if no subcommand is needed, to get help of the program itself, you can pass []."#
                    },
                    "program": {
                        "type": "string",
                        "description": "The program path you want to get help, \
                            program in the PATH, \
                            relative path and absolute path are available."
                    }
                },
                "required": ["program", "subcommands"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if let Some(invalid_arg) = args
            .subcommands
            .iter()
            .find(|s| s.starts_with('-') || s.contains(char::is_whitespace))
        {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "invalid argument: {}, subcommand should not start with \'-\' and should not contain whitespace",
                    invalid_arg
                ),
            ))?
        }
        let mut command = tokio::process::Command::new(args.program);
        command
            .args(args.subcommands)
            .arg("--help")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        debug!(target: "tool-help", "Calling command {:?}...", command);
        let output = command.output().await?;
        Ok(format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
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
}

impl Tool for Man {
    const NAME: &'static str = "man";

    type Error = io::Error;

    type Args = ManArgs;

    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Get man page help messages".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entry": {
                        "type": "string",
                        "description": "Man page entry name."
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
        let mut command = tokio::process::Command::new(man_program);
        if let Some(section) = args.section {
            command.arg(section.to_string());
        }
        command
            .arg(entry)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        debug!(target: "tool-man", "Calling command {:?}...", command);
        let output = command.output().await?;
        Ok(format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
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

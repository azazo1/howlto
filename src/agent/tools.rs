use std::{io::ErrorKind, path::PathBuf, process::Stdio};

use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::json;
use tokio::io;
use tracing::debug;

fn default_start_line() -> usize {
    0
}

fn default_read_lines() -> usize {
    50
}

/// 获取 --help 内容
pub struct Help;

#[derive(serde::Deserialize)]
pub struct HelpArgs {
    /// 要执行 `--help` 的程序, 可以使用 PATH 中的程序而不提供绝对路径.
    program: PathBuf,
    /// 子命令, 比如 `git add --help` 中的 `add` 就是一个子命令, 可以添加多层的子命令,
    /// 形成类似 `program a b c --help` 的效果.
    /// 此参数可以为空.
    #[serde(default)]
    subcommands: Vec<String>,
    /// `--help` 中从指定行开始返回内容, 为 [`None`] 则默认为 0 行.
    #[serde(default = "default_start_line")]
    start_line: usize,
    /// `--help` 中读取指定行数, 为 [`None`] 则默认为 50 行.
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
                            if no subcommand is needed, to get help of the program itself, just skip this parameter. \
                            Your query should start with [] or not given, getting the help of program itself, and then call again for the specific subcommands. \
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
                        "description": "Skip `start_line` lines, if you want to scan through the content, increase this value, default is 0.",
                    },
                    "read_lines": {
                        "type": "number",
                        "description": "Read `read_lines` lines, preventing from reading too much, default is 50, which is a reasonable value. \
                            Calling with `read_lines` unchanged will not automatically scan through the content, see `start_line` instead.",
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
    #[serde(default = "default_start_line")]
    start_line: usize,
    /// same as: [`HelpArgs::read_lines`]
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
                        "description": "Skip `start_line` lines, if you want to scan through the content, increase this value, default is 0.",
                    },
                    "read_lines": {
                        "type": "number",
                        "description": "Read `read_lines` lines, preventing from reading too much, default is 50, which is a reasonable value. \
                            Calling with `read_lines` unchanged will not automatically scan through the content, see `start_line` instead.",
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

        // 这里要边调用边读取其输出, 不然过长的内容会导致子程序认为输出缓冲区满了停止输出, 进入等待, 导致死锁.
        let mut stdout = Vec::new();
        tokio::io::copy(child2.stdout.as_mut().unwrap(), &mut stdout).await?;

        child2.wait().await?;
        let start_line = args.start_line;
        let read_lines = args.read_lines;
        Ok(format!(
            "stdout(line: {0}-{1}):\n{2}\n(lines after was omitted, change arguments to check)\nstderr(line: {0}-{1}):\n{3}\n(lines after was omitted, change arguments to check)",
            start_line,
            read_lines + start_line - 1,
            String::from_utf8_lossy(&stdout)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<String>(),
            String::from_utf8_lossy(&stderr)
                .lines()
                .skip(start_line)
                .take(read_lines)
                .collect::<String>()
        ))
    }
}

/// 调用 tldr 获取帮助.
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
            description: "Get tldr (Too Long Don't Read) help.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "page": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "description": "A layer of page"
                        },
                        "description": "The page name you want to query, e.g. if you want to query git help, pass [\"git\"], if you want to query git commit, pass [\"git\", \"commit\"]."
                    }
                },
                "required": ["page"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let tldr_program = which::which("tldr").map_err(|_| {
            io::Error::new(
                ErrorKind::NotFound,
                "tldr program not found, this tool is invalid now.",
            )
        })?;
        // 尝试使用空格拆分.
        let page: Vec<_> = args
            .page
            .iter()
            .flat_map(|x| x.split_whitespace())
            .collect();

        let mut command = tokio::process::Command::new(tldr_program);
        command.args(page);

        debug!(target: "tool-tldr", "Calling command: {:?}...", command);

        let output = command.output().await?;
        Ok(format!(
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// 调用外部 thefuck 工具修复命令
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
            description: "Fix a command automatically, when you need to fix command, you should try it before fixing yourself, but be aware whether it fits user's requirement.".into(),
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
        let command = args.command;
        let Ok(thefuck_program) = which::which("thefuck") else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "thefuck program not found, this tool is not available now.",
            ))?
        };
        let output = tokio::process::Command::new(thefuck_program)
            .env("TF_SHELL", &self.shell_name)
            .env("TF_ALIAS", "fuck")
            .env("PYTHONIOENCODING", "utf-8")
            .arg(command)
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

/// 结束输出, 给定输出结果
pub struct FinishResponse;

/// 输出结果
#[derive(serde::Deserialize)]
pub struct FinishResponseArgs {
    pub results: Vec<String>,
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

#[cfg(test)]
mod test {
    use rig::tool::Tool;
    use tracing::Level;

    use crate::agent::tools::{Man, ManArgs};

    #[tokio::test]
    async fn man() {
        tracing_subscriber::fmt()
            .with_max_level(Level::DEBUG)
            .init();
        let man = Man;
        // 需要使用类似 ffmpeg 或者其他 man 内容过长的内容进行测试, 测试会不会进入死锁.
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

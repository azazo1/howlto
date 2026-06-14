use std::{io::ErrorKind, path::PathBuf, process::Stdio};

use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::json;
use tokio::io;
use tracing::debug;

use crate::agent::sandbox::{self, Sandbox};
use crate::tui::elevate;

/// 把 stdout/stderr 按行分页格式化, 复用给 [`Explore`] 与 [`Elevate`].
fn format_paged_output(
    stdout: &[u8],
    stderr: &[u8],
    start_line: usize,
    read_lines: usize,
) -> String {
    let take_lines = |src: &[u8]| {
        String::from_utf8_lossy(src)
            .lines()
            .skip(start_line)
            .take(read_lines)
            .collect::<String>()
    };
    let stdout_total = String::from_utf8_lossy(stdout).lines().count();
    let stderr_total = String::from_utf8_lossy(stderr).lines().count();
    format!(
        "stdout(line: {0}-{1} of {stdout_total}):\n{2}\n(lines after was omitted, change arguments to check)\nstderr(line: {0}-{1} of {stderr_total}):\n{3}\n(lines after was omitted, change arguments to check)",
        start_line,
        read_lines + start_line - 1,
        take_lines(stdout),
        take_lines(stderr)
    )
}

const DEFAULT_START_LINE: usize = 0;
const DEFAULT_READ_LINES: usize = 500;

fn default_start_line() -> usize {
    DEFAULT_START_LINE
}

fn default_read_lines() -> usize {
    DEFAULT_READ_LINES
}

/// 在只读沙箱中执行外部程序, 用于**采集信息**(读取帮助、列出当前目录、查询版本、检查环境等),
/// 而非仅查询帮助文档.
///
/// 不强制追加任何参数, 由调用方提供任意参数,
/// 通过系统沙箱 (macOS Seatbelt / Linux Bubblewrap) 保证无写副作用与无网络.
pub struct Explore {
    sandbox: Option<Sandbox>,
}

impl Default for Explore {
    fn default() -> Self {
        Self::new()
    }
}

impl Explore {
    /// 自动探测当前平台的沙箱后端; 若不可用则 [`Self::sandbox`] 为 [`None`],
    /// 调用工具时会返回错误, 由模型自行降级到 man/tldr/elevate.
    pub fn new() -> Self {
        Self {
            sandbox: sandbox::detect(),
        }
    }
}

#[derive(serde::Deserialize)]
pub struct ExploreArgs {
    /// 要执行的程序, 可以使用 PATH 中的程序而不提供绝对路径.
    program: PathBuf,
    /// 命令参数. 比如 `git add --help` 中的 `add --help` 就是参数.
    /// 此参数可以为空, 此时等价于直接执行 `program`.
    #[serde(default)]
    args: Vec<String>,
    /// 从指定行开始返回内容, 为 [`None`] 则默认为 [`DEFAULT_START_LINE`] 行.
    #[serde(default = "default_start_line")]
    start_line: usize,
    /// 读取指定行数, 为 [`None`] 则默认为 [`DEFAULT_READ_LINES`] 行.
    #[serde(default = "default_read_lines")]
    read_lines: usize,
}

impl Tool for Explore {
    const NAME: &'static str = "explore";

    type Error = io::Error;

    type Args = ExploreArgs;

    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Sandboxed, READ-ONLY execution of an arbitrary program to GATHER INFORMATION \
                (not limited to help text). \
                The program runs inside an OS-level sandbox that blocks ALL file writes and network access, \
                so it is safe and has no side effects. \
                Use it to: read CLI help (`--help`, `-h`, `help <sub>`), inspect the current directory \
                (e.g. `ls`, `find`, `git status`, `cat README.md`, `head package.json`), \
                query versions (`--version`), list available subcommands/plugins, \
                or run any other command whose purpose is to RETURN INFORMATION rather than to CHANGE state. \
                Writes/edits/deletes/installs/network are denied by the sandbox, so attempting them just wastes a call. \
                Don't read too many lines at a time. \
                Call this multiple times to scan for the messages you need. \
                If the sandbox backend is unavailable, this tool returns an error; \
                fall back to man/tldr or, as a last resort, elevate (which asks the user to confirm)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "program": {
                        "type": "string",
                        "description": "The program to run. May be a name in PATH, a relative path, or an absolute path."
                    },
                    "args": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "description": "One argument per item. Pass read-only / informational arguments, \
                                e.g. [\"--help\"], [\"-h\"], [\"--version\"], [\"status\"], [\"log\", \"--oneline\"], \
                                or subcommand paths like [\"add\", \"--help\"] for `git add --help`."
                        },
                        "description": "Arguments to pass to the program. Empty by default."
                    },
                    "start_line": {
                        "type": "number",
                        "description": format!("Skip `start_line` lines, if you want to scan through the content, increase this value, default is {}.", DEFAULT_START_LINE),
                    },
                    "read_lines": {
                        "type": "number",
                        "description": format!("Read `read_lines` lines, preventing from reading too much, default is {}, which is a reasonable value. \
                            Calling with `read_lines` unchanged will not automatically scan through the content, see `start_line` instead.", DEFAULT_READ_LINES),
                    }
                },
                "required": ["program"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let Some(ref sandbox) = self.sandbox else {
            return Err(io::Error::new(
                ErrorKind::NotFound,
                "no sandbox backend available on this platform; \
                 try man/tldr or elevate instead",
            ));
        };
        let mut command = sandbox.wrap(&args.program, &args.args)?;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        debug!(target: "tool-explore", sandbox = sandbox.name(), "Calling command {:?}...", command);
        let output = command.output().await?;
        Ok(format_paged_output(
            &output.stdout,
            &output.stderr,
            args.start_line,
            args.read_lines,
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
    /// same as: [`ExploreArgs::start_line`]
    #[serde(default = "default_start_line")]
    start_line: usize,
    /// same as: [`ExploreArgs::read_lines`]
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
                        "description": format!("Skip `start_line` lines, if you want to scan through the content, increase this value, default is {}.", DEFAULT_START_LINE),
                    },
                    "read_lines": {
                        "type": "number",
                        "description": format!("Read `read_lines` lines, preventing from reading too much, default is {}, which is a reasonable value. \
                            Calling with `read_lines` unchanged will not automatically scan through the content, see `start_line` instead.", DEFAULT_READ_LINES),
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

/// 结束输出, 给定输出结果.
///
/// 回答在数据结构层面就是**互斥**的两种模式之一:
/// - [`AnswerBody::Commands`]: 一组 shell 命令, 进选择框, 可被复制/执行/写入 shell 输入缓冲区.
/// - [`AnswerBody::Text`]: 单条纯文本/markdown 回答, 直接打印到终端, **不**经过选择框, **不**执行.
pub struct Answer;

/// 回答的正文.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AnswerBody {
    /// 命令模式: 一组可复制/执行的 shell 命令, 进入选择框.
    Commands {
        /// 候选命令列表.
        commands: Vec<CommandItem>,
    },
    /// 文本模式: 单条 markdown/纯文本解释, 直接展示, 不执行, 不进选择框.
    /// 当无法用单一命令回答 (解释、多步骤说明、对比等) 时使用.
    Text {
        /// markdown/纯文本内容.
        content: String,
    },
}

/// 单条 shell 命令项 (仅用于 [`AnswerBody::Commands`]).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CommandItem {
    pub content: String,
    /// 命令项的简短描述 (几个词, 与其它候选区分).
    #[serde(default)]
    pub desc: String,
}

/// [`Answer`] 工具的参数: 一条互斥模式的回答.
#[derive(serde::Deserialize)]
pub struct AnswerArgs {
    pub answer: AnswerBody,
}

#[derive(thiserror::Error, Debug)]
pub enum NoError {}

impl Tool for Answer {
    const NAME: &'static str = "answer";

    type Error = NoError;

    type Args = AnswerArgs;

    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "The mandatory tool used to finalize the interaction and present the answer to the user. \
                The `answer` field is an EXCLUSIVE CHOICE between two modes (set `mode` to pick one): \
                `commands` (a list of shell commands) \
                OR `text` (a single markdown/plain-text). \
                Prefer `commands` whenever a command can answer the question."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "answer": {
                        "type": "object",
                        "description": "The final answer.",
                        "oneOf": [
                            {
                                "type": "object",
                                "description": "Command mode: a list of candidate shell commands. These are shown in a selection UI and may be executed/copied.",
                                "properties": {
                                    "mode": { "type": "string", "const": "commands" },
                                    "commands": {
                                        "type": "array",
                                        "description": "Candidate shell commands. Each must be a single syntactically valid shell command.",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "content": {
                                                    "type": "string",
                                                    "description": "A single syntactically valid shell command suitable for direct execution on the target shell/OS. \
                                                        No markdown, no quoting the whole command with `` '' or \"\"."
                                                },
                                                "desc": {
                                                    "type": "string",
                                                    "description": "A short description (a few words) distinguishing this command from the other candidates. REQUIRED in command mode."
                                                }
                                            },
                                            "required": ["content", "desc"]
                                        }
                                    }
                                },
                                "required": ["mode", "commands"]
                            },
                            {
                                "type": "object",
                                "description": "Text mode: a single markdown text shown directly to the user.",
                                "properties": {
                                    "mode": { "type": "string", "const": "text" },
                                    "content": {
                                        "type": "string",
                                        "description": "markdown/plain-text."
                                    }
                                },
                                "required": ["mode", "content"]
                            }
                        ]
                    }
                },
                "required": ["answer"],
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        // 在 stream_prompt 的输出被监听.
        Ok("ok".into())
    }
}

/// 执行**任意**程序, 不做沙箱限制, 但每次执行前都会弹出 TUI 让用户**确认**.
///
/// 适用于 [`Explore`] (沙箱只读) 无法完成的场景:
/// 命令本身需要写文件、联网、或者会改变系统状态,
/// 例如 `git clone`、`npm view <pkg>`(需联网)、`make`(需写构建产物) 等.
///
/// 名称中的 "elevate" 表示: 相较沙箱只读的 [`Explore`], 这里相当于获得了用户的**提权授权**.
pub struct Elevate;

#[derive(serde::Deserialize)]
pub struct ElevateArgs {
    /// 要执行的程序, 可以使用 PATH 中的程序而不提供绝对路径.
    program: PathBuf,
    /// 命令参数
    #[serde(default)]
    args: Vec<String>,
    /// 指定行开始返回内容, 为 [`None`] 则默认为 [`DEFAULT_START_LINE`] 行.
    #[serde(default = "default_start_line")]
    start_line: usize,
    /// 读取指定行数, 为 [`None`] 则默认为 [`DEFAULT_READ_LINES`] 行.
    #[serde(default = "default_read_lines")]
    read_lines: usize,
}

impl Tool for Elevate {
    const NAME: &'static str = "elevate";

    type Error = io::Error;

    type Args = ElevateArgs;

    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Execute ANY program with full privileges (writes, network, side effects all allowed), \
                BUT each call first pops up a TUI asking the user to APPROVE the exact command. \
                Use it ONLY when explore (sandboxed read-only) cannot do the job, \
                e.g. you genuinely need to write a file, reach the network, or run a command that mutates state \
                in order to gather the information you need. \
                Prefer explore whenever the operation is read-only. \
                If the user rejects the command, the tool returns their rejection reason; \
                do not retry the same command, try a different approach or give up gracefully. \
                Don't read too many lines at a time, call this multiple times to scan for the messages you need."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "program": {
                        "type": "string",
                        "description": "The program to run. May be a name in PATH, a relative path, or an absolute path."
                    },
                    "args": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "description": "One argument per item."
                        },
                        "description": "Arguments to pass to the program. Empty by default."
                    },
                    "start_line": {
                        "type": "number",
                        "description": format!("Skip `start_line` lines, if you want to scan through the content, increase this value, default is {}.", DEFAULT_START_LINE),
                    },
                    "read_lines": {
                        "type": "number",
                        "description": format!("Read `read_lines` lines, preventing from reading too much, default is {}, which is a reasonable value. \
                            Calling with `read_lines` unchanged will not automatically scan through the content, see `start_line` instead.", DEFAULT_READ_LINES),
                    }
                },
                "required": ["program"],
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        elevate::confirm_elevate(&args.program, &args.args)
            .await
            .map_err(io::Error::other)?;
        let mut command = tokio::process::Command::new(args.program);
        command
            .args(args.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        debug!(target: "tool-elevate", "Calling command {:?}...", command);
        let output = command.output().await?;
        Ok(format_paged_output(
            &output.stdout,
            &output.stderr,
            args.start_line,
            args.read_lines,
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

    #[test]
    fn answer_args_parses_commands_mode() {
        use crate::agent::tools::{AnswerArgs, AnswerBody};
        let args: AnswerArgs = serde_json::from_str(
            r#"{"answer":{"mode":"commands","commands":[
                {"content":"ls -la","desc":"list"},
                {"content":"ls","desc":"brief"}
            ]}}"#,
        )
        .unwrap();
        let AnswerBody::Commands { commands } = args.answer else {
            panic!("expected commands mode");
        };
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].content, "ls -la");
        assert_eq!(commands[0].desc, "list");
    }

    #[test]
    fn answer_args_parses_text_mode() {
        use crate::agent::tools::{AnswerArgs, AnswerBody};
        let args: AnswerArgs =
            serde_json::from_str(r##"{"answer":{"mode":"text","content":"# heading"}}"##).unwrap();
        let AnswerBody::Text { content } = args.answer else {
            panic!("expected text mode");
        };
        assert_eq!(content, "# heading");
    }

    #[test]
    fn command_item_desc_defaults_to_empty() {
        use crate::agent::tools::CommandItem;
        // 缺省 desc 时默认为空字符串.
        let item: CommandItem = serde_json::from_str(r#"{"content":"ls"}"#).unwrap();
        assert_eq!(item.content, "ls");
        assert!(item.desc.is_empty(), "desc should default to empty");
    }
}

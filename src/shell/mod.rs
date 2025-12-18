use std::{
    path::{Path, PathBuf},
    str::FromStr,
};
use sysinfo::{ProcessRefreshKind, RefreshKind, System, get_current_pid};
use tracing::debug;

mod fish;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Integration {
    Fish,
    Bash,
    Zsh,
    Nushell,
    Cmd,
    PowerShell,
    Pwsh,
    Elvish,
}

impl FromStr for Integration {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use Integration::*;
        match s {
            "fish" => Ok(Fish),
            // todo 增加其他 shell 的支持
            "bash" => Ok(Fish),
            "zsh" => Ok(Zsh),
            _ => Err(()),
        }
    }
}

impl Integration {
    fn init(self) -> String {
        match self {
            Self::Fish => fish::script_init(),
            Self::Bash => todo!(),
            Self::Zsh => todo!(),
            Self::Nushell => todo!(),
            Self::Cmd => todo!(),
            Self::PowerShell => todo!(),
            Self::Pwsh => todo!(),
            Self::Elvish => todo!(),
        }
    }
}

#[derive(Debug)]
pub struct Shell {
    name: String,
    path: PathBuf,
    integration: Option<Integration>,
}

const SHELLS: &[&str] = &[
    // ------------------------------------
    // Unix/Linux 常用和经典 Shells (补充了 ksh 的实现)
    "sh",    // Bourne Shell (经典，许多现代 Shell 的基础)
    "bash",  // Bourne-Again Shell (最常用)
    "zsh",   // Z Shell (功能强大，插件丰富)
    "ksh",   // Korn Shell (功能全面，兼容 sh)
    "csh",   // C Shell (语法类似 C 语言)
    "tcsh",  // Enhanced C Shell
    "dash",  // Debian Almquist Shell (轻量级，常作为 /bin/sh)
    "fish",  // Friendly Interactive Shell (交互友好)
    "pdksh", // Public Domain Korn Shell (ksh 的早期和替代实现)
    // ------------------------------------
    // Windows 及其跨平台 Shells (已包含)
    "cmd",        // Command Prompt (Windows 传统命令行)
    "powershell", // Windows PowerShell (基于 .NET，处理对象)
    "pwsh",       // PowerShell Core (跨平台版本)
    // ------------------------------------
    // 嵌入式和特殊 Shells (补充了 busybox)
    "ash",     // Almquist Shell (轻量级，常用于嵌入式系统)
    "busybox", // BusyBox (集成工具套件中的 sh/ash 实现)
    "rc",      // Bell Labs Shell (Plan 9 操作系统的默认 Shell)
    "es",      // Extensible Shell
    "scsh",    // Scheme Shell (使用 Scheme 语言作为脚本语言)
    // ------------------------------------
    // 其他和较不常见的 Shells
    "yash", // Yet Another Shell
    "sash", // Standalone Shell (常用于系统恢复)
    "ion",  // Ion Shell (受 fish 启发，使用 Rust 编写)
    // 现代和创新 Shells (新增)
    "nushell", // NuShell / Nush (使用 Rust 编写，核心概念是处理结构化数据而非原始文本)
    "oil",     // Oil Shell (旨在兼容 Bash 并提供更健壮的脚本语言 YSH)
    "elvish",  // Elvish Shell (具有独特的设计哲学，强调可编程性和交互式界面的分离)
    "mksh",    // MirBSD Korn Shell (KSH 的一个活跃分支，注重可移植性)
    "wish",    // Windowing Shell (基于 Tcl/Tk 的图形化 Shell)
];

fn is_known_shell(s: &str) -> bool {
    for shell in SHELLS {
        if s.contains(shell) {
            return true;
        }
    }
    false
}

#[bon::bon]
impl Shell {
    #[builder]
    fn new(name: String, path: PathBuf, integration: Option<Integration>) -> Self {
        Self {
            name,
            path,
            integration,
        }
    }
}

impl Shell {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 获取当前 shell 的字符串表示和可执行文件路径.
    pub fn detect_shell() -> Shell {
        let default_shell_path: PathBuf = if cfg!(unix) {
            std::env::var("SHELL").unwrap_or("/bin/sh".into()).into()
        } else if cfg!(windows) {
            which::which("cmd.exe").unwrap_or("C:/Windows/System32/cmd.exe".into())
        } else {
            PathBuf::new()
        };
        let default_shell_name = default_shell_path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("unknown");

        macro_rules! fall_back_to_unknown {
            ($e:expr) => {{
                let Some(x) = $e else {
                    debug!("detect shell failed, fallback to default shell.");
                    return Shell::builder()
                        .name(default_shell_name.to_string())
                        .path(default_shell_path.clone())
                        .build();
                };
                x
            }};
        }

        let system = System::new_with_specifics(
            RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
        );
        let mut pid = fall_back_to_unknown!(get_current_pid().ok());
        loop {
            let cur_proc = fall_back_to_unknown!(system.process(pid));
            let parent_pid = fall_back_to_unknown!(cur_proc.parent());
            let parent = fall_back_to_unknown!(system.process(parent_pid));
            debug!("detecting shell proc: {:?}", parent.name());
            pid = parent_pid;
            let Some(name) = parent.name().to_str() else {
                continue;
            };
            if !is_known_shell(name) {
                continue;
            }
            break Shell::builder()
                .maybe_integration(name.parse().ok())
                .name(name.into())
                .path(parent.exe().unwrap_or(Path::new("")).into())
                .build();
        }
    }
}

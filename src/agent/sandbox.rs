//! 系统级只读沙箱后端.
//!
//! 用于安全地执行 agent 触发的外部程序 (主要是 `Explore` 工具),
//! 避免具有副作用的命令在静默状态下造成破坏 (e.g. `mkdir --help` 真的去创建目录).
//!
//! 安全策略统一为: **只读 + 禁网**.
//! - macOS: Seatbelt (`sandbox-exec`).
//! - Linux: Bubblewrap (`bwrap`).
//!
//! 不使用 trait object (`dyn`), 也不使用枚举, 直接通过条件编译让 [`Sandbox`]
//! 在不同平台持有不同字段与实现. 不支持的平台直接编译失败
//! (见下方的 [`compile_error!`]).

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("sandbox backend is only supported on macOS (Seatbelt) and Linux (Bubblewrap)");

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use tokio::process::Command;

/// 纯只读 + 禁网的 Seatbelt profile.
///
/// 注意: `(deny ...)` 必须在 `(allow default)` 之前, 否则 default 会先放行.
#[cfg(target_os = "macos")]
const SEATBELT_PROFILE: &str = "\
(version 1)\n\
 (deny file-write*)\n\
 (deny network*)\n\
 (allow default)\n";

/// 系统沙箱后端.
///
/// 字段在不同平台下不同 (条件编译), 调用方无需关心具体平台, 只需通过
/// [`Sandbox::wrap`] 包装命令, [`detect`] 探测可用性.
#[derive(Debug)]
pub struct Sandbox {
    #[cfg(target_os = "macos")]
    sandbox_exec: PathBuf,
    #[cfg(target_os = "linux")]
    bwrap: PathBuf,
}

impl Sandbox {
    /// 包装命令, 使其在只读 + 禁网的沙箱里执行.
    pub fn wrap(&self, program: &Path, args: &[String]) -> io::Result<Command> {
        wrap_impl(self, program, args)
    }

    /// 后端的人类可读名称, 用于日志.
    pub fn name(&self) -> &'static str {
        name_impl(self)
    }
}

// ---------------------------------------------------------------------------
// macOS: Seatbelt (sandbox-exec)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn wrap_impl(sb: &Sandbox, program: &Path, args: &[String]) -> io::Result<Command> {
    let mut command = Command::new(&sb.sandbox_exec);
    command.arg("-p").arg(SEATBELT_PROFILE).arg(program);
    command.args(args);
    Ok(command)
}

#[cfg(target_os = "macos")]
fn name_impl(_sb: &Sandbox) -> &'static str {
    "seatbelt"
}

// ---------------------------------------------------------------------------
// Linux: Bubblewrap (bwrap)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn wrap_impl(sb: &Sandbox, program: &Path, args: &[String]) -> io::Result<Command> {
    let mut command = Command::new(&sb.bwrap);
    command
        // 以只读方式挂载根文件系统, 保证无写副作用.
        .arg("--ro-bind")
        .arg("/")
        .arg("/")
        // 提供 /dev /proc, 保证程序能基本启动与读取自身信息.
        .arg("--dev")
        .arg("/dev")
        .arg("--proc")
        .arg("/proc")
        // 隔离一切命名空间, 其中包含 --unshare-net 实现禁网.
        .arg("--unshare-all")
        // 与父进程生命周期绑定, 防止僵尸进程.
        .arg("--die-with-parent");
    command.arg(program);
    command.args(args);
    Ok(command)
}

#[cfg(target_os = "linux")]
fn name_impl(_sb: &Sandbox) -> &'static str {
    "bubblewrap"
}

// ---------------------------------------------------------------------------
// 自动探测
// ---------------------------------------------------------------------------

fn which<S: AsRef<OsStr>>(name: S) -> Option<PathBuf> {
    which::which(name).ok()
}

/// 根据当前平台与可用二进制自动探测沙箱后端.
///
/// - macOS: 优先返回基于 `sandbox-exec` 的 [`Sandbox`].
/// - Linux: 优先返回基于 `bwrap` 的 [`Sandbox`].
///
/// 返回 [`None`] 仅表示对应二进制未安装 (运行时探测失败),
/// 不支持的平台会在编译期由 `compile_error!` 拦截, 不会走到这里.
#[cfg(target_os = "macos")]
pub fn detect() -> Option<Sandbox> {
    let sandbox_exec = which("sandbox-exec")?;
    Some(Sandbox { sandbox_exec })
}

#[cfg(target_os = "linux")]
pub fn detect() -> Option<Sandbox> {
    let bwrap = which("bwrap")?;
    Some(Sandbox { bwrap })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn seatbelt_denies_writes_allows_reads() {
        use std::process::Stdio;
        let sb = detect().expect("macos should have seatbelt backend");
        assert_eq!(sb.name(), "seatbelt");
        // 写文件应被拒.
        let tmp = format!("/tmp/howlto_sb_test_{}", std::process::id());
        let mut cmd = sb
            .wrap(
                Path::new("/bin/sh"),
                &["-c".into(), format!("echo x > {tmp}")],
            )
            .unwrap();
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = cmd.output().await.unwrap();
        assert!(
            !out.status.success(),
            "write inside seatbelt sandbox should fail, status={:?}",
            out.status
        );
        assert!(
            !Path::new(&tmp).exists(),
            "file should not have been created"
        );
        // 读应正常.
        let mut cmd = sb
            .wrap(Path::new("/bin/echo"), &["hi-from-seatbelt".into()])
            .unwrap();
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = cmd.output().await.unwrap();
        assert!(out.status.success(), "echo should succeed in sandbox");
        assert!(String::from_utf8_lossy(&out.stdout).contains("hi-from-seatbelt"));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn bubblewrap_denies_writes_allows_reads() {
        use std::process::Stdio;
        let Some(sb) = detect() else {
            return; // 当前环境没装 bwrap, 跳过.
        };
        assert_eq!(sb.name(), "bubblewrap");
        let tmp = format!("/tmp/howlto_bwrap_test_{}", std::process::id());
        let mut cmd = sb
            .wrap(
                Path::new("/bin/sh"),
                &["-c".into(), format!("echo x > {tmp}")],
            )
            .unwrap();
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = cmd.output().await.unwrap();
        assert!(
            !out.status.success(),
            "write inside bubblewrap sandbox should fail, status={:?}",
            out.status
        );
        assert!(
            !Path::new(&tmp).exists(),
            "file should not have been created"
        );
        let mut cmd = sb
            .wrap(Path::new("/bin/echo"), &["hi-from-bwrap".into()])
            .unwrap();
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = cmd.output().await.unwrap();
        assert!(out.status.success(), "echo should succeed in sandbox");
        assert!(String::from_utf8_lossy(&out.stdout).contains("hi-from-bwrap"));
    }
}

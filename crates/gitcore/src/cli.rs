//! 写路径:调用系统 git。凭据、签名、hooks 都由用户自己的 git 配置决定。

use crate::{Error, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 取消标记。UI 点“取消”时置位,正在运行的 git 进程会被结束。
#[derive(Clone, Default, Debug)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl GitVersion {
    /// 依赖 `git switch/restore`、`--force-with-lease` 等能力的最低版本。
    pub const MIN_SUPPORTED: GitVersion = GitVersion {
        major: 2,
        minor: 30,
        patch: 0,
    };

    pub fn parse(s: &str) -> Option<Self> {
        // "git version 2.39.3 (Apple Git-146)"
        let ver = s.split_whitespace().nth(2)?;
        let mut it = ver.split('.').map(|p| {
            p.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .ok()
        });
        Some(Self {
            major: it.next()??,
            minor: it.next().flatten().unwrap_or(0),
            patch: it.next().flatten().unwrap_or(0),
        })
    }
}

impl std::fmt::Display for GitVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// 在某个工作区里执行 git 命令。
#[derive(Clone, Debug)]
pub struct Git {
    workdir: PathBuf,
    program: PathBuf,
}

impl Git {
    pub fn new(workdir: impl Into<PathBuf>) -> Self {
        Self {
            workdir: workdir.into(),
            program: PathBuf::from("git"),
        }
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// 检测系统 git 版本。
    pub fn version() -> Result<GitVersion> {
        let out = Command::new("git")
            .arg("--version")
            .output()
            .map_err(|_| Error::GitNotFound)?;
        let s = String::from_utf8_lossy(&out.stdout);
        GitVersion::parse(&s).ok_or_else(|| Error::other(format!("无法识别 git 版本:{s}")))
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(args)
            .current_dir(&self.workdir)
            // 没有终端可以输入密码,凭据必须来自凭据助手(macOS 钥匙串)
            .env("GIT_TERMINAL_PROMPT", "0")
            // 编辑器不会弹出:rebase/merge 的提交信息由我们提供
            .env("GIT_EDITOR", "true")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// 执行并返回 stdout;非零退出码视为错误。
    pub fn run(&self, args: &[&str]) -> Result<String> {
        self.run_with(args, None, &Cancel::new())
    }

    /// 执行,可传入 stdin 内容与取消标记。
    pub fn run_with(&self, args: &[&str], stdin: Option<&[u8]>, cancel: &Cancel) -> Result<String> {
        log::debug!("git {}", args.join(" "));
        let mut cmd = self.command(args);
        if stdin.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::GitNotFound,
            _ => Error::Io(e),
        })?;

        if let Some(input) = stdin {
            let mut pipe = child.stdin.take().expect("stdin piped");
            let input = input.to_vec();
            std::thread::spawn(move || {
                let _ = pipe.write_all(&input);
            });
        }

        // stdout/stderr 在独立线程读取,避免管道写满导致死锁
        let mut stdout = child.stdout.take().expect("stdout piped");
        let mut stderr = child.stderr.take().expect("stderr piped");
        let out_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            buf
        });
        let err_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf);
            buf
        });

        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if cancel.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(15));
        };

        let out = out_thread.join().unwrap_or_default();
        let err = err_thread.join().unwrap_or_default();
        if status.success() {
            Ok(String::from_utf8_lossy(&out).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&err).trim().to_string();
            let stdout = String::from_utf8_lossy(&out).trim().to_string();
            Err(Error::Cli {
                args: args.join(" "),
                code: status.code(),
                stderr: if stderr.is_empty() { stdout } else { stderr },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_versions() {
        assert_eq!(
            GitVersion::parse("git version 2.39.3 (Apple Git-146)"),
            Some(GitVersion {
                major: 2,
                minor: 39,
                patch: 3
            })
        );
        assert_eq!(
            GitVersion::parse("git version 2.45.0.windows.1").map(|v| v.minor),
            Some(45)
        );
        assert!(GitVersion::parse("nonsense").is_none());
        assert!(GitVersion::parse("git version 2.29.0").unwrap() < GitVersion::MIN_SUPPORTED);
    }
}

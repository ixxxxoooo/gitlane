//! 更新安装:下载 dmg → 校验 sha256 → 挂载取出 app 暂存 → 退出后由脚本替换并重启。
//!
//! 暂存目录由调用方传入(生产环境用 [`staging_root`]),测试用临时目录,不碰真实的应用支持目录。
//!
//! @author ixxxxoooo

use super::{UpdateInfo, is_newer};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 暂存好的更新(已下载并校验)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedUpdate {
    pub version: String,
    pub build: u64,
    /// 暂存的 app 路径。
    pub app_path: String,
    pub notes: Option<String>,
}

/// 下载进度:下载 → 校验 → 解包验签三段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Progress {
    /// 已收到的字节数与总字节数(发布源没给长度时为 `None`)。
    Downloading { received: u64, total: Option<u64> },
    /// 正在校验 sha256。
    Verifying,
    /// 正在解包并验证签名。
    #[default]
    Unpacking,
}

impl Progress {
    /// 简短文字:`42%` / `已下载 34.2 MB` / `校验中` / `解包中`。
    pub fn short(&self) -> String {
        match *self {
            Progress::Downloading {
                received,
                total: Some(t),
            } if t > 0 => {
                format!("{}%", (received * 100 / t).min(100))
            }
            Progress::Downloading { received, .. } => format!("已下载 {}", format_bytes(received)),
            Progress::Verifying => "校验中".into(),
            Progress::Unpacking => "解包中".into(),
        }
    }
}

/// 字节数的可读文字(1024 进制,一位小数):`812 B` / `34.2 KB` / `82.0 MB`。
pub fn format_bytes(n: u64) -> String {
    const K: f64 = 1024.;
    let n = n as f64;
    if n < K {
        format!("{n:.0} B")
    } else if n < K * K {
        format!("{:.1} KB", n / K)
    } else {
        format!("{:.1} MB", n / K / K)
    }
}

/// 生产环境的暂存根目录 `~/Library/Application Support/Gitlane/updates`。
pub fn staging_root() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join("Library/Application Support/Gitlane/updates")
}

/// 读取已暂存且比当前版本新的更新;过期的暂存会被清理。
///
/// 参数:`root` 暂存根目录;`current` 当前版本;`build` 当前构建号。
pub fn load_staged(root: &Path, current: &str, build: u64) -> Option<StagedUpdate> {
    let staged: StagedUpdate =
        serde_json::from_slice(&std::fs::read(root.join("ready.json")).ok()?).ok()?;
    if !Path::new(&staged.app_path).exists()
        || !is_newer(&staged.version, staged.build, current, build)
    {
        let _ = std::fs::remove_dir_all(root);
        return None;
    }
    Some(staged)
}

/// 文件的 sha256(小写十六进制),调用系统 `shasum`。
fn sha256(file: &Path) -> Result<String, String> {
    let out = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(file)
        .output()
        .map_err(|e| format!("无法计算校验值:{e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_lowercase)
        .ok_or_else(|| "无法计算校验值".to_string())
}

/// 运行系统命令,失败时返回 stderr。
fn run(tool: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(tool)
        .args(args)
        .output()
        .map_err(|e| format!("{tool}:{e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{tool} 失败:{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// 用 curl 下载到 `dest`,下载期间轮询文件大小来报告进度(每 150 ms 一次)。
fn fetch(
    url: &str,
    dest: &Path,
    total: Option<u64>,
    on: &mut dyn FnMut(Progress),
) -> Result<(), String> {
    let mut child = Command::new("/usr/bin/curl")
        .args(["-fsSL", "--max-time", "900", "-o"])
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法运行 curl:{e}"))?;
    let mut last = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| format!("下载中断:{e}"))? {
            break s;
        }
        if last.elapsed() >= Duration::from_millis(150) {
            last = Instant::now();
            let received = std::fs::metadata(dest).map_or(0, |m| m.len());
            on(Progress::Downloading { received, total });
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if status.success() {
        return Ok(());
    }
    let mut err = String::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut s, &mut err);
    }
    Err(format!("下载失败:{}", err.trim()))
}

/// 下载并校验、暂存更新(阻塞,界面在后台线程调用)。
///
/// 参数:`root` 暂存根目录;`info` 版本信息(必须带 https 地址与 sha256);`on` 进度回调。
/// 返回:暂存结果或错误提示。
pub fn download(
    root: &Path,
    info: &UpdateInfo,
    on: &mut dyn FnMut(Progress),
) -> Result<StagedUpdate, String> {
    if !info.url.starts_with("https://") {
        return Err("下载地址必须是 https".into());
    }
    let expected = info
        .sha256
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .ok_or("发布源缺少安装包校验值(sha256),已拒绝安装")?;
    let _ = std::fs::remove_dir_all(root);
    let dir = root.join(&info.version);
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建暂存目录:{e}"))?;
    let dmg = dir.join("update.dmg");
    on(Progress::Downloading {
        received: 0,
        total: info.size,
    });
    let result = stage(root, &dir, &dmg, &expected, info, on);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(root);
    }
    result
}

/// `download` 的主体:下载 → 校验 → 解包验签 → 写 `ready.json`。出错由调用方清理暂存。
fn stage(
    root: &Path,
    dir: &Path,
    dmg: &Path,
    expected: &str,
    info: &UpdateInfo,
    on: &mut dyn FnMut(Progress),
) -> Result<StagedUpdate, String> {
    fetch(&info.url, dmg, info.size, on)?;
    on(Progress::Verifying);
    if sha256(dmg)? != expected {
        return Err("安装包校验失败(sha256 不一致)".into());
    }
    on(Progress::Unpacking);
    let app = extract_app(dmg, dir)?;
    let _ = std::fs::remove_file(dmg);
    run(
        "/usr/bin/codesign",
        &["--verify", "--deep", "--strict", &app.to_string_lossy()],
    )
    .map_err(|e| format!("安装包签名无效:{e}"))?;
    let staged = StagedUpdate {
        version: info.version.clone(),
        build: info.build.unwrap_or(0),
        app_path: app.to_string_lossy().to_string(),
        notes: info.notes.clone(),
    };
    let json = serde_json::to_vec(&staged).map_err(|e| e.to_string())?;
    std::fs::write(root.join("ready.json"), json).map_err(|e| format!("无法记录暂存:{e}"))?;
    Ok(staged)
}

/// 挂载 dmg,把其中的 .app 拷到暂存目录后卸载。
fn extract_app(dmg: &Path, dir: &Path) -> Result<PathBuf, String> {
    let mount = dir.join("mnt");
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    run(
        "/usr/bin/hdiutil",
        &[
            "attach",
            "-nobrowse",
            "-readonly",
            "-noverify",
            "-mountpoint",
            &mount.to_string_lossy(),
            &dmg.to_string_lossy(),
        ],
    )?;
    let result = (|| {
        let name = std::fs::read_dir(&mount)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .find(|n| n.ends_with(".app"))
            .ok_or("安装包中没有应用")?;
        let dest = dir.join("Gitlane.app");
        run(
            "/usr/bin/ditto",
            &[&mount.join(name).to_string_lossy(), &dest.to_string_lossy()],
        )?;
        Ok(dest)
    })();
    let _ = run(
        "/usr/bin/hdiutil",
        &["detach", "-quiet", "-force", &mount.to_string_lossy()],
    );
    let _ = std::fs::remove_dir_all(&mount);
    result
}

#[cfg(test)]
#[path = "install_tests.rs"]
mod tests;

//! 作者头像:按提交邮箱联网查询,缓存到本地。
//!
//! 来源按顺序尝试:
//! 1. GitHub 的 noreply 邮箱(`123+name@users.noreply.github.com`)能直接得到账号名,取 `github.com/<账号>.png`;
//! 2. 其余邮箱走 Gravatar(用邮箱的 SHA-256,`d=404` 没有头像时返回 404,不会拿到默认图)。
//!
//! 联网走系统 `curl`,哈希走系统 `shasum`,与更新、写操作同一思路,不引入 HTTP/哈希依赖。
//! 查不到的邮箱写一个 `.none` 标记,一天内不再重试。
//!
//! @author ixxxxoooo

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// 查不到头像的邮箱多久后再重试。
const RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// 缓存目录 `~/Library/Caches/Gitlane/avatars`。
pub fn cache_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join("Library/Caches/Gitlane/avatars")
}

/// GitHub noreply 邮箱里的账号名;不是 noreply 邮箱返回 `None`。
///
/// 参数:`email` 提交邮箱。两种格式:`12345+name@users.noreply.github.com` 与 `name@users.noreply.github.com`。
pub fn github_login(email: &str) -> Option<String> {
    let local = email
        .trim()
        .to_lowercase()
        .strip_suffix("@users.noreply.github.com")?
        .to_string();
    let login = local.rsplit_once('+').map_or(local.as_str(), |(_, l)| l);
    (!login.is_empty() && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .then(|| login.to_string())
}

/// 文本的 sha256(小写十六进制),调用系统 `shasum`。
fn sha256(text: &str) -> Option<String> {
    let mut child = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(text.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .map(str::to_lowercase)
}

/// 邮箱对应的候选头像地址,按优先级排序。
pub fn candidate_urls(email: &str) -> Vec<String> {
    let email = email.trim().to_lowercase();
    let mut urls = Vec::new();
    if let Some(login) = github_login(&email) {
        urls.push(format!("https://github.com/{login}.png?size=64"));
    }
    if let Some(hash) = sha256(&email) {
        urls.push(format!("https://gravatar.com/avatar/{hash}?s=64&d=404"));
    }
    urls
}

/// 缓存文件名的前缀:邮箱小写后的 sha256,避免把邮箱直接写进文件名。
fn key(email: &str) -> Option<String> {
    sha256(&email.trim().to_lowercase())
}

/// 读取缓存里的头像,不联网。有头像返回 `Some(Some(路径))`;确认没有(且还没到重试时间)返回
/// `Some(None)`;没有任何缓存返回 `None`。
pub fn cached(dir: &Path, email: &str) -> Option<Option<PathBuf>> {
    let key = key(email)?;
    let file = dir.join(format!("{key}.img"));
    if file.exists() {
        return Some(Some(file));
    }
    let age = std::fs::metadata(dir.join(format!("{key}.none")))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())?;
    (age < RETRY_AFTER).then_some(None)
}

/// 取头像:先看缓存,没有再联网依次尝试候选地址(阻塞,界面在后台线程调用)。
/// 返回头像文件路径;查不到返回 `None`,并留下标记避免反复请求。
pub fn fetch(dir: &Path, email: &str) -> Option<PathBuf> {
    if let Some(hit) = cached(dir, email) {
        return hit;
    }
    std::fs::create_dir_all(dir).ok()?;
    let key = key(email)?;
    let file = dir.join(format!("{key}.img"));
    let tmp = dir.join(format!("{key}.tmp"));
    for url in candidate_urls(email) {
        let ok = Command::new("/usr/bin/curl")
            .args(["-fsSL", "--max-time", "8", "-o"])
            .arg(&tmp)
            .arg(&url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok && std::fs::metadata(&tmp).is_ok_and(|m| m.len() > 0) {
            std::fs::rename(&tmp, &file).ok()?;
            return Some(file);
        }
    }
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::write(dir.join(format!("{key}.none")), b"");
    None
}

#[cfg(test)]
#[path = "avatar_tests.rs"]
mod tests;

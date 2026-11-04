//! 应用更新:读取发布源(GitHub Release 附带的 `appcast.json`)并与当前版本比较;
//! 下载、校验、暂存与替换见 `install`。
//!
//! 联网一律走系统自带的 `curl`(与写操作走系统 git 同一思路),不为更新引入 HTTP/TLS 依赖。
//!
//! 发布源格式(至少需要 `stable`):
//! `{ "stable": { "version": "0.2.0", "build": 12, "url": "https://…/Gitlane-0.2.0.dmg", "sha256": "…", "size": 123, "notes": "…" } }`
//!
//! @author ixxxxoooo

mod install;
mod replace;

pub use install::{Progress, StagedUpdate, download, format_bytes, load_staged, staging_root};
pub use replace::{launch_replacer, take_install_failure};

use serde::{Deserialize, Serialize};
use std::process::Command;

/// 默认发布源:最新正式版 Release 附带的 `appcast.json`(由 `scripts/publish.sh` 或发布流水线上传)。
pub const DEFAULT_FEED_URL: &str =
    "https://github.com/ixxxxoooo/gitlane/releases/latest/download/appcast.json";

/// 一个可用版本的描述。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    #[serde(default)]
    pub build: Option<u64>,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    /// 安装包字节数,仅用于显示下载百分比。
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// 检查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateResult {
    UpToDate,
    Available(UpdateInfo),
    Failed(String),
}

/// 当前构建号:发布脚本通过环境变量 `GITLANE_BUILD` 编进二进制,开发版为 0。
pub fn current_build() -> u64 {
    option_env!("GITLANE_BUILD")
        .and_then(|b| b.parse().ok())
        .unwrap_or(0)
}

/// 比较点分版本号(忽略数字后的后缀,缺位按 0)。
///
/// 参数:`a`、`b` 两个版本号。返回:a 大于 b 为 1,小于为 -1,相等为 0。
pub fn compare(a: &str, b: &str) -> i32 {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return if p > q { 1 } else { -1 };
        }
    }
    0
}

/// 版本 `(version, build)` 是否比当前运行的更新:版本号相同时看构建号。
pub(crate) fn is_newer(version: &str, build: u64, current: &str, current_build: u64) -> bool {
    let order = compare(version, current);
    order > 0 || (order == 0 && build > current_build)
}

/// 解析发布源并与当前版本比较。下载地址必须是 https。
///
/// 参数:`data` 发布源 JSON;`current` 当前版本;`build` 当前构建号。
pub fn evaluate(data: &[u8], current: &str, build: u64) -> UpdateResult {
    let Ok(feed) = serde_json::from_slice::<std::collections::BTreeMap<String, UpdateInfo>>(data)
    else {
        return UpdateResult::Failed("发布源格式不正确".into());
    };
    let Some(info) = feed.get("stable") else {
        return UpdateResult::Failed("发布源缺少 stable 通道".into());
    };
    if !info.url.starts_with("https://") {
        return UpdateResult::Failed("发布源中的下载地址必须是 https".into());
    }
    if is_newer(&info.version, info.build.unwrap_or(0), current, build) {
        UpdateResult::Available(info.clone())
    } else {
        UpdateResult::UpToDate
    }
}

/// 拉取发布源并检查更新(阻塞,界面在后台线程调用)。
///
/// 参数:`feed_url` 发布源,必须 https;`current` 当前版本;`build` 当前构建号。
pub fn check(feed_url: &str, current: &str, build: u64) -> UpdateResult {
    if !feed_url.starts_with("https://") {
        return UpdateResult::Failed("更新源必须使用 https".into());
    }
    let out = Command::new("/usr/bin/curl")
        .args(["-fsSL", "--max-time", "12", feed_url])
        .output();
    match out {
        Ok(o) if o.status.success() => evaluate(&o.stdout, current, build),
        // curl 退出码 22:HTTP 状态 >= 400,发布源还不存在时就是 404
        Ok(o) if o.status.code() == Some(22) => {
            UpdateResult::Failed("还没有发布过版本(发布源不存在)".into())
        }
        Ok(o) => UpdateResult::Failed(format!(
            "获取更新信息失败:{}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => UpdateResult::Failed(format!("无法运行 curl:{e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"{"stable":{"version":"0.2.0","build":12,"url":"https://x/y.dmg","sha256":"ab","notes":"n"}}"#;

    #[test]
    fn compares_versions() {
        assert_eq!(compare("0.2.0", "0.1.9"), 1);
        assert_eq!(compare("v1.0", "1.0.0"), 0);
        assert_eq!(compare("0.9.9", "0.10.0"), -1);
        assert_eq!(compare("1.2.3-beta", "1.2.3"), 0);
    }

    #[test]
    fn detects_newer_version_and_build() {
        assert!(
            matches!(evaluate(FEED.as_bytes(), "0.1.0", 1), UpdateResult::Available(i) if i.version == "0.2.0")
        );
        assert_eq!(
            evaluate(FEED.as_bytes(), "0.2.0", 12),
            UpdateResult::UpToDate
        );
        assert!(matches!(
            evaluate(FEED.as_bytes(), "0.2.0", 3),
            UpdateResult::Available(_)
        ));
        assert_eq!(
            evaluate(FEED.as_bytes(), "0.3.0", 0),
            UpdateResult::UpToDate
        );
    }

    #[test]
    fn rejects_bad_feeds() {
        assert!(matches!(
            evaluate(b"nope", "0.1.0", 0),
            UpdateResult::Failed(_)
        ));
        assert!(matches!(
            evaluate(b"{}", "0.1.0", 0),
            UpdateResult::Failed(_)
        ));
        let http = FEED.replace("https://", "http://");
        assert!(matches!(
            evaluate(http.as_bytes(), "0.1.0", 0),
            UpdateResult::Failed(_)
        ));
        assert!(matches!(
            check("http://x/appcast.json", "0.1.0", 0),
            UpdateResult::Failed(_)
        ));
    }
}

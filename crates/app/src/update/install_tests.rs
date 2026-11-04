//! `install` 的单元测试:只用临时目录,不碰真实的应用支持目录。
//!
//! @author ixxxxoooo

use super::*;
use crate::update::take_install_failure;

fn info(url: &str, sha: Option<&str>) -> UpdateInfo {
    UpdateInfo {
        version: "0.2.0".into(),
        build: Some(5),
        url: url.into(),
        sha256: sha.map(Into::into),
        size: None,
        notes: None,
    }
}

#[test]
fn 字节数与进度文字() {
    assert_eq!(format_bytes(812), "812 B");
    assert_eq!(format_bytes(35_000), "34.2 KB");
    assert_eq!(format_bytes(86_000_000), "82.0 MB");
    assert_eq!(
        Progress::Downloading {
            received: 999,
            total: Some(1000)
        }
        .short(),
        "99%"
    );
    assert_eq!(
        Progress::Downloading {
            received: 5 << 20,
            total: None
        }
        .short(),
        "已下载 5.0 MB"
    );
    assert_eq!(Progress::Verifying.short(), "校验中");
}

#[test]
fn 拒绝不安全或缺校验值的更新() {
    let tmp = tempfile::tempdir().unwrap();
    let no_sha = download(tmp.path(), &info("https://x/y.dmg", None), &mut |_| {});
    assert!(no_sha.unwrap_err().contains("sha256"));
    let http = download(tmp.path(), &info("http://x/y.dmg", Some("ab")), &mut |_| {});
    assert!(http.unwrap_err().contains("https"));
}

#[test]
fn 暂存只在比当前新时有效() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("updates");
    std::fs::create_dir_all(root.join("app.app")).unwrap();
    let staged = StagedUpdate {
        version: "0.2.0".into(),
        build: 5,
        app_path: root.join("app.app").to_string_lossy().into(),
        notes: None,
    };
    std::fs::write(
        root.join("ready.json"),
        serde_json::to_vec(&staged).unwrap(),
    )
    .unwrap();
    assert_eq!(load_staged(&root, "0.1.0", 1), Some(staged));
    // 当前已是同版本同构建:暂存作废并被清理
    assert_eq!(load_staged(&root, "0.2.0", 5), None);
    assert!(!root.exists());
}

#[test]
fn 读取并清理安装失败记录() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("updates");
    assert_eq!(take_install_failure(&root), None);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("failed"), "move\n").unwrap();
    assert_eq!(take_install_failure(&root).as_deref(), Some("move:-"));
    assert!(!root.exists());
}

//! 文件监听:工作区或 .git 有变化时通知上层刷新。防抖由应用层负责。

use crate::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Component, Path, PathBuf};

/// 变化发生在哪里,上层据此决定刷新范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// 工作区文件或 .git 里只影响 status 的文件(如 index):刷新 status。
    Worktree,
    /// HEAD、引用、进行中的 merge/rebase 等:刷新 status、引用与历史。
    Repository,
}

/// 持有即监听,drop 后停止。
pub struct RepoWatcher {
    _watcher: RecommendedWatcher,
}

impl RepoWatcher {
    pub fn new(
        workdir: &Path,
        git_dir: &Path,
        on_change: impl Fn(Change) + Send + 'static,
    ) -> Result<Self> {
        let git_dir = git_dir
            .canonicalize()
            .unwrap_or_else(|_| git_dir.to_path_buf());
        let workdir_c = workdir
            .canonicalize()
            .unwrap_or_else(|_| workdir.to_path_buf());
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let mut change = None;
            for path in &event.paths {
                match classify(path, &workdir_c, &git_dir) {
                    Some(Change::Repository) => {
                        change = Some(Change::Repository);
                        break;
                    }
                    Some(Change::Worktree) => change = Some(Change::Worktree),
                    None => {}
                }
            }
            if let Some(c) = change {
                on_change(c);
            }
        })
        .map_err(|e| crate::Error::other(format!("无法监听文件变化:{e}")))?;
        watcher
            .watch(workdir, RecursiveMode::Recursive)
            .map_err(|e| crate::Error::other(format!("无法监听文件变化:{e}")))?;
        Ok(Self { _watcher: watcher })
    }
}

fn classify(path: &Path, workdir: &Path, git_dir: &Path) -> Option<Change> {
    if let Ok(rel) = path.strip_prefix(git_dir) {
        return classify_git_path(rel);
    }
    let rel = path.strip_prefix(workdir).unwrap_or(path);
    // 链接式 worktree 或子模块里的 .git
    if rel
        .components()
        .any(|c| c == Component::Normal(".git".as_ref()))
    {
        let inner: PathBuf = rel
            .components()
            .skip_while(|c| *c != Component::Normal(".git".as_ref()))
            .skip(1)
            .collect();
        return classify_git_path(&inner);
    }
    Some(Change::Worktree)
}

fn classify_git_path(rel: &Path) -> Option<Change> {
    let s = rel.to_string_lossy();
    // 对象写入与锁文件非常频繁,真正的变化会随后体现在 ref / index 上
    if s.starts_with("objects") || s.ends_with(".lock") || s.starts_with("logs") {
        return None;
    }
    // 只有 HEAD、引用和进行中的操作会改变提交图;其它(index、config、FETCH_HEAD…)只影响 status。
    // 编辑器后台跑 `git status` 会不停刷新 index,不能每次都整体重载历史。
    const HISTORY: &[&str] = &[
        "HEAD",
        "refs",
        "packed-refs",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
        "BISECT_LOG",
    ];
    let first = rel
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy());
    if first.is_some_and(|f| HISTORY.contains(&f.as_ref())) {
        Some(Change::Repository)
    } else {
        Some(Change::Worktree)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_paths() {
        let wd = Path::new("/r");
        let gd = Path::new("/r/.git");
        assert_eq!(
            classify(Path::new("/r/src/a.rs"), wd, gd),
            Some(Change::Worktree)
        );
        assert_eq!(
            classify(Path::new("/r/.git/HEAD"), wd, gd),
            Some(Change::Repository)
        );
        assert_eq!(
            classify(Path::new("/r/.git/index"), wd, gd),
            Some(Change::Worktree),
            "index 变化只刷新 status"
        );
        assert_eq!(
            classify(Path::new("/r/.git/refs/heads/main"), wd, gd),
            Some(Change::Repository)
        );
        assert_eq!(
            classify(Path::new("/r/.git/packed-refs"), wd, gd),
            Some(Change::Repository)
        );
        assert_eq!(classify(Path::new("/r/.git/index.lock"), wd, gd), None);
        assert_eq!(classify(Path::new("/r/.git/objects/ab/cd"), wd, gd), None);
    }
}

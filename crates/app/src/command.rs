//! 命令层:所有写操作都表示成 `Command`,在后台线程同步执行,并按需生成撤销记录。
//!
//! 这一层不依赖 GPUI,可以直接用临时仓库测试。

use gitcore::diff::FileDiff;
use gitcore::ops::{CommitOptions, ResetMode};
use gitcore::patch::PatchDirection;
use gitcore::undo::{self, UndoEntry};
use gitcore::{Cancel, Error, Repo, Result, short_id};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

/// 正在执行的命令对应工具栏的哪个按钮,用来让那个按钮转圈。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusyAction {
    Fetch,
    Pull,
    Push,
    Stash,
    /// 没有对应按钮的命令(暂存、提交、撤销…)。
    Other,
}

#[derive(Clone, Debug)]
pub enum Command {
    StagePaths(Vec<String>),
    UnstagePaths(Vec<String>),
    StageAll,
    UnstageAll,
    /// 暂存/取消暂存/丢弃某个 hunk 的部分行;`lines` 为 None 表示整个 hunk。
    ApplySelection {
        file: Arc<FileDiff>,
        hunk: usize,
        lines: Option<BTreeSet<usize>>,
        dir: PatchDirection,
    },
    /// 丢弃文件改动:已跟踪的恢复到索引版本,未跟踪的删除。
    Discard {
        tracked: Vec<String>,
        untracked: Vec<String>,
    },
    Commit {
        message: String,
        amend: bool,
    },
    Checkout(String),
    CheckoutRemote(String),
    CheckoutDetached(String),
    CreateBranch {
        name: String,
        start: String,
        checkout: bool,
    },
    RenameBranch {
        old: String,
        new: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    Fetch,
    Pull,
    Push {
        force: bool,
    },
    StashPush {
        message: Option<String>,
    },
    StashApply {
        index: usize,
        pop: bool,
    },
    StashDrop(usize),
    Reset {
        rev: String,
        mode: ResetMode,
    },
    CherryPick(String),
    Revert(String),
    Merge(String),
    CreateTag {
        name: String,
        rev: String,
        message: Option<String>,
    },
    DeleteTag(String),
    AbortInProgress,
}

impl Command {
    /// 进度提示里显示的名称。
    pub fn label(&self) -> String {
        match self {
            Command::StagePaths(p) => format!("暂存 {}", summarize(p)),
            Command::UnstagePaths(p) => format!("取消暂存 {}", summarize(p)),
            Command::StageAll => "暂存全部".into(),
            Command::UnstageAll => "取消暂存全部".into(),
            Command::ApplySelection { file, dir, .. } => match dir {
                PatchDirection::Stage => format!("暂存 {} 的部分改动", file.path),
                PatchDirection::Unstage => format!("取消暂存 {} 的部分改动", file.path),
                PatchDirection::Discard => format!("丢弃 {} 的部分改动", file.path),
            },
            Command::Discard { tracked, untracked } => {
                let all: Vec<String> = tracked.iter().chain(untracked).cloned().collect();
                format!("丢弃 {}", summarize(&all))
            }
            Command::Commit { message, amend } => {
                let first = message.lines().next().unwrap_or_default();
                if *amend {
                    format!("修改上一次提交:{first}")
                } else {
                    format!("提交:{first}")
                }
            }
            Command::Checkout(b) | Command::CheckoutRemote(b) => format!("检出 {b}"),
            Command::CheckoutDetached(r) => format!("检出 {}", short_rev(r)),
            Command::CreateBranch { name, .. } => format!("新建分支 {name}"),
            Command::RenameBranch { old, new } => format!("重命名分支 {old} → {new}"),
            Command::DeleteBranch { name, .. } => format!("删除分支 {name}"),
            Command::Fetch => "Fetch".into(),
            Command::Pull => "Pull".into(),
            Command::Push { force: false } => "Push".into(),
            Command::Push { force: true } => "Force push".into(),
            Command::StashPush { .. } => "Stash".into(),
            Command::StashApply { index, pop } => {
                format!("{} stash@{{{index}}}", if *pop { "Pop" } else { "Apply" })
            }
            Command::StashDrop(i) => format!("删除 stash@{{{i}}}"),
            Command::Reset { rev, mode } => {
                format!(
                    "Reset({}) 到 {}",
                    mode.flag().trim_start_matches("--"),
                    short_rev(rev)
                )
            }
            Command::CherryPick(r) => format!("Cherry-pick {}", short_rev(r)),
            Command::Revert(r) => format!("Revert {}", short_rev(r)),
            Command::Merge(r) => format!("合并 {r}"),
            Command::CreateTag { name, .. } => format!("新建标签 {name}"),
            Command::DeleteTag(n) => format!("删除标签 {n}"),
            Command::AbortInProgress => "中止进行中的操作".into(),
        }
    }

    /// 工具栏上与这条命令对应的按钮;执行期间由那个按钮转圈。没有对应按钮的是 `Other`。
    pub fn busy_action(&self) -> BusyAction {
        match self {
            Command::Fetch => BusyAction::Fetch,
            Command::Pull => BusyAction::Pull,
            Command::Push { .. } => BusyAction::Push,
            Command::StashPush { .. } => BusyAction::Stash,
            _ => BusyAction::Other,
        }
    }

    /// 需要联网、可能耗时较长的命令:显示进度并允许取消。
    pub fn is_network(&self) -> bool {
        matches!(self, Command::Fetch | Command::Pull | Command::Push { .. })
    }

    /// 执行后是否需要重新加载历史与引用(否则只刷新 status)。
    pub fn touches_history(&self) -> bool {
        !matches!(
            self,
            Command::StagePaths(_)
                | Command::UnstagePaths(_)
                | Command::StageAll
                | Command::UnstageAll
                | Command::ApplySelection { .. }
                | Command::Discard { .. }
        )
    }
}

fn summarize(paths: &[String]) -> String {
    match paths {
        [one] => one.clone(),
        _ => format!("{} 个文件", paths.len()),
    }
}

fn short_rev(r: &str) -> String {
    if r.len() == 40 && r.chars().all(|c| c.is_ascii_hexdigit()) {
        r[..7].to_string()
    } else {
        r.to_string()
    }
}

/// 在后台线程执行命令。返回的撤销记录由调用方压入撤销栈。
pub fn execute(path: &Path, cmd: &Command, cancel: &Cancel) -> Result<Option<UndoEntry>> {
    let repo = Repo::open(path)?;
    let git = repo.git();
    let label = cmd.label();
    let refs = |mode: ResetMode, save_wip: bool, op: &dyn Fn() -> Result<()>| {
        undo::record_refs(&repo, label.clone(), mode, save_wip, op).map(Some)
    };

    match cmd {
        Command::StagePaths(p) => git.stage_paths(&strs(p)).map(|_| None),
        Command::UnstagePaths(p) => git.unstage_paths(&strs(p)).map(|_| None),
        Command::StageAll => git.stage_all().map(|_| None),
        Command::UnstageAll => git.unstage_all().map(|_| None),
        Command::ApplySelection {
            file,
            hunk,
            lines,
            dir,
        } => {
            let apply = || git.apply_selection(file, *hunk, lines.as_ref(), *dir);
            if *dir == PatchDirection::Discard {
                undo::record_files(&repo, label.clone(), &[file.path.as_str()], apply).map(Some)
            } else {
                apply().map(|_| None)
            }
        }
        Command::Discard { tracked, untracked } => {
            let all: Vec<&str> = tracked
                .iter()
                .chain(untracked)
                .map(String::as_str)
                .collect();
            undo::record_files(&repo, label.clone(), &all, || {
                git.discard_paths(&strs(tracked), &strs(untracked))
            })
            .map(Some)
        }
        Command::Commit { message, amend } => {
            if message.trim().is_empty() {
                return Err(Error::other("提交信息不能为空"));
            }
            let opts = CommitOptions {
                amend: *amend,
                ..Default::default()
            };
            let entry = undo::record_refs(&repo, label.clone(), ResetMode::Soft, false, || {
                git.commit(message, &opts)
            })?;
            // 撤销记录的描述里带上新提交的短 SHA,便于在菜单里辨认
            let head = repo.head()?.oid().map(short_id).unwrap_or_default();
            Ok(Some(UndoEntry {
                label: format!("{} ({head})", entry.label),
                ..entry
            }))
        }
        Command::Checkout(b) => refs(ResetMode::Keep, false, &|| git.checkout(b)),
        Command::CheckoutRemote(b) => refs(ResetMode::Keep, false, &|| git.checkout_remote(b)),
        Command::CheckoutDetached(r) => refs(ResetMode::Keep, false, &|| git.checkout_detached(r)),
        Command::CreateBranch {
            name,
            start,
            checkout,
        } => refs(ResetMode::Keep, false, &|| {
            git.create_branch(name, start, *checkout)
        }),
        Command::RenameBranch { old, new } => {
            refs(ResetMode::Keep, false, &|| git.rename_branch(old, new))
        }
        Command::DeleteBranch { name, force } => {
            refs(ResetMode::Keep, false, &|| git.delete_branch(name, *force))
        }
        Command::Fetch => git.fetch(cancel).map(|_| None),
        Command::Pull => refs(ResetMode::Keep, false, &|| git.pull(cancel)),
        Command::Push { force } => {
            let head = repo.head()?;
            let branch = head
                .branch()
                .ok_or_else(|| Error::other("分离 HEAD 状态下无法推送,请先检出分支"))?
                .to_string();
            if repo.has_upstream(&branch) {
                git.push(None, *force, cancel)?;
            } else {
                let remotes = git.remotes()?;
                let remote = remotes
                    .iter()
                    .find(|r| *r == "origin")
                    .or(remotes.first())
                    .ok_or_else(|| Error::other("仓库没有配置远程"))?;
                git.push(Some((remote, &branch)), *force, cancel)?;
            }
            Ok(None)
        }
        Command::StashPush { message } => git.stash_push(message.as_deref(), true).map(|_| None),
        Command::StashApply { index, pop } => git.stash_apply(*index, *pop).map(|_| None),
        Command::StashDrop(i) => git.stash_drop(*i).map(|_| None),
        Command::Reset { rev, mode } => {
            refs(*mode, *mode == ResetMode::Hard, &|| git.reset(rev, *mode))
        }
        Command::CherryPick(r) => refs(ResetMode::Keep, false, &|| git.cherry_pick(r)),
        Command::Revert(r) => refs(ResetMode::Keep, false, &|| git.revert(r)),
        Command::Merge(r) => refs(ResetMode::Keep, false, &|| git.merge(r)),
        Command::CreateTag { name, rev, message } => refs(ResetMode::Keep, false, &|| {
            git.create_tag(name, rev, message.as_deref())
        }),
        Command::DeleteTag(n) => refs(ResetMode::Keep, false, &|| git.delete_tag(n)),
        Command::AbortInProgress => git.abort_in_progress(repo.state()).map(|_| None),
    }
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcore::Git;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.name", "T"],
            vec!["config", "user.email", "t@e.com"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            git.run(&args).unwrap();
        }
        dir
    }

    #[test]
    fn commit_produces_undo_entry_with_sha() {
        let dir = repo();
        std::fs::write(dir.path().join("a"), "1").unwrap();
        let c = Cancel::new();
        execute(dir.path(), &Command::StageAll, &c).unwrap();
        let e = execute(
            dir.path(),
            &Command::Commit {
                message: "init".into(),
                amend: false,
            },
            &c,
        )
        .unwrap()
        .unwrap();
        assert!(e.label.starts_with("提交:init ("), "{}", e.label);
    }

    #[test]
    fn empty_commit_message_is_rejected() {
        let dir = repo();
        let r = execute(
            dir.path(),
            &Command::Commit {
                message: "  \n".into(),
                amend: false,
            },
            &Cancel::new(),
        );
        assert!(r.is_err());
    }

    #[test]
    fn push_without_remote_reports_error() {
        let dir = repo();
        std::fs::write(dir.path().join("a"), "1").unwrap();
        let c = Cancel::new();
        execute(dir.path(), &Command::StageAll, &c).unwrap();
        execute(
            dir.path(),
            &Command::Commit {
                message: "init".into(),
                amend: false,
            },
            &c,
        )
        .unwrap();
        let err = execute(dir.path(), &Command::Push { force: false }, &c).unwrap_err();
        assert!(err.to_string().contains("远程"), "{err}");
    }

    #[test]
    fn labels() {
        assert_eq!(
            Command::StagePaths(vec!["a.rs".into()]).label(),
            "暂存 a.rs"
        );
        assert_eq!(
            Command::CheckoutDetached("0123456789012345678901234567890123456789".into()).label(),
            "检出 0123456"
        );
        assert!(Command::Fetch.is_network());
        // 工具栏上哪个按钮转圈:fetch / pull / push / stash 各有对应,其余没有
        assert_eq!(Command::Fetch.busy_action(), BusyAction::Fetch);
        assert_eq!(Command::Pull.busy_action(), BusyAction::Pull);
        assert_eq!(
            Command::Push { force: true }.busy_action(),
            BusyAction::Push
        );
        assert_eq!(
            Command::StashPush { message: None }.busy_action(),
            BusyAction::Stash
        );
        assert_eq!(Command::StageAll.busy_action(), BusyAction::Other);
        assert!(!Command::StageAll.touches_history());
    }
}

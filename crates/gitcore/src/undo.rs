//! 撤销与重做。
//!
//! 写操作执行前后各拍一次快照,撤销 = 恢复到“前”快照,重做 = 恢复到“后”快照。
//! 快照分两类:
//! - 引用快照:HEAD 指向与全部本地分支、标签的位置,覆盖 commit、checkout、reset、
//!   删除/创建/重命名分支、cherry-pick、revert、rebase 等;
//! - 文件快照:操作涉及文件的内容(写入对象库的 blob),覆盖丢弃改动。
//!
//! hard reset 会丢掉工作区改动,所以额外用 `git stash create` 保存一份,撤销时再应用回来。

use crate::ops::ResetMode;
use crate::{Error, Repo, Result};
use git2::Oid;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeadState {
    Branch(String),
    Detached(Oid),
    Unborn(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefSnapshot {
    pub head: HeadState,
    /// `refs/heads/*` 与 `refs/tags/*` → 目标对象。
    pub refs: BTreeMap<String, Oid>,
}

impl RefSnapshot {
    fn head_branch_ref(&self) -> Option<String> {
        match &self.head {
            HeadState::Branch(b) | HeadState::Unborn(b) => Some(format!("refs/heads/{b}")),
            HeadState::Detached(_) => None,
        }
    }

    pub fn head_oid(&self) -> Option<Oid> {
        match &self.head {
            HeadState::Detached(oid) => Some(*oid),
            HeadState::Branch(_) | HeadState::Unborn(_) => self
                .head_branch_ref()
                .and_then(|r| self.refs.get(&r).copied()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSnapshot {
    pub path: String,
    /// 文件内容 blob;`None` 表示文件不存在。
    pub blob: Option<Oid>,
}

#[derive(Clone, Debug)]
pub enum UndoKind {
    Refs {
        before: RefSnapshot,
        after: RefSnapshot,
        /// HEAD 所在分支移动时使用的 reset 方式。
        mode: ResetMode,
        /// 操作前的工作区改动(stash 提交),撤销后重新应用。
        wip_before: Option<String>,
    },
    Files {
        before: Vec<FileSnapshot>,
        after: Vec<FileSnapshot>,
    },
}

#[derive(Clone, Debug)]
pub struct UndoEntry {
    /// 展示给用户的描述,如“提交 a1b2c3d”。
    pub label: String,
    pub kind: UndoKind,
}

impl UndoEntry {
    /// 前后快照完全相同(操作没有产生效果)时不入栈。
    pub fn is_noop(&self) -> bool {
        match &self.kind {
            UndoKind::Refs { before, after, .. } => before == after,
            UndoKind::Files { before, after } => before == after,
        }
    }
}

pub fn capture_refs(repo: &Repo) -> Result<RefSnapshot> {
    let raw = repo.raw();
    let head_ref = raw.find_reference("HEAD")?;
    let head = match head_ref.symbolic_target()? {
        Some(target) => {
            let name = target
                .strip_prefix("refs/heads/")
                .unwrap_or(target)
                .to_string();
            if raw.find_reference(target).is_ok() {
                HeadState::Branch(name)
            } else {
                HeadState::Unborn(name)
            }
        }
        None => HeadState::Detached(head_ref.target().ok_or_else(|| Error::other("HEAD 无效"))?),
    };
    let mut refs = BTreeMap::new();
    for glob in ["refs/heads/*", "refs/tags/*"] {
        for r in raw.references_glob(glob)? {
            let r = r?;
            if let (Ok(name), Some(target)) = (r.name(), r.target()) {
                refs.insert(name.to_string(), target);
            }
        }
    }
    Ok(RefSnapshot { head, refs })
}

/// 把若干文件的当前内容写入对象库,返回快照。
pub fn capture_files(repo: &Repo, paths: &[&str]) -> Result<Vec<FileSnapshot>> {
    paths
        .iter()
        .map(|p| {
            let full = repo.workdir().join(p);
            let blob = if full.is_file() {
                Some(repo.raw().blob_path(&full)?)
            } else {
                None
            };
            Ok(FileSnapshot {
                path: p.to_string(),
                blob,
            })
        })
        .collect()
}

fn restore_files(repo: &Repo, snaps: &[FileSnapshot]) -> Result<()> {
    for s in snaps {
        let full = repo.workdir().join(&s.path);
        match s.blob {
            Some(oid) => {
                let blob = repo.raw().find_blob(oid)?;
                if let Some(dir) = full.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&full, blob.content())?;
            }
            None => {
                if full.is_file() {
                    std::fs::remove_file(&full)?;
                }
            }
        }
    }
    Ok(())
}

fn restore_refs(repo: &Repo, target: &RefSnapshot, mode: ResetMode) -> Result<()> {
    let git = repo.git();
    let current = capture_refs(repo)?;
    let cur_head_ref = current.head_branch_ref();

    // 1. 先改不在检出状态的引用(包括即将检出的目标分支)
    let names: std::collections::BTreeSet<&String> =
        target.refs.keys().chain(current.refs.keys()).collect();
    for name in &names {
        if Some(name.as_str()) == cur_head_ref.as_deref() {
            continue;
        }
        let (want, have) = (target.refs.get(*name), current.refs.get(*name));
        if want != have {
            git.update_ref(name, want.map(|o| o.to_string()).as_deref())?;
        }
    }

    // 2. HEAD
    let same_branch = match (&target.head, &current.head) {
        (
            HeadState::Branch(a) | HeadState::Unborn(a),
            HeadState::Branch(b) | HeadState::Unborn(b),
        ) => a == b,
        _ => false,
    };
    if same_branch {
        let r = cur_head_ref.expect("same branch");
        match (target.refs.get(&r), current.refs.get(&r)) {
            (Some(want), Some(have)) if want != have => git.reset(&want.to_string(), mode)?,
            (Some(want), None) => git.update_ref(&r, Some(&want.to_string()))?,
            // 撤销首个提交:删除分支引用,索引保持不变,相当于 soft reset
            (None, Some(_)) => git.update_ref(&r, None)?,
            _ => {}
        }
    } else {
        match &target.head {
            HeadState::Branch(b) => git.checkout(b)?,
            HeadState::Detached(oid) => git.checkout_detached(&oid.to_string())?,
            HeadState::Unborn(b) => {
                git.run(&["symbolic-ref", "HEAD", &format!("refs/heads/{b}")])?;
            }
        }
        // 3. 原来检出的分支现在可以安全地改了
        if let Some(r) = cur_head_ref {
            let (want, have) = (target.refs.get(&r), current.refs.get(&r));
            if want != have {
                git.update_ref(&r, want.map(|o| o.to_string()).as_deref())?;
            }
        }
    }
    Ok(())
}

/// 撤销一条记录。
pub fn undo(repo: &Repo, entry: &UndoEntry) -> Result<()> {
    match &entry.kind {
        UndoKind::Refs {
            before,
            mode,
            wip_before,
            ..
        } => {
            restore_refs(repo, before, *mode)?;
            if let Some(wip) = wip_before {
                repo.git().run(&["stash", "apply", "--index", wip])?;
            }
            Ok(())
        }
        UndoKind::Files { before, .. } => restore_files(repo, before),
    }
}

/// 重做一条记录。
pub fn redo(repo: &Repo, entry: &UndoEntry) -> Result<()> {
    match &entry.kind {
        UndoKind::Refs { after, mode, .. } => restore_refs(repo, after, *mode),
        UndoKind::Files { after, .. } => restore_files(repo, after),
    }
}

/// 每个仓库一个撤销栈,保存在内存中。
#[derive(Debug, Default)]
pub struct UndoStack {
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
}

impl UndoStack {
    const LIMIT: usize = 100;

    pub fn push(&mut self, entry: UndoEntry) {
        if entry.is_noop() {
            return;
        }
        self.undo.push(entry);
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn peek_undo(&self) -> Option<&UndoEntry> {
        self.undo.last()
    }

    pub fn peek_redo(&self) -> Option<&UndoEntry> {
        self.redo.last()
    }

    pub fn take_undo(&mut self) -> Option<UndoEntry> {
        self.undo.pop()
    }

    pub fn take_redo(&mut self) -> Option<UndoEntry> {
        self.redo.pop()
    }

    /// 撤销成功后把记录放入重做栈。
    pub fn did_undo(&mut self, entry: UndoEntry) {
        self.redo.push(entry);
    }

    /// 重做成功后把记录放回撤销栈。
    pub fn did_redo(&mut self, entry: UndoEntry) {
        self.undo.push(entry);
    }

    /// 撤销/重做失败时放回原栈,保证记录不丢。
    pub fn restore_undo(&mut self, entry: UndoEntry) {
        self.undo.push(entry);
    }

    pub fn restore_redo(&mut self, entry: UndoEntry) {
        self.redo.push(entry);
    }
}

/// 执行一个会移动引用的操作并生成撤销记录。`save_wip` 为 true 时先保存工作区改动
/// (hard reset 之类会丢改动的操作使用)。操作失败时不产生记录。
pub fn record_refs(
    repo: &Repo,
    label: impl Into<String>,
    mode: ResetMode,
    save_wip: bool,
    op: impl FnOnce() -> Result<()>,
) -> Result<UndoEntry> {
    let before = capture_refs(repo)?;
    let wip_before = if save_wip {
        repo.git().stash_create()?
    } else {
        None
    };
    op()?;
    let after = capture_refs(repo)?;
    Ok(UndoEntry {
        label: label.into(),
        kind: UndoKind::Refs {
            before,
            after,
            mode,
            wip_before,
        },
    })
}

/// 执行一个会改写文件内容的操作(丢弃改动)并生成撤销记录。
pub fn record_files(
    repo: &Repo,
    label: impl Into<String>,
    paths: &[&str],
    op: impl FnOnce() -> Result<()>,
) -> Result<UndoEntry> {
    let before = capture_files(repo, paths)?;
    op()?;
    let after = capture_files(repo, paths)?;
    Ok(UndoEntry {
        label: label.into(),
        kind: UndoKind::Files { before, after },
    })
}

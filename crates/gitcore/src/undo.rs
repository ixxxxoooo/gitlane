//! 撤销与重做。
//!
//! 写操作执行前后各拍一次快照,撤销 = 恢复到“前”快照,重做 = 恢复到“后”快照。
//! 快照分三类:
//! - 引用快照:HEAD 指向、全部本地分支与标签的位置、分支的上游配置,覆盖 commit、checkout、
//!   reset、删除/创建/重命名分支、cherry-pick、revert、merge、pull 等;
//! - 文件快照:操作涉及文件的内容(写入对象库的 blob),覆盖丢弃改动;
//! - 删除的 stash:记下 stash 提交,撤销时放回 stash 列表。
//!
//! 恢复时只动这条记录本身改过的东西,并先确认它们还停在操作后的状态:
//! 之后别处的变化(fetch 带回的标签、终端里的提交、丢弃后又改的文件)不会被悄悄覆盖。
//!
//! hard reset 会丢掉工作区改动,所以额外用 `git stash create` 保存一份,撤销时再应用回来。

use crate::ops::ResetMode;
use crate::{Error, Repo, Result};
use git2::{ObjectType, Oid};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeadState {
    Branch(String),
    Detached(Oid),
    Unborn(String),
}

impl HeadState {
    /// 同一分支的“未出生”和“已有提交”算同一个 HEAD(撤销首个提交就在两者间切换)。
    fn same(&self, other: &HeadState) -> bool {
        match (self, other) {
            (
                HeadState::Branch(a) | HeadState::Unborn(a),
                HeadState::Branch(b) | HeadState::Unborn(b),
            ) => a == b,
            (HeadState::Detached(a), HeadState::Detached(b)) => a == b,
            _ => false,
        }
    }
}

/// 分支的上游配置(`branch.<name>.remote` / `branch.<name>.merge`)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Upstream {
    pub remote: Option<String>,
    pub merge: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefSnapshot {
    pub head: HeadState,
    /// `refs/heads/*` 与 `refs/tags/*` → 目标对象。
    pub refs: BTreeMap<String, Oid>,
    /// 分支名 → 上游配置。删除、重命名分支时 git 会连带改掉这些配置,撤销要一并恢复。
    pub upstreams: BTreeMap<String, Upstream>,
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
    /// 可执行位;写回时一并恢复,否则撤销丢弃会让脚本失去执行权限。
    pub executable: bool,
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
    /// 删除了一条 stash。`commit` 是 stash 提交,`message` 是 stash 列表里显示的描述。
    StashDrop { commit: String, message: String },
}

#[derive(Clone, Debug)]
pub struct UndoEntry {
    /// 展示给用户的描述,如“提交 a1b2c3d”。
    pub label: String,
    pub kind: UndoKind,
    /// 撤销提交时放回提交框的提交信息;提交时清空了输入框,不放回来就得重打一遍。
    pub commit_message: Option<String>,
}

impl UndoEntry {
    /// 前后快照完全相同(操作没有产生效果)时不入栈。
    pub fn is_noop(&self) -> bool {
        match &self.kind {
            UndoKind::Refs { before, after, .. } => before == after,
            UndoKind::Files { before, after } => before == after,
            UndoKind::StashDrop { .. } => false,
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
    Ok(RefSnapshot {
        head,
        refs,
        upstreams: capture_upstreams(repo)?,
    })
}

/// 每次重新打开仓库自己的 config 文件读取:git2 缓存的配置对象未必看得到系统 git 刚写入的改动。
/// 分支配置只会在仓库级(链接工作树也共用主仓库的 config)。
fn capture_upstreams(repo: &Repo) -> Result<BTreeMap<String, Upstream>> {
    let mut out: BTreeMap<String, Upstream> = BTreeMap::new();
    let path = repo.raw().commondir().join("config");
    if !path.is_file() {
        return Ok(out);
    }
    let cfg = git2::Config::open(&path)?;
    let mut entries = cfg.entries(Some(r"^branch\..*\.(remote|merge)$"))?;
    while let Some(e) = entries.next() {
        let e = e?;
        let (Ok(name), Ok(value)) = (e.name(), e.value()) else {
            continue;
        };
        // 分支名本身可以带点,所以从两头剥
        let Some(rest) = name.strip_prefix("branch.") else {
            continue;
        };
        let Some((branch, key)) = rest.rsplit_once('.') else {
            continue;
        };
        let up = out.entry(branch.to_string()).or_default();
        match key {
            "remote" => up.remote = Some(value.to_string()),
            "merge" => up.merge = Some(value.to_string()),
            _ => {}
        }
    }
    Ok(out)
}

fn file_state(repo: &Repo, path: &str) -> Result<(Option<Oid>, bool)> {
    let full = repo.workdir().join(path);
    if !full.is_file() {
        return Ok((None, false));
    }
    let oid = Oid::hash_file(ObjectType::Blob, &full)?;
    Ok((Some(oid), is_executable(&full)))
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &std::path::Path) -> bool {
    false
}

/// 把若干文件的当前内容写入对象库,返回快照。
///
/// 存原始字节而不是 `blob_path`:后者会套用 autocrlf 等过滤器,写回时换行符就变了,
/// 也和 `file_state` 的哈希对不上。
pub fn capture_files(repo: &Repo, paths: &[&str]) -> Result<Vec<FileSnapshot>> {
    paths
        .iter()
        .map(|p| {
            let full = repo.workdir().join(p);
            let (blob, executable) = if full.is_file() {
                let bytes = std::fs::read(&full)?;
                (Some(repo.raw().blob(&bytes)?), is_executable(&full))
            } else {
                (None, false)
            };
            Ok(FileSnapshot {
                path: p.to_string(),
                blob,
                executable,
            })
        })
        .collect()
}

/// 把文件恢复到 `target`。每个文件必须还是 `expect` 的内容(或已经是目标内容):
/// 丢弃之后用户又改了文件时,写回旧内容会覆盖这些新改动,宁可报错。
fn restore_files(repo: &Repo, expect: &[FileSnapshot], target: &[FileSnapshot]) -> Result<()> {
    let mut todo = Vec::new();
    for (want, exp) in target.iter().zip(expect) {
        let (have, _) = file_state(repo, &want.path)?;
        if have == want.blob {
            continue;
        }
        if have != exp.blob {
            return Err(Error::other(format!(
                "{} 在这次操作之后又被修改过,为避免覆盖新的改动,没有恢复",
                want.path
            )));
        }
        todo.push(want);
    }
    for s in todo {
        let full = repo.workdir().join(&s.path);
        match s.blob {
            Some(oid) => {
                let blob = repo.raw().find_blob(oid)?;
                if let Some(dir) = full.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(&full, blob.content())?;
                set_executable(&full, s.executable)?;
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

#[cfg(unix)]
fn set_executable(path: &std::path::Path, exec: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path)?.permissions();
    let mode = perm.mode();
    let mode = if exec {
        mode | ((mode & 0o444) >> 2)
    } else {
        mode & !0o111
    };
    perm.set_mode(mode);
    std::fs::set_permissions(path, perm)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable(_: &std::path::Path, _: bool) -> Result<()> {
    Ok(())
}

fn short_ref(name: &str) -> &str {
    name.strip_prefix("refs/heads/")
        .or_else(|| name.strip_prefix("refs/tags/"))
        .unwrap_or(name)
}

/// 把仓库从 `expect`(这条记录的一端)恢复到 `target`(另一端)。
///
/// 只改两端之间不同的引用、HEAD 和上游配置,其余保持现状。改之前先确认这些东西
/// 现在要么还是 `expect`,要么已经是 `target`(上次恢复做到一半);否则说明之后别处改过,报错不动。
/// `guard_dirty` 为 true 时,如果要对检出的分支做 hard reset 而工作区有改动,也报错不动。
fn restore_refs(
    repo: &Repo,
    expect: &RefSnapshot,
    target: &RefSnapshot,
    mode: ResetMode,
    guard_dirty: bool,
) -> Result<()> {
    let git = repo.git();
    let current = capture_refs(repo)?;

    let changed: BTreeSet<&String> = expect
        .refs
        .keys()
        .chain(target.refs.keys())
        .filter(|n| expect.refs.get(*n) != target.refs.get(*n))
        .collect();
    for name in &changed {
        let have = current.refs.get(*name);
        if have != expect.refs.get(*name) && have != target.refs.get(*name) {
            return Err(Error::other(format!(
                "{} 在这次操作之后又有变化,为避免丢失提交,没有恢复",
                short_ref(name)
            )));
        }
    }
    let switch_head = !expect.head.same(&target.head) && !current.head.same(&target.head);
    if switch_head && !current.head.same(&expect.head) {
        return Err(Error::other(
            "HEAD 在这次操作之后已切换到别处,为避免打乱工作区,没有恢复",
        ));
    }

    let cur_head_ref = current.head_branch_ref();
    let head_moves = |r: &String| {
        changed.contains(r) && current.refs.get(r) != target.refs.get(r) && !switch_head
    };
    if guard_dirty
        && mode == ResetMode::Hard
        && cur_head_ref.as_ref().is_some_and(head_moves)
        && git.stash_create()?.is_some()
    {
        return Err(Error::other(
            "工作区有未提交的改动,恢复 hard reset 会覆盖它们;请先提交或储藏",
        ));
    }

    // 1. 先改不在检出状态的引用(包括即将检出的目标分支)
    for name in &changed {
        if Some(name.as_str()) == cur_head_ref.as_deref() {
            continue;
        }
        let (want, have) = (target.refs.get(*name), current.refs.get(*name));
        if want != have {
            git.update_ref(name, want.map(|o| o.to_string()).as_deref())?;
        }
    }

    // 2. HEAD
    if switch_head {
        match &target.head {
            HeadState::Branch(b) => git.checkout(b)?,
            HeadState::Detached(oid) => git.checkout_detached(&oid.to_string())?,
            HeadState::Unborn(b) => {
                git.run(&["symbolic-ref", "HEAD", &format!("refs/heads/{b}")])?;
            }
        }
        // 3. 原来检出的分支现在可以安全地改了
        if let Some(r) = cur_head_ref.filter(|r| changed.contains(r)) {
            let (want, have) = (target.refs.get(&r), current.refs.get(&r));
            if want != have {
                git.update_ref(&r, want.map(|o| o.to_string()).as_deref())?;
            }
        }
    } else if let Some(r) = cur_head_ref.filter(|r| changed.contains(r)) {
        // 检出的分支本身移动:用记录的 reset 方式,让工作区和索引跟着走
        match (target.refs.get(&r), current.refs.get(&r)) {
            (Some(want), Some(have)) if want != have => git.reset(&want.to_string(), mode)?,
            (Some(want), None) => git.update_ref(&r, Some(&want.to_string()))?,
            // 撤销首个提交:删除分支引用,索引保持不变,相当于 soft reset
            (None, Some(_)) => git.update_ref(&r, None)?,
            _ => {}
        }
    }

    restore_upstreams(repo, expect, target, &current)
}

fn restore_upstreams(
    repo: &Repo,
    expect: &RefSnapshot,
    target: &RefSnapshot,
    current: &RefSnapshot,
) -> Result<()> {
    let git = repo.git();
    let names: BTreeSet<&String> = expect
        .upstreams
        .keys()
        .chain(target.upstreams.keys())
        .collect();
    let none = Upstream::default();
    for name in names {
        let want = target.upstreams.get(name).unwrap_or(&none);
        if expect.upstreams.get(name).unwrap_or(&none) == want {
            continue;
        }
        let have = current.upstreams.get(name).unwrap_or(&none);
        for (key, w, h) in [
            ("remote", &want.remote, &have.remote),
            ("merge", &want.merge, &have.merge),
        ] {
            if w == h {
                continue;
            }
            let k = format!("branch.{name}.{key}");
            match w {
                Some(v) => git.run(&["config", &k, v])?,
                None => git.run(&["config", "--unset", &k])?,
            };
        }
    }
    Ok(())
}

/// 撤销一条记录。
pub fn undo(repo: &Repo, entry: &UndoEntry) -> Result<()> {
    match &entry.kind {
        UndoKind::Refs {
            before,
            after,
            mode,
            wip_before,
        } => {
            restore_refs(repo, after, before, *mode, true)?;
            if let Some(wip) = wip_before {
                repo.git().run(&["stash", "apply", "--index", wip])?;
            }
            Ok(())
        }
        UndoKind::Files { before, after } => restore_files(repo, after, before),
        UndoKind::StashDrop { commit, message } => {
            repo.git().stash_store(commit, message)?;
            Ok(())
        }
    }
}

/// 重做一条记录,返回更新后的记录(下一次撤销用它)。
///
/// 重做 hard reset 时,工作区里是上次撤销应用回来的改动,可能又改过:重新保存一份作为
/// 下次撤销要应用的改动,这样重做不会丢东西。
pub fn redo(repo: &Repo, entry: &UndoEntry) -> Result<UndoEntry> {
    match &entry.kind {
        UndoKind::Refs {
            before,
            after,
            mode,
            wip_before,
        } => {
            let wip = if *mode == ResetMode::Hard {
                repo.git().stash_create()?
            } else {
                wip_before.clone()
            };
            restore_refs(repo, before, after, *mode, false)?;
            Ok(UndoEntry {
                kind: UndoKind::Refs {
                    before: before.clone(),
                    after: after.clone(),
                    mode: *mode,
                    wip_before: wip,
                },
                ..entry.clone()
            })
        }
        UndoKind::Files { before, after } => {
            restore_files(repo, before, after)?;
            Ok(entry.clone())
        }
        UndoKind::StashDrop { commit, .. } => {
            let git = repo.git();
            let index = git
                .stash_index_of(commit)?
                .ok_or_else(|| Error::other("这条 stash 已经不在列表里"))?;
            git.stash_drop(index)?;
            Ok(entry.clone())
        }
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
        commit_message: None,
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
        commit_message: None,
    })
}

/// 删除第 `index` 条 stash 并生成撤销记录。
pub fn record_stash_drop(repo: &Repo, label: impl Into<String>, index: usize) -> Result<UndoEntry> {
    let git = repo.git();
    let (commit, message) = git.stash_entry(index)?;
    git.stash_drop(index)?;
    Ok(UndoEntry {
        label: label.into(),
        kind: UndoKind::StashDrop { commit, message },
        commit_message: None,
    })
}

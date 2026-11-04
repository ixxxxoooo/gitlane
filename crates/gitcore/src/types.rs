//! 读路径返回的纯数据类型,可以跨线程传递(全部 `Send + Clone`)。

use git2::Oid;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct CommitInfo {
    pub id: Oid,
    pub parents: Vec<Oid>,
    pub author: String,
    pub email: String,
    /// 作者时间,Unix 秒。
    pub time: i64,
    /// 时区偏移,分钟。
    pub tz_offset: i32,
    pub summary: String,
}

impl CommitInfo {
    pub fn short_id(&self) -> String {
        short_id(self.id)
    }
}

pub fn short_id(id: Oid) -> String {
    let mut s = id.to_string();
    s.truncate(7);
    s
}

/// 提交历史:按拓扑 + 时间排序,子提交在前。`parent_rows` 是父提交在 `commits` 中的下标,
/// 不在列表中的父提交(超出加载上限)记为 `u32::MAX`,可直接喂给 graph 布局。
#[derive(Clone, Debug, Default)]
pub struct History {
    pub commits: Vec<CommitInfo>,
    pub parent_rows: Vec<Vec<u32>>,
    pub row_of: HashMap<Oid, u32>,
    /// 是否因为上限截断。
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct CommitDetail {
    pub info: CommitInfo,
    pub committer: String,
    pub committer_email: String,
    pub commit_time: i64,
    pub message: String,
    pub files: Vec<FileChange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChange,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    /// 单字母标记,和 `git status --short` 一致。
    pub fn letter(self) -> &'static str {
        match self {
            ChangeKind::Added => "A",
            ChangeKind::Modified => "M",
            ChangeKind::Deleted => "D",
            ChangeKind::Renamed => "R",
            ChangeKind::Copied => "C",
            ChangeKind::TypeChange => "T",
            ChangeKind::Untracked => "?",
            ChangeKind::Conflicted => "U",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RefKind {
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug)]
pub struct RefInfo {
    /// 完整名,如 `refs/heads/feature/login`。
    pub name: String,
    /// 简写,如 `feature/login`、`origin/main`、`v1.0`。
    pub short: String,
    pub kind: RefKind,
    pub target: Oid,
    /// 本地分支的上游简写,如 `origin/main`。
    pub upstream: Option<String>,
    /// (领先, 落后) 上游的提交数。
    pub ahead_behind: Option<(usize, usize)>,
    pub is_head: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    Branch {
        name: String,
        oid: Oid,
    },
    /// 新仓库还没有任何提交。
    Unborn {
        name: String,
    },
    Detached(Oid),
}

impl Head {
    pub fn oid(&self) -> Option<Oid> {
        match self {
            Head::Branch { oid, .. } | Head::Detached(oid) => Some(*oid),
            Head::Unborn { .. } => None,
        }
    }

    pub fn branch(&self) -> Option<&str> {
        match self {
            Head::Branch { name, .. } | Head::Unborn { name } => Some(name),
            Head::Detached(_) => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Head::Branch { name, .. } | Head::Unborn { name } => name.clone(),
            Head::Detached(oid) => format!("分离于 {}", short_id(*oid)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StashInfo {
    pub index: usize,
    pub id: Oid,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Refs {
    pub head: Head,
    pub refs: Vec<RefInfo>,
    pub stashes: Vec<StashInfo>,
}

impl Refs {
    /// 按提交聚合的引用,用于在提交图上画徽章。
    pub fn by_target(&self) -> HashMap<Oid, Vec<usize>> {
        let mut m: HashMap<Oid, Vec<usize>> = HashMap::new();
        for (i, r) in self.refs.iter().enumerate() {
            m.entry(r.target).or_default().push(i);
        }
        m
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoState {
    Clean,
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
    Other,
}

#[derive(Clone, Debug)]
pub struct StatusEntry {
    pub path: String,
    pub old_path: Option<String>,
    /// 索引相对 HEAD 的改动(已暂存)。
    pub staged: Option<ChangeKind>,
    /// 工作区相对索引的改动(未暂存)。
    pub unstaged: Option<ChangeKind>,
    /// 已暂存部分的 (新增, 删除) 行数;文件数超过上限或二进制文件时为 `None`。
    pub staged_lines: Option<(usize, usize)>,
    /// 未暂存部分的 (新增, 删除) 行数;含未跟踪文件(整个文件算新增)。
    pub unstaged_lines: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub struct RepoStatus {
    pub entries: Vec<StatusEntry>,
    pub state: RepoState,
}

impl RepoStatus {
    pub fn staged(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries.iter().filter(|e| e.staged.is_some())
    }

    pub fn unstaged(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries.iter().filter(|e| e.unstaged.is_some())
    }

    pub fn conflicted(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries
            .iter()
            .filter(|e| e.unstaged == Some(ChangeKind::Conflicted))
    }

    pub fn is_clean(&self) -> bool {
        self.entries.is_empty()
    }
}

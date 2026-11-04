//! 读路径:基于 git2 的历史、引用、status 与提交详情。

use crate::types::*;
use crate::{Error, Git, Result};
use git2::{BranchType, Oid, Repository, Sort, Status, StatusOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 打开的仓库。git2 的 `Repository` 不是 `Sync`,每个后台任务各自打开一份即可(很便宜)。
pub struct Repo {
    inner: Repository,
    workdir: PathBuf,
}

impl Repo {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let inner = Repository::discover(path).map_err(|_| Error::NotARepo(path.to_path_buf()))?;
        let workdir = inner
            .workdir()
            .ok_or_else(|| Error::other("暂不支持裸仓库"))?
            .to_path_buf();
        Ok(Self { inner, workdir })
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub fn git_dir(&self) -> &Path {
        self.inner.path()
    }

    pub fn git(&self) -> Git {
        Git::new(&self.workdir)
    }

    pub fn raw(&self) -> &Repository {
        &self.inner
    }

    /// 仓库显示名:工作区目录名。
    pub fn name(&self) -> String {
        self.workdir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.workdir.display().to_string())
    }

    pub fn head(&self) -> Result<Head> {
        match self.inner.head() {
            Ok(r) => {
                let oid = r
                    .target()
                    .ok_or_else(|| Error::other("HEAD 没有指向提交"))?;
                if self.inner.head_detached()? {
                    Ok(Head::Detached(oid))
                } else {
                    Ok(Head::Branch {
                        name: r.shorthand().unwrap_or("HEAD").to_string(),
                        oid,
                    })
                }
            }
            Err(e) if e.code() == git2::ErrorCode::UnbornBranch => {
                let r = self.inner.find_reference("HEAD")?;
                let name = r
                    .symbolic_target()
                    .ok()
                    .flatten()
                    .and_then(|t| t.strip_prefix("refs/heads/"))
                    .unwrap_or("main")
                    .to_string();
                Ok(Head::Unborn { name })
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn state(&self) -> RepoState {
        use git2::RepositoryState as S;
        match self.inner.state() {
            S::Clean => RepoState::Clean,
            S::Merge => RepoState::Merge,
            S::Rebase | S::RebaseInteractive | S::RebaseMerge => RepoState::Rebase,
            S::CherryPick | S::CherryPickSequence => RepoState::CherryPick,
            S::Revert | S::RevertSequence => RepoState::Revert,
            S::Bisect => RepoState::Bisect,
            _ => RepoState::Other,
        }
    }

    /// 加载提交历史:所有本地分支、远程分支、标签与 HEAD 可达的提交,最多 `limit` 个。
    pub fn history(&self, limit: usize) -> Result<History> {
        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        if self.inner.head().is_ok() {
            walk.push_head()?;
        }
        for glob in ["refs/heads", "refs/remotes", "refs/tags"] {
            // 指向非提交对象的标签会让 push_glob 失败,忽略即可
            let _ = walk.push_glob(glob);
        }

        let mut commits = Vec::new();
        let mut truncated = false;
        for oid in walk {
            if commits.len() >= limit {
                truncated = true;
                break;
            }
            let oid = oid?;
            let c = self.inner.find_commit(oid)?;
            commits.push(commit_info(&c));
        }

        let row_of: HashMap<Oid, u32> = commits
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id, i as u32))
            .collect();
        let parent_rows = commits
            .iter()
            .map(|c| {
                c.parents
                    .iter()
                    .map(|p| row_of.get(p).copied().unwrap_or(u32::MAX))
                    .collect()
            })
            .collect();
        Ok(History {
            commits,
            parent_rows,
            row_of,
            truncated,
        })
    }

    pub fn refs(&mut self) -> Result<Refs> {
        let head = self.head()?;
        let mut refs = Vec::new();

        for b in self.inner.branches(None)? {
            let (branch, ty) = b?;
            let Some(target) = branch.get().target() else {
                continue; // origin/HEAD 这类符号引用
            };
            let name = branch.get().name().unwrap_or_default().to_string();
            let short = branch.name()?.unwrap_or_default().to_string();
            if ty == BranchType::Remote && short.ends_with("/HEAD") {
                continue;
            }
            let (kind, upstream, ahead_behind) = match ty {
                BranchType::Local => {
                    let up = branch.upstream().ok();
                    let up_name = up
                        .as_ref()
                        .and_then(|u| u.name().ok().flatten().map(str::to_string));
                    let ab = up
                        .and_then(|u| u.get().target())
                        .and_then(|u| self.inner.graph_ahead_behind(target, u).ok());
                    (RefKind::Local, up_name, ab)
                }
                BranchType::Remote => (RefKind::Remote, None, None),
            };
            let is_head = kind == RefKind::Local && head.branch() == Some(short.as_str());
            refs.push(RefInfo {
                name,
                short,
                kind,
                target,
                upstream,
                ahead_behind,
                is_head,
            });
        }

        self.inner.tag_foreach(|oid, name| {
            let name = String::from_utf8_lossy(name).into_owned();
            // 附注标签指向 tag 对象,剥到提交
            let target = self
                .inner
                .find_object(oid, None)
                .and_then(|o| o.peel_to_commit())
                .map(|c| c.id())
                .unwrap_or(oid);
            refs.push(RefInfo {
                short: name.trim_start_matches("refs/tags/").to_string(),
                name,
                kind: RefKind::Tag,
                target,
                upstream: None,
                ahead_behind: None,
                is_head: false,
            });
            true
        })?;

        refs.sort_by(|a, b| (a.kind, &a.short).cmp(&(b.kind, &b.short)));

        let mut stashes = Vec::new();
        self.inner.stash_foreach(|index, message, id| {
            stashes.push(StashInfo {
                index,
                id: *id,
                message: message.to_string(),
            });
            true
        })?;

        Ok(Refs {
            head,
            refs,
            stashes,
        })
    }

    pub fn status(&self) -> Result<RepoStatus> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .include_ignored(false);
        let statuses = self.inner.statuses(Some(&mut opts))?;
        let mut entries = Vec::with_capacity(statuses.len());
        for s in statuses.iter() {
            let st = s.status();
            let path = s.path().unwrap_or_default().to_string();
            let old_path = s
                .head_to_index()
                .and_then(|d| d.old_file().path())
                .map(|p| p.to_string_lossy().into_owned())
                .filter(|p| *p != path);
            let (staged, unstaged) = if st.contains(Status::CONFLICTED) {
                (None, Some(ChangeKind::Conflicted))
            } else {
                (index_kind(st), worktree_kind(st))
            };
            if staged.is_none() && unstaged.is_none() {
                continue;
            }
            entries.push(StatusEntry {
                path,
                old_path,
                staged,
                unstaged,
            });
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(RepoStatus {
            entries,
            state: self.state(),
        })
    }

    pub fn commit_detail(&self, id: Oid) -> Result<CommitDetail> {
        let c = self.inner.find_commit(id)?;
        let tree = c.tree()?;
        let parent_tree = c.parent(0).ok().map(|p| p.tree()).transpose()?;
        let mut diff = self
            .inner
            .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        diff.find_similar(None)?;
        let files = crate::diff::file_changes(&diff)?;
        let committer = c.committer();
        Ok(CommitDetail {
            info: commit_info(&c),
            committer: committer.name().unwrap_or_default().to_string(),
            committer_email: committer.email().unwrap_or_default().to_string(),
            commit_time: committer.when().seconds(),
            message: c.message().unwrap_or_default().to_string(),
            files,
        })
    }

    /// 本地分支是否配置了上游。
    pub fn has_upstream(&self, branch: &str) -> bool {
        self.inner
            .find_branch(branch, BranchType::Local)
            .and_then(|b| b.upstream().map(|_| ()))
            .is_ok()
    }

    /// 读取用户配置的签名身份,提交前用于提示。
    pub fn signature_name(&self) -> Option<String> {
        self.inner
            .signature()
            .ok()
            .and_then(|s| s.name().ok().map(str::to_string))
    }
}

fn commit_info(c: &git2::Commit<'_>) -> CommitInfo {
    let author = c.author();
    CommitInfo {
        id: c.id(),
        parents: c.parent_ids().collect(),
        author: author.name().unwrap_or_default().to_string(),
        email: author.email().unwrap_or_default().to_string(),
        time: author.when().seconds(),
        tz_offset: author.when().offset_minutes(),
        summary: c.summary().ok().flatten().unwrap_or_default().to_string(),
    }
}

fn index_kind(s: Status) -> Option<ChangeKind> {
    if s.contains(Status::INDEX_NEW) {
        Some(ChangeKind::Added)
    } else if s.contains(Status::INDEX_MODIFIED) {
        Some(ChangeKind::Modified)
    } else if s.contains(Status::INDEX_DELETED) {
        Some(ChangeKind::Deleted)
    } else if s.contains(Status::INDEX_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if s.contains(Status::INDEX_TYPECHANGE) {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}

fn worktree_kind(s: Status) -> Option<ChangeKind> {
    if s.contains(Status::WT_NEW) {
        Some(ChangeKind::Untracked)
    } else if s.contains(Status::WT_MODIFIED) {
        Some(ChangeKind::Modified)
    } else if s.contains(Status::WT_DELETED) {
        Some(ChangeKind::Deleted)
    } else if s.contains(Status::WT_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if s.contains(Status::WT_TYPECHANGE) {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}

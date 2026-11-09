//! 写操作。全部经由系统 git 执行,调用方负责在执行前后做撤销快照(见 `undo`)。

use crate::patch::{PatchDirection, build_patch};
use crate::{Cancel, Error, Git, Result, diff::FileDiff};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
    /// `git reset --keep`:移动 HEAD,保留不冲突的本地改动。撤销 rebase 等操作时使用。
    Keep,
}

impl ResetMode {
    pub fn flag(self) -> &'static str {
        match self {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
            ResetMode::Keep => "--keep",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CommitOptions {
    pub amend: bool,
    /// 强制签名(`-S`);为 false 时沿用用户的 commit.gpgsign 配置。
    pub sign: bool,
    pub allow_empty: bool,
}

impl Git {
    // ---- 暂存 ----

    pub fn stage_paths(&self, paths: &[&str]) -> Result<()> {
        let mut args = vec!["add", "-A", "--"];
        args.extend_from_slice(paths);
        self.run(&args).map(drop)
    }

    pub fn stage_all(&self) -> Result<()> {
        self.run(&["add", "-A"]).map(drop)
    }

    pub fn unstage_paths(&self, paths: &[&str]) -> Result<()> {
        // 新仓库(还没有 HEAD)不能 reset,只能从索引移除
        let has_head = self.run(&["rev-parse", "--verify", "-q", "HEAD"]).is_ok();
        let mut args: Vec<&str> = if has_head {
            vec!["reset", "-q", "HEAD", "--"]
        } else {
            vec!["rm", "--cached", "-q", "-r", "--"]
        };
        args.extend_from_slice(paths);
        self.run(&args).map(drop)
    }

    pub fn unstage_all(&self) -> Result<()> {
        let has_head = self.run(&["rev-parse", "--verify", "-q", "HEAD"]).is_ok();
        if has_head {
            self.run(&["reset", "-q", "HEAD"]).map(drop)
        } else {
            self.run(&["rm", "--cached", "-q", "-r", "."]).map(drop)
        }
    }

    /// 应用部分补丁。`file` 必须是与方向匹配的 diff:
    /// Stage/Discard 用 `DiffTarget::Unstaged`,Unstage 用 `DiffTarget::Staged`。
    pub fn apply_selection(
        &self,
        file: &FileDiff,
        hunk: usize,
        lines: Option<&BTreeSet<usize>>,
        dir: PatchDirection,
    ) -> Result<()> {
        let patch = build_patch(file, hunk, lines, dir)
            .ok_or_else(|| Error::other("没有选中任何改动行"))?;
        let args: &[&str] = match dir {
            PatchDirection::Stage => &["apply", "--cached", "--recount", "-"],
            PatchDirection::Unstage => &["apply", "--cached", "--reverse", "--recount", "-"],
            PatchDirection::Discard => &["apply", "--reverse", "--recount", "-"],
        };
        self.run_with(args, Some(patch.as_bytes()), &Cancel::new())
            .map(drop)
    }

    /// 丢弃工作区改动:已跟踪文件恢复到索引版本,未跟踪文件删除。
    pub fn discard_paths(&self, tracked: &[&str], untracked: &[&str]) -> Result<()> {
        if !tracked.is_empty() {
            let mut args = vec!["checkout", "--"];
            args.extend_from_slice(tracked);
            self.run(&args)?;
        }
        for p in untracked {
            let full = self.workdir().join(p);
            if full.is_dir() {
                std::fs::remove_dir_all(full)?;
            } else if full.exists() {
                std::fs::remove_file(full)?;
            }
        }
        Ok(())
    }

    // ---- 提交 ----

    pub fn commit(&self, message: &str, opts: &CommitOptions) -> Result<()> {
        let mut args = vec!["commit", "-F", "-", "--cleanup=strip"];
        if opts.amend {
            args.push("--amend");
        }
        if opts.sign {
            args.push("-S");
        }
        if opts.allow_empty {
            args.push("--allow-empty");
        }
        self.run_with(&args, Some(message.as_bytes()), &Cancel::new())
            .map(drop)
    }

    // ---- 分支 ----

    pub fn checkout(&self, target: &str) -> Result<()> {
        self.run(&["checkout", "-q", target]).map(drop)
    }

    /// 检出远程分支:创建同名本地跟踪分支。
    pub fn checkout_remote(&self, remote_branch: &str) -> Result<()> {
        self.run(&["checkout", "-q", "--track", remote_branch])
            .map(drop)
    }

    pub fn checkout_detached(&self, rev: &str) -> Result<()> {
        self.run(&["checkout", "-q", "--detach", rev]).map(drop)
    }

    pub fn create_branch(&self, name: &str, start: &str, checkout: bool) -> Result<()> {
        if checkout {
            self.run(&["checkout", "-q", "-b", name, start]).map(drop)
        } else {
            self.run(&["branch", name, start]).map(drop)
        }
    }

    pub fn rename_branch(&self, old: &str, new: &str) -> Result<()> {
        self.run(&["branch", "-m", old, new]).map(drop)
    }

    pub fn delete_branch(&self, name: &str, force: bool) -> Result<()> {
        self.run(&["branch", if force { "-D" } else { "-d" }, name])
            .map(drop)
    }

    pub fn update_ref(&self, name: &str, oid: Option<&str>) -> Result<()> {
        match oid {
            Some(oid) => self.run(&["update-ref", name, oid]).map(drop),
            None => self.run(&["update-ref", "-d", name]).map(drop),
        }
    }

    // ---- 同步 ----

    pub fn fetch(&self, cancel: &Cancel) -> Result<()> {
        self.run_with(&["fetch", "--all", "--prune", "--tags"], None, cancel)
            .map(drop)
    }

    /// pull 的合并方式沿用用户的 pull.rebase / pull.ff 配置。
    pub fn pull(&self, cancel: &Cancel) -> Result<()> {
        self.run_with(&["pull"], None, cancel).map(drop)
    }

    /// `set_upstream` 为远程名时会推送当前分支并建立跟踪关系。
    pub fn push(
        &self,
        set_upstream: Option<(&str, &str)>,
        force: bool,
        cancel: &Cancel,
    ) -> Result<()> {
        let mut args = vec!["push"];
        if force {
            args.push("--force-with-lease");
        }
        if let Some((remote, branch)) = set_upstream {
            args.extend_from_slice(&["-u", remote, branch]);
        }
        self.run_with(&args, None, cancel).map(drop)
    }

    pub fn remotes(&self) -> Result<Vec<String>> {
        Ok(self
            .run(&["remote"])?
            .lines()
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .collect())
    }

    // ---- 常用 Git 能力 ----

    pub fn reset(&self, rev: &str, mode: ResetMode) -> Result<()> {
        self.run(&["reset", "-q", mode.flag(), rev]).map(drop)
    }

    pub fn stash_push(&self, message: Option<&str>, include_untracked: bool) -> Result<()> {
        let mut args = vec!["stash", "push"];
        if include_untracked {
            args.push("--include-untracked");
        }
        if let Some(m) = message {
            args.extend_from_slice(&["-m", m]);
        }
        self.run(&args).map(drop)
    }

    pub fn stash_apply(&self, index: usize, pop: bool) -> Result<()> {
        let r = format!("stash@{{{index}}}");
        self.run(&["stash", if pop { "pop" } else { "apply" }, "--index", &r])
            .map(drop)
    }

    pub fn stash_drop(&self, index: usize) -> Result<()> {
        let r = format!("stash@{{{index}}}");
        self.run(&["stash", "drop", &r]).map(drop)
    }

    /// 第 `index` 条 stash 的提交与列表里显示的描述。
    pub fn stash_entry(&self, index: usize) -> Result<(String, String)> {
        let out = self.run(&["stash", "list", "--format=%H%x00%gs"])?;
        let line = out
            .lines()
            .nth(index)
            .ok_or_else(|| Error::other(format!("没有 stash@{{{index}}}")))?;
        let (oid, msg) = line
            .split_once('\0')
            .ok_or_else(|| Error::other("无法解析 stash 列表"))?;
        Ok((oid.to_string(), msg.to_string()))
    }

    /// stash 提交 `commit` 当前在列表里的位置。
    pub fn stash_index_of(&self, commit: &str) -> Result<Option<usize>> {
        let out = self.run(&["stash", "list", "--format=%H"])?;
        Ok(out.lines().position(|l| l.trim() == commit))
    }

    /// 把一个 stash 提交放回 stash 列表顶部(撤销删除 stash 时使用)。
    pub fn stash_store(&self, commit: &str, message: &str) -> Result<()> {
        self.run(&["stash", "store", "-m", message, commit])
            .map(drop)
    }

    /// 不改动工作区地把当前改动做成一个 stash 提交,返回其 oid;没有改动时返回 None。
    pub fn stash_create(&self) -> Result<Option<String>> {
        let out = self.run(&["stash", "create"])?;
        let oid = out.trim();
        Ok((!oid.is_empty()).then(|| oid.to_string()))
    }

    pub fn cherry_pick(&self, rev: &str) -> Result<()> {
        self.run(&["cherry-pick", rev]).map(drop)
    }

    pub fn revert(&self, rev: &str) -> Result<()> {
        self.run(&["revert", "--no-edit", rev]).map(drop)
    }

    pub fn create_tag(&self, name: &str, rev: &str, message: Option<&str>) -> Result<()> {
        match message {
            Some(m) => self.run(&["tag", "-a", name, rev, "-m", m]).map(drop),
            None => self.run(&["tag", name, rev]).map(drop),
        }
    }

    pub fn delete_tag(&self, name: &str) -> Result<()> {
        self.run(&["tag", "-d", name]).map(drop)
    }

    pub fn merge(&self, rev: &str) -> Result<()> {
        self.run(&["merge", "--no-edit", rev]).map(drop)
    }

    pub fn abort_in_progress(&self, state: crate::RepoState) -> Result<()> {
        use crate::RepoState::*;
        let args: &[&str] = match state {
            Merge => &["merge", "--abort"],
            Rebase => &["rebase", "--abort"],
            CherryPick => &["cherry-pick", "--abort"],
            Revert => &["revert", "--abort"],
            Bisect => &["bisect", "reset"],
            Clean | Other => return Ok(()),
        };
        self.run(args).map(drop)
    }
}

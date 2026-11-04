//! 给“根据变更生成提交信息”准备的材料:待提交改动的文本补丁与最近几条提交标题。
//!
//! 优先取已暂存的改动;一个文件都没暂存时退回工作区的全部已跟踪改动(提交时会 `-a`
//! 之外的行为由调用方决定,这里只管读)。补丁整体与单个文件都有长度上限,生成的 / 巨大的
//! 文件不会把上下文占满。
//!
//! @author ixxxxoooo

use crate::{Repo, Result};
use git2::{DiffFormat, DiffOptions};

/// 单个文件最多带多少字节的补丁。
const PER_FILE_LIMIT: usize = 8_000;

/// 待提交改动的文本材料。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeContext {
    /// 统一 diff 文本,已按长度上限截断。
    pub patch: String,
    /// 是否有内容被截断(整体超限或单个文件超限)。
    pub truncated: bool,
    /// 取的是已暂存的改动;为 false 表示没有暂存、取的是工作区改动。
    pub staged: bool,
    /// 涉及的文件数。
    pub files: usize,
}

/// 读取待提交改动。`limit` 是补丁总字节上限。没有任何改动时 `files` 为 0、`patch` 为空。
pub fn change_context(repo: &Repo, limit: usize) -> Result<ChangeContext> {
    let raw = repo.raw();
    let head = raw.head().ok().and_then(|h| h.peel_to_tree().ok());
    let mut opts = DiffOptions::new();
    let mut diff = raw.diff_tree_to_index(head.as_ref(), None, Some(&mut opts))?;
    let mut staged = true;
    if diff.deltas().len() == 0 {
        let mut opts = DiffOptions::new();
        diff = raw.diff_index_to_workdir(None, Some(&mut opts))?;
        staged = false;
    }
    let files = diff.deltas().len();
    let mut out = ChangeContext {
        staged,
        files,
        ..Default::default()
    };
    let mut file_bytes = 0usize;
    let mut last_file = String::new();
    diff.print(DiffFormat::Patch, |delta, _, line| {
        let path = delta
            .new_file()
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path != last_file {
            last_file = path;
            file_bytes = 0;
        }
        let mut text = String::new();
        if matches!(line.origin(), '+' | '-' | ' ') {
            text.push(line.origin());
        }
        text.push_str(&String::from_utf8_lossy(line.content()));
        file_bytes += text.len();
        if file_bytes > PER_FILE_LIMIT || out.patch.len() + text.len() > limit {
            out.truncated = true;
        } else {
            out.patch.push_str(&text);
        }
        true
    })?;
    Ok(out)
}

/// 当前分支最近 `n` 条提交的标题,用来让生成的信息沿用仓库已有的写法。空仓库返回空列表。
pub fn recent_subjects(repo: &Repo, n: usize) -> Vec<String> {
    let raw = repo.raw();
    let Ok(mut walk) = raw.revwalk() else {
        return Vec::new();
    };
    if walk.push_head().is_err() {
        return Vec::new();
    }
    walk.take(n)
        .filter_map(|id| raw.find_commit(id.ok()?).ok())
        .map(|c| c.summary().ok().flatten().unwrap_or_default().to_string())
        .collect()
}

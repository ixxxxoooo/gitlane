//! Diff 读取与字符级差异。

use crate::types::{ChangeKind, FileChange};
use crate::{Repo, Result};
use git2::{Delta, DiffOptions, Oid, Patch};
use std::ops::Range;

/// 超过这个行数的 diff 默认只显示摘要,用户点“仍然显示”再加载。
pub const LARGE_DIFF_LINES: usize = 20_000;
/// diff 默认的上下文行数(与 git 一致),可在设置里修改。
pub const DEFAULT_CONTEXT_LINES: u32 = 3;
/// 超过这个长度的行不做字符级差异,避免 O(n·d) 的开销。
const INLINE_DIFF_MAX_LINE: usize = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffTarget {
    /// 提交相对第一个父提交。
    Commit(Oid),
    /// 工作区相对索引(未暂存)。
    Unstaged,
    /// 索引相对 HEAD(已暂存)。
    Staged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Deleted,
}

impl LineKind {
    pub fn prefix(self) -> char {
        match self {
            LineKind::Context => ' ',
            LineKind::Added => '+',
            LineKind::Deleted => '-',
        }
    }
}

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    /// 行内容,不含换行符。
    pub text: String,
    /// 这一行后面是否有 “\ No newline at end of file”。
    pub no_newline: bool,
}

#[derive(Clone, Debug)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, Default)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    /// 新文件(旧侧是 /dev/null)。
    pub is_new: bool,
    /// 删除的文件(新侧是 /dev/null)。
    pub is_deleted: bool,
    pub binary: bool,
    /// 超过 LARGE_DIFF_LINES,只返回了统计,没有 hunks。
    pub too_large: bool,
    pub additions: usize,
    pub deletions: usize,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    pub fn line_count(&self) -> usize {
        self.hunks.iter().map(|h| h.lines.len() + 1).sum()
    }
}

fn delta_kind(d: Delta) -> ChangeKind {
    match d {
        Delta::Added => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Renamed => ChangeKind::Renamed,
        Delta::Copied => ChangeKind::Copied,
        Delta::Typechange => ChangeKind::TypeChange,
        Delta::Untracked => ChangeKind::Untracked,
        Delta::Conflicted => ChangeKind::Conflicted,
        _ => ChangeKind::Modified,
    }
}

/// 汇总 diff 中每个文件的改动类型与增删行数。
pub fn file_changes(diff: &git2::Diff<'_>) -> Result<Vec<FileChange>> {
    let mut files = Vec::with_capacity(diff.deltas().len());
    for (i, delta) in diff.deltas().enumerate() {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let old_path = delta
            .old_file()
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| *p != path);
        let (additions, deletions) = match Patch::from_diff(diff, i)? {
            Some(p) => {
                let (_, a, d) = p.line_stats()?;
                (a, d)
            }
            None => (0, 0),
        };
        files.push(FileChange {
            path,
            old_path,
            kind: delta_kind(delta.status()),
            additions,
            deletions,
        });
    }
    Ok(files)
}

/// 读取单个文件的 diff。`force_full` 为 true 时忽略大文件保护;`context_lines` 至少为 1,
/// 部分暂存生成的补丁需要上下文才能被 `git apply` 定位。
pub fn file_diff(
    repo: &Repo,
    target: DiffTarget,
    path: &str,
    force_full: bool,
    context_lines: u32,
) -> Result<FileDiff> {
    let context_lines = context_lines.max(1);
    let raw = repo.raw();
    let mut opts = DiffOptions::new();
    opts.pathspec(path)
        .disable_pathspec_match(true)
        .context_lines(context_lines);

    let mut diff = match target {
        DiffTarget::Commit(id) => {
            let c = raw.find_commit(id)?;
            let tree = c.tree()?;
            let parent = c.parent(0).ok().map(|p| p.tree()).transpose()?;
            // 重命名检测需要看到全部文件,所以提交 diff 不限定 pathspec,下面再挑出目标文件
            let mut all = DiffOptions::new();
            all.context_lines(context_lines);
            let mut d = raw.diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut all))?;
            d.find_similar(None)?;
            d
        }
        DiffTarget::Staged => {
            let head = raw.head().ok().and_then(|h| h.peel_to_tree().ok());
            raw.diff_tree_to_index(head.as_ref(), None, Some(&mut opts))?
        }
        DiffTarget::Unstaged => {
            opts.include_untracked(true)
                .recurse_untracked_dirs(true)
                .show_untracked_content(true);
            raw.diff_index_to_workdir(None, Some(&mut opts))?
        }
    };
    if !matches!(target, DiffTarget::Commit(_)) {
        diff.find_similar(None)?;
    }

    let idx = diff.deltas().position(|d| {
        d.new_file()
            .path()
            .is_some_and(|p| p.to_string_lossy() == path)
            || d.old_file()
                .path()
                .is_some_and(|p| p.to_string_lossy() == path)
    });
    let Some(idx) = idx else {
        return Ok(FileDiff {
            path: path.to_string(),
            ..Default::default()
        });
    };

    let delta = diff.get_delta(idx).expect("delta exists");
    let mut out = FileDiff {
        path: path.to_string(),
        old_path: delta
            .old_file()
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| p != path),
        is_new: matches!(delta.status(), Delta::Added | Delta::Untracked),
        is_deleted: delta.status() == Delta::Deleted,
        binary: delta.flags().is_binary(),
        ..Default::default()
    };

    let Some(patch) = Patch::from_diff(&diff, idx)? else {
        out.binary = true;
        return Ok(out);
    };
    let (_, additions, deletions) = patch.line_stats()?;
    out.additions = additions;
    out.deletions = deletions;
    if !force_full && additions + deletions > LARGE_DIFF_LINES {
        out.too_large = true;
        return Ok(out);
    }

    for h in 0..patch.num_hunks() {
        let (hunk, n) = patch.hunk(h)?;
        let mut lines: Vec<DiffLine> = Vec::with_capacity(n);
        for l in 0..n {
            let line = patch.line_in_hunk(h, l)?;
            let kind = match line.origin() {
                '+' => LineKind::Added,
                '-' => LineKind::Deleted,
                ' ' => LineKind::Context,
                // '=' '>' '<':文件末尾没有换行的标记
                _ => {
                    if let Some(last) = lines.last_mut() {
                        last.no_newline = true;
                    }
                    continue;
                }
            };
            let mut text = String::from_utf8_lossy(line.content()).into_owned();
            if text.ends_with('\n') {
                text.pop();
                if text.ends_with('\r') {
                    text.pop();
                }
            }
            lines.push(DiffLine {
                kind,
                old_no: line.old_lineno(),
                new_no: line.new_lineno(),
                text,
                no_newline: false,
            });
        }
        out.hunks.push(Hunk {
            header: String::from_utf8_lossy(hunk.header())
                .trim_end()
                .to_string(),
            old_start: hunk.old_start(),
            old_lines: hunk.old_lines(),
            new_start: hunk.new_start(),
            new_lines: hunk.new_lines(),
            lines,
        });
    }
    Ok(out)
}

/// 读取 diff 两侧的完整文件内容(旧侧, 新侧),供语法高亮从文件开头解析。
/// 新增 / 删除的文件只有一侧;读不到、二进制或超过 `max_bytes` 的一侧为 `None`。
pub fn side_texts(
    repo: &Repo,
    target: DiffTarget,
    file: &FileDiff,
    max_bytes: usize,
) -> (Option<String>, Option<String>) {
    let raw = repo.raw();
    let old_path = file.old_path.as_deref().unwrap_or(&file.path);
    let blob_text = |id: Oid| -> Option<String> {
        let blob = raw.find_blob(id).ok()?;
        (!blob.is_binary() && blob.size() <= max_bytes)
            .then(|| String::from_utf8_lossy(blob.content()).into_owned())
    };
    let in_tree = |tree: Option<git2::Tree<'_>>, path: &str| {
        let id = tree?.get_path(std::path::Path::new(path)).ok()?.id();
        blob_text(id)
    };
    let in_index = |path: &str| {
        let index = raw.index().ok()?;
        let id = index.get_path(std::path::Path::new(path), 0)?.id;
        blob_text(id)
    };
    let head_tree = || raw.head().ok().and_then(|h| h.peel_to_tree().ok());

    let (old, new) = match target {
        DiffTarget::Commit(id) => {
            let Ok(c) = raw.find_commit(id) else {
                return (None, None);
            };
            let parent = c.parent(0).ok().and_then(|p| p.tree().ok());
            (
                in_tree(parent, old_path),
                in_tree(c.tree().ok(), &file.path),
            )
        }
        DiffTarget::Staged => (in_tree(head_tree(), old_path), in_index(&file.path)),
        DiffTarget::Unstaged => {
            let new = std::fs::read(repo.workdir().join(&file.path))
                .ok()
                .filter(|b| b.len() <= max_bytes && !b.contains(&0))
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            (in_index(old_path), new)
        }
    };
    (
        old.filter(|_| !file.is_new),
        new.filter(|_| !file.is_deleted),
    )
}

/// 把 hunk 中相邻的“删除块 + 新增块”逐行配对,返回 (删除行下标, 新增行下标)。
/// 配对后的两行用于字符级差异和并排视图对齐。
pub fn pair_changed_lines(hunk: &Hunk) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let lines = &hunk.lines;
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind != LineKind::Deleted {
            i += 1;
            continue;
        }
        let del_start = i;
        while i < lines.len() && lines[i].kind == LineKind::Deleted {
            i += 1;
        }
        let add_start = i;
        while i < lines.len() && lines[i].kind == LineKind::Added {
            i += 1;
        }
        let n = (add_start - del_start).min(i - add_start);
        pairs.extend((0..n).map(|k| (del_start + k, add_start + k)));
    }
    pairs
}

/// 字符级差异:返回旧行与新行中发生变化的字节区间。
/// 只有少量变化时才有意义;两行几乎完全不同则返回空,避免整行花屏。
pub fn inline_changes(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    if old.len() > INLINE_DIFF_MAX_LINE || new.len() > INLINE_DIFF_MAX_LINE {
        return (Vec::new(), Vec::new());
    }
    let diff = similar::TextDiff::configure()
        .algorithm(similar::Algorithm::Patience)
        .diff_chars(old, new);
    if diff.ratio() < 0.3 {
        return (Vec::new(), Vec::new());
    }
    let mut old_ranges: Vec<Range<usize>> = Vec::new();
    let mut new_ranges: Vec<Range<usize>> = Vec::new();
    let (mut oi, mut ni) = (0usize, 0usize);
    for change in diff.iter_all_changes() {
        let len = change.value().len();
        match change.tag() {
            similar::ChangeTag::Equal => {
                oi += len;
                ni += len;
            }
            similar::ChangeTag::Delete => {
                push_range(&mut old_ranges, oi..oi + len);
                oi += len;
            }
            similar::ChangeTag::Insert => {
                push_range(&mut new_ranges, ni..ni + len);
                ni += len;
            }
        }
    }
    (old_ranges, new_ranges)
}

fn push_range(ranges: &mut Vec<Range<usize>>, r: Range<usize>) {
    match ranges.last_mut() {
        Some(last) if last.end == r.start => last.end = r.end,
        _ => ranges.push(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: LineKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no: None,
            new_no: None,
            text: text.into(),
            no_newline: false,
        }
    }

    #[test]
    fn inline_changes_marks_only_changed_chars() {
        let (o, n) = inline_changes("let x = 1;", "let x = 42;");
        assert_eq!(&"let x = 1;"[o[0].clone()], "1");
        assert_eq!(&"let x = 42;"[n[0].clone()], "42");
    }

    #[test]
    fn inline_changes_skips_unrelated_lines() {
        let (o, n) = inline_changes("abcdef", "uvwxyz");
        assert!(o.is_empty() && n.is_empty());
    }

    #[test]
    fn inline_changes_handles_multibyte() {
        let old = "提交信息";
        let new = "提交说明";
        let (o, n) = inline_changes(old, new);
        assert_eq!(&old[o[0].clone()], "信息");
        assert_eq!(&new[n[0].clone()], "说明");
    }

    #[test]
    fn pairs_delete_add_blocks() {
        use LineKind::*;
        let hunk = Hunk {
            header: String::new(),
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: 0,
            lines: vec![
                line(Context, "a"),
                line(Deleted, "b"),
                line(Deleted, "c"),
                line(Added, "B"),
                line(Context, "d"),
                line(Added, "e"),
            ],
        };
        assert_eq!(pair_changed_lines(&hunk), vec![(1, 3)]);
    }
}

//! hunk 级与行级暂存:从 diff 中挑出部分行生成补丁,交给 `git apply --cached`。

use crate::diff::{FileDiff, LineKind};
use std::collections::BTreeSet;

/// 补丁应用方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchDirection {
    /// 暂存:diff 来自“索引 → 工作区”,正向应用到索引。
    Stage,
    /// 取消暂存:diff 来自“HEAD → 索引”,用 `--reverse` 应用到索引。
    Unstage,
    /// 丢弃工作区改动:diff 来自“索引 → 工作区”,用 `--reverse` 应用到工作区。
    Discard,
}

/// 生成只包含指定 hunk、指定行的补丁。
///
/// `selected` 是 hunk 内行下标的集合;`None` 表示整个 hunk。未选中的行按方向处理:
/// - 正向(Stage):未选中的 `+` 行丢弃,未选中的 `-` 行变为上下文;
/// - 反向(Unstage/Discard):未选中的 `+` 行变为上下文,未选中的 `-` 行丢弃。
///
/// 没有任何改动行被选中时返回 `None`。
pub fn build_patch(
    file: &FileDiff,
    hunk_index: usize,
    selected: Option<&BTreeSet<usize>>,
    dir: PatchDirection,
) -> Option<String> {
    let hunk = file.hunks.get(hunk_index)?;
    let reverse = dir != PatchDirection::Stage;
    let is_selected = |i: usize| selected.is_none_or(|s| s.contains(&i));

    let mut body = String::new();
    let (mut old_count, mut new_count) = (0u32, 0u32);
    let mut changed = false;
    for (i, line) in hunk.lines.iter().enumerate() {
        let out_kind = match (line.kind, is_selected(i), reverse) {
            (LineKind::Context, _, _) => Some(' '),
            (LineKind::Added, true, _) => Some('+'),
            (LineKind::Deleted, true, _) => Some('-'),
            (LineKind::Added, false, false) => None,
            (LineKind::Deleted, false, false) => Some(' '),
            (LineKind::Added, false, true) => Some(' '),
            (LineKind::Deleted, false, true) => None,
        };
        let Some(k) = out_kind else { continue };
        match k {
            ' ' => {
                old_count += 1;
                new_count += 1;
            }
            '+' => {
                new_count += 1;
                changed = true;
            }
            _ => {
                old_count += 1;
                changed = true;
            }
        }
        body.push(k);
        body.push_str(&line.text);
        body.push('\n');
        if line.no_newline {
            body.push_str("\\ No newline at end of file\n");
        }
    }
    if !changed {
        return None;
    }

    let old_path = file.old_path.as_deref().unwrap_or(&file.path);
    // 部分暂存新文件时,索引里还没有这个文件,旧侧必须是 /dev/null;
    // 但取消暂存的部分行时文件已在索引中,补丁必须保持与 diff 相同的形状。
    let old_side = if file.is_new && old_count == 0 {
        "/dev/null".to_string()
    } else {
        format!("a/{old_path}")
    };
    let new_side = if file.is_deleted && new_count == 0 {
        "/dev/null".to_string()
    } else {
        format!("b/{}", file.path)
    };

    let mut out = format!("diff --git a/{old_path} b/{}\n", file.path);
    if old_side == "/dev/null" {
        out.push_str("new file mode 100644\n");
    } else if new_side == "/dev/null" {
        out.push_str("deleted file mode 100644\n");
    }
    out.push_str(&format!("--- {old_side}\n+++ {new_side}\n"));
    let old_start = if old_count == 0 {
        0
    } else {
        hunk.old_start.max(1)
    };
    let new_start = if new_count == 0 {
        0
    } else {
        hunk.new_start.max(1)
    };
    out.push_str(&format!(
        "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
    ));
    out.push_str(&body);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffLine, Hunk};

    fn l(kind: LineKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no: None,
            new_no: None,
            text: text.into(),
            no_newline: false,
        }
    }

    fn sample() -> FileDiff {
        use LineKind::*;
        FileDiff {
            path: "a.txt".into(),
            hunks: vec![Hunk {
                header: "@@ -1,3 +1,3 @@".into(),
                old_start: 1,
                old_lines: 3,
                new_start: 1,
                new_lines: 3,
                lines: vec![
                    l(Context, "one"),
                    l(Deleted, "two"),
                    l(Deleted, "three"),
                    l(Added, "TWO"),
                    l(Added, "THREE"),
                ],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn whole_hunk() {
        let p = build_patch(&sample(), 0, None, PatchDirection::Stage).unwrap();
        assert!(p.contains("@@ -1,3 +1,3 @@\n one\n-two\n-three\n+TWO\n+THREE\n"));
    }

    #[test]
    fn stage_single_line_pair() {
        let sel: BTreeSet<usize> = [1, 3].into();
        let p = build_patch(&sample(), 0, Some(&sel), PatchDirection::Stage).unwrap();
        // 未选中的 '-three' 变为上下文,未选中的 '+THREE' 丢弃
        assert!(
            p.ends_with("@@ -1,3 +1,3 @@\n one\n-two\n three\n+TWO\n"),
            "{p}"
        );
    }

    #[test]
    fn unstage_single_line_pair() {
        let sel: BTreeSet<usize> = [1, 3].into();
        let p = build_patch(&sample(), 0, Some(&sel), PatchDirection::Unstage).unwrap();
        // 反向:未选中的 '-three' 丢弃,未选中的 '+THREE' 变为上下文
        assert!(
            p.ends_with("@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n THREE\n"),
            "{p}"
        );
    }

    #[test]
    fn nothing_selected() {
        let sel: BTreeSet<usize> = [0].into();
        assert!(build_patch(&sample(), 0, Some(&sel), PatchDirection::Stage).is_none());
    }
}

//! diff 视图与 git 操作之间的桥:把组件里选中的行范围换算成 (hunk, 行),再生成暂存 / 丢弃命令;
//! 以及复制与语法高亮的接线。
//!
//! @author ixxxxoooo

use crate::colors::syntax_color;
use crate::repo_view::RepoView;
use app::Command;
use gitcore::diff::{DiffTarget, FileDiff};
use gitcore::patch::PatchDirection;
use gpui::component::WindowExt as _;
use gpui::component::diff::{DiffEvent, DiffLineRange, DiffSide};
use gpui::*;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::Arc;

/// 当前显示的 diff:按文件指针识别是否换了文件,`file` 供暂存时按 hunk 与行号生成补丁。
pub struct DiffCache {
    pub key: usize,
    pub file: Arc<FileDiff>,
}

/// 组件选中的行范围对应到文件里的 (hunk, 行) 集合。
///
/// 范围的两端各带“哪一侧 + 该侧行号”,按显示顺序取两端之间的所有行;
/// `copy_only` 为真(提交 diff,只读)时上下文行也算,否则只留改动行(能暂存的行)。
/// 找不到端点(行号不在这个 diff 里)时返回空集合。
pub fn lines_in_range(
    file: &FileDiff,
    range: &DiffLineRange,
    copy_only: bool,
) -> BTreeSet<(usize, usize)> {
    let flat: Vec<(usize, usize)> = file
        .hunks
        .iter()
        .enumerate()
        .flat_map(|(h, hunk)| (0..hunk.lines.len()).map(move |l| (h, l)))
        .collect();
    let locate = |side: DiffSide, no: usize| {
        flat.iter().position(|&(h, l)| {
            let line = &file.hunks[h].lines[l];
            let own = match side {
                DiffSide::Original => line.old_no,
                DiffSide::Modified => line.new_no,
            };
            own == Some(no as u32)
        })
    };
    let (Some(a), Some(b)) = (
        locate(range.side(), range.start()),
        locate(range.end_side(), range.end()),
    ) else {
        return BTreeSet::new();
    };
    let (a, b) = (a.min(b), a.max(b));
    flat[a..=b]
        .iter()
        .copied()
        .filter(|&(h, l)| {
            copy_only || file.hunks[h].lines[l].kind != gitcore::diff::LineKind::Context
        })
        .collect()
}

/// 把 gitcore 的高亮名换成组件要的 (区间, 样式),颜色按亮暗主题取。
///
/// 参数:`dark` 是否暗色主题。返回的闭包接收 (语言, 一侧源码),语言我们约定传文件路径。
pub fn syntax_highlighter(
    dark: bool,
) -> impl Fn(&str, &str) -> Vec<(Range<usize>, HighlightStyle)> + Send + Sync + 'static {
    move |path, text| {
        gitcore::highlight::highlight_source(path, text)
            .into_iter()
            .filter_map(|(range, name)| {
                let color = syntax_color(name, dark)?;
                Some((
                    range,
                    HighlightStyle {
                        color: Some(color),
                        ..Default::default()
                    },
                ))
            })
            .collect()
    }
}

impl RepoView {
    /// 提交 diff 只读,选行是为了复制(上下文行也能选)。
    fn diff_copy_only(&self, cx: &App) -> bool {
        self.repo
            .read(cx)
            .diff
            .as_ref()
            .is_some_and(|d| matches!(d.target, DiffTarget::Commit(_)))
    }

    /// 组件的选区变化:同步成 (hunk, 行) 集合,头部的“暂存选中的 N 行”按钮据此出现。
    pub(crate) fn on_diff_event(&mut self, event: &DiffEvent, cx: &mut Context<Self>) {
        let DiffEvent::SelectionChanged(range) = event else {
            return;
        };
        self.select_range(range.as_ref(), cx);
    }

    /// 设置(或清除)选中的行。
    pub(crate) fn select_range(&mut self, range: Option<&DiffLineRange>, cx: &mut Context<Self>) {
        let copy_only = self.diff_copy_only(cx);
        self.selected_lines = match (range, &self.diff_cache) {
            (Some(range), Some(cache)) => lines_in_range(&cache.file, range, copy_only),
            _ => BTreeSet::new(),
        };
        cx.notify();
    }

    /// 清掉选中的行(组件里的高亮一并清掉)。
    fn clear_selected(&mut self, cx: &mut Context<Self>) {
        self.selected_lines.clear();
        self.diff_state
            .update(cx, |s, cx| s.set_selected_lines(None, cx));
    }

    /// 把选中的行按 hunk 分组,各生成一个补丁命令排队执行。
    pub(crate) fn apply_selected(&mut self, dir: PatchDirection, cx: &mut Context<Self>) {
        let Some(cache) = &self.diff_cache else {
            return;
        };
        let file = cache.file.clone();
        let mut by_hunk: BTreeMap<usize, BTreeSet<usize>> = Default::default();
        for &(h, l) in &self.selected_lines {
            by_hunk.entry(h).or_default().insert(l);
        }
        // 从后往前应用,前面 hunk 的行号不受影响
        for (hunk, lines) in by_hunk.into_iter().rev() {
            self.run(
                Command::ApplySelection {
                    file: file.clone(),
                    hunk,
                    lines: Some(lines),
                    dir,
                },
                cx,
            );
        }
        self.clear_selected(cx);
        cx.notify();
    }

    /// 整个 hunk 暂存 / 取消暂存 / 丢弃。
    pub(crate) fn apply_hunk(&mut self, hunk: usize, dir: PatchDirection, cx: &mut Context<Self>) {
        let Some(cache) = &self.diff_cache else {
            return;
        };
        let file = cache.file.clone();
        self.run(
            Command::ApplySelection {
                file,
                hunk,
                lines: None,
                dir,
            },
            cx,
        );
        self.clear_selected(cx);
    }

    /// 选中的 diff 行的文字(按行号顺序,不含 +/- 前缀),没有选中返回 `None`。
    pub(crate) fn selected_text(&self) -> Option<String> {
        let cache = self.diff_cache.as_ref()?;
        let lines: Vec<&str> = self
            .selected_lines
            .iter()
            .filter_map(|&(h, l)| cache.file.hunks.get(h)?.lines.get(l))
            .map(|x| x.text.as_str())
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }

    /// ⌘C / 右键“复制”:优先复制组件里拖选的文字,其次是按行号选中的行;都没有时不处理,让别处的复制照常工作。
    pub(crate) fn on_copy_diff_lines(
        &mut self,
        _: &crate::actions::CopyDiffLines,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picked = self.diff_state.read(cx).selected_text(cx);
        let text = if picked.is_empty() {
            self.selected_text()
        } else {
            Some(picked)
        };
        match text {
            Some(text) => {
                let n = text.lines().count();
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                window.push_notification(
                    gpui::component::notification::Notification::info(format!("已复制 {n} 行")),
                    cx,
                );
            }
            None => cx.propagate(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::lines_in_range;
    use gitcore::diff::{DiffLine, FileDiff, Hunk, LineKind};
    use gpui::component::diff::{DiffLineRange, DiffSide};

    fn line(kind: LineKind, old: Option<u32>, new: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            old_no: old,
            new_no: new,
            text: String::new(),
            no_newline: false,
        }
    }

    /// 上下文 1 / 删除 2 / 新增 2 / 新增 3 / 上下文 3→4
    fn sample() -> FileDiff {
        use LineKind::*;
        FileDiff {
            path: "a.rs".into(),
            old_path: None,
            is_new: false,
            is_deleted: false,
            binary: false,
            too_large: false,
            additions: 2,
            deletions: 1,
            hunks: vec![Hunk {
                header: "@@ -1,3 +1,4 @@".into(),
                old_start: 1,
                old_lines: 3,
                new_start: 1,
                new_lines: 4,
                lines: vec![
                    line(Context, Some(1), Some(1)),
                    line(Deleted, Some(2), None),
                    line(Added, None, Some(2)),
                    line(Added, None, Some(3)),
                    line(Context, Some(3), Some(4)),
                ],
            }],
        }
    }

    #[test]
    fn range_on_modified_side_covers_the_deleted_line_in_between() {
        let r = DiffLineRange::new("a.rs", DiffSide::Modified, 1, 3);
        let got = lines_in_range(&sample(), &r, false);
        // 上下文行不能暂存;删除行夹在中间,按显示顺序算选中
        assert_eq!(
            got.into_iter().collect::<Vec<_>>(),
            vec![(0, 1), (0, 2), (0, 3)]
        );
    }

    #[test]
    fn copy_only_keeps_context_lines() {
        let r = DiffLineRange::new("a.rs", DiffSide::Modified, 1, 4);
        assert_eq!(lines_in_range(&sample(), &r, true).len(), 5);
    }

    #[test]
    fn range_from_original_to_modified_side_and_unknown_lines() {
        let r =
            DiffLineRange::new("a.rs", DiffSide::Original, 2, 2).with_end_side(DiffSide::Modified);
        let got = lines_in_range(&sample(), &r, false);
        assert_eq!(got.into_iter().collect::<Vec<_>>(), vec![(0, 1), (0, 2)]);
        // 行号不在 diff 里:空集合
        let missing = DiffLineRange::new("a.rs", DiffSide::Modified, 40, 41);
        assert!(lines_in_range(&sample(), &missing, false).is_empty());
    }
}

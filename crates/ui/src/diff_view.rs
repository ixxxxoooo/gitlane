//! Diff 视图:统一 / 并排两种布局、字符级差异、minimap,工作区 diff 支持 hunk 与单行暂存。

use crate::colors::diff_colors;
use crate::repo_view::RepoView;
use app::Command;
use gitcore::diff::{DiffTarget, FileDiff, LineKind, inline_changes, pair_changed_lines};
use gitcore::patch::PatchDirection;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::{ActiveTheme, Sizable, h_flex, v_flex};
use gpui::prelude::*;
use gpui::*;
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::sync::Arc;

const LINE_HEIGHT: f32 = 20.;
const MINIMAP_WIDTH: f32 = 10.;

#[derive(Clone, Copy, Debug)]
pub enum DiffRow {
    Header(usize),
    /// 统一视图的一行:(hunk, 行)。
    Line(usize, usize),
    /// 并排视图的一行:左侧旧版本、右侧新版本。
    Split(Option<(usize, usize)>, Option<(usize, usize)>),
}

/// 按 diff 内容缓存的行布局与字符级差异,diff 或视图模式变化时重建。
pub struct DiffCache {
    pub key: usize,
    pub split: bool,
    pub file: Arc<FileDiff>,
    pub rows: Vec<DiffRow>,
    pub inline: HashMap<(usize, usize), Vec<Range<usize>>>,
}

impl DiffCache {
    fn build(file: Arc<FileDiff>, split: bool) -> Self {
        let mut rows = Vec::new();
        let mut inline = HashMap::new();
        for (h, hunk) in file.hunks.iter().enumerate() {
            rows.push(DiffRow::Header(h));
            for (d, a) in pair_changed_lines(hunk) {
                let (o, n) = inline_changes(&hunk.lines[d].text, &hunk.lines[a].text);
                if !o.is_empty() {
                    inline.insert((h, d), o);
                }
                if !n.is_empty() {
                    inline.insert((h, a), n);
                }
            }
            if !split {
                rows.extend((0..hunk.lines.len()).map(|i| DiffRow::Line(h, i)));
                continue;
            }
            // 并排:上下文两侧同一行;连续的删除块与新增块逐行对齐
            let lines = &hunk.lines;
            let mut i = 0;
            while i < lines.len() {
                match lines[i].kind {
                    LineKind::Context => {
                        rows.push(DiffRow::Split(Some((h, i)), Some((h, i))));
                        i += 1;
                    }
                    _ => {
                        let mut dels = Vec::new();
                        while i < lines.len() && lines[i].kind == LineKind::Deleted {
                            dels.push(i);
                            i += 1;
                        }
                        let mut adds = Vec::new();
                        while i < lines.len() && lines[i].kind == LineKind::Added {
                            adds.push(i);
                            i += 1;
                        }
                        for k in 0..dels.len().max(adds.len()) {
                            rows.push(DiffRow::Split(
                                dels.get(k).map(|&l| (h, l)),
                                adds.get(k).map(|&l| (h, l)),
                            ));
                        }
                    }
                }
            }
        }
        Self {
            key: Arc::as_ptr(&file) as usize,
            split,
            file,
            rows,
            inline,
        }
    }
}

/// 制表符展开为 4 个空格,同时平移高亮区间。
fn expand_tabs(text: &str, ranges: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    if !text.contains('\t') {
        return (text.to_string(), ranges.to_vec());
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut map = Vec::with_capacity(text.len() + 1);
    for (i, ch) in text.char_indices() {
        while map.len() < i {
            map.push(out.len());
        }
        map.push(out.len());
        if ch == '\t' {
            out.push_str("    ");
        } else {
            out.push(ch);
        }
    }
    while map.len() <= text.len() {
        map.push(out.len());
    }
    let ranges = ranges.iter().map(|r| map[r.start]..map[r.end]).collect();
    (out, ranges)
}

impl RepoView {
    pub(crate) fn render_diff(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let split = self
            .workspace
            .upgrade()
            .is_some_and(|ws| ws.read(cx).settings.split_diff);
        let repo = self.repo.read(cx);
        let Some(state) = repo.diff.as_ref() else {
            return div().into_any_element();
        };
        let target = state.target;
        let path = state.path.clone();
        let error = state.error.clone();
        let file = state.file.clone();

        let header = self.render_diff_header(&path, target, file.as_deref(), split, cx);
        let body: AnyElement = match (error, file) {
            (Some(e), _) => div()
                .p_4()
                .text_color(theme.danger)
                .child(e)
                .into_any_element(),
            (None, None) => div()
                .p_4()
                .text_color(theme.muted_foreground)
                .child("加载中…")
                .into_any_element(),
            (None, Some(f)) if f.binary => div()
                .p_4()
                .text_color(theme.muted_foreground)
                .child("二进制文件,不显示内容")
                .into_any_element(),
            (None, Some(f)) if f.too_large => v_flex()
                .p_4()
                .gap_2()
                .items_start()
                .child(format!(
                    "改动较大(+{} −{}),为避免卡顿默认只显示摘要。",
                    f.additions, f.deletions
                ))
                .child(Button::new("show-full").small().label("仍然显示").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.repo.update(cx, |r, cx| r.open_full_diff(cx))
                    }),
                ))
                .into_any_element(),
            (None, Some(f)) if f.hunks.is_empty() => div()
                .p_4()
                .text_color(theme.muted_foreground)
                .child("没有文本改动(可能只改了权限)")
                .into_any_element(),
            (None, Some(f)) => {
                let key = Arc::as_ptr(&f) as usize;
                if self
                    .diff_cache
                    .as_ref()
                    .is_none_or(|c| c.key != key || c.split != split)
                {
                    self.diff_cache = Some(DiffCache::build(f, split));
                }
                let rows = self.diff_cache.as_ref().map_or(0, |c| c.rows.len());
                let list = uniform_list(
                    "diff-lines",
                    rows,
                    cx.processor(move |this, range: Range<usize>, window, cx| {
                        this.render_diff_rows(range, target, window, cx)
                    }),
                )
                .track_scroll(&self.diff_scroll)
                .flex_1()
                .h_full();
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(list)
                    .child(self.render_minimap(window, cx))
                    .into_any_element()
            }
        };

        v_flex()
            .size_full()
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn render_diff_header(
        &self,
        path: &str,
        target: DiffTarget,
        file: Option<&FileDiff>,
        split: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let n_selected = self.selected_lines.len();
        let mut bar = h_flex()
            .h(px(32.))
            .px_2()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .font_family("Menlo")
                    .text_sm()
                    .child(path.to_string()),
            )
            .children(file.map(|f| {
                h_flex()
                    .gap_1()
                    .text_xs()
                    .child(
                        div()
                            .text_color(theme.green)
                            .child(format!("+{}", f.additions)),
                    )
                    .child(
                        div()
                            .text_color(theme.red)
                            .child(format!("−{}", f.deletions)),
                    )
            }));
        if n_selected > 0 {
            match target {
                DiffTarget::Unstaged => {
                    bar = bar
                        .child(
                            Button::new("stage-lines")
                                .xsmall()
                                .primary()
                                .label(format!("暂存选中的 {n_selected} 行"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.apply_selected(PatchDirection::Stage, cx)
                                })),
                        )
                        .child(
                            Button::new("discard-lines")
                                .xsmall()
                                .danger()
                                .label("丢弃选中行")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.apply_selected(PatchDirection::Discard, cx)
                                })),
                        );
                }
                DiffTarget::Staged => {
                    bar = bar.child(
                        Button::new("unstage-lines")
                            .xsmall()
                            .primary()
                            .label(format!("取消暂存选中的 {n_selected} 行"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.apply_selected(PatchDirection::Unstage, cx)
                            })),
                    );
                }
                DiffTarget::Commit(_) => {}
            }
        }
        bar.child(
            Button::new("toggle-split")
                .xsmall()
                .ghost()
                .label(if split { "统一" } else { "并排" })
                .tooltip("切换统一 / 并排(⌘\\)")
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::ToggleSplitDiff), cx)
                }),
        )
        .child(
            Button::new("close-diff")
                .xsmall()
                .ghost()
                .icon(gpui::component::IconName::Close)
                .on_click(
                    cx.listener(|this, _, _, cx| this.repo.update(cx, |r, cx| r.close_diff(cx))),
                ),
        )
    }

    /// 把选中的行按 hunk 分组,各生成一个补丁命令排队执行。
    fn apply_selected(&mut self, dir: PatchDirection, cx: &mut Context<Self>) {
        let Some(cache) = &self.diff_cache else {
            return;
        };
        let file = cache.file.clone();
        let mut by_hunk: std::collections::BTreeMap<usize, BTreeSet<usize>> = Default::default();
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
        self.selected_lines.clear();
        cx.notify();
    }

    fn apply_hunk(&mut self, hunk: usize, dir: PatchDirection, cx: &mut Context<Self>) {
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
        self.selected_lines.clear();
    }

    fn toggle_line(&mut self, hunk: usize, line: usize, shift: bool, cx: &mut Context<Self>) {
        let Some(cache) = &self.diff_cache else {
            return;
        };
        let is_change = |h: usize, l: usize| {
            cache.file.hunks[h]
                .lines
                .get(l)
                .is_some_and(|x| x.kind != LineKind::Context)
        };
        if !is_change(hunk, line) {
            return;
        }
        match (shift, self.last_clicked_line) {
            // Shift 点击:选中同一 hunk 内从上次点击处到这里的全部改动行
            (true, Some((h, l))) if h == hunk => {
                let (a, b) = if l <= line { (l, line) } else { (line, l) };
                for i in a..=b {
                    if is_change(hunk, i) {
                        self.selected_lines.insert((hunk, i));
                    }
                }
            }
            _ => {
                if !self.selected_lines.remove(&(hunk, line)) {
                    self.selected_lines.insert((hunk, line));
                }
            }
        }
        self.last_clicked_line = Some((hunk, line));
        cx.notify();
    }

    fn render_diff_rows(
        &mut self,
        range: Range<usize>,
        target: DiffTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(cache) = &self.diff_cache else {
            return Vec::new();
        };
        let theme = cx.theme().clone();
        let colors = diff_colors(theme.is_dark());
        let text_style = window.text_style();
        let selectable = !matches!(target, DiffTarget::Commit(_));
        let file = cache.file.clone();
        let rows: Vec<DiffRow> = cache.rows[range.clone()].to_vec();

        let line_cell =
            |h: usize, l: usize, show_old: bool, show_new: bool, selected: bool| -> Div {
                let line = &file.hunks[h].lines[l];
                let (bg, word_bg) = match line.kind {
                    LineKind::Added => (colors.added_bg, colors.added_word),
                    LineKind::Deleted => (colors.deleted_bg, colors.deleted_word),
                    LineKind::Context => (gpui::transparent_black(), gpui::transparent_black()),
                };
                let empty = Vec::new();
                let ranges = cache.inline.get(&(h, l)).unwrap_or(&empty);
                let (text, ranges) = expand_tabs(&line.text, ranges);
                let highlights: Vec<(Range<usize>, HighlightStyle)> = ranges
                    .into_iter()
                    .map(|r| {
                        (
                            r,
                            HighlightStyle {
                                background_color: Some(word_bg),
                                ..Default::default()
                            },
                        )
                    })
                    .collect();
                let no = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(if selected { colors.selected_bg } else { bg })
                    .when(show_old, |d| {
                        d.child(
                            div()
                                .w(px(44.))
                                .flex_none()
                                .pr_1()
                                .text_right()
                                .text_color(theme.muted_foreground)
                                .child(no(line.old_no)),
                        )
                    })
                    .when(show_new, |d| {
                        d.child(
                            div()
                                .w(px(44.))
                                .flex_none()
                                .pr_1()
                                .text_right()
                                .text_color(theme.muted_foreground)
                                .child(no(line.new_no)),
                        )
                    })
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .text_color(theme.muted_foreground)
                            .child(line.kind.prefix().to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(
                                StyledText::new(text)
                                    .with_default_highlights(&text_style, highlights),
                            ),
                    )
            };

        rows.into_iter()
            .enumerate()
            .map(|(i, row)| {
                let ix = range.start + i;
                let base = h_flex()
                    .id(("diff-row", ix))
                    .h(px(LINE_HEIGHT))
                    .w_full()
                    .font_family("Menlo")
                    .text_xs();
                match row {
                    DiffRow::Header(h) => {
                        let hunk = &file.hunks[h];
                        let mut el = base
                            .bg(colors.hunk_bg)
                            .px_2()
                            .gap_2()
                            .text_color(theme.muted_foreground)
                            .child(div().flex_1().truncate().child(hunk.header.clone()));
                        match target {
                            DiffTarget::Unstaged => {
                                el = el
                                    .child(
                                        Button::new(("stage-hunk", h))
                                            .xsmall()
                                            .ghost()
                                            .label("暂存 hunk")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.apply_hunk(h, PatchDirection::Stage, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new(("discard-hunk", h))
                                            .xsmall()
                                            .ghost()
                                            .label("丢弃 hunk")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.apply_hunk(h, PatchDirection::Discard, cx)
                                            })),
                                    );
                            }
                            DiffTarget::Staged => {
                                el = el.child(
                                    Button::new(("unstage-hunk", h))
                                        .xsmall()
                                        .ghost()
                                        .label("取消暂存 hunk")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.apply_hunk(h, PatchDirection::Unstage, cx)
                                        })),
                                );
                            }
                            DiffTarget::Commit(_) => {}
                        }
                        el.into_any_element()
                    }
                    DiffRow::Line(h, l) => {
                        let selected = self.selected_lines.contains(&(h, l));
                        base.child(line_cell(h, l, true, true, selected))
                            .when(selectable, |d| {
                                d.cursor_pointer().on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                        this.toggle_line(h, l, e.modifiers.shift, cx)
                                    }),
                                )
                            })
                            .into_any_element()
                    }
                    DiffRow::Split(left, right) => {
                        let side = |cell: Option<(usize, usize)>, old: bool| -> AnyElement {
                            match cell {
                                Some((h, l)) => {
                                    let selected = self.selected_lines.contains(&(h, l));
                                    let mut d = div()
                                        .id(SharedString::from(format!(
                                            "{ix}-{}",
                                            if old { "l" } else { "r" }
                                        )))
                                        .flex_1()
                                        .min_w_0()
                                        .h_full()
                                        .flex()
                                        .child(line_cell(h, l, old, !old, selected));
                                    if selectable
                                        && file.hunks[h].lines[l].kind != LineKind::Context
                                    {
                                        d = d.cursor_pointer().on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                                this.toggle_line(h, l, e.modifiers.shift, cx)
                                            }),
                                        );
                                    }
                                    d.into_any_element()
                                }
                                None => div()
                                    .flex_1()
                                    .h_full()
                                    .bg(theme.muted.opacity(0.4))
                                    .into_any_element(),
                            }
                        };
                        base.child(side(left, true))
                            .child(div().w(px(1.)).h_full().bg(theme.border))
                            .child(side(right, false))
                            .into_any_element()
                    }
                }
            })
            .collect()
    }

    /// 右侧 minimap:按比例标出新增与删除行的位置,点击跳转。
    fn render_minimap(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let colors = diff_colors(cx.theme().is_dark());
        let marks: Vec<u8> = self
            .diff_cache
            .as_ref()
            .map(|c| {
                c.rows
                    .iter()
                    .map(|r| {
                        let kind = |cell: Option<(usize, usize)>| {
                            cell.map(|(h, l)| c.file.hunks[h].lines[l].kind)
                        };
                        let k = match *r {
                            DiffRow::Header(_) => None,
                            DiffRow::Line(h, l) => kind(Some((h, l))),
                            DiffRow::Split(a, b) => match (kind(a), kind(b)) {
                                (Some(LineKind::Deleted), _) => Some(LineKind::Deleted),
                                (_, Some(LineKind::Added)) => Some(LineKind::Added),
                                _ => None,
                            },
                        };
                        match k {
                            Some(LineKind::Added) => 1,
                            Some(LineKind::Deleted) => 2,
                            _ => 0,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let n = marks.len().max(1);
        let scroll = self.diff_scroll.clone();
        let painted = std::rc::Rc::new(std::cell::Cell::new(Bounds::<Pixels>::default()));
        let painted_w = painted.clone();
        div()
            .id("minimap")
            .w(px(MINIMAP_WIDTH))
            .h_full()
            .flex_none()
            .bg(cx.theme().muted.opacity(0.3))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        painted_w.set(bounds);
                        let h = f32::from(bounds.size.height);
                        // 按像素分桶,超大 diff 也只画几百个小方块
                        let mut last_y = -1i32;
                        for (i, m) in marks.iter().enumerate() {
                            if *m == 0 {
                                continue;
                            }
                            let y = (i as f32 / n as f32 * h) as i32;
                            if y == last_y {
                                continue;
                            }
                            last_y = y;
                            let color = if *m == 1 {
                                colors.added_mark
                            } else {
                                colors.deleted_mark
                            };
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(bounds.origin.x + px(2.), bounds.origin.y + px(y as f32)),
                                    size(px(MINIMAP_WIDTH - 4.), px(2.)),
                                ),
                                color,
                            ));
                        }
                    },
                )
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, _| {
                let b = painted.get();
                let h = f32::from(b.size.height);
                if h > 0. {
                    let frac = (f32::from(e.position.y - b.origin.y) / h).clamp(0., 1.);
                    scroll.scroll_to_item((frac * n as f32) as usize, ScrollStrategy::Center);
                    window.refresh();
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::expand_tabs;

    #[test]
    fn tabs_shift_ranges() {
        let (s, r) = expand_tabs("\tab", std::slice::from_ref(&(1..2)));
        assert_eq!(s, "    ab");
        assert_eq!(&s[r[0].clone()], "a");
    }
}

//! Diff 视图:统一 / 并排两种布局、字符级差异、minimap,工作区 diff 支持 hunk 与单行暂存。

use crate::colors::{diff_colors, syntax_color};
use crate::repo_view::RepoView;
use app::Command;
use gitcore::diff::{DiffTarget, FileDiff, LineKind, inline_changes, pair_changed_lines};
use gitcore::highlight::DiffSyntax;
use gitcore::patch::PatchDirection;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui::prelude::*;
use gpui::*;
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::sync::Arc;

const LINE_HEIGHT: f32 = 20.;
const MINIMAP_WIDTH: f32 = 10.;
const CODE_FONT: &str = "Menlo";
const CODE_SIZE: f32 = 12.;
const LINE_NO_WIDTH: f32 = 40.;

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
    pub syntax: Option<Arc<DiffSyntax>>,
    pub rows: Vec<DiffRow>,
    pub inline: HashMap<(usize, usize), Vec<Range<usize>>>,
}

impl DiffCache {
    fn build(file: Arc<FileDiff>, syntax: Option<Arc<DiffSyntax>>, split: bool) -> Self {
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
            emphasize_rewritten(h, hunk, &mut inline);
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
            syntax,
            rows,
            inline,
        }
    }
}

/// 同一改动块里既有删除又有新增时,没能做字符级配对的行整行用深色标出(与 VS Code 一致),
/// 一眼能分清“改了几个字”和“整行重写”。纯新增 / 纯删除的块只有浅色行底,新文件不会一片深绿。
fn emphasize_rewritten(
    h: usize,
    hunk: &gitcore::diff::Hunk,
    inline: &mut HashMap<(usize, usize), Vec<Range<usize>>>,
) {
    let lines = &hunk.lines;
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind == LineKind::Context {
            i += 1;
            continue;
        }
        let start = i;
        let (mut dels, mut adds) = (0, 0);
        while i < lines.len() && lines[i].kind != LineKind::Context {
            match lines[i].kind {
                LineKind::Deleted => dels += 1,
                _ => adds += 1,
            }
            i += 1;
        }
        if dels == 0 || adds == 0 {
            continue;
        }
        for (l, line) in lines.iter().enumerate().take(i).skip(start) {
            let len = line.text.len();
            if len > 0 {
                inline
                    .entry((h, l))
                    .or_insert_with(|| std::iter::once(0..len).collect());
            }
        }
    }
}

/// 把语法颜色(前景)与字符级差异(背景)合成互不重叠、按位置排好的高亮区间。
fn merge_highlights(
    text: &str,
    syntax: &[(Range<usize>, Hsla)],
    words: &[Range<usize>],
    word_bg: Hsla,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let ok = |r: &Range<usize>| {
        r.start < r.end
            && r.end <= text.len()
            && text.is_char_boundary(r.start)
            && text.is_char_boundary(r.end)
    };
    let mut cuts: Vec<usize> = syntax
        .iter()
        .map(|(r, _)| r)
        .chain(words)
        .filter(|r| ok(r))
        .flat_map(|r| [r.start, r.end])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let (mut si, mut wi) = (0, 0);
    let mut out: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        while si < syntax.len() && syntax[si].0.end <= a {
            si += 1;
        }
        while wi < words.len() && words[wi].end <= a {
            wi += 1;
        }
        let color = syntax
            .get(si)
            .filter(|(r, _)| r.start <= a && ok(r))
            .map(|(_, c)| *c);
        let background_color = words
            .get(wi)
            .filter(|r| r.start <= a && ok(r))
            .map(|_| word_bg);
        if color.is_none() && background_color.is_none() {
            continue;
        }
        let style = HighlightStyle {
            color,
            background_color,
            ..Default::default()
        };
        match out.last_mut() {
            Some((r, s)) if r.end == a && *s == style => r.end = b,
            _ => out.push((a..b, style)),
        }
    }
    out
}

/// 制表符展开为 4 个空格,同时平移高亮区间。
fn expand_tabs(
    text: &str,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
) -> (String, Vec<(Range<usize>, HighlightStyle)>) {
    if !text.contains('\t') {
        return (text.to_string(), highlights);
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
    let highlights = highlights
        .into_iter()
        .map(|(r, s)| (map[r.start]..map[r.end], s))
        .collect();
    (out, highlights)
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
        let syntax = state.syntax.clone();

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
                    self.diff_cache = Some(DiffCache::build(f, syntax, split));
                } else if let Some(c) = &mut self.diff_cache {
                    // 首次打开时高亮晚于 diff 到达,只换高亮,不重建行布局
                    c.syntax = syntax;
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
        let (dir, name) = match path.rsplit_once('/') {
            Some((d, n)) => (Some(d.to_string()), n.to_string()),
            None => (None, path.to_string()),
        };
        let mut bar = h_flex()
            .h(px(34.))
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            // 标题栏底色比 secondary 浅很多,压住存在感,让代码行更突出
            .bg(theme.secondary.opacity(0.35))
            .child(crate::file_icons::file_icon(path, &theme).xsmall())
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .text_sm()
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    )
                    .children(dir.map(|d| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(d)
                    })),
            )
            .children(file.map(|f| {
                h_flex()
                    .gap_1p5()
                    .text_xs()
                    .font_family(CODE_FONT)
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
            Button::new("maximize-diff")
                .xsmall()
                .ghost()
                .icon(Icon::from(if self.diff_maximized {
                    gpui::assets::IconName::Minimize2
                } else {
                    gpui::assets::IconName::Maximize2
                }))
                .tooltip(if self.diff_maximized {
                    "还原(显示提交图,Esc)"
                } else {
                    "撑满中间区域"
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.diff_maximized = !this.diff_maximized;
                    cx.notify();
                })),
        )
        .child(
            Button::new("close-diff")
                .xsmall()
                .ghost()
                .icon(gpui::component::IconName::Close)
                .on_click(cx.listener(|this, _, _, cx| {
                    // 关掉 diff 时一并还原撑满状态,下次打开仍是上下分栏
                    this.diff_maximized = false;
                    this.repo.update(cx, |r, cx| r.close_diff(cx))
                })),
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
        let dark = theme.is_dark();
        let colors = diff_colors(dark);
        // 文字样式要显式给出等宽字体:StyledText 的字体来自这里,而不是外层 div 的 font_family
        let mut text_style = window.text_style();
        text_style.font_family = CODE_FONT.into();
        text_style.font_size = px(CODE_SIZE).into();
        text_style.color = theme.foreground;
        let selectable = !matches!(target, DiffTarget::Commit(_));
        let file = cache.file.clone();
        let syntax = cache.syntax.clone();
        let rows: Vec<DiffRow> = cache.rows[range.clone()].to_vec();

        let line_cell =
            |h: usize, l: usize, show_old: bool, show_new: bool, selected: bool| -> Div {
                let line = &file.hunks[h].lines[l];
                let none = gpui::transparent_black();
                let (bg, word_bg, gutter_bg, sign) = match line.kind {
                    LineKind::Added => (
                        colors.added_bg,
                        colors.added_word,
                        colors.added_gutter,
                        colors.added_sign,
                    ),
                    LineKind::Deleted => (
                        colors.deleted_bg,
                        colors.deleted_word,
                        colors.deleted_gutter,
                        colors.deleted_sign,
                    ),
                    LineKind::Context => (none, none, none, theme.muted_foreground),
                };
                let spans: Vec<(Range<usize>, Hsla)> = syntax
                    .as_ref()
                    .map(|s| {
                        s.line(h, l)
                            .iter()
                            .filter_map(|(r, n)| syntax_color(n, dark).map(|c| (r.clone(), c)))
                            .collect()
                    })
                    .unwrap_or_default();
                let words = cache.inline.get(&(h, l)).map_or(&[][..], Vec::as_slice);
                let (text, highlights) = expand_tabs(
                    &line.text,
                    merge_highlights(&line.text, &spans, words, word_bg),
                );
                let number = |n: Option<u32>| {
                    div()
                        .w(px(LINE_NO_WIDTH))
                        .flex_none()
                        .pr_2()
                        .text_right()
                        .text_color(theme.muted_foreground.opacity(0.75))
                        .child(n.map(|n| n.to_string()).unwrap_or_default())
                };
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .bg(if selected { colors.selected_bg } else { bg })
                    .child(
                        h_flex()
                            .flex_none()
                            .h_full()
                            .when(!selected, |d| d.bg(gutter_bg))
                            .when(show_old, |d| d.child(number(line.old_no)))
                            .when(show_new, |d| d.child(number(line.new_no))),
                    )
                    .child(
                        div()
                            .w(px(18.))
                            .flex_none()
                            .text_center()
                            .text_color(sign)
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
                    .font_family(CODE_FONT)
                    .text_size(px(CODE_SIZE));
                match row {
                    DiffRow::Header(h) => {
                        let hunk = &file.hunks[h];
                        let mut el = base
                            .bg(colors.hunk_bg)
                            .px_3()
                            .gap_2()
                            .text_color(colors.hunk_fg)
                            .child(div().flex_1().truncate().child(hunk.header.clone()));
                        match target {
                            DiffTarget::Unstaged => {
                                el = el
                                    .child(
                                        Button::new(("stage-hunk", h))
                                            .xsmall()
                                            .ghost()
                                            .icon(Icon::from(gpui::assets::IconName::Plus))
                                            .tooltip("暂存这个 hunk")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.apply_hunk(h, PatchDirection::Stage, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new(("discard-hunk", h))
                                            .xsmall()
                                            .ghost()
                                            .icon(Icon::from(gpui::assets::IconName::Undo2))
                                            .tooltip("丢弃这个 hunk")
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
                                        .icon(Icon::from(gpui::assets::IconName::Minus))
                                        .tooltip("取消暂存这个 hunk")
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
    use super::{emphasize_rewritten, expand_tabs, merge_highlights};
    use gitcore::diff::{DiffLine, Hunk, LineKind};
    use gpui::{HighlightStyle, Hsla, hsla};
    use std::collections::HashMap;

    #[test]
    fn tabs_shift_ranges() {
        let style = HighlightStyle::default();
        let (s, r) = expand_tabs("\tab", vec![(1..2, style)]);
        assert_eq!(s, "    ab");
        assert_eq!(&s[r[0].0.clone()], "a");
    }

    #[test]
    fn merges_syntax_and_word_ranges_without_overlap() {
        let red: Hsla = hsla(0., 1., 0.5, 1.);
        let bg: Hsla = hsla(0.3, 1., 0.5, 1.);
        // 语法:0..5 红色;字符级差异:3..8
        let out = merge_highlights(
            "let x = 1;",
            &[(0..5, red)],
            std::slice::from_ref(&(3..8)),
            bg,
        );
        let got: Vec<_> = out
            .iter()
            .map(|(r, s)| (r.clone(), s.color.is_some(), s.background_color.is_some()))
            .collect();
        assert_eq!(
            got,
            vec![(0..3, true, false), (3..5, true, true), (5..8, false, true)]
        );
        // 越界、不在字符边界上的区间被忽略
        assert!(merge_highlights("中", &[(0..1, red), (0..9, red)], &[], bg).is_empty());
    }

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
    fn rewritten_lines_are_emphasized_only_in_mixed_blocks() {
        use LineKind::*;
        let hunk = Hunk {
            header: String::new(),
            old_start: 1,
            old_lines: 0,
            new_start: 1,
            new_lines: 0,
            lines: vec![
                line(Deleted, "a"),
                line(Added, "b"),
                line(Added, "c"),
                line(Context, "x"),
                line(Added, "only added"),
            ],
        };
        let mut inline = HashMap::new();
        let whole = || -> Vec<std::ops::Range<usize>> { std::iter::once(0..1).collect() };
        inline.insert((0, 1), whole());
        emphasize_rewritten(0, &hunk, &mut inline);
        assert_eq!(inline.get(&(0, 0)), Some(&whole()));
        assert_eq!(inline.get(&(0, 2)), Some(&whole()));
        // 已有字符级差异的保持不变;纯新增块不加深
        assert_eq!(inline.get(&(0, 1)), Some(&whole()));
        assert!(!inline.contains_key(&(0, 4)));
    }
}

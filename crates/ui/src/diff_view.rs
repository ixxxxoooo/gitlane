//! Diff 视图:头部工具条 + gpui-component 的 `Diff` 组件(虚拟列表、并排 / 统一、拖选复制、横向滚动都由它负责)。
//! 本文件只负责把 gitcore 的 diff 喂给组件,以及头部与暂存按钮;选区换算与暂存命令在 `diff_stage`。
//!
//! @author ixxxxoooo

use crate::diff_stage::{DiffCache, syntax_highlighter};
use crate::repo_view::RepoView;
use diffview::{
    Diff, DiffChangeIndicator, DiffFile, DiffGutterMarker, DiffLinePosition, DiffMode, DiffSide,
};
use gitcore::diff::{DiffTarget, FileDiff, LineKind};
use gitcore::patch::{PatchDirection, display_patch};
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui::component::{ActiveTheme, Icon, Sizable, h_flex, v_flex};
use gpui::prelude::*;
use gpui::*;
use std::sync::Arc;

pub(crate) const CODE_FONT: &str = "Menlo";
pub(crate) const CODE_SIZE: f32 = 12.;

impl RepoView {
    pub(crate) fn render_diff(
        &mut self,
        _window: &mut Window,
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
                self.sync_diff_state(f.clone(), split, cx);
                self.render_diff_body(&f, target, cx)
            }
        };

        v_flex()
            .size_full()
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// 让组件的状态跟上当前文件、并排 / 统一模式与亮暗主题(高亮颜色按主题取)。
    fn sync_diff_state(&mut self, file: Arc<FileDiff>, split: bool, cx: &mut Context<Self>) {
        let key = Arc::as_ptr(&file) as usize;
        if self.diff_cache.as_ref().is_none_or(|c| c.key != key) {
            // 语言约定传文件路径,高亮函数按路径识别 Vue / TS 等
            let files: Vec<DiffFile> = DiffFile::parse(&display_patch(&file))
                .unwrap_or_default()
                .into_iter()
                .map(|f| f.with_language(file.path.clone()))
                .collect();
            self.diff_state.update(cx, |s, cx| s.set_files(files, cx));
            self.diff_cache = Some(DiffCache { key, file });
        }
        let mode = if split {
            DiffMode::Split
        } else {
            DiffMode::Unified
        };
        self.diff_state.update(cx, |s, cx| {
            if s.mode() != mode {
                s.set_mode(mode, cx);
            }
        });
        let dark = cx.theme().is_dark();
        if self.diff_dark != Some(dark) {
            self.diff_dark = Some(dark);
            self.diff_state.update(cx, |s, cx| {
                s.set_syntax_highlighter(syntax_highlighter(dark), cx)
            });
        }
    }

    /// 组件本体。工作区 diff 在每个 hunk 第一条改动行旁放一个暂存 / 取消暂存按钮。
    fn render_diff_body(
        &self,
        file: &FileDiff,
        target: DiffTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = cx.entity().downgrade();
        let dir = match target {
            DiffTarget::Unstaged => Some((
                PatchDirection::Stage,
                gpui::assets::IconName::Plus,
                "暂存这个 hunk",
            )),
            DiffTarget::Staged => Some((
                PatchDirection::Unstage,
                gpui::assets::IconName::Minus,
                "取消暂存这个 hunk",
            )),
            DiffTarget::Commit(_) => None,
        };
        let markers: Vec<DiffGutterMarker> = dir.map(|_| hunk_markers(file)).unwrap_or_default();
        let diff = Diff::new(&self.diff_state)
            .header_visible(false)
            .change_indicator(DiffChangeIndicator::Signs)
            // 不换行:diffview 按编辑器的方式让各栏跟可视区等宽、栏内横向滚动(见 decisions/0008)
            .soft_wrap(false)
            .gutter_markers(markers)
            .render_gutter_marker(move |marker, _, _| {
                let hunk: usize = marker.label().parse().unwrap_or(0);
                let (hunk_dir, icon, tip) =
                    dir.unwrap_or((PatchDirection::Stage, gpui::assets::IconName::Plus, ""));
                let view = view.clone();
                Button::new(("hunk-action", hunk))
                    .xsmall()
                    .ghost()
                    .icon(Icon::from(icon))
                    .tooltip(tip)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| this.apply_hunk(hunk, hunk_dir, cx))
                            .ok();
                    })
            });
        div()
            .id("diff-body")
            .flex_1()
            .min_h_0()
            .font_family(CODE_FONT)
            .text_size(px(CODE_SIZE))
            .child(diff.size_full())
            .context_menu(|menu, _, _| {
                menu.item(PopupMenuItem::new("复制").on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::CopyDiffLines), cx)
                }))
            })
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
                    this.save_layout(cx);
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
                    this.save_layout(cx);
                    this.repo.update(cx, |r, cx| r.close_diff(cx))
                })),
        )
    }
}

/// 每个 hunk 第一条改动行旁的标记,标签是 hunk 序号;标记放在新侧行号上,纯删除的 hunk 放在旧侧。
fn hunk_markers(file: &FileDiff) -> Vec<DiffGutterMarker> {
    file.hunks
        .iter()
        .enumerate()
        .filter_map(|(h, hunk)| {
            let line = hunk.lines.iter().find(|l| l.kind != LineKind::Context)?;
            let (side, no) = match (line.new_no, line.old_no) {
                (Some(n), _) => (DiffSide::Modified, n),
                (None, Some(n)) => (DiffSide::Original, n),
                _ => return None,
            };
            let position = DiffLinePosition::new(file.path.clone(), side, no as usize);
            Some(DiffGutterMarker::new(("hunk", h), position).with_label(h.to_string()))
        })
        .collect()
}

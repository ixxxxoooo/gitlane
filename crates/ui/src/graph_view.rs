//! 提交图:固定行高、虚拟滚动,只渲染可见行。最上方固定一行 Working changes。

use crate::format;
use crate::graph_canvas::{graph_cell, graph_width, working_cell};
use crate::repo_view::RepoView;
use app::{Command, Selection};
use gitcore::ops::ResetMode;
use gitcore::{Oid, RefKind};
use gpui::base::TestSupportExt as _;
use gpui::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui::component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex};
use gpui::prelude::*;
use gpui::*;
use std::collections::HashSet;

pub const ROW_HEIGHT: f32 = 26.;
const AUTHOR_WIDTH: f32 = 110.;
const TIME_WIDTH: f32 = 80.;
const SHA_WIDTH: f32 = 56.;
/// 一行最多显示几个引用徽章,其余收成 “+N”。
const MAX_BADGES: usize = 2;

impl RepoView {
    pub(crate) fn render_graph(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let repo = self.repo.read(cx);
        let theme = cx.theme().clone();
        let Some(data) = repo.data.clone() else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_color(theme.muted_foreground)
                .child(match &repo.load_error {
                    Some(e) => format!("加载失败:{e}"),
                    None => "正在加载提交历史…".to_string(),
                })
                .into_any_element();
        };
        let count = data.history.commits.len();
        let columns = data.graph.max_columns().min(24);
        let working_count = repo.status.as_ref().map_or(0, |s| s.entries.len());
        let working_selected = repo.selection == Selection::WorkingChanges;

        // 工作区节点画在 HEAD 所在的列,用虚线与 HEAD 提交连起来
        let dark = theme.mode.is_dark();
        let head_row = data
            .refs
            .head
            .oid()
            .and_then(|oid| data.history.row_of.get(&oid))
            .map(|r| *r as usize);
        let (head_col, head_color) = head_row.map_or((0, 0), |r| {
            let n = data.graph.node(r);
            (n.column, n.color)
        });

        let working_row = h_flex()
            .id("working-changes")
            .h(px(ROW_HEIGHT))
            .pr_2()
            .gap_2()
            .text_sm()
            .cursor_pointer()
            .when(working_selected, |d| d.bg(theme.list_active))
            .hover(|s| s.bg(theme.list_hover))
            .child(
                div()
                    .flex_none()
                    .h_full()
                    .child(working_cell(head_col, head_color, columns, dark)),
            )
            .child(
                div()
                    .italic()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Working changes"),
            )
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(if working_count == 0 {
                        "无改动".to_string()
                    } else {
                        format!("{working_count} 个文件")
                    }),
            )
            .on_click(
                cx.listener(|this, _, _, cx| this.repo.update(cx, |r, cx| r.select_working(cx))),
            )
            .test_support();

        let list = uniform_list(
            "commit-graph",
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                this.render_commit_rows(range, cx)
            }),
        )
        .track_scroll(&self.graph_scroll)
        .flex_1()
        .min_h_0();

        let header = h_flex()
            .h(px(28.))
            .pr_2()
            .gap_2()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.muted_foreground)
            .border_b_1()
            .border_color(theme.border)
            .child(div().w(graph_width(columns)).flex_none())
            .child(div().flex_1().child("提交信息"))
            .child(div().w(px(AUTHOR_WIDTH)).flex_none().child("作者"))
            .child(div().w(px(TIME_WIDTH)).flex_none().child("时间"))
            .child(div().w(px(SHA_WIDTH)).flex_none().child("SHA"));

        let mut col = v_flex()
            .key_context("CommitGraph")
            .track_focus(&self.graph_focus)
            .size_full()
            .child(header)
            .child(working_row)
            .child(list);
        if data.history.truncated {
            col = col.child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("只加载了最近 {count} 个提交")),
            );
        }
        col.on_action(
            cx.listener(|this, a: &CommitAction, window, cx| this.on_commit_action(a, window, cx)),
        )
        .into_any_element()
    }

    fn render_commit_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let repo = self.repo.read(cx);
        let Some(data) = repo.data.clone() else {
            return Vec::new();
        };
        let theme = cx.theme().clone();
        let dark = theme.is_dark();
        let selected = match repo.selection {
            Selection::Commit(oid) => Some(oid),
            Selection::WorkingChanges => None,
        };
        let head_oid = data.refs.head.oid();
        let searching = !repo.search_query.trim().is_empty();
        let hits: HashSet<usize> = if searching {
            repo.search_hits.iter().copied().collect()
        } else {
            HashSet::new()
        };
        let columns = data.graph.max_columns().min(24);
        let now = format::now();

        range
            .map(|row| {
                let c = &data.history.commits[row];
                let oid = c.id;
                let is_selected = selected == Some(oid);
                let dimmed = searching && !hits.contains(&row);
                let badges = data
                    .badges
                    .get(&oid)
                    .map(|ix| ref_badges(&data.refs.refs, ix, &theme))
                    .unwrap_or_default();
                h_flex()
                    .id(("commit", row))
                    // uniform_list 按内容宽度排行,不撑满时摘要列的 flex_1 不生效,后面几列就对不齐
                    .w_full()
                    .h(px(ROW_HEIGHT))
                    .pr_2()
                    .gap_2()
                    .text_sm()
                    .when(is_selected, |d| d.bg(theme.list_active))
                    .when(!is_selected, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .when(dimmed, |d| d.opacity(0.35))
                    .child(div().flex_none().h_full().child(graph_cell(
                        data.graph.row_segments(row).to_vec(),
                        data.graph.node(row),
                        columns,
                        dark,
                        if is_selected {
                            theme.list_active
                        } else {
                            theme.background
                        },
                        head_oid == Some(oid),
                        // HEAD 是第一行时,上面的“工作区改动”虚线连到它
                        row == 0 && head_oid == Some(oid),
                    )))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .children(badges)
                            .child(div().truncate().child(c.summary.clone())),
                    )
                    .child(
                        div()
                            .w(px(110.))
                            .flex_none()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(c.author.clone()),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .flex_none()
                            .whitespace_nowrap()
                            .text_color(theme.muted_foreground)
                            .child(format::relative_time(c.time, now)),
                    )
                    .child(
                        div()
                            .w(px(56.))
                            .flex_none()
                            .whitespace_nowrap()
                            .font_family("Menlo")
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(c.short_id()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.graph_focus.focus(window, cx);
                        this.repo.update(cx, |r, cx| r.select_commit(oid, cx));
                    }))
                    .context_menu(move |menu, _, _| commit_menu(menu, oid))
                    .into_any_element()
            })
            .collect()
    }

    fn on_commit_action(&mut self, a: &CommitAction, window: &mut Window, cx: &mut Context<Self>) {
        let rev = a.oid.clone();
        match a.kind {
            CommitActionKind::Checkout => self.run(Command::CheckoutDetached(rev), cx),
            CommitActionKind::NewBranch => self.prompt_new_branch(rev, window, cx),
            CommitActionKind::CherryPick => self.run(Command::CherryPick(rev), cx),
            CommitActionKind::Revert => self.run(Command::Revert(rev), cx),
            CommitActionKind::ResetSoft => self.run(
                Command::Reset {
                    rev,
                    mode: ResetMode::Soft,
                },
                cx,
            ),
            CommitActionKind::ResetMixed => self.run(
                Command::Reset {
                    rev,
                    mode: ResetMode::Mixed,
                },
                cx,
            ),
            CommitActionKind::ResetHard => self.confirm_reset(rev, ResetMode::Hard, window, cx),
            CommitActionKind::Tag => {
                let view = cx.entity().downgrade();
                crate::dialogs::prompt_text(
                    window,
                    cx,
                    "新建标签",
                    "标签名,如 v1.0.0",
                    String::new(),
                    move |name, _, cx| {
                        let rev = rev.clone();
                        let _ = view.update(cx, |v, cx| {
                            v.run(
                                Command::CreateTag {
                                    name,
                                    rev,
                                    message: None,
                                },
                                cx,
                            )
                        });
                    },
                );
            }
            CommitActionKind::CopySha => cx.write_to_clipboard(ClipboardItem::new_string(rev)),
        }
    }
}

/// 提交行上的引用徽章。本地分支与同名的远程分支指向同一提交时合成一个(如 main + origin/main),
/// 超过 `MAX_BADGES` 个的收成 “+N”。
fn ref_badges(
    refs: &[gitcore::RefInfo],
    ix: &[usize],
    theme: &gpui::component::Theme,
) -> Vec<AnyElement> {
    let remote_of = |local: &str| {
        ix.iter().copied().find(|&j| {
            refs[j].kind == RefKind::Remote
                && refs[j].short.split_once('/').map(|(_, n)| n) == Some(local)
        })
    };
    let mut merged = Vec::new();
    let mut items: Vec<(usize, bool)> = Vec::new();
    for &i in ix {
        if refs[i].kind == RefKind::Local {
            let remote = remote_of(&refs[i].short);
            merged.extend(remote);
            items.push((i, remote.is_some()));
        }
    }
    items.extend(
        ix.iter()
            .filter(|&&i| refs[i].kind != RefKind::Local && !merged.contains(&i))
            .map(|&i| (i, false)),
    );
    // HEAD 所在分支排最前
    items.sort_by_key(|&(i, _)| !refs[i].is_head);

    let extra = items.len().saturating_sub(MAX_BADGES);
    let mut out: Vec<AnyElement> = items
        .into_iter()
        .take(MAX_BADGES)
        .map(|(i, with_remote)| {
            let r = &refs[i];
            let (bg, fg, border) = match r.kind {
                RefKind::Local if r.is_head => {
                    (theme.primary, theme.primary_foreground, theme.primary)
                }
                RefKind::Local => (
                    theme.primary.opacity(0.1),
                    theme.primary,
                    theme.primary.opacity(0.35),
                ),
                // 远程分支:偏冷的中性灰蓝,比组件库的 secondary 灰有层次,又不抢本地分支的风头
                RefKind::Remote => (
                    theme.foreground.opacity(0.06),
                    theme.foreground.opacity(0.68),
                    theme.foreground.opacity(0.16),
                ),
                // 标签:柔和的琥珀色,而不是高饱和的纯黄
                RefKind::Tag => crate::colors::tag_badge(theme.mode.is_dark()),
            };
            let icon = |name: gpui::assets::IconName| {
                Icon::from(name).xsmall().text_color(fg).into_any_element()
            };
            let icons: Vec<AnyElement> = match r.kind {
                RefKind::Local if with_remote => vec![
                    icon(gpui::assets::IconName::Laptop),
                    Icon::new(IconName::Globe)
                        .xsmall()
                        .text_color(fg)
                        .into_any_element(),
                ],
                RefKind::Local => vec![icon(gpui::assets::IconName::Laptop)],
                RefKind::Remote => vec![
                    Icon::new(IconName::Globe)
                        .xsmall()
                        .text_color(fg)
                        .into_any_element(),
                ],
                RefKind::Tag => vec![icon(gpui::assets::IconName::Tag)],
            };
            h_flex()
                .flex_none()
                .h(px(18.))
                .max_w(px(180.))
                .px_1p5()
                .gap_1()
                .rounded(px(4.))
                .border_1()
                .border_color(border)
                .bg(bg)
                .text_xs()
                .text_color(fg)
                .when(r.is_head, |d| d.font_weight(FontWeight::SEMIBOLD))
                .children(icons)
                .child(div().truncate().child(r.short.clone()))
                .into_any_element()
        })
        .collect();
    if extra > 0 {
        out.push(
            div()
                .flex_none()
                .h(px(18.))
                .px_1p5()
                .rounded(px(4.))
                .border_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(format!("+{extra}"))
                .into_any_element(),
        );
    }
    out
}

fn commit_menu(
    menu: gpui::component::menu::PopupMenu,
    oid: Oid,
) -> gpui::component::menu::PopupMenu {
    let item = |label: &'static str, kind: CommitActionKind| {
        let oid = oid.to_string();
        PopupMenuItem::new(label).on_click(move |_, window, cx| {
            window.dispatch_action(
                Box::new(CommitAction {
                    oid: oid.clone(),
                    kind,
                }),
                cx,
            )
        })
    };
    menu.item(item("检出此提交(分离 HEAD)", CommitActionKind::Checkout))
        .item(item("从这里新建分支…", CommitActionKind::NewBranch))
        .item(item("在这里打标签…", CommitActionKind::Tag))
        .separator()
        .item(item("Cherry-pick 到当前分支", CommitActionKind::CherryPick))
        .item(item("Revert 此提交", CommitActionKind::Revert))
        .separator()
        .item(item(
            "Reset 当前分支到这里(soft)",
            CommitActionKind::ResetSoft,
        ))
        .item(item(
            "Reset 当前分支到这里(mixed)",
            CommitActionKind::ResetMixed,
        ))
        .item(item(
            "Reset 当前分支到这里(hard)…",
            CommitActionKind::ResetHard,
        ))
        .separator()
        .item(item("复制 SHA", CommitActionKind::CopySha))
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CommitActionKind {
    Checkout,
    NewBranch,
    CherryPick,
    Revert,
    ResetSoft,
    ResetMixed,
    ResetHard,
    Tag,
    CopySha,
}

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = gitlane, no_json)]
pub struct CommitAction {
    pub oid: String,
    pub kind: CommitActionKind,
}

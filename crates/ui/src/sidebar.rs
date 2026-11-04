//! 左侧引用树:本地分支(按前缀分组)、远程(按远程名分组)、标签、Stash。

use crate::repo_view::RepoView;
use app::Command;
use gitcore::{RefInfo, RefKind};
use gpui::base::TestSupportExt as _;
use gpui::component::input::Input;
use gpui::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui::component::{ActiveTheme, Icon, IconName, Sizable, h_flex};
use gpui::prelude::*;
use gpui::*;
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;

const ROW_HEIGHT: f32 = 26.;

/// 引用树拍平后的一行。树可能有上千个引用,所以用固定行高的虚拟列表,只渲染可见行。
#[derive(Clone, Debug)]
pub(crate) enum SidebarRow {
    Section {
        title: &'static str,
        count: usize,
    },
    /// 按前缀分组的文件夹,`key` 用于记录折叠状态。
    Group {
        key: String,
        prefix: String,
        count: usize,
    },
    /// `index` 是在 `refs.refs` 中的下标。
    Ref {
        index: usize,
        label: String,
        depth: usize,
    },
    /// `pos` 是在 `refs.stashes` 中的下标。
    Stash {
        pos: usize,
    },
}

/// 按当前折叠状态拍平引用树。`filter` 非空时只保留名称包含它的引用(不区分大小写),
/// 此时忽略折叠状态、全部展开,并隐藏没有命中的分区。
pub(crate) fn flatten_refs(
    refs: &gitcore::Refs,
    collapsed: &HashSet<String>,
    filter: &str,
) -> Vec<SidebarRow> {
    let mut rows = Vec::new();
    let filter = filter.trim().to_lowercase();
    let filtering = !filter.is_empty();
    let no_collapse = HashSet::new();
    let collapsed = if filtering { &no_collapse } else { collapsed };
    let tree = |rows: &mut Vec<SidebarRow>, scope: &str, title: &'static str, kind: RefKind| {
        let items: Vec<usize> = (0..refs.refs.len())
            .filter(|&i| refs.refs[i].kind == kind)
            .filter(|&i| !filtering || refs.refs[i].short.to_lowercase().contains(&filter))
            .collect();
        if filtering && items.is_empty() {
            return;
        }
        rows.push(SidebarRow::Section {
            title,
            count: items.len(),
        });
        if collapsed.contains(title) {
            return;
        }
        if kind == RefKind::Tag {
            rows.extend(items.into_iter().map(|index| SidebarRow::Ref {
                index,
                label: refs.refs[index].short.clone(),
                depth: 1,
            }));
            return;
        }
        // 名称里带 `/` 的按第一段分组成树,如 feature/a、feature/b → feature
        let mut groups: BTreeMap<&str, Vec<(usize, &str)>> = BTreeMap::new();
        for &index in &items {
            let short = refs.refs[index].short.as_str();
            match short.split_once('/') {
                Some((prefix, rest)) => groups.entry(prefix).or_default().push((index, rest)),
                None => rows.push(SidebarRow::Ref {
                    index,
                    label: short.to_string(),
                    depth: 1,
                }),
            }
        }
        for (prefix, members) in groups {
            let key = format!("{scope}:{prefix}");
            let open = !collapsed.contains(&key);
            rows.push(SidebarRow::Group {
                key,
                prefix: prefix.to_string(),
                count: members.len(),
            });
            if open {
                rows.extend(members.into_iter().map(|(index, rest)| SidebarRow::Ref {
                    index,
                    label: rest.to_string(),
                    depth: 2,
                }));
            }
        }
    };
    tree(&mut rows, "local", "本地分支", RefKind::Local);
    tree(&mut rows, "remote", "远程", RefKind::Remote);
    tree(&mut rows, "tag", "标签", RefKind::Tag);
    let stashes: Vec<usize> = (0..refs.stashes.len())
        .filter(|&i| !filtering || refs.stashes[i].message.to_lowercase().contains(&filter))
        .collect();
    if !filtering || !stashes.is_empty() {
        rows.push(SidebarRow::Section {
            title: "Stash",
            count: stashes.len(),
        });
        if !collapsed.contains("Stash") {
            rows.extend(stashes.into_iter().map(|pos| SidebarRow::Stash { pos }));
        }
    }
    rows
}

impl RepoView {
    pub(crate) fn render_sidebar(
        &mut self,
        width: Pixels,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        self.sidebar_rows = match self.repo.read(cx).data.as_ref() {
            Some(data) => Rc::new(flatten_refs(
                &data.refs,
                &self.collapsed,
                self.sidebar_filter.read(cx).value().as_ref(),
            )),
            None => Rc::default(),
        };
        let count = self.sidebar_rows.len();

        div()
            .w(width)
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.sidebar)
            .on_action(cx.listener(|this, a: &StashAction, _, cx| {
                this.run(
                    Command::StashApply {
                        index: a.index,
                        pop: a.pop,
                    },
                    cx,
                )
            }))
            .on_action(
                cx.listener(|this, a: &StashDropAction, _, cx| {
                    this.run(Command::StashDrop(a.0), cx)
                }),
            )
            .on_action(
                cx.listener(|this, a: &RefAction, window, cx| this.on_ref_action(a, window, cx)),
            )
            .child(
                div()
                    .flex_none()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .child(Input::new(&self.sidebar_filter).small().cleanable(true)),
            )
            .child(
                div().flex_1().min_h_0().child(
                    uniform_list(
                        "sidebar",
                        count,
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            this.render_sidebar_rows(range, cx)
                        }),
                    )
                    .track_scroll(&self.sidebar_scroll)
                    .size_full(),
                ),
            )
            .id("sidebar")
            .test_support()
    }

    fn render_sidebar_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let rows = self.sidebar_rows.clone();
        let Some(data) = self.repo.read(cx).data.clone() else {
            return Vec::new();
        };
        rows[range.start.min(rows.len())..range.end.min(rows.len())]
            .iter()
            .map(|row| match row {
                SidebarRow::Section { title, count } => {
                    self.section(title, *count, cx).into_any_element()
                }
                SidebarRow::Group { key, prefix, count } => {
                    self.group_row(key, prefix, *count, cx).into_any_element()
                }
                SidebarRow::Ref {
                    index,
                    label,
                    depth,
                } => self
                    .ref_row(&data.refs.refs[*index], label.clone(), *depth, cx)
                    .into_any_element(),
                SidebarRow::Stash { pos } => self
                    .stash_row(&data.refs.stashes[*pos], cx)
                    .into_any_element(),
            })
            .collect()
    }

    fn toggle_collapsed(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_string());
        }
        cx.notify();
    }

    fn section(
        &self,
        title: &'static str,
        count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let collapsed = self.collapsed.contains(title);
        let theme = cx.theme().clone();
        let icon = match title {
            "本地分支" => Icon::from(gpui::assets::IconName::Laptop),
            "远程" => Icon::from(IconName::Globe),
            "标签" => Icon::from(gpui::assets::IconName::Tag),
            _ => Icon::from(gpui::assets::IconName::Archive),
        };
        h_flex()
            .id(title)
            .h(px(ROW_HEIGHT))
            .px_2()
            .gap_1p5()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.foreground.opacity(0.8))
            .cursor_pointer()
            // 第一个分区之外用细线分开,折叠后也能看清分区边界
            .when(title != "本地分支", |d| {
                d.border_t_1().border_color(theme.border.opacity(0.6))
            })
            .hover(|s| s.text_color(theme.foreground))
            .child(
                Icon::new(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .xsmall(),
            )
            .child(icon.xsmall().text_color(theme.muted_foreground))
            .child(div().flex_1().child(title))
            .child(
                div()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(theme.muted_foreground)
                    .child(count.to_string()),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapsed(title, cx)))
    }

    fn group_row(
        &self,
        key: &str,
        prefix: &str,
        count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let collapsed = self.collapsed.contains(key);
        let theme = cx.theme().clone();
        let k = key.to_string();
        h_flex()
            .id(SharedString::from(key.to_string()))
            .pl(px(14.))
            .pr_2()
            .h(px(ROW_HEIGHT))
            .gap_1()
            .text_sm()
            .rounded_sm()
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|s| s.bg(theme.list_hover))
            .child(
                Icon::new(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .xsmall(),
            )
            .child(
                Icon::new(if collapsed {
                    IconName::Folder
                } else {
                    IconName::FolderOpen
                })
                .xsmall(),
            )
            .child(div().flex_1().truncate().child(prefix.to_string()))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground.opacity(0.7))
                    .child(count.to_string()),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapsed(&k, cx)))
    }

    fn stash_row(
        &self,
        s: &gitcore::StashInfo,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let index = s.index;
        let oid = s.id;
        h_flex()
            .id(SharedString::from(format!("stash-{index}")))
            .pl(px(20.))
            .pr_2()
            .h(px(ROW_HEIGHT))
            .gap_1()
            .text_sm()
            .rounded_sm()
            .hover(|s| s.bg(theme.list_hover))
            .child(
                Icon::from(gpui::assets::IconName::Archive)
                    .xsmall()
                    .text_color(theme.muted_foreground),
            )
            .child(div().flex_1().truncate().child(s.message.clone()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.repo.update(cx, |r, cx| r.select_commit(oid, cx))
            }))
            .context_menu(move |menu, _, _| {
                menu.item(stash_item("Apply", index, false))
                    .item(stash_item("Pop", index, true))
                    .separator()
                    .item(PopupMenuItem::new("删除").on_click(move |_, window, cx| {
                        window.dispatch_action(Box::new(StashDropAction(index)), cx)
                    }))
            })
    }

    fn ref_row(
        &self,
        r: &RefInfo,
        label: String,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let target = r.target;
        let selected = self.repo.read(cx).selection == app::Selection::Commit(target);
        let kind = r.kind;
        let short = r.short.clone();
        let icon = match kind {
            RefKind::Local => Icon::from(gpui::assets::IconName::GitBranch),
            RefKind::Remote => Icon::from(IconName::Globe),
            RefKind::Tag => Icon::from(gpui::assets::IconName::Tag),
        };
        let ab = r.ahead_behind.filter(|(a, b)| *a > 0 || *b > 0);
        let name_for_menu = short.clone();
        h_flex()
            .id(SharedString::from(r.name.clone()))
            // 左侧 2px 竖条只在当前分支上着色,其余透明,保证各行内容对齐
            .border_l_2()
            .border_color(if r.is_head {
                theme.primary
            } else {
                gpui::transparent_black()
            })
            .pl(px(6. + 12. * depth as f32))
            .pr_2()
            .h(px(ROW_HEIGHT))
            .gap_1p5()
            .text_sm()
            .when(r.is_head, |d| {
                d.font_weight(FontWeight::MEDIUM)
                    .bg(theme.primary.opacity(0.1))
            })
            .when(selected, |d| d.bg(theme.list_active))
            .when(!selected, |d| d.hover(|s| s.bg(theme.list_hover)))
            .child(icon.xsmall().text_color(if r.is_head {
                theme.primary
            } else {
                theme.muted_foreground
            }))
            .child(
                div()
                    .id(SharedString::from(format!("ref:{}", r.name)))
                    .flex_1()
                    .truncate()
                    .child(label)
                    .test_support(),
            )
            .children(ab.map(|(a, b)| {
                h_flex()
                    .gap_1()
                    .text_xs()
                    .font_weight(FontWeight::NORMAL)
                    .text_color(theme.muted_foreground)
                    .when(a > 0, |d| d.child(format!("↑{a}")))
                    .when(b > 0, |d| d.child(format!("↓{b}")))
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    if e.click_count >= 2 {
                        let action = match kind {
                            RefKind::Local => RefAction::Checkout(short.clone()),
                            RefKind::Remote => RefAction::CheckoutRemote(short.clone()),
                            RefKind::Tag => RefAction::CheckoutDetached(target.to_string()),
                        };
                        this.on_ref_action(&action, window, cx);
                    } else {
                        this.repo.update(cx, |r, cx| r.select_commit(target, cx));
                        if let Some(row) = this
                            .repo
                            .read(cx)
                            .data
                            .as_ref()
                            .and_then(|d| d.history.row_of.get(&target))
                        {
                            this.graph_scroll
                                .scroll_to_item(*row as usize, ScrollStrategy::Center);
                        }
                    }
                }),
            )
            .context_menu(move |menu, _, _| ref_menu(menu, kind, &name_for_menu, target))
    }

    fn on_ref_action(&mut self, a: &RefAction, window: &mut Window, cx: &mut Context<Self>) {
        match a.clone() {
            RefAction::Checkout(b) => self.run(Command::Checkout(b), cx),
            RefAction::CheckoutRemote(b) => {
                // 已有同名本地分支时直接检出它
                let local = b
                    .split_once('/')
                    .map(|(_, n)| n.to_string())
                    .unwrap_or(b.clone());
                let exists = self.repo.read(cx).data.as_ref().is_some_and(|d| {
                    d.refs
                        .refs
                        .iter()
                        .any(|r| r.kind == RefKind::Local && r.short == local)
                });
                self.run(
                    if exists {
                        Command::Checkout(local)
                    } else {
                        Command::CheckoutRemote(b)
                    },
                    cx,
                )
            }
            RefAction::CheckoutDetached(r) => self.run(Command::CheckoutDetached(r), cx),
            RefAction::Merge(b) => self.run(Command::Merge(b), cx),
            RefAction::Rename(old) => {
                let view = cx.entity().downgrade();
                let o = old.clone();
                crate::dialogs::prompt_text(
                    window,
                    cx,
                    format!("重命名分支 {old}"),
                    "新名称",
                    old,
                    move |new, _, cx| {
                        let old = o.clone();
                        let _ =
                            view.update(cx, |v, cx| v.run(Command::RenameBranch { old, new }, cx));
                    },
                );
            }
            RefAction::Delete(name) => {
                let view = cx.entity().downgrade();
                let n = name.clone();
                crate::dialogs::confirm(
                    window,
                    cx,
                    format!("删除分支 {name}"),
                    vec![
                        format!("将强制删除本地分支 {name}(git branch -D)。"),
                        "未合并的提交仍可通过撤销(⌘Z)恢复。".into(),
                    ],
                    "删除",
                    move |_, cx| {
                        let name = n.clone();
                        let _ = view.update(cx, |v, cx| {
                            v.run(Command::DeleteBranch { name, force: true }, cx)
                        });
                    },
                );
            }
            RefAction::DeleteTag(name) => self.run(Command::DeleteTag(name), cx),
            RefAction::NewBranchFrom(rev) => self.prompt_new_branch(rev, window, cx),
        }
    }
}

fn ref_menu(
    menu: gpui::component::menu::PopupMenu,
    kind: RefKind,
    name: &str,
    target: gitcore::Oid,
) -> gpui::component::menu::PopupMenu {
    let item = |label: String, action: RefAction| {
        PopupMenuItem::new(label)
            .on_click(move |_, window, cx| window.dispatch_action(Box::new(action.clone()), cx))
    };
    let n = name.to_string();

    match kind {
        RefKind::Local => menu
            .item(item(format!("检出 {n}"), RefAction::Checkout(n.clone())))
            .item(item(
                format!("合并 {n} 到当前分支"),
                RefAction::Merge(n.clone()),
            ))
            .item(item(
                "从这里新建分支…".into(),
                RefAction::NewBranchFrom(target.to_string()),
            ))
            .separator()
            .item(item("重命名…".into(), RefAction::Rename(n.clone())))
            .item(item("删除…".into(), RefAction::Delete(n.clone()))),
        RefKind::Remote => menu
            .item(item(
                format!("检出 {n}"),
                RefAction::CheckoutRemote(n.clone()),
            ))
            .item(item(
                format!("合并 {n} 到当前分支"),
                RefAction::Merge(n.clone()),
            ))
            .item(item(
                "从这里新建分支…".into(),
                RefAction::NewBranchFrom(target.to_string()),
            )),
        RefKind::Tag => menu
            .item(item(
                "检出(分离 HEAD)".into(),
                RefAction::CheckoutDetached(target.to_string()),
            ))
            .item(item(
                "从这里新建分支…".into(),
                RefAction::NewBranchFrom(target.to_string()),
            ))
            .separator()
            .item(item("删除标签".into(), RefAction::DeleteTag(n.clone()))),
    }
}

fn stash_item(label: &'static str, index: usize, pop: bool) -> PopupMenuItem {
    PopupMenuItem::new(label).on_click(move |_, window, cx| {
        window.dispatch_action(Box::new(StashAction { index, pop }), cx)
    })
}

/// 菜单项通过动作把参数送回视图,避免在菜单闭包里持有视图引用。
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = gitlane, no_json)]
pub enum RefAction {
    Checkout(String),
    CheckoutRemote(String),
    CheckoutDetached(String),
    Merge(String),
    Rename(String),
    Delete(String),
    DeleteTag(String),
    NewBranchFrom(String),
}

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = gitlane, no_json)]
pub struct StashAction {
    pub index: usize,
    pub pop: bool,
}

#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = gitlane, no_json)]
pub struct StashDropAction(pub usize);

#[cfg(test)]
mod tests {
    // 不能 `use super::*`:会把 gpui 的 `test` 宏一起导入,遮住标准的 #[test]
    use super::{SidebarRow, flatten_refs};
    use gitcore::{Head, Oid, RefInfo, RefKind, Refs};
    use std::collections::HashSet;

    fn r(short: &str, kind: RefKind) -> RefInfo {
        let prefix = match kind {
            RefKind::Local => "refs/heads/",
            RefKind::Remote => "refs/remotes/",
            RefKind::Tag => "refs/tags/",
        };
        RefInfo {
            name: format!("{prefix}{short}"),
            short: short.into(),
            kind,
            target: Oid::ZERO_SHA1,
            upstream: None,
            ahead_behind: None,
            is_head: false,
        }
    }

    fn shape(rows: &[SidebarRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                SidebarRow::Section { title, count } => format!("# {title} {count}"),
                SidebarRow::Group { prefix, .. } => format!("+ {prefix}"),
                SidebarRow::Ref { label, depth, .. } => format!("{}{label}", "  ".repeat(*depth)),
                SidebarRow::Stash { pos } => format!("stash {pos}"),
            })
            .collect()
    }

    #[test]
    fn flattens_groups_and_respects_collapse() {
        let refs = Refs {
            head: Head::Unborn {
                name: "main".into(),
            },
            refs: vec![
                r("feature/a", RefKind::Local),
                r("feature/b", RefKind::Local),
                r("main", RefKind::Local),
                r("origin/main", RefKind::Remote),
                r("v1", RefKind::Tag),
            ],
            stashes: vec![],
        };
        let mut collapsed = HashSet::new();
        assert_eq!(
            shape(&flatten_refs(&refs, &collapsed, "")),
            vec![
                "# 本地分支 3",
                "  main",
                "+ feature",
                "    a",
                "    b",
                "# 远程 1",
                "+ origin",
                "    main",
                "# 标签 1",
                "  v1",
                "# Stash 0",
            ]
        );
        // 筛选:忽略折叠、全部展开,隐藏没有命中的分区
        assert_eq!(
            shape(&flatten_refs(&refs, &collapsed, "MAIN")),
            vec!["# 本地分支 1", "  main", "# 远程 1", "+ origin", "    main"]
        );
        collapsed.insert("local:feature".to_string());
        collapsed.insert("标签".to_string());
        assert_eq!(
            shape(&flatten_refs(&refs, &collapsed, "")),
            vec![
                "# 本地分支 3",
                "  main",
                "+ feature",
                "# 远程 1",
                "+ origin",
                "    main",
                "# 标签 1",
                "# Stash 0"
            ]
        );
    }
}

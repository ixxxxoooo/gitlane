//! 右栏:上方提交详情,下方暂存区与提交框。

use crate::format;
use crate::repo_view::RepoView;
use app::{Command, Selection};
use gitcore::diff::DiffTarget;
use gitcore::{ChangeKind, StatusEntry};
use gpui::base::TestSupportExt as _;
use gpui::component::Disableable as _;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::checkbox::Checkbox;
use gpui::component::input::Textarea;
use gpui::component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex};
use gpui::prelude::*;
use gpui::*;

/// 拆成(目录, 文件名),列表里文件名在前、目录用浅色跟在后面。
fn split_path(path: &str) -> (Option<String>, String) {
    match path.rsplit_once('/') {
        Some((d, n)) => (Some(d.to_string()), n.to_string()),
        None => (None, path.to_string()),
    }
}

/// 面板里没有内容时的提示。
fn empty_hint(text: &'static str, theme: &gpui::component::Theme) -> Div {
    div()
        .px_3()
        .py_1()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(text)
}

fn kind_color(kind: ChangeKind, theme: &gpui::component::Theme) -> Hsla {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => theme.green,
        ChangeKind::Deleted => theme.red,
        ChangeKind::Conflicted => theme.warning,
        ChangeKind::Renamed | ChangeKind::Copied => theme.blue,
        _ => theme.yellow,
    }
}

impl RepoView {
    pub(crate) fn render_side_panel(
        &mut self,
        width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let selection = self.repo.read(cx).selection;
        // 选中历史提交时只显示这个提交的详情(没有暂存区和提交框,历史提交不能再改);
        // 只有选中 Working changes 才显示暂存区:已暂存在上、未暂存在下、固定在底部的提交框
        let panel = v_flex().w(width).flex_none().h_full();
        let panel = if matches!(selection, Selection::Commit(_)) {
            panel.child(self.render_details(cx))
        } else {
            let (unstaged, staged, commit_box) = self.render_staging(window, cx);
            panel.child(staged).child(unstaged).child(commit_box)
        };
        panel.id("side-panel").test_support()
    }

    fn render_details(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let repo = self.repo.read(cx);
        let theme = cx.theme().clone();
        let dark = theme.is_dark();
        let icon = Icon::from(gpui::assets::IconName::GitCommitHorizontal);
        let Some(detail) = repo.detail.clone() else {
            let loading = div()
                .p_3()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("加载中…")
                .into_any_element();
            return self.side_section(
                "details",
                false,
                (Some(icon), "提交详情".into()),
                None::<Div>,
                loading,
                cx,
            );
        };
        let sha = detail.info.id.to_string();
        let short = gitcore::short_id(detail.info.id);
        let body = detail
            .message
            .trim()
            .split_once('\n')
            .map(|x| x.1)
            .unwrap_or("")
            .trim()
            .to_string();
        let (additions, deletions) = detail
            .files
            .iter()
            .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions));
        let avatar = self.avatar_for(&detail.info.email, cx);
        let initial = detail
            .info
            .author
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        let meta_row = |icon: Icon, text: String| {
            h_flex()
                .gap_2()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(icon.xsmall())
                .child(div().truncate().child(text))
        };

        let meta = v_flex()
            .gap_3()
            .p_3()
            .child(
                v_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .whitespace_normal()
                            .child(detail.info.summary.clone()),
                    )
                    .when(!body.is_empty(), |d| {
                        d.child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .whitespace_normal()
                                .child(body),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        // 有联网查到的头像就显示头像,否则是按作者名取色的首字母圆圈
                        div()
                            .size(px(28.))
                            .flex_none()
                            .rounded_full()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(crate::colors::avatar_color(&detail.info.author, dark))
                            .text_color(gpui::white())
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .map(|d| match avatar {
                                Some(path) => {
                                    d.child(img(path).size_full().object_fit(ObjectFit::Cover))
                                }
                                None => d.child(initial),
                            }),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(detail.info.author.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(detail.info.email.clone()),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(meta_row(
                        Icon::from(gpui::assets::IconName::Clock),
                        format::datetime(detail.info.time, detail.info.tz_offset),
                    ))
                    .when(detail.committer != detail.info.author, |d| {
                        d.child(meta_row(
                            Icon::new(IconName::User),
                            format!(
                                "提交者 {} · {}",
                                detail.committer,
                                format::datetime(detail.commit_time, 0)
                            ),
                        ))
                    })
                    .child(
                        meta_row(Icon::from(gpui::assets::IconName::Hash), short)
                            .id("sha")
                            .font_family("Menlo")
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.foreground))
                            .tooltip(|window, cx| {
                                gpui::component::tooltip::Tooltip::new("点击复制完整 SHA")
                                    .build(window, cx)
                            })
                            .on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(sha.clone()));
                                use gpui::component::WindowExt as _;
                                window.push_notification(
                                    gpui::component::notification::Notification::info("已复制 SHA"),
                                    cx,
                                );
                            }),
                    )
                    .when(detail.info.parents.len() > 1, |d| {
                        d.child(meta_row(
                            Icon::from(gpui::assets::IconName::GitBranch),
                            format!(
                                "合并提交 · 父提交 {}",
                                detail
                                    .info
                                    .parents
                                    .iter()
                                    .map(|p| gitcore::short_id(*p))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            ),
                        ))
                    }),
            );

        let files_header = h_flex()
            .h(px(32.))
            .px_3()
            .gap_2()
            .text_xs()
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(format!("{} 个文件改动", detail.files.len())),
            )
            .child(div().text_color(theme.green).child(format!("+{additions}")))
            .child(div().text_color(theme.red).child(format!("−{deletions}")));

        let n = detail.files.len();
        let body = v_flex()
            .size_full()
            .child(
                div()
                    .id("details")
                    .flex_none()
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .child(meta),
            )
            .child(files_header)
            .child(
                uniform_list(
                    "commit-files",
                    n,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        this.render_commit_files(range, cx)
                    }),
                )
                .flex_1()
                .min_h_0(),
            )
            .into_any_element();
        self.side_section(
            "details",
            false,
            (Some(icon), "提交详情".into()),
            None::<Div>,
            body,
            cx,
        )
    }

    /// 提交改动文件列表的可见行。一个提交可能改上千个文件,所以用虚拟列表。
    fn render_commit_files(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let repo = self.repo.read(cx);
        let Some(detail) = repo.detail.clone() else {
            return Vec::new();
        };
        let theme = cx.theme().clone();
        let oid = detail.info.id;
        let open_path = repo.diff.as_ref().map(|d| d.path.clone());
        detail.files[range.start.min(detail.files.len())..range.end.min(detail.files.len())]
            .iter()
            .map(|f| {
                let path = f.path.clone();
                let active = open_path.as_deref() == Some(f.path.as_str());
                let (dir, name) = split_path(&f.path);
                let secondary = match &f.old_path {
                    Some(old) => Some(format!("← {old}")),
                    None => dir,
                };
                h_flex()
                    .id(SharedString::from(format!("cf-{}", f.path)))
                    // 虚拟列表里的行默认只有内容那么宽,不撑满的话增删行数和状态字母会紧跟在路径后面
                    .w_full()
                    .px_3()
                    .h(px(26.))
                    .gap_2()
                    .text_sm()
                    .cursor_pointer()
                    .when(active, |d| d.bg(theme.list_active))
                    .when(!active, |d| d.hover(|s| s.bg(theme.list_hover)))
                    .child(crate::file_icons::file_icon(&f.path, &theme).xsmall())
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .gap_1p5()
                            .child(div().flex_none().child(name))
                            .children(secondary.map(|d| {
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(d)
                            })),
                    )
                    .child(
                        h_flex()
                            .flex_none()
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
                            ),
                    )
                    .child(
                        div()
                            .w(px(12.))
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(kind_color(f.kind, &theme))
                            .child(f.kind.letter()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let path = path.clone();
                        this.repo
                            .update(cx, |r, cx| r.open_diff(DiffTarget::Commit(oid), path, cx))
                    }))
                    .into_any_element()
            })
            .collect()
    }

    /// 返回 (未暂存面板, 已暂存面板, 提交框)。
    fn render_staging(
        &mut self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, AnyElement, AnyElement) {
        let repo = self.repo.read(cx);
        let theme = cx.theme().clone();
        let status = repo.status.clone();
        let open = repo.diff.as_ref().map(|d| (d.target, d.path.clone()));
        let busy = repo.busy.is_some();
        let (unstaged, staged): (Vec<StatusEntry>, Vec<StatusEntry>) = status
            .as_ref()
            .map(|s| {
                (
                    s.unstaged().cloned().collect(),
                    s.staged().cloned().collect(),
                )
            })
            .unwrap_or_default();

        let file_row = |e: &StatusEntry, staged_list: bool, cx: &mut Context<Self>| {
            let kind =
                if staged_list { e.staged } else { e.unstaged }.unwrap_or(ChangeKind::Modified);
            let target = if staged_list {
                DiffTarget::Staged
            } else {
                DiffTarget::Unstaged
            };
            let path = e.path.clone();
            let active = open
                .as_ref()
                .is_some_and(|(t, p)| *t == target && *p == e.path);
            let (dir, name) = split_path(&e.path);
            let p_open = path.clone();
            let p_action = path.clone();
            let p_discard = path.clone();
            let untracked = kind == ChangeKind::Untracked;
            // 增删行数;二进制文件或超过统计上限时没有数字
            let lines = if staged_list {
                e.staged_lines
            } else {
                e.unstaged_lines
            }
            .filter(|(a, d)| a + d > 0);
            h_flex()
                .id(SharedString::from(format!(
                    "{}-{}",
                    if staged_list { "s" } else { "u" },
                    e.path
                )))
                .group("file-row")
                .px_3()
                .h(px(24.))
                .gap_2()
                .text_sm()
                .cursor_pointer()
                .when(active, |d| d.bg(theme.list_active))
                .hover(|s| s.bg(theme.list_hover))
                .child(crate::file_icons::file_icon(&e.path, &theme).xsmall())
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(div().flex_none().child(name))
                        .children(dir.map(|d| {
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(d)
                        })),
                )
                .children(lines.map(|(a, d)| {
                    h_flex()
                        .flex_none()
                        .gap_1()
                        .text_xs()
                        .child(div().text_color(theme.green).child(format!("+{a}")))
                        .when(d > 0, |x| {
                            x.child(div().text_color(theme.red).child(format!("−{d}")))
                        })
                }))
                .child(
                    div()
                        .w(px(12.))
                        .flex_none()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(kind_color(kind, &theme))
                        .child(kind.letter()),
                )
                .when(!staged_list, |d| {
                    d.child(
                        Button::new(SharedString::from(format!("discard-{}", e.path)))
                            .xsmall()
                            .ghost()
                            .icon(Icon::from(gpui::assets::IconName::Undo2))
                            .tooltip("丢弃这个文件的改动")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_discard(
                                    vec![(p_discard.clone(), untracked)],
                                    window,
                                    cx,
                                )
                            })),
                    )
                })
                .child(
                    Button::new(SharedString::from(format!(
                        "toggle-{}-{}",
                        staged_list, e.path
                    )))
                    .xsmall()
                    .ghost()
                    .icon(Icon::from(if staged_list {
                        gpui::assets::IconName::Minus
                    } else {
                        gpui::assets::IconName::Plus
                    }))
                    .tooltip(if staged_list {
                        "取消暂存"
                    } else {
                        "暂存"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let p = vec![p_action.clone()];
                        this.run(
                            if staged_list {
                                Command::UnstagePaths(p)
                            } else {
                                Command::StagePaths(p)
                            },
                            cx,
                        )
                    })),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let path = p_open.clone();
                    this.repo.update(cx, |r, cx| {
                        r.select_working(cx);
                        r.open_diff(target, path, cx)
                    })
                }))
                .test_support()
        };

        let all_unstaged: Vec<(String, bool)> = unstaged
            .iter()
            .map(|e| (e.path.clone(), e.unstaged == Some(ChangeKind::Untracked)))
            .collect();

        // 标题栏右侧的图标按钮:丢弃全部(只有多于一个文件时)+ 全部暂存
        let stage_all = (!unstaged.is_empty()).then(|| {
            h_flex()
                .gap_0p5()
                .when(unstaged.len() > 1, |d| {
                    d.child(
                        Button::new("discard-all")
                            .xsmall()
                            .ghost()
                            .icon(Icon::from(gpui::assets::IconName::Undo2))
                            .tooltip("丢弃全部未暂存改动")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_discard(all_unstaged.clone(), window, cx)
                            })),
                    )
                })
                .child(
                    Button::new("stage-all")
                        .xsmall()
                        .ghost()
                        .icon(Icon::from(gpui::assets::IconName::Plus))
                        .tooltip("全部暂存")
                        .on_click(cx.listener(|this, _, _, cx| this.run(Command::StageAll, cx))),
                )
        });
        let unstaged_body = v_flex()
            .children(unstaged.iter().map(|e| file_row(e, false, cx)))
            .when(unstaged.is_empty(), |d| {
                d.child(empty_hint("没有未暂存的改动", &theme))
            });
        let unstage_all = (!staged.is_empty()).then(|| {
            Button::new("unstage-all")
                .xsmall()
                .ghost()
                .icon(Icon::from(gpui::assets::IconName::Minus))
                .tooltip("全部取消暂存")
                .on_click(cx.listener(|this, _, _, cx| this.run(Command::UnstageAll, cx)))
        });
        let staged_body = v_flex()
            .children(staged.iter().map(|e| file_row(e, true, cx)))
            .when(staged.is_empty(), |d| {
                d.child(empty_hint("没有已暂存的改动", &theme))
            });
        let scroll = |id: &'static str, body: Div| {
            div()
                .id(id)
                .size_full()
                .overflow_y_scroll()
                .child(body)
                .into_any_element()
        };
        let unstaged_panel = self.side_section(
            "unstaged",
            true,
            (None, format!("未暂存 {}", unstaged.len())),
            stage_all,
            scroll("staging-unstaged", unstaged_body),
            cx,
        );
        let staged_panel = self.side_section(
            "staged",
            true,
            (None, format!("已暂存 {}", staged.len())),
            unstage_all,
            scroll("staging-staged", staged_body),
            cx,
        );

        let amend = self.amend;
        let commit_box = v_flex()
            .id("commit-box")
            .test_support()
            // 两个面板都收起时中间有空余高度,提交框仍沉在最底部
            .mt_auto()
            .flex_none()
            .p_3()
            .gap_2()
            // 不再加顶边框:上面最后一个面板自带底边框,两条叠在一起会变成 2px 的粗线
            .child(Textarea::new(&self.message).h(px(96.)))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Checkbox::new("amend")
                            .label("修改上一次提交(amend)")
                            .checked(amend)
                            .on_click(cx.listener(|this, checked: &bool, window, cx| {
                                this.amend = *checked;
                                // 勾选 amend 且输入框为空时,带出上一次的提交信息
                                if *checked && this.message.read(cx).value().trim().is_empty() {
                                    let last = this.repo.read(cx).data.as_ref().and_then(|d| {
                                        let head = d.refs.head.oid()?;
                                        let row = *d.history.row_of.get(&head)?;
                                        Some(d.history.commits[row as usize].summary.clone())
                                    });
                                    if let Some(msg) = last {
                                        this.message
                                            .update(cx, |m, cx| m.set_value(msg, window, cx));
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("commit")
                            .small()
                            .primary()
                            .label(if amend { "修改提交" } else { "提交" })
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_commit(&crate::actions::Commit, window, cx)
                            })),
                    ),
            );

        (unstaged_panel, staged_panel, commit_box.into_any_element())
    }

    pub(crate) fn confirm_discard(
        &mut self,
        files: Vec<(String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut lines = vec![format!(
            "将丢弃以下 {} 个文件的未暂存改动(可以撤销):",
            files.len()
        )];
        lines.extend(files.iter().take(20).map(|(p, untracked)| {
            format!(
                "  {p}{}",
                if *untracked {
                    "(未跟踪,将删除)"
                } else {
                    ""
                }
            )
        }));
        if files.len() > 20 {
            lines.push(format!("  …以及另外 {} 个文件", files.len() - 20));
        }
        let view = cx.entity().downgrade();
        crate::dialogs::confirm(window, cx, "丢弃改动", lines, "丢弃", move |_, cx| {
            let (untracked, tracked): (Vec<_>, Vec<_>) =
                files.iter().cloned().partition(|(_, u)| *u);
            let _ = view.update(cx, |v, cx| {
                v.run(
                    Command::Discard {
                        tracked: tracked.into_iter().map(|(p, _)| p).collect(),
                        untracked: untracked.into_iter().map(|(p, _)| p).collect(),
                    },
                    cx,
                )
            });
        });
    }
}

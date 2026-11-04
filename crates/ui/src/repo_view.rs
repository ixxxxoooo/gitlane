//! 单个仓库标签页的主视图:左侧引用树 + 中间提交图与 diff + 右侧详情与暂存区。
//! 具体区域的渲染拆在 sidebar / graph_view / diff_view / side_panel 中,都是 `impl RepoView`。

use crate::actions::*;
use crate::diff_stage::DiffCache;
use crate::panels::{self, DiffDrag, PanelDrag, PanelSide};
use app::{Command, NoticeKind, RepoEvent, RepoModel, Selection, Workspace};
use gitcore::RepoState;
use gitcore::ops::ResetMode;
use gpui::base::TestSupportExt as _;
use gpui::component::Disableable as _;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::input::{Input, InputEvent, InputState, TextareaState};
use gpui::component::notification::Notification;
use gpui::component::{ActiveTheme, Icon, IconName, Sizable, WindowExt, h_flex, v_flex};
use gpui::prelude::*;
use gpui::*;
use std::collections::{BTreeSet, HashSet};

pub struct RepoView {
    pub(crate) repo: Entity<RepoModel>,
    pub(crate) workspace: WeakEntity<Workspace>,
    pub(crate) focus: FocusHandle,
    pub(crate) graph_focus: FocusHandle,
    pub(crate) graph_scroll: UniformListScrollHandle,
    /// diff 组件的状态(文件、模式、选区、滚动都在里面)。
    pub(crate) diff_state: Entity<diffview::DiffState>,
    /// 组件当前按哪种主题配的高亮色。
    pub(crate) diff_dark: Option<bool>,
    pub(crate) search: Entity<InputState>,
    pub(crate) message: Entity<TextareaState>,
    pub(crate) amend: bool,
    /// 当前工作区 diff 中选中的行:(hunk, 行)。
    pub(crate) selected_lines: BTreeSet<(usize, usize)>,
    pub(crate) diff_cache: Option<DiffCache>,
    /// 搜索结果中当前定位到第几个。
    pub(crate) search_cursor: usize,
    pub(crate) collapsed: HashSet<String>,
    /// diff 区域是否撑满中间栏(隐藏提交图),方便看大段对比。
    pub(crate) diff_maximized: bool,
    /// 作者头像缓存:邮箱 → 本地头像文件(`None` 表示没有头像或还没查到)。
    pub(crate) avatars: std::collections::HashMap<String, Option<std::path::PathBuf>>,
    /// 右栏收起的面板("details" / "unstaged" / "staged")。
    pub(crate) side_collapsed: HashSet<&'static str>,
    /// 左侧引用树顶部的筛选框。
    pub(crate) sidebar_filter: Entity<InputState>,
    pub(crate) sidebar_rows: std::rc::Rc<Vec<crate::sidebar::SidebarSection>>,
    /// 侧栏每个面板各一个滚动句柄,按面板顺序。
    pub(crate) sidebar_scrolls: Vec<UniformListScrollHandle>,
    _subscriptions: Vec<Subscription>,
}

impl RepoView {
    pub fn new(
        repo: Entity<RepoModel>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // 上次退出时的布局状态(折叠、撑满),新开的标签也沿用
        let saved = workspace
            .upgrade()
            .map(|ws| ws.read(cx).settings.clone())
            .unwrap_or_default();
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索提交信息 / 作者 / SHA"));
        let sidebar_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("筛选分支 / 标签…"));
        let message = cx.new(|cx| TextareaState::new(window, cx).placeholder("提交信息(⌘↵ 提交)"));

        let diff_state = cx.new(|cx| {
            // 不做词级强调:组件按“词”比较,中文每个字算一个词,两行注释会被切成忽深忽浅的碎片
            diffview::DiffState::new(Vec::new(), cx)
                .with_context_lines(None)
                .with_inline_unit(None)
        });
        let subscriptions = vec![
            cx.subscribe(&diff_state, |this, _, e: &diffview::DiffEvent, cx| {
                this.on_diff_event(e, cx)
            }),
            cx.observe(&repo, |this, _, cx| {
                this.prune_selection(cx);
                cx.notify();
            }),
            cx.subscribe_in(&repo, window, Self::on_repo_event),
            cx.subscribe(&sidebar_filter, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &search,
                window,
                |this, state, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        let q = state.read(cx).value().to_string();
                        this.search_cursor = 0;
                        this.repo.update(cx, |r, cx| r.set_search(q, cx));
                        this.jump_to_search_hit(0, window, cx);
                    }
                    InputEvent::PressEnter { shift, .. } => {
                        let delta = if *shift { -1 } else { 1 };
                        this.jump_to_search_hit(delta, window, cx);
                    }
                    _ => {}
                },
            ),
        ];

        Self {
            repo,
            workspace,
            focus: cx.focus_handle(),
            graph_focus: cx.focus_handle(),
            graph_scroll: UniformListScrollHandle::new(),
            diff_state,
            diff_dark: None,
            search,
            message,
            amend: false,
            selected_lines: BTreeSet::new(),
            diff_cache: None,
            search_cursor: 0,
            collapsed: saved.sidebar_collapsed.iter().cloned().collect(),
            diff_maximized: saved.diff_maximized,
            avatars: Default::default(),
            side_collapsed: ["details", "unstaged", "staged"]
                .into_iter()
                .filter(|k| saved.side_collapsed.iter().any(|s| s == k))
                .collect(),
            sidebar_filter,
            sidebar_rows: Default::default(),
            sidebar_scrolls: (0..crate::sidebar::SECTION_COUNT)
                .map(|_| UniformListScrollHandle::new())
                .collect(),
            _subscriptions: subscriptions,
        }
    }

    /// 某个邮箱的头像文件。第一次遇到时在后台联网查询(或读本地缓存),查完再刷新界面;
    /// 查询期间、查不到或关闭了联网获取时返回 `None`,界面显示首字母。
    pub(crate) fn avatar_for(
        &mut self,
        email: &str,
        cx: &mut Context<Self>,
    ) -> Option<std::path::PathBuf> {
        if let Some(found) = self.avatars.get(email) {
            return found.clone();
        }
        let enabled = self
            .workspace
            .upgrade()
            .is_some_and(|ws| ws.read(cx).avatars_enabled());
        if !enabled || email.trim().is_empty() {
            return None;
        }
        // 先占位,避免每次渲染都重复发起查询
        self.avatars.insert(email.to_string(), None);
        let email = email.to_string();
        cx.spawn(async move |this, cx| {
            let e = email.clone();
            let path = cx
                .background_spawn(async move { app::avatar::fetch(&app::avatar::cache_dir(), &e) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.avatars.insert(email, path);
                cx.notify();
            });
        })
        .detach();
        None
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    fn on_repo_event(
        &mut self,
        _: &Entity<RepoModel>,
        event: &RepoEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            RepoEvent::Notice { kind, message } => {
                let note = match kind {
                    NoticeKind::Info => Notification::info(message.clone()),
                    NoticeKind::Success => Notification::success(message.clone()),
                    NoticeKind::Error => Notification::error(message.clone()),
                };
                window.push_notification(note, cx);
            }
            RepoEvent::Committed => {
                self.amend = false;
                self.message.update(cx, |m, cx| m.set_value("", window, cx));
            }
        }
    }

    /// diff 换了文件后,旧的行选择失效。
    fn prune_selection(&mut self, cx: &mut Context<Self>) {
        let key = self
            .repo
            .read(cx)
            .diff
            .as_ref()
            .and_then(|d| d.file.as_ref())
            .map(|f| std::sync::Arc::as_ptr(f) as usize);
        if self.diff_cache.as_ref().map(|c| c.key) != key {
            self.selected_lines.clear();
        }
    }

    pub(crate) fn run(&mut self, cmd: Command, cx: &mut Context<Self>) {
        self.repo.update(cx, |r, cx| r.run(cmd, cx));
    }

    fn jump_to_search_hit(&mut self, delta: isize, _: &mut Window, cx: &mut Context<Self>) {
        let hits = self.repo.read(cx).search_hits.clone();
        if hits.is_empty() {
            return;
        }
        let n = hits.len() as isize;
        if delta != 0 {
            self.search_cursor = ((self.search_cursor as isize + delta).rem_euclid(n)) as usize;
        }
        let row = hits[self.search_cursor.min(hits.len() - 1)];
        self.select_row(row, cx);
    }

    pub(crate) fn select_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let oid = self
            .repo
            .read(cx)
            .data
            .as_ref()
            .and_then(|d| d.history.commits.get(row))
            .map(|c| c.id);
        if let Some(oid) = oid {
            self.graph_scroll
                .scroll_to_item(row, ScrollStrategy::Center);
            self.repo.update(cx, |r, cx| r.select_commit(oid, cx));
        }
    }

    // ---- 动作处理 ----

    fn on_refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.repo.update(cx, |r, cx| r.refresh(true, cx));
    }

    fn on_fetch(&mut self, _: &Fetch, _: &mut Window, cx: &mut Context<Self>) {
        self.run(Command::Fetch, cx);
    }

    fn on_pull(&mut self, _: &Pull, _: &mut Window, cx: &mut Context<Self>) {
        self.run(Command::Pull, cx);
    }

    fn on_push(&mut self, _: &Push, _: &mut Window, cx: &mut Context<Self>) {
        self.run(Command::Push { force: false }, cx);
    }

    fn on_force_push(&mut self, _: &ForcePush, window: &mut Window, cx: &mut Context<Self>) {
        let repo = self.repo.read(cx);
        let Some(data) = repo.data.as_ref() else {
            return;
        };
        let head = data.refs.refs.iter().find(|r| r.is_head);
        let mut lines = vec!["将使用 --force-with-lease 覆盖远程分支。".to_string()];
        match head.and_then(|h| h.upstream.clone().zip(h.ahead_behind)) {
            Some((up, (_, behind))) if behind > 0 => lines.push(format!(
                "{up} 上有 {behind} 个本地没有的提交,推送后它们会从远程分支上消失。"
            )),
            Some((up, _)) => lines.push(format!("{up} 没有本地缺少的提交。")),
            None => lines.push("当前分支没有上游。".into()),
        }
        let view = cx.entity().downgrade();
        crate::dialogs::confirm(
            window,
            cx,
            "Force push",
            lines,
            "强制推送",
            move |_, cx| {
                let _ = view.update(cx, |v, cx| v.run(Command::Push { force: true }, cx));
            },
        );
    }

    fn on_new_branch(&mut self, _: &NewBranch, window: &mut Window, cx: &mut Context<Self>) {
        let start = match self.repo.read(cx).selection {
            Selection::Commit(oid) => oid.to_string(),
            Selection::WorkingChanges => "HEAD".into(),
        };
        self.prompt_new_branch(start, window, cx);
    }

    pub(crate) fn prompt_new_branch(
        &mut self,
        start: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity().downgrade();
        let at = if start == "HEAD" {
            "HEAD".to_string()
        } else {
            gitcore::short_id(start.parse().unwrap_or(gitcore::Oid::ZERO_SHA1))
        };
        crate::dialogs::prompt_text(
            window,
            cx,
            format!("从 {at} 新建分支并检出"),
            "分支名,如 feature/login",
            String::new(),
            move |name, _, cx| {
                let start = start.clone();
                let _ = view.update(cx, |v, cx| {
                    v.run(
                        Command::CreateBranch {
                            name,
                            start,
                            checkout: true,
                        },
                        cx,
                    )
                });
            },
        );
    }

    fn on_stash(&mut self, _: &Stash, _: &mut Window, cx: &mut Context<Self>) {
        self.run(Command::StashPush { message: None }, cx);
    }

    fn on_stash_pop(&mut self, _: &StashPop, _: &mut Window, cx: &mut Context<Self>) {
        self.run(
            Command::StashApply {
                index: 0,
                pop: true,
            },
            cx,
        );
    }

    fn on_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.repo.update(cx, |r, cx| r.undo(cx));
    }

    fn on_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.repo.update(cx, |r, cx| r.redo(cx));
    }

    pub(crate) fn on_commit(&mut self, _: &Commit, window: &mut Window, cx: &mut Context<Self>) {
        // 选中历史提交时界面上没有提交框,不响应 ⌘↵,避免看不见的提交
        if self.repo.read(cx).selection != app::Selection::WorkingChanges {
            return;
        }
        let message = self.message.read(cx).value().to_string();
        if message.trim().is_empty() {
            window.push_notification(Notification::warning("请先填写提交信息"), cx);
            self.message.focus_handle(cx).focus(window, cx);
            return;
        }
        let has_staged = self
            .repo
            .read(cx)
            .status
            .as_ref()
            .is_some_and(|s| s.staged().next().is_some());
        if !has_staged && !self.amend {
            window.push_notification(Notification::warning("没有已暂存的改动"), cx);
            return;
        }
        let amend = self.amend;
        self.run(Command::Commit { message, amend }, cx);
    }

    fn on_stage_all(&mut self, _: &StageAll, _: &mut Window, cx: &mut Context<Self>) {
        self.run(Command::StageAll, cx);
    }

    fn on_unstage_all(&mut self, _: &UnstageAll, _: &mut Window, cx: &mut Context<Self>) {
        self.run(Command::UnstageAll, cx);
    }

    fn on_focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search.focus_handle(cx).focus(window, cx);
    }

    /// Esc:diff 撑满时还原成上下分栏;没撑满时不处理,让 Esc 继续传给输入框等控件。
    fn on_exit_maximized(&mut self, _: &ExitMaximizedDiff, _: &mut Window, cx: &mut Context<Self>) {
        if self.diff_maximized {
            self.diff_maximized = false;
            self.save_layout(cx);
            cx.notify();
        } else {
            cx.propagate();
        }
    }

    fn on_focus_message(
        &mut self,
        _: &FocusCommitMessage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 提交框只在 Working changes 下存在:先切过去,等下一帧渲染出来再聚焦
        if self.repo.read(cx).selection != app::Selection::WorkingChanges {
            self.repo.update(cx, |r, cx| r.select_working(cx));
            let handle = self.message.focus_handle(cx);
            window.on_next_frame(move |window, cx| handle.focus(window, cx));
            return;
        }
        self.message.focus_handle(cx).focus(window, cx);
    }

    fn on_abort(&mut self, _: &AbortOperation, _: &mut Window, cx: &mut Context<Self>) {
        self.run(Command::AbortInProgress, cx);
    }

    fn on_toggle_split(&mut self, _: &ToggleSplitDiff, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| {
                let split = !ws.settings.split_diff;
                ws.set_split_diff(split, cx)
            });
        }
        cx.notify();
    }

    fn on_select_next(&mut self, _: &SelectNextCommit, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(1, cx);
    }

    fn on_select_prev(&mut self, _: &SelectPrevCommit, _: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(-1, cx);
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let repo = self.repo.read(cx);
        let Some(data) = repo.data.as_ref() else {
            return;
        };
        let n = data.history.commits.len();
        if n == 0 {
            return;
        }
        let current = match repo.selection {
            Selection::Commit(oid) => data.history.row_of.get(&oid).map(|r| *r as isize),
            Selection::WorkingChanges => None,
        };
        match current {
            None if delta > 0 => self.select_row(0, cx),
            None => {}
            Some(0) if delta < 0 => self.repo.update(cx, |r, cx| r.select_working(cx)),
            Some(r) => self.select_row((r + delta).clamp(0, n as isize - 1) as usize, cx),
        }
    }

    pub(crate) fn confirm_reset(
        &mut self,
        rev: String,
        mode: ResetMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = gitcore::short_id(rev.parse().unwrap_or(gitcore::Oid::ZERO_SHA1));
        let mut lines = vec![format!("把当前分支移动到 {label}。")];
        if let Some(st) = self
            .repo
            .read(cx)
            .status
            .as_ref()
            .filter(|_| mode == ResetMode::Hard)
            && !st.is_clean()
        {
            lines.push(format!(
                "工作区与暂存区的 {} 个文件改动将被丢弃(可以撤销):",
                st.entries.len()
            ));
            lines.extend(st.entries.iter().take(12).map(|e| format!("  {}", e.path)));
        }
        // 列出将离开当前分支的提交
        let path = self.repo.read(cx).path.clone();
        let rev2 = rev.clone();
        let task = cx.background_spawn(async move {
            gitcore::Git::new(path).run(&["log", "--oneline", "-n", "20", &format!("{rev2}..HEAD")])
        });
        let view = cx.entity().downgrade();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(out) = task.await
                && !out.trim().is_empty()
            {
                lines.push("以下提交将不再属于当前分支:".into());
                lines.extend(out.lines().map(|l| format!("  {l}")));
            }
            let _ = cx.update(|window, cx| {
                let title = format!("Reset {} 到 {label}", mode.flag().trim_start_matches("--"));
                crate::dialogs::confirm(window, cx, title, lines, "Reset", move |_, cx| {
                    let rev = rev.clone();
                    let _ = view.update(cx, |v, cx| v.run(Command::Reset { rev, mode }, cx));
                });
            });
        })
        .detach();
    }

    // ---- 左右栏宽度 ----

    /// 设定宽度来自全局设置(所有标签共用),按当前窗口宽度得出实际显示宽度。
    fn panel_widths(&self, window: &Window, cx: &App) -> (f32, f32) {
        let (left, right) = self
            .workspace
            .upgrade()
            .map(|ws| {
                let s = &ws.read(cx).settings;
                (s.sidebar_width, s.side_panel_width)
            })
            .unwrap_or((panels::LEFT_DEFAULT, panels::RIGHT_DEFAULT));
        panels::layout(f32::from(window.viewport_size().width), left, right)
    }

    fn panel_handle(
        &self,
        id: &'static str,
        side: PanelSide,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        // 左侧栏底色与中间不同:分隔条留白填侧栏底色,线贴着侧栏边缘
        let fill = (side == PanelSide::Left).then_some(theme.sidebar);
        panels::handle(id, side, theme.border, theme.primary, fill)
            .on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
                // 双击恢复默认宽度
                if e.click_count() >= 2 {
                    this.set_panel_width(side, side.default_width(), cx);
                    this.save_panel_widths(cx);
                }
            }))
            .test_support()
    }

    fn on_panel_drag(
        &mut self,
        e: &DragMoveEvent<PanelDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let side = e.drag(cx).0;
        let b = e.bounds;
        let x = e.event.position.x;
        let total = f32::from(b.size.width);
        let (left, right) = self.panel_widths(window, cx);
        let width = match side {
            PanelSide::Left => panels::dragged_width(side, f32::from(x - b.left()), total, right),
            PanelSide::Right => panels::dragged_width(side, f32::from(b.right() - x), total, left),
        };
        self.set_panel_width(side, width, cx);
    }

    /// 拖动中只改内存里的设置,松手时再写盘。
    fn set_panel_width(&mut self, side: PanelSide, width: f32, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| {
                match side {
                    PanelSide::Left => ws.settings.sidebar_width = width,
                    PanelSide::Right => ws.settings.side_panel_width = width,
                }
                cx.notify();
            });
        }
        cx.notify();
    }

    /// 拖动提交图与 diff 之间的分隔条:鼠标位置换算成 diff 高度比例,只改内存,松手时写盘。
    fn on_diff_drag(&mut self, e: &DragMoveEvent<DiffDrag>, cx: &mut Context<Self>) {
        let b = e.bounds;
        let ratio = panels::dragged_diff_ratio(
            f32::from(b.bottom() - e.event.position.y),
            f32::from(b.size.height),
        );
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| {
                ws.settings.diff_ratio = ratio;
                cx.notify();
            });
        }
        cx.notify();
    }

    fn reset_diff_ratio(&mut self, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| {
                ws.update_settings(cx, |s| s.diff_ratio = panels::DIFF_RATIO_DEFAULT)
            });
        }
        cx.notify();
    }

    /// 把折叠与撑满状态写进设置,下次启动恢复。
    pub(crate) fn save_layout(&self, cx: &mut Context<Self>) {
        let sorted = |v: Vec<String>| {
            let mut v = v;
            v.sort();
            v
        };
        let sidebar = sorted(self.collapsed.iter().cloned().collect());
        let side = sorted(self.side_collapsed.iter().map(|k| k.to_string()).collect());
        let maximized = self.diff_maximized;
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| {
                ws.update_settings(cx, |s| {
                    s.sidebar_collapsed = sidebar;
                    s.side_collapsed = side;
                    s.diff_maximized = maximized;
                })
            });
        }
    }

    fn save_panel_widths(&mut self, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| ws.update_settings(cx, |_| {}));
        }
    }

    // ---- 渲染 ----

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let repo = self.repo.read(cx);
        let busy = repo.busy.is_some();
        // 正在执行的是哪个工具栏动作:那个按钮自己转圈(替换图标、保持可见),其余按钮置灰
        let running = repo.busy.as_ref().map(|b| b.action);
        let cancellable = repo.busy.as_ref().is_some_and(|b| b.cancel.is_some());
        let spin = |a: app::BusyAction| running == Some(a);
        let (undo_label, redo_label) = (
            repo.undo.peek_undo().map(|e| format!("撤销:{}", e.label)),
            repo.undo.peek_redo().map(|e| format!("重做:{}", e.label)),
        );
        let head = repo.head_label();
        // 当前分支相对上游的 (领先, 落后);没有上游或没有差异时都是 0
        let (ahead, behind) = repo
            .data
            .as_ref()
            .and_then(|d| d.refs.refs.iter().find(|r| r.is_head))
            .and_then(|r| r.ahead_behind)
            .unwrap_or((0, 0));
        let theme = cx.theme().clone();
        let sep = || div().w(px(1.)).h(px(18.)).mx_1().bg(theme.border);
        let git_icon = |name: gpui::assets::IconName| Icon::from(name);
        h_flex()
            .id("toolbar")
            .test_support()
            .h(px(36.))
            .px_3()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            // 三段式:左边分支名、中间操作按钮、右边搜索;左右各占剩余宽度的一半,中间这组才是真正居中
            .child(
                h_flex().flex_1().min_w_0().child(
                    h_flex()
                        .id("head")
                        .px_2()
                        .gap_1p5()
                        .max_w(px(260.))
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(
                            git_icon(gpui::assets::IconName::GitBranch)
                                .small()
                                .text_color(theme.muted_foreground),
                        )
                        .child(div().truncate().child(head))
                        .when(ahead > 0 || behind > 0, |d| {
                            d.child(
                                h_flex()
                                    .gap_1()
                                    .text_xs()
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(theme.muted_foreground)
                                    .when(ahead > 0, |d| d.child(format!("↑{ahead}")))
                                    .when(behind > 0, |d| d.child(format!("↓{behind}"))),
                            )
                        }),
                ),
            )
            .child(
                h_flex()
                    .id("toolbar-actions")
                    .test_support()
                    .flex_none()
                    .gap_1()
                    .child(
                        Button::new("undo")
                            .small()
                            .ghost()
                            .icon(IconName::Undo2)
                            .tooltip(
                                undo_label
                                    .clone()
                                    .unwrap_or_else(|| "没有可撤销的操作".into()),
                            )
                            .disabled(busy || undo_label.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.repo.update(cx, |r, cx| r.undo(cx))
                            })),
                    )
                    .child(
                        Button::new("redo")
                            .small()
                            .ghost()
                            .icon(IconName::Redo2)
                            .tooltip(
                                redo_label
                                    .clone()
                                    .unwrap_or_else(|| "没有可重做的操作".into()),
                            )
                            .disabled(busy || redo_label.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.repo.update(cx, |r, cx| r.redo(cx))
                            })),
                    )
                    .child(sep())
                    .child(
                        Button::new("fetch")
                            .small()
                            .ghost()
                            .icon(IconName::RefreshCw)
                            .label("Fetch")
                            .loading(spin(app::BusyAction::Fetch))
                            .disabled(busy && !spin(app::BusyAction::Fetch))
                            .on_click(cx.listener(|this, _, _, cx| this.run(Command::Fetch, cx))),
                    )
                    .child(
                        Button::new("pull")
                            .small()
                            .ghost()
                            .icon(git_icon(gpui::assets::IconName::ArrowDownToLine))
                            .label(if behind > 0 {
                                format!("Pull ↓{behind}")
                            } else {
                                "Pull".to_string()
                            })
                            .when(behind > 0, |b| {
                                b.tooltip(format!("上游有 {behind} 个本地没有的提交,点击拉取"))
                            })
                            .loading(spin(app::BusyAction::Pull))
                            .disabled(busy && !spin(app::BusyAction::Pull))
                            .on_click(cx.listener(|this, _, _, cx| this.run(Command::Pull, cx))),
                    )
                    .child(
                        Button::new("push")
                            .small()
                            .ghost()
                            .icon(git_icon(gpui::assets::IconName::ArrowUpFromLine))
                            .label(if ahead > 0 {
                                format!("Push ↑{ahead}")
                            } else {
                                "Push".to_string()
                            })
                            .when(ahead > 0, |b| {
                                b.tooltip(format!("本地有 {ahead} 个提交还没推送,点击推送"))
                            })
                            .loading(spin(app::BusyAction::Push))
                            .disabled(busy && !spin(app::BusyAction::Push))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.run(Command::Push { force: false }, cx)
                            })),
                    )
                    .child(sep())
                    .child(
                        Button::new("branch")
                            .small()
                            .ghost()
                            .icon(git_icon(gpui::assets::IconName::GitBranch))
                            .label("Branch")
                            .tooltip("新建分支(⌘⇧B)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_new_branch(&NewBranch, window, cx)
                            })),
                    )
                    .child(
                        Button::new("stash")
                            .small()
                            .ghost()
                            .icon(git_icon(gpui::assets::IconName::Archive))
                            .label("Stash")
                            .loading(spin(app::BusyAction::Stash))
                            .disabled(busy && !spin(app::BusyAction::Stash))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.run(Command::StashPush { message: None }, cx)
                            })),
                    )
                    // 联网操作(fetch / pull / push)进行中才出现,点一下取消
                    .children(cancellable.then(|| {
                        Button::new("cancel")
                            .small()
                            .ghost()
                            .icon(IconName::Close)
                            .label("取消")
                            .tooltip("取消正在进行的联网操作")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.repo.update(cx, |r, cx| r.cancel_running(cx))
                            }))
                    })),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .justify_end()
                    .gap_1()
                    .child(
                        div()
                            .w(px(260.))
                            .child(Input::new(&self.search).small().cleanable(true)),
                    )
                    .child(
                        Button::new("palette")
                            .small()
                            .ghost()
                            .label("⌘K")
                            .tooltip("命令面板")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(CommandPalette), cx)
                            }),
                    ),
            )
    }

    fn render_banner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let repo = self.repo.read(cx);
        let state = repo.status.as_ref().map(|s| s.state)?;
        let text = match state {
            RepoState::Clean => return None,
            RepoState::Merge => "正在合并:解决冲突后提交,或中止合并",
            RepoState::Rebase => "正在 rebase:解决冲突后在命令行 git rebase --continue,或中止",
            RepoState::CherryPick => "正在 cherry-pick:解决冲突后提交,或中止",
            RepoState::Revert => "正在 revert:解决冲突后提交,或中止",
            RepoState::Bisect => "正在 bisect",
            RepoState::Other => "仓库处于未完成的操作中",
        };
        let theme = cx.theme().clone();
        Some(
            h_flex()
                .px_3()
                .py_1()
                .gap_2()
                .bg(theme.warning.opacity(0.15))
                .border_b_1()
                .border_color(theme.border)
                .child(div().flex_1().text_sm().child(text))
                .child(
                    Button::new("abort")
                        .xsmall()
                        .danger()
                        .label("中止")
                        .on_click(
                            cx.listener(|this, _, _, cx| this.run(Command::AbortInProgress, cx)),
                        ),
                ),
        )
    }
}

impl Focusable for RepoView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RepoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let has_diff = self.repo.read(cx).diff.is_some();
        let toolbar = self.render_toolbar(cx);
        let banner = self.render_banner(cx);
        let (left_w, right_w) = self.panel_widths(window, cx);
        let sidebar = self.render_sidebar(px(left_w), window, cx);
        let graph = self.render_graph(window, cx);
        let diff = has_diff.then(|| self.render_diff(window, cx));
        let side = self.render_side_panel(px(right_w), window, cx);

        v_flex()
            .key_context(REPO_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(cx.listener(Self::on_refresh))
            .on_action(cx.listener(Self::on_fetch))
            .on_action(cx.listener(Self::on_pull))
            .on_action(cx.listener(Self::on_push))
            .on_action(cx.listener(Self::on_force_push))
            .on_action(cx.listener(Self::on_new_branch))
            .on_action(cx.listener(Self::on_stash))
            .on_action(cx.listener(Self::on_stash_pop))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_action(cx.listener(Self::on_commit))
            .on_action(cx.listener(Self::on_stage_all))
            .on_action(cx.listener(Self::on_unstage_all))
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_focus_message))
            .on_action(cx.listener(Self::on_exit_maximized))
            .on_action(cx.listener(Self::on_copy_diff_lines))
            .on_action(cx.listener(Self::on_abort))
            .on_action(cx.listener(Self::on_toggle_split))
            .on_action(cx.listener(Self::on_select_next))
            .on_action(cx.listener(Self::on_select_prev))
            .child(toolbar)
            .children(banner)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .on_drag_move(cx.listener(Self::on_panel_drag))
                    .on_drop(cx.listener(|this, _: &PanelDrag, _, cx| this.save_panel_widths(cx)))
                    .child(sidebar)
                    .child(self.panel_handle("resize-left", PanelSide::Left, cx))
                    .child({
                        let ratio = self
                            .workspace
                            .upgrade()
                            .map_or(panels::DIFF_RATIO_DEFAULT, |ws| {
                                panels::clamp_diff_ratio(ws.read(cx).settings.diff_ratio)
                            });
                        let maximized = self.diff_maximized && diff.is_some();
                        let handle = panels::row_handle("resize-diff", theme.border, theme.primary)
                            .on_click(cx.listener(|this, e: &ClickEvent, _, cx| {
                                // 双击恢复默认比例
                                if e.click_count() >= 2 {
                                    this.reset_diff_ratio(cx);
                                }
                            }))
                            .test_support();
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .on_drag_move(cx.listener(
                                |this, e: &DragMoveEvent<DiffDrag>, _, cx| this.on_diff_drag(e, cx),
                            ))
                            .on_drop(
                                cx.listener(|this, _: &DiffDrag, _, cx| this.save_panel_widths(cx)),
                            )
                            // 撑满时不渲染提交图;否则提交图占剩余高度,diff 按比例取高度
                            .when(!maximized, |d| {
                                d.child(div().flex_1().min_h_0().child(graph))
                            })
                            .when(diff.is_some() && !maximized, |d| d.child(handle))
                            .children(diff.map(|d| {
                                div()
                                    .id("diff-panel")
                                    .test_support()
                                    .when(maximized, |x| x.flex_1())
                                    .when(!maximized, |x| x.h(relative(ratio)).flex_none())
                                    .min_h_0()
                                    .when(maximized, |x| x.border_t_1().border_color(theme.border))
                                    .child(d)
                            }))
                    })
                    .child(self.panel_handle("resize-right", PanelSide::Right, cx))
                    .child(side),
            )
    }
}

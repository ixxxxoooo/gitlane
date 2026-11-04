//! 窗口根视图:仓库标签栏 + 当前仓库视图;没有仓库时显示欢迎页。

use crate::actions::*;
use crate::palette::PaletteView;
use crate::repo_view::RepoView;
use crate::scan_dialog::ScanView;
use crate::settings_view::SettingsView;
use crate::updater::UpdateState;
use app::{ThemePreference, Workspace};
use gpui::base::Root;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::{
    ActiveTheme, IconName, Sizable, Theme, ThemeMode, WindowExt, h_flex, v_flex,
};
use gpui::prelude::*;
use gpui::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

pub struct GitlaneWindow {
    workspace: Entity<Workspace>,
    views: HashMap<EntityId, Entity<RepoView>>,
    focus: FocusHandle,
    /// 当前已应用的主题偏好;设置窗口改了主题后,在观察工作区时对比并重新应用。
    applied_theme: ThemePreference,
    /// 已打开的设置窗口,再次打开时直接激活它。
    settings_window: Option<WindowHandle<Root>>,
    /// 应用更新流程的状态,见 `updater.rs`。
    pub(crate) update: UpdateState,
    _subscriptions: Vec<Subscription>,
}

impl GitlaneWindow {
    pub fn new(
        workspace: Entity<Workspace>,
        initial: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme = workspace.read(cx).settings.theme;
        apply_theme(theme, window, cx);
        for p in initial {
            let r = workspace.update(cx, |ws, cx| ws.open_repo(&p, cx));
            if let Err(e) = r {
                window.push_notification(gpui::component::notification::Notification::error(e), cx);
            }
        }
        let sub = cx.observe_in(&workspace, window, |this, ws, window, cx| {
            let theme = ws.read(cx).settings.theme;
            if theme != this.applied_theme {
                this.applied_theme = theme;
                apply_theme(theme, window, cx);
                window.refresh();
            }
            this.sync_views(window, cx);
            cx.notify();
        });
        // 跟随系统时,系统切换外观要跟着换
        let appearance = cx.observe_window_appearance(window, |this, window, cx| {
            if this.applied_theme == ThemePreference::System {
                apply_theme(ThemePreference::System, window, cx);
                window.refresh();
            }
        });
        let mut this = Self {
            workspace,
            views: HashMap::new(),
            focus: cx.focus_handle(),
            applied_theme: theme,
            settings_window: None,
            update: UpdateState::default(),
            _subscriptions: vec![sub, appearance],
        };
        this.sync_views(window, cx);
        this
    }

    /// 为新打开的仓库创建视图,清理已关闭仓库的视图。
    fn sync_views(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repos = self.workspace.read(cx).repos.clone();
        let ws = self.workspace.downgrade();
        let mut created = None;
        for repo in &repos {
            self.views.entry(repo.entity_id()).or_insert_with(|| {
                let v = cx.new(|cx| RepoView::new(repo.clone(), ws.clone(), window, cx));
                created = Some(v.clone());
                v
            });
        }
        self.views
            .retain(|id, _| repos.iter().any(|r| r.entity_id() == *id));
        if let Some(v) = created {
            v.read(cx).focus_handle().clone().focus(window, cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn settings_window(&self) -> Option<WindowHandle<Root>> {
        self.settings_window
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn workspace(&self) -> &Entity<Workspace> {
        &self.workspace
    }

    pub(crate) fn active_view(&self, cx: &App) -> Option<Entity<RepoView>> {
        let repo = self.workspace.read(cx).active_repo()?;
        self.views.get(&repo.entity_id()).cloned()
    }

    fn open_repository(&mut self, _: &OpenRepository, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("打开仓库".into()),
        });
        let ws = self.workspace.clone();
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = cx.update(|window, cx| {
                if let Err(e) = ws.update(cx, |ws, cx| ws.open_repo(&path, cx)) {
                    window.push_notification(
                        gpui::component::notification::Notification::error(e),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    fn scan_directory(&mut self, _: &ScanDirectory, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("扫描".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(root) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.open_scan_dialog(root, window, cx)
            });
        })
        .detach();
    }

    /// 扫描 `root` 并弹出勾选列表。
    pub(crate) fn open_scan_dialog(
        &mut self,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ws = self.workspace.clone();
        let on_add: crate::scan_dialog::OnAdd = Rc::new(move |paths, window, cx| {
            let total = paths.len();
            let errors = ws.update(cx, |ws, cx| ws.open_repos(&paths, cx));
            let ok = total - errors.len();
            if ok > 0 {
                window.push_notification(
                    gpui::component::notification::Notification::success(format!(
                        "已添加 {ok} 个仓库"
                    )),
                    cx,
                );
            }
            for (p, e) in errors {
                window.push_notification(
                    gpui::component::notification::Notification::error(format!(
                        "{}:{e}",
                        p.display()
                    )),
                    cx,
                );
            }
        });
        let view = cx.new(|cx| ScanView::new(root, self.workspace.downgrade(), on_add, window, cx));
        view.read(cx).filter_focus(cx).focus(window, cx);
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("扫描目录")
                .w(px(640.))
                .child(view.clone())
                .footer(div())
        });
    }

    fn close_tab(&mut self, _: &CloseTab, _: &mut Window, cx: &mut Context<Self>) {
        self.workspace.update(cx, |ws, cx| {
            let i = ws.active;
            ws.close_repo(i, cx)
        });
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_tab(1, window, cx);
    }

    fn prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_tab(-1, window, cx);
    }

    fn cycle_tab(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.update(cx, |ws, cx| {
            let n = ws.repos.len() as isize;
            if n > 0 {
                let i = (ws.active as isize + delta).rem_euclid(n) as usize;
                ws.activate(i, cx);
            }
        });
        if let Some(v) = self.active_view(cx) {
            v.read(cx).focus_handle().clone().focus(window, cx);
        }
    }

    fn toggle_theme(&mut self, _: &ToggleTheme, window: &mut Window, cx: &mut Context<Self>) {
        // 跟随系统时切到与当前外观相反的固定主题
        let next = if cx.theme().mode.is_dark() {
            ThemePreference::Light
        } else {
            ThemePreference::Dark
        };
        self.applied_theme = next;
        self.workspace.update(cx, |ws, cx| ws.set_theme(next, cx));
        apply_theme(next, window, cx);
        // 提交图与 diff 的颜色按主题缓存在视图里,切换后整体重绘
        window.refresh();
    }

    /// 打开设置窗口;已经打开就激活它。
    fn open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let ws = self.workspace.clone();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(860.), px(620.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("设置".into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(640.), px(420.))),
            ..Default::default()
        };
        match gpui::open_window(options, cx, move |window, cx| {
            cx.new(|cx| SettingsView::new(ws, window, cx))
        }) {
            Ok((handle, _)) => self.settings_window = handle.downcast::<Root>(),
            Err(e) => log::warn!("打开设置窗口失败:{e}"),
        }
    }

    fn open_in_terminal(
        &mut self,
        _: &OpenInTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = self.workspace.read(cx).settings.terminal.app_name();
        self.open_active_repo(vec!["-a".into(), app.into()], window, cx);
    }

    fn reveal_in_finder(
        &mut self,
        _: &RevealInFinder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_active_repo(Vec::new(), window, cx);
    }

    /// 用 `open` 打开当前仓库目录(默认是 Finder,`-a` 指定应用)。
    fn open_active_repo(&mut self, args: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .workspace
            .read(cx)
            .active_repo()
            .map(|r| r.read(cx).path.clone())
        else {
            return;
        };
        let task = cx.background_spawn(async move {
            std::process::Command::new("open")
                .args(&args)
                .arg(&path)
                .output()
        });
        cx.spawn_in(window, async move |_, cx| {
            let error = match task.await {
                Ok(out) if out.status.success() => return,
                Ok(out) => String::from_utf8_lossy(&out.stderr).trim().to_string(),
                Err(e) => e.to_string(),
            };
            let _ = cx.update(|window, cx| {
                window.push_notification(
                    gpui::component::notification::Notification::error(format!("打开失败:{error}")),
                    cx,
                )
            });
        })
        .detach();
    }

    fn command_palette(&mut self, _: &CommandPalette, window: &mut Window, cx: &mut Context<Self>) {
        // 选中动作后先把焦点还给仓库视图,再派发,保证仓库级动作能被处理
        let target = self
            .active_view(cx)
            .map(|v| v.read(cx).focus_handle().clone())
            .unwrap_or_else(|| self.focus.clone());
        let on_pick: crate::palette::OnPick = Rc::new(move |action, window, cx| {
            target.focus(window, cx);
            window.defer(cx, move |window, cx| window.dispatch_action(action, cx));
        });
        let palette = cx.new(|cx| PaletteView::new(on_pick, window, cx));
        palette.read(cx).input_focus(cx).focus(window, cx);
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title("命令面板")
                .w(px(560.))
                .margin_top(px(80.))
                .child(palette.clone())
                .footer(div())
        });
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let update_badge = self.render_update_badge(cx);
        let ws = self.workspace.read(cx);
        let theme = cx.theme().clone();
        let active = ws.active;
        let tabs = ws.repos.iter().enumerate().map(|(i, repo)| {
            let r = repo.read(cx);
            let is_active = i == active;
            let busy = r.busy.is_some();
            h_flex()
                .id(("tab", i))
                .h_full()
                .pl_3()
                .pr_1()
                .gap_1()
                .text_sm()
                .border_r_1()
                .border_color(theme.border)
                .cursor_pointer()
                .when(is_active, |d| {
                    d.bg(theme.tab_active)
                        .text_color(theme.tab_active_foreground)
                })
                .when(!is_active, |d| {
                    d.text_color(theme.muted_foreground)
                        .hover(|s| s.bg(theme.list_hover))
                })
                .child(r.name.clone())
                .when(busy, |d| {
                    d.child(gpui::component::spinner::Spinner::new().xsmall())
                })
                .child(
                    Button::new(("close-tab", i))
                        .xsmall()
                        .ghost()
                        .icon(IconName::Close)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.workspace.update(cx, |ws, cx| ws.close_repo(i, cx))
                        })),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.workspace.update(cx, |ws, cx| ws.activate(i, cx));
                    if let Some(v) = this.active_view(cx) {
                        v.read(cx).focus_handle().clone().focus(window, cx);
                    }
                }))
        });
        h_flex()
            .h(px(34.))
            .flex_none()
            .bg(theme.tab_bar)
            .border_b_1()
            .border_color(theme.border)
            .children(tabs)
            .child(
                Button::new("open-repo")
                    .xsmall()
                    .ghost()
                    .icon(IconName::Plus)
                    .tooltip("打开仓库(⌘O)")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenRepository), cx)),
            )
            .child(
                Button::new("scan-repos")
                    .xsmall()
                    .ghost()
                    .icon(IconName::Search)
                    .tooltip("扫描目录,批量添加仓库(⌘⇧O)")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ScanDirectory), cx)),
            )
            .child(div().flex_1())
            .children(update_badge)
            .children(
                ws.git_warning
                    .clone()
                    .map(|w| div().px_2().text_xs().text_color(theme.warning).child(w)),
            )
            .child(
                Button::new("open-settings")
                    .xsmall()
                    .ghost()
                    .mr_1()
                    .icon(IconName::Settings)
                    .tooltip("设置(⌘,)")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx)),
            )
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let recent = self.workspace.read(cx).recent_repos().to_vec();
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .child("Gitlane"),
            )
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child("打开一个本地 Git 仓库开始使用"),
            )
            .child(
                Button::new("welcome-open")
                    .primary()
                    .label("打开仓库…(⌘O)")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenRepository), cx)),
            )
            .child(
                Button::new("welcome-scan")
                    .label("扫描目录…(⌘⇧O)")
                    .tooltip("查找目录下的 Git 仓库,勾选后批量添加")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ScanDirectory), cx)),
            )
            .when(!recent.is_empty(), |d| {
                d.child(
                    div()
                        .pt_4()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("最近打开"),
                )
                .children(recent.into_iter().take(10).enumerate().map(|(i, p)| {
                    let path = p.clone();
                    div()
                        .id(("recent", i))
                        .text_sm()
                        .cursor_pointer()
                        .text_color(theme.link)
                        .hover(|s| s.underline())
                        .child(p.display().to_string())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Err(e) =
                                this.workspace.update(cx, |ws, cx| ws.open_repo(&path, cx))
                            {
                                window.push_notification(
                                    gpui::component::notification::Notification::error(e),
                                    cx,
                                );
                            }
                        }))
                }))
            })
    }
}

pub fn apply_theme(pref: ThemePreference, window: &mut Window, cx: &mut App) {
    match pref {
        ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
        ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
    }
    crate::app_icon::set_dock_icon(cx.theme().mode.is_dark());
}

impl Render for GitlaneWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let content: AnyElement = match self.active_view(cx) {
            Some(v) => v.into_any_element(),
            None => self.render_welcome(cx).into_any_element(),
        };
        v_flex()
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(".SystemUIFont")
            .on_action(cx.listener(Self::open_repository))
            .on_action(cx.listener(Self::scan_directory))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::prev_tab))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(Self::command_palette))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::open_in_terminal))
            .on_action(cx.listener(Self::reveal_in_finder))
            .on_action(cx.listener(Self::on_check_for_updates))
            .on_action(cx.listener(Self::on_install_update))
            .child(self.render_tabs(cx))
            .child(div().flex_1().min_h_0().child(content))
    }
}

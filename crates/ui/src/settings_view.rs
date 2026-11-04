//! 设置窗口(⌘,):通用、Git、提交图与 Diff、集成四页。
//!
//! 应用设置改的是 `Workspace::settings`,改完立即保存并同步到已打开的仓库;
//! Git 页改的是全局 `~/.gitconfig`,经 `Workspace` 在后台用系统 git 写入,本视图不直接调用 git。

use crate::actions::CloseTab;
use app::{Settings as AppSettings, TerminalApp, ThemePreference, Workspace};
use gitcore::config::{ConfigChange, GlobalConfig, PullMode};
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings};
use gpui::component::{ActiveTheme, Icon, IconName};
use gpui::prelude::*;
use gpui::*;
use std::rc::Rc;

/// Git 页里的文本项:失焦或回车时才写入,避免每敲一个字就改一次全局配置。
#[derive(Clone, Copy)]
enum TextKey {
    UserName,
    UserEmail,
    DefaultBranch,
}

impl TextKey {
    const ALL: [TextKey; 3] = [
        TextKey::UserName,
        TextKey::UserEmail,
        TextKey::DefaultBranch,
    ];

    fn get(self, c: &GlobalConfig) -> &str {
        match self {
            TextKey::UserName => &c.user_name,
            TextKey::UserEmail => &c.user_email,
            TextKey::DefaultBranch => &c.default_branch,
        }
    }

    fn change(self, value: String) -> ConfigChange {
        match self {
            TextKey::UserName => ConfigChange::UserName(value),
            TextKey::UserEmail => ConfigChange::UserEmail(value),
            TextKey::DefaultBranch => ConfigChange::DefaultBranch(value),
        }
    }
}

pub struct SettingsView {
    workspace: Entity<Workspace>,
    /// 与 `TextKey::ALL` 一一对应。
    inputs: [Entity<InputState>; 3],
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        workspace.update(cx, |ws, cx| ws.load_git_config(cx));
        let placeholders = [
            "例如:张三",
            "例如:zhangsan@example.com",
            "未设置时为 master",
        ];
        let inputs = placeholders.map(|p| cx.new(|cx| InputState::new(window, cx).placeholder(p)));

        let mut subs: Vec<Subscription> = TextKey::ALL
            .iter()
            .zip(&inputs)
            .map(|(&key, input)| {
                cx.subscribe_in(input, window, move |this, _, e: &InputEvent, _, cx| {
                    if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.commit_text(key, cx);
                    }
                })
            })
            .collect();
        // 全局配置加载完成(或写入失败后重新读取)时,把值填进没在编辑的输入框
        subs.push(cx.observe_in(&workspace, window, |this, _, window, cx| {
            this.sync_inputs(window, cx);
            cx.notify();
        }));

        let mut this = Self {
            workspace,
            inputs,
            focus: cx.focus_handle(),
            _subscriptions: subs,
        };
        this.sync_inputs(window, cx);
        this.focus.focus(window, cx);
        this
    }

    fn input(&self, key: TextKey) -> &Entity<InputState> {
        &self.inputs[key as usize]
    }

    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config) = self.workspace.read(cx).git_config.clone() else {
            return;
        };
        for key in TextKey::ALL {
            let input = self.input(key).clone();
            let want = key.get(&config);
            if !input.focus_handle(cx).is_focused(window) && input.read(cx).value() != want {
                let want = want.to_string();
                input.update(cx, |s, cx| s.set_value(want, window, cx));
            }
        }
    }

    fn commit_text(&mut self, key: TextKey, cx: &mut Context<Self>) {
        let value = self.input(key).read(cx).value().trim().to_string();
        let Some(current) = self
            .workspace
            .read(cx)
            .git_config
            .as_ref()
            .map(|c| key.get(c).to_string())
        else {
            // 还没读到现有配置,不能判断是否改动,也不能贸然覆盖
            return;
        };
        if value != current {
            self.workspace
                .update(cx, |ws, cx| ws.set_git_config(key.change(value), cx));
        }
    }

    fn on_close(&mut self, _: &CloseTab, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    fn general_page(&self) -> SettingPage {
        let ws = &self.workspace;
        SettingPage::new("通用")
            .icon(IconName::Settings)
            .resettable(true)
            .groups([
                SettingGroup::new().title("外观").item(
                    SettingItem::new(
                        "主题",
                        choice(
                            ws,
                            vec![
                                (ThemePreference::Light, "浅色"),
                                (ThemePreference::Dark, "深色"),
                                (ThemePreference::System, "跟随系统"),
                            ],
                            AppSettings::default().theme,
                            |s| s.theme,
                            |s, v| s.theme = v,
                        ),
                    )
                    .description("也可以用 ⌘⇧T 在浅色与深色之间切换。"),
                ),
                SettingGroup::new().title("启动").item(
                    SettingItem::new(
                        "恢复上次打开的仓库",
                        switch(ws, true, |s| s.restore_tabs, |s, v| s.restore_tabs = v),
                    )
                    .description("关闭后启动时显示欢迎页;命令行传入的仓库总会打开。"),
                ),
                SettingGroup::new().title("更新").item(
                    SettingItem::new(
                        "自动检查更新",
                        switch(
                            ws,
                            true,
                            |s| s.auto_update_check,
                            |s, v| s.auto_update_check = v,
                        ),
                    )
                    .description(
                        "启动时和之后每 3 小时检查一次;发现新版本后台下载并校验,重启更新由你确认。也可以在菜单里手动“检查更新…”。",
                    ),
                ),
                SettingGroup::new().title("头像").item(
                    SettingItem::new(
                        "联网获取作者头像",
                        switch(
                            ws,
                            true,
                            |s| s.fetch_avatars,
                            |s, v| s.fetch_avatars = v,
                        ),
                    )
                    .description(
                        "按提交邮箱查询:GitHub noreply 邮箱取对应账号头像,其余邮箱查 Gravatar;结果缓存在本地,查不到时显示首字母。关闭后不联网。",
                    ),
                ),
                SettingGroup::new().title("同步").item(
                    SettingItem::new(
                        "自动 fetch",
                        choice(
                            ws,
                            vec![
                                (0, "关闭"),
                                (5, "每 5 分钟"),
                                (10, "每 10 分钟"),
                                (30, "每 30 分钟"),
                                (60, "每小时"),
                            ],
                            AppSettings::default().auto_fetch_minutes,
                            |s| s.auto_fetch_minutes,
                            |s, v| s.auto_fetch_minutes = v,
                        ),
                    )
                    .description(
                        "后台对所有打开的仓库执行 fetch --all --prune,仓库忙时跳过;失败不弹提示,只写日志。",
                    ),
                ),
            ])
    }

    fn git_page(&self, cx: &App) -> SettingPage {
        let ws = &self.workspace;
        let text_item = |title: &'static str, key: TextKey| {
            let input = self.input(key).clone();
            SettingItem::new(
                title,
                SettingField::render(move |_, _, _| Input::new(&input).w(px(260.))),
            )
        };
        let loaded = ws.read(cx).git_config.is_some();
        SettingPage::new("Git")
            .icon(Icon::from(gpui::assets::IconName::GitBranch))
            .description("修改的是全局配置 ~/.gitconfig;仓库自己的 .git/config 里设置的值优先。")
            .resettable(false)
            .groups([
                SettingGroup::new()
                    .title("身份")
                    .description("提交的作者信息。输入框失去焦点或按回车时保存,清空表示删除该项。")
                    .items([
                        text_item("用户名(user.name)", TextKey::UserName).disabled(!loaded),
                        text_item("邮箱(user.email)", TextKey::UserEmail).disabled(!loaded),
                    ])
                    .footer({
                        let ws = ws.clone();
                        move |_, cx| {
                            let error = ws.read(cx).git_config_error.clone();
                            div()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .children(error)
                        }
                    }),
                SettingGroup::new().title("拉取与提交").items([
                    SettingItem::new(
                        "Pull 方式",
                        git_choice(
                            ws,
                            vec![
                                (PullMode::Merge, "合并(merge)"),
                                (PullMode::Rebase, "变基(rebase)"),
                                (PullMode::FastForwardOnly, "仅快进(ff-only)"),
                            ],
                            |c| c.pull,
                            ConfigChange::Pull,
                        ),
                    )
                    .description("对应 pull.rebase / pull.ff,工具栏的 Pull 按这里的方式合并。")
                    .disabled(!loaded),
                    SettingItem::new(
                        "签名提交(commit.gpgsign)",
                        SettingField::switch(
                            {
                                let ws = ws.clone();
                                move |cx| {
                                    ws.read(cx).git_config.as_ref().is_some_and(|c| c.gpg_sign)
                                }
                            },
                            {
                                let ws = ws.clone();
                                move |on, cx| {
                                    ws.update(cx, |ws, cx| {
                                        ws.set_git_config(ConfigChange::GpgSign(on), cx)
                                    })
                                }
                            },
                        ),
                    )
                    .description("需要事先配置好 GPG 或 SSH 签名密钥(user.signingkey)。")
                    .disabled(!loaded),
                    text_item("新仓库默认分支(init.defaultBranch)", TextKey::DefaultBranch)
                        .disabled(!loaded),
                ]),
            ])
    }

    fn graph_page(&self) -> SettingPage {
        let ws = &self.workspace;
        SettingPage::new("提交图与 Diff")
            .icon(Icon::from(gpui::assets::IconName::GitGraph))
            .resettable(true)
            .groups([
                SettingGroup::new().title("提交图").item(
                    SettingItem::new(
                        "最多加载的提交数",
                        choice(
                            ws,
                            vec![
                                (10_000, "1 万"),
                                (50_000, "5 万"),
                                (100_000, "10 万"),
                                (200_000, "20 万"),
                                (500_000, "50 万"),
                            ],
                            AppSettings::default().history_limit,
                            |s| s.history_limit,
                            |s, v| s.history_limit = v,
                        ),
                    )
                    .description(
                        "超出部分不显示。调大会增加加载时间与内存,修改后已打开的仓库会重新加载。",
                    ),
                ),
                SettingGroup::new().title("Diff").items([
                    SettingItem::new(
                        "默认并排显示",
                        switch(ws, false, |s| s.split_diff, |s, v| s.split_diff = v),
                    )
                    .description("也可以在 diff 视图里用 ⌘\\ 切换。"),
                    SettingItem::new(
                        "上下文行数",
                        choice(
                            ws,
                            vec![
                                (1, "1 行"),
                                (3, "3 行"),
                                (5, "5 行"),
                                (10, "10 行"),
                                (25, "25 行"),
                            ],
                            AppSettings::default().diff_context_lines,
                            |s| s.diff_context_lines,
                            |s, v| s.diff_context_lines = v,
                        ),
                    )
                    .description("改动前后显示的不变行数;行数越多,相邻改动越容易并成一个块。"),
                ]),
            ])
    }

    fn integration_page(&self) -> SettingPage {
        let ws = &self.workspace;
        SettingPage::new("集成")
            .icon(IconName::SquareTerminal)
            .resettable(true)
            .group(
                SettingGroup::new().title("外部工具").item(
                    SettingItem::new(
                        "终端",
                        choice(
                            ws,
                            TerminalApp::ALL.map(|t| (t, t.app_name())).to_vec(),
                            TerminalApp::default(),
                            |s| s.terminal,
                            |s, v| s.terminal = v,
                        ),
                    )
                    .description("“在终端中打开”(⌥⌘T)使用的应用;“在 Finder 中显示”是 ⌥⌘R。"),
                ),
            )
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let pages = [
            self.general_page(),
            self.git_page(cx),
            self.graph_page(),
            self.integration_page(),
        ];
        div()
            .id("settings-root")
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(".SystemUIFont")
            .on_action(cx.listener(Self::on_close))
            .child(
                Settings::new("gitlane-settings")
                    .sidebar_width(px(200.))
                    .pages(pages),
            )
            .test_support()
    }
}

/// 应用设置的开关项。
fn switch(
    ws: &Entity<Workspace>,
    default: bool,
    get: fn(&AppSettings) -> bool,
    set: fn(&mut AppSettings, bool),
) -> SettingField<bool> {
    let w = ws.clone();
    let ws = ws.clone();
    SettingField::switch(
        move |cx| get(&w.read(cx).settings),
        move |v, cx| ws.update(cx, |ws, cx| ws.update_settings(cx, |s| set(s, v))),
    )
    .default_value(default)
}

/// 应用设置的下拉项。下拉框的值是选项下标,当前值不在列表里时(手改过设置文件)按原值显示。
fn choice<T: Copy + PartialEq + std::fmt::Debug + 'static>(
    ws: &Entity<Workspace>,
    options: Vec<(T, &'static str)>,
    default: T,
    get: fn(&AppSettings) -> T,
    set: fn(&mut AppSettings, T),
) -> SettingField<SharedString> {
    let options = Rc::new(options);
    let index_of = {
        let options = options.clone();
        move |v: T| -> SharedString {
            options
                .iter()
                .position(|(o, _)| *o == v)
                .map(|i| i.to_string().into())
                .unwrap_or_else(|| format!("{v:?}").into())
        }
    };
    let items = options
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            (
                SharedString::from(i.to_string()),
                SharedString::from(*label),
            )
        })
        .collect();
    let default_key = index_of(default);
    let (w, read) = (ws.clone(), index_of);
    let ws = ws.clone();
    SettingField::dropdown(
        items,
        move |cx| read(get(&w.read(cx).settings)),
        move |key, cx| {
            if let Some(&(v, _)) = key.parse::<usize>().ok().and_then(|i| options.get(i)) {
                ws.update(cx, |ws, cx| ws.update_settings(cx, |s| set(s, v)));
            }
        },
    )
    .default_value(default_key)
}

/// 全局 git 配置的下拉项。
fn git_choice<T: Copy + PartialEq + 'static>(
    ws: &Entity<Workspace>,
    options: Vec<(T, &'static str)>,
    get: fn(&GlobalConfig) -> T,
    change: fn(T) -> ConfigChange,
) -> SettingField<SharedString> {
    let options = Rc::new(options);
    let items = options
        .iter()
        .enumerate()
        .map(|(i, (_, label))| {
            (
                SharedString::from(i.to_string()),
                SharedString::from(*label),
            )
        })
        .collect();
    let (w, opts) = (ws.clone(), options.clone());
    let ws = ws.clone();
    SettingField::dropdown(
        items,
        move |cx| {
            w.read(cx)
                .git_config
                .as_ref()
                .and_then(|c| opts.iter().position(|(o, _)| *o == get(c)))
                .map(|i| SharedString::from(i.to_string()))
                .unwrap_or_default()
        },
        move |key, cx| {
            if let Some(&(v, _)) = key.parse::<usize>().ok().and_then(|i| options.get(i)) {
                ws.update(cx, |ws, cx| ws.set_git_config(change(v), cx));
            }
        },
    )
}

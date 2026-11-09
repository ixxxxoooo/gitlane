//! Gitlane:Rust + GPUI 的 macOS Git 客户端。
//!
//! 用法:`gitlane [仓库路径…]`。不带参数时恢复上次打开的仓库。

mod actions;
mod app_icon;
mod assets;
mod colors;
mod commit_ai;
mod copy_menu;
mod dialogs;
mod diff_stage;
mod diff_view;
mod file_icons;
mod format;
mod graph_canvas;
mod graph_view;
mod palette;
mod panels;
mod repo_view;
mod root;
mod scan_dialog;
mod settings_ai;
mod settings_view;
mod side_panel;
mod side_section;
mod sidebar;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ai;
#[cfg(test)]
mod tests_layout;
#[cfg(test)]
mod tests_panels;
#[cfg(test)]
mod tests_selection;
#[cfg(test)]
mod tests_sidebar;
#[cfg(test)]
mod tests_welcome;
mod themes;
mod updater;
mod window_state;

use actions::*;
use gpui::*;
use root::GitlaneWindow;
use std::path::PathBuf;

fn menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Gitlane".into(),
            items: vec![
                MenuItem::action("检查更新…", CheckForUpdates),
                MenuItem::action("设置…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("命令面板", CommandPalette),
                MenuItem::action("切换主题", ToggleTheme),
                MenuItem::separator(),
                MenuItem::action("退出 Gitlane", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "文件".into(),
            items: vec![
                MenuItem::action("打开仓库…", OpenRepository),
                MenuItem::action("扫描目录…", ScanDirectory),
                MenuItem::action("关闭标签", CloseTab),
                MenuItem::action("关闭窗口", CloseWindow),
                MenuItem::separator(),
                MenuItem::action("在终端中打开", OpenInTerminal),
                MenuItem::action("在 Finder 中显示", RevealInFinder),
                MenuItem::separator(),
                MenuItem::action("刷新", Refresh),
            ],
            disabled: false,
        },
        Menu {
            name: "仓库".into(),
            items: vec![
                MenuItem::action("Fetch", Fetch),
                MenuItem::action("Pull", Pull),
                MenuItem::action("Push", Push),
                MenuItem::action("Force push…", ForcePush),
                MenuItem::separator(),
                MenuItem::action("新建分支…", NewBranch),
                MenuItem::action("Stash", Stash),
                MenuItem::action("Stash pop", StashPop),
                MenuItem::separator(),
                MenuItem::action("暂存全部", StageAll),
                MenuItem::action("取消暂存全部", UnstageAll),
                MenuItem::action("提交", Commit),
                MenuItem::separator(),
                MenuItem::action("中止进行中的操作", AbortOperation),
            ],
            disabled: false,
        },
        Menu {
            name: "编辑".into(),
            items: vec![
                MenuItem::action("撤销操作", Undo),
                MenuItem::action("重做操作", Redo),
                MenuItem::separator(),
                MenuItem::action("搜索提交", FocusSearch),
            ],
            disabled: false,
        },
        Menu {
            name: "显示".into(),
            items: vec![
                MenuItem::action("切换统一 / 并排 diff", ToggleSplitDiff),
                MenuItem::action("下一个标签", NextTab),
                MenuItem::action("上一个标签", PrevTab),
            ],
            disabled: false,
        },
    ]
}

/// 当前的主窗口。⌘W 关掉后应用仍在运行,点 Dock 图标时据此判断要不要重新打开。
struct MainWindow(Option<AnyWindowHandle>);

impl Global for MainWindow {}

/// 打开主窗口:恢复上次的位置大小与打开的仓库。`initial` 是命令行传入的仓库。
fn open_main_window(
    initial: Vec<PathBuf>,
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<GitlaneWindow>)> {
    // 恢复上次的窗口位置、大小与显示方式;保存的位置不在任何屏幕上时回到主屏居中
    let bounds = window_state::initial_bounds(
        app::Settings::load().window,
        (1440., 900.),
        window_state::MAIN_MIN,
        cx,
    );
    let options = WindowOptions {
        window_bounds: Some(bounds),
        // 标签栏画在顶栏里,系统标题栏透明、红绿灯保留;拖动与双击由 TitleBar 自己处理
        titlebar: Some(gpui::component::TitleBar::title_bar_options()),
        app_owns_titlebar_drag: true,
        window_min_size: Some(size(
            px(window_state::MAIN_MIN.0),
            px(window_state::MAIN_MIN.1),
        )),
        ..Default::default()
    };
    match gpui::open_window(options, cx, move |window, cx| {
        let workspace = cx.new(app::Workspace::load);
        cx.new(|cx| GitlaneWindow::new(workspace, initial, window, cx))
    }) {
        Ok((handle, root)) => {
            cx.set_global(MainWindow(Some(handle)));
            Some((handle, root))
        }
        Err(e) => {
            log::error!("无法打开窗口:{e}");
            None
        }
    }
}

/// 点 Dock 图标:主窗口还在就激活它,已经关掉就重新打开。
fn reopen_main_window(cx: &mut App) {
    let current = cx.try_global::<MainWindow>().and_then(|m| m.0);
    if let Some(handle) = current
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    open_main_window(Vec::new(), cx);
    cx.activate(true);
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let initial: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();

    let application = gpui::application().with_assets(assets::AppAssets);
    application.on_reopen(reopen_main_window);
    application.run(move |cx: &mut App| {
        gpui::init(cx);
        diffview::init(cx);
        actions::bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(menus());

        let (handle, root) = open_main_window(initial.clone(), cx).expect("无法打开窗口");
        // 更新检查会联网,只在正式启动时触发,测试里的窗口不触发
        let _ = handle.update(cx, |_, window, cx| {
            root.update(cx, |this, cx| this.startup_update(window, cx))
        });
        cx.activate(true);
    });
}

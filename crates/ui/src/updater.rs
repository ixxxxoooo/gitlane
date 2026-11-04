//! 应用更新的界面侧:检查 → 后台下载校验 → 用户确认后重启替换。
//! 网络与安装逻辑在 `app::update`;UI 线程只改状态、弹通知,下载在后台线程里跑。

use crate::actions::InstallUpdate;
use crate::dialogs;
use crate::root::GitlaneWindow;
use app::update::{self, Progress, StagedUpdate, UpdateInfo, UpdateResult};
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::notification::Notification;
use gpui::component::{ActiveTheme, Sizable, WindowExt};
use gpui::prelude::*;
use gpui::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 自动检查的间隔;启动时的那一次另算。
const CHECK_INTERVAL: Duration = Duration::from_secs(3 * 60 * 60);

/// 更新流程的状态。
#[derive(Clone, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    /// 下载中:版本号 + 当前进度(下载 / 校验 / 解包)。
    Downloading(String, Progress),
    /// 已下载并校验,等用户确认重启。
    Ready(StagedUpdate),
}

/// 当前运行的 `.app` 包路径;不是从应用包启动(`cargo run`、开发实例)时为 `None`,此时不能自动替换。
fn current_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(PathBuf::from)
}

impl GitlaneWindow {
    /// 启动后调用一次(测试不调用,避免联网):提示上次安装失败、恢复已暂存的更新、
    /// 按设置自动检查,并开启周期检查。开发构建(构建号为 0)不自动检查。
    pub fn startup_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = update::staging_root();
        if let Some(why) = update::take_install_failure(&root) {
            let msg = format!(
                "上次更新安装失败({why});常见原因是应用所在目录不可写,可到发布页手动下载安装"
            );
            window.push_notification(Notification::warning(msg), cx);
        } else if let Some(staged) =
            update::load_staged(&root, env!("CARGO_PKG_VERSION"), update::current_build())
        {
            self.update = UpdateState::Ready(staged.clone());
            self.announce_ready(&staged, window, cx);
        } else if self.auto_check_enabled(cx) {
            self.check_update(false, window, cx);
        }
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(CHECK_INTERVAL).await;
                let alive = this.update_in(cx, |this, window, cx| {
                    if this.auto_check_enabled(cx) && matches!(this.update, UpdateState::Idle) {
                        this.check_update(false, window, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn auto_check_enabled(&self, cx: &App) -> bool {
        update::current_build() > 0 && self.workspace().read(cx).settings.auto_update_check
    }

    /// 菜单 / 命令面板的“检查更新”。
    pub(crate) fn on_check_for_updates(
        &mut self,
        _: &crate::actions::CheckForUpdates,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.update.clone() {
            UpdateState::Ready(s) => self.announce_ready(&s, window, cx),
            UpdateState::Idle => self.check_update(true, window, cx),
            _ => window.push_notification(Notification::info("正在检查或下载更新…"), cx),
        }
    }

    /// 后台检查更新。`manual` 为真时“已是最新”和失败也会提示,自动检查只记日志。
    fn check_update(&mut self, manual: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.update = UpdateState::Checking;
        cx.notify();
        let task = cx.background_spawn(async {
            update::check(
                update::DEFAULT_FEED_URL,
                env!("CARGO_PKG_VERSION"),
                update::current_build(),
            )
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.update = UpdateState::Idle;
                match result {
                    UpdateResult::Available(info) => this.download_update(info, window, cx),
                    UpdateResult::UpToDate if manual => {
                        window.push_notification(Notification::info("已是最新版本"), cx)
                    }
                    UpdateResult::Failed(e) if manual => window
                        .push_notification(Notification::error(format!("检查更新失败:{e}")), cx),
                    UpdateResult::Failed(e) => log::warn!("检查更新失败:{e}"),
                    UpdateResult::UpToDate => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 后台下载并校验;进度写进共享变量,界面每 200 ms 取一次,避免每个数据块都重绘。
    fn download_update(&mut self, info: UpdateInfo, window: &mut Window, cx: &mut Context<Self>) {
        let version = info.version.clone();
        self.update = UpdateState::Downloading(version.clone(), Progress::default());
        window.push_notification(
            Notification::info(format!("发现新版本 v{version},正在后台下载…")),
            cx,
        );
        let progress = Arc::new(Mutex::new(Progress::default()));
        let done = Arc::new(AtomicBool::new(false));
        let (p, d) = (progress.clone(), done.clone());
        let task = cx.background_spawn(async move {
            let result = update::download(&update::staging_root(), &info, &mut |now| {
                if let Ok(mut g) = p.lock() {
                    *g = now;
                }
            });
            d.store(true, Ordering::Release);
            result
        });
        cx.spawn_in(window, async move |this, cx| {
            while !done.load(Ordering::Acquire) {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let now = progress.lock().map(|g| *g).unwrap_or_default();
                let alive = this.update_in(cx, |this, _, cx| {
                    if let UpdateState::Downloading(_, cur) = &mut this.update {
                        *cur = now;
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(staged) => {
                        this.announce_ready(&staged, window, cx);
                        this.update = UpdateState::Ready(staged);
                    }
                    Err(e) => {
                        this.update = UpdateState::Idle;
                        window.push_notification(
                            Notification::error(format!("更新下载失败:{e}")),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 弹出“已下载,重启即可更新”的通知,带一个重启按钮。
    fn announce_ready(&self, staged: &StagedUpdate, window: &mut Window, cx: &mut Context<Self>) {
        let note = Notification::success(format!("v{} 已下载并校验,重启即可更新", staged.version))
            .action(|_, _, cx| {
                Button::new("install-update")
                    .primary()
                    .label("重启更新")
                    .on_click(cx.listener(|note, _, window, cx| {
                        note.dismiss(window, cx);
                        window.dispatch_action(Box::new(InstallUpdate), cx);
                    }))
            });
        window.push_notification(note, cx);
    }

    /// 确认后退出,由脚本替换应用并重启。不是从应用包运行时只提示。
    pub(crate) fn on_install_update(
        &mut self,
        _: &InstallUpdate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let UpdateState::Ready(staged) = self.update.clone() else {
            return;
        };
        let Some(bundle) = current_bundle() else {
            window.push_notification(
                Notification::warning("当前不是从应用包(.app)运行,无法自动替换;请手动安装新版本"),
                cx,
            );
            return;
        };
        let lines = vec![
            format!("将更新到 v{}(构建 {})。", staged.version, staged.build),
            "应用会退出并自动重启;已打开的仓库标签会恢复。".to_string(),
        ];
        dialogs::confirm(
            window,
            cx,
            "重启并更新",
            lines,
            "重启更新",
            move |window, cx| match update::launch_replacer(
                &update::staging_root(),
                &staged,
                &bundle,
            ) {
                Ok(()) => cx.quit(),
                Err(e) => window.push_notification(Notification::error(e), cx),
            },
        );
    }

    /// 标签栏右侧的更新状态:下载中显示进度,下载完成显示重启按钮。
    pub(crate) fn render_update_badge(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let muted = cx.theme().muted_foreground;
        match &self.update {
            UpdateState::Downloading(v, p) => Some(
                div()
                    .px_2()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("更新 v{v} · {}", p.short()))
                    .into_any_element(),
            ),
            UpdateState::Ready(s) => Some(
                Button::new("update-ready")
                    .xsmall()
                    .primary()
                    .label(format!("重启更新 v{}", s.version))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(InstallUpdate), cx))
                    .into_any_element(),
            ),
            _ => None,
        }
    }
}

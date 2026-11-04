//! 窗口位置与大小的保存和恢复。主窗口与设置窗口共用:启动时按上次的位置打开,
//! 窗口移动 / 缩放后防抖写进设置。

use app::{WindowMode, WindowState, Workspace};
use gpui::{
    App, Bounds, Context, Entity, Subscription, Task, Window, WindowBounds, point, px, size,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

pub const MAIN_MIN: (f32, f32) = (960., 600.);
pub const SETTINGS_MIN: (f32, f32) = (640., 420.);
/// 窗口停止移动 / 缩放多久后才写盘,拖动过程中不会反复写。
const SAVE_DELAY: Duration = Duration::from_millis(400);

/// 保存的是哪个窗口。
#[derive(Clone, Copy)]
pub enum WindowKind {
    Main,
    Settings,
}

/// 读取窗口当前的位置、大小与显示方式(最大化 / 全屏时记录还原后的位置与大小)。
pub fn capture(window: &Window) -> WindowState {
    let (b, mode) = match window.window_bounds() {
        WindowBounds::Windowed(b) => (b, WindowMode::Windowed),
        WindowBounds::Maximized(b) => (b, WindowMode::Maximized),
        WindowBounds::Fullscreen(b) => (b, WindowMode::Fullscreen),
    };
    WindowState {
        x: f32::from(b.origin.x),
        y: f32::from(b.origin.y),
        width: f32::from(b.size.width),
        height: f32::from(b.size.height),
        mode,
    }
}

/// 打开窗口时用的位置与大小:上次保存的仍然落在某块屏幕上就恢复它,否则在主屏居中、用默认大小。
///
/// 参数:`saved` 上次保存的窗口;`default` 默认 `(宽, 高)`;`min` 最小 `(宽, 高)`。
pub fn initial_bounds(
    saved: Option<WindowState>,
    default: (f32, f32),
    min: (f32, f32),
    cx: &mut App,
) -> WindowBounds {
    let displays: Vec<_> = cx
        .displays()
        .iter()
        .map(|d| {
            let b = d.bounds();
            (
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            )
        })
        .collect();
    match saved.and_then(|s| s.fit(&displays, min)) {
        Some(s) => {
            let b = Bounds::new(point(px(s.x), px(s.y)), size(px(s.width), px(s.height)));
            match s.mode {
                WindowMode::Windowed => WindowBounds::Windowed(b),
                WindowMode::Maximized => WindowBounds::Maximized(b),
                WindowMode::Fullscreen => WindowBounds::Fullscreen(b),
            }
        }
        None => WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(default.0), px(default.1)),
            cx,
        )),
    }
}

/// 监听窗口的移动与缩放,停下来后把新的位置大小写进设置。返回的订阅要存在视图里,视图销毁时自动取消。
pub fn track<T: 'static>(
    window: &mut Window,
    cx: &mut Context<T>,
    workspace: Entity<Workspace>,
    kind: WindowKind,
) -> Subscription {
    // 新的变化到来时替换旧任务,旧任务随之取消,等于防抖
    let pending: Rc<RefCell<Option<Task<()>>>> = Rc::default();
    cx.observe_window_bounds(window, move |_, window, cx| {
        let state = capture(window);
        let workspace = workspace.clone();
        let task = cx.spawn(async move |_, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            workspace.update(cx, |ws, cx| {
                ws.update_settings(cx, |s| match kind {
                    WindowKind::Main => s.window = Some(state),
                    WindowKind::Settings => s.settings_window = Some(state),
                })
            });
        });
        *pending.borrow_mut() = Some(task);
    })
}

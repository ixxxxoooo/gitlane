//! 欢迎页(没有打开任何仓库)上的动作能被响应:回归测试,曾因焦点不在窗口里,按钮点了没反应。

use crate::root::GitlaneWindow;
use app::{Settings, Workspace};
use gpui::base::Root;
use gpui::{AppContext as _, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size};

#[gpui::test]
async fn actions_work_on_the_welcome_page(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui::init(cx);
        crate::actions::bind_keys(cx);
    });
    let (window, root) = cx.update(|cx| {
        let (window, root) = gpui::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1000.), px(700.)),
                })),
                ..Default::default()
            },
            cx,
            |window, cx| {
                let ws = cx.new(|cx| Workspace::new(Settings::default(), true, cx));
                cx.new(|cx| GitlaneWindow::new(ws, Vec::new(), window, cx))
            },
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Root"), root)
    });
    // 欢迎页上的按钮就是用 dispatch_action 发动作的;用“设置…”代替会弹系统文件选择框的两个动作
    cx.update_window(window.into(), |_, window, cx| {
        window.dispatch_action(Box::new(crate::actions::OpenSettings), cx)
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(
            root.read(cx).settings_window().is_some(),
            "欢迎页上动作没有被处理"
        )
    });
}

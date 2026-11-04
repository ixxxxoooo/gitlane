//! 布局状态的保存与恢复:折叠、撑满、当前标签。窗口位置的校验在 `app::settings` 里测。

use crate::root::GitlaneWindow;
use crate::tests::{TIMEOUT, fixture};
use app::{Settings, Workspace};
use gpui::base::Root;
use gpui::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui::{
    AppContext as _, Bounds, Entity, Pixels, Point, TestAppContext, WindowBounds, WindowHandle,
    WindowOptions, px, size,
};

fn open_with(
    cx: &mut TestAppContext,
    settings: Settings,
    path: &std::path::Path,
) -> (WindowHandle<Root>, Entity<GitlaneWindow>) {
    cx.update(|cx| {
        gpui::init(cx);
        crate::actions::bind_keys(cx);
    });
    let path = path.to_path_buf();
    cx.update(|cx| {
        let (window, root) = gpui::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1400.), px(900.)),
                })),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                let ws = cx.new(|cx| Workspace::new(settings, true, cx));
                cx.new(|cx| GitlaneWindow::new(ws, vec![path], window, cx))
            },
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Root"), root)
    })
}

fn height(handle: WindowHandle<Root>, id: &'static str, cx: &mut TestAppContext) -> Pixels {
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.find(id).bounds().size.height
    })
    .unwrap()
}

#[gpui::test]
async fn saved_collapse_state_is_restored_on_open(cx: &mut TestAppContext) {
    let dir = fixture();
    let settings = Settings {
        sidebar_collapsed: vec!["远程".into()],
        side_collapsed: vec!["staged".into()],
        ..Settings::default()
    };
    let (handle, root) = open_with(cx, settings, dir.path());
    let r = cx.update(|cx| root.read(cx).workspace().read(cx).repos[0].clone());
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.data.is_some() && m.status.is_some()
    })
    .await;
    // 左侧“远程”(第 2 个面板)与右栏“已暂存”都是只剩标题行
    assert!(height(handle, "sidebar-panel-1", cx) < px(40.));
    assert!(height(handle, "side-section-staged", cx) < px(40.));
    // 其余仍然展开
    assert!(height(handle, "sidebar-panel-0", cx) > px(60.));
    assert!(height(handle, "side-section-unstaged", cx) > px(60.));
}

#[gpui::test]
async fn toggling_a_panel_writes_the_layout_back_to_settings(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open_with(cx, Settings::default(), dir.path());
    let r = cx.update(|cx| root.read(cx).workspace().read(cx).repos[0].clone());
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.data.is_some() && m.status.is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("远程", cx);
        window.click("side-toggle-unstaged", cx);
    })
    .unwrap();
    let ws = cx.update(|cx| root.read(cx).workspace().clone());
    cx.update(|cx| {
        let s = &ws.read(cx).settings;
        assert_eq!(s.sidebar_collapsed, vec!["远程".to_string()]);
        assert_eq!(s.side_collapsed, vec!["unstaged".to_string()]);
    });
}

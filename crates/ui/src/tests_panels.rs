//! 左右栏宽度:拖动分隔条改变宽度,窗口缩放时保持不变。

use crate::tests::{TIMEOUT, fixture, open, repo};
use gpui::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui::{AppContext as _, Pixels, TestAppContext, point, px, size};

fn widths(
    handle: gpui::WindowHandle<gpui::base::Root>,
    cx: &mut TestAppContext,
) -> (Pixels, Pixels) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        (
            window.find("sidebar").bounds().size.width,
            window.find("side-panel").bounds().size.width,
        )
    })
    .unwrap()
}

#[gpui::test]
async fn panels_resize_by_drag_but_not_by_window(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let r = repo(&root, cx);
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).data.is_some()
    })
    .await;
    assert_eq!(widths(handle, cx), (px(240.), px(360.)));

    // 窗口从 1400 拉到 1900:左右栏不变
    cx.simulate_window_resize(handle.into(), size(px(1900.), px(900.)));
    assert_eq!(widths(handle, cx), (px(240.), px(360.)));

    // 拖左侧分隔条到 x = 400
    cx.update_window(handle.into(), |_, window, cx| {
        let h = window.find("resize-left").bounds();
        let from = h.center();
        window.drag(from, point(px(400.), from.y), cx);
    })
    .unwrap();
    let (left, right) = widths(handle, cx);
    assert!((left - px(400.)).abs() <= px(3.), "left = {left:?}");
    assert_eq!(right, px(360.));

    // 拖右侧分隔条,让右栏变成 500 宽
    cx.update_window(handle.into(), |_, window, cx| {
        let h = window.find("resize-right").bounds();
        let from = h.center();
        window.drag(from, point(px(1900. - 500.), from.y), cx);
    })
    .unwrap();
    let (_, right) = widths(handle, cx);
    assert!((right - px(500.)).abs() <= px(3.), "right = {right:?}");

    // 再缩窗口:设定宽度保持,只有中间变窄
    cx.simulate_window_resize(handle.into(), size(px(1500.), px(900.)));
    let (l2, r2) = widths(handle, cx);
    assert!((l2 - left).abs() < px(0.5) && (r2 - right).abs() < px(0.5));

    // 松手后写入设置(测试里的工作区不落盘,但内存里的设定值要更新)
    let ws = cx.update(|cx| root.read(cx).workspace().clone());
    cx.update(|cx| {
        let s = &ws.read(cx).settings;
        assert!((s.sidebar_width - 400.).abs() <= 3.);
        assert!((s.side_panel_width - 500.).abs() <= 3.);
    });
}

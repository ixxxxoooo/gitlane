//! Gitlane 改动的布局测试:不换行时各栏跟可视区等宽,长行不会把并排的新版本一栏挤出屏幕。
//!
//! @author ixxxxoooo

use crate::document::fixture;
use crate::{Diff, DiffMode, DiffState};
use gpui::test::TestWindowExt as _;
use gpui::{
    AppContext as _, Bounds, Context, Entity, IntoElement, ParentElement as _, Pixels, Render,
    Styled as _, TestAppContext, Window, WindowBounds, WindowOptions, div, point, px, size,
};

struct View {
    state: Entity<DiffState>,
}

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(Diff::new(&self.state).size_full())
    }
}

/// 打开 800×400 的窗口显示一个含超长行的 diff,返回各行代码区("source")在窗口里的位置。
fn source_bounds(cx: &mut TestAppContext, mode: DiffMode) -> Vec<Bounds<Pixels>> {
    cx.update(|cx| {
        gpui::init(cx);
        crate::init(cx);
    });
    let long = "x".repeat(400);
    let old = format!("a\n{long}\nb\n");
    let new = format!("a\n{long}\nc\n");
    let file = fixture::modified("a.rs", &old, &new);
    let (window, _) = cx.update(|cx| {
        gpui::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(px(800.), px(400.)),
                })),
                ..Default::default()
            },
            cx,
            move |_, cx| {
                let state = cx.new(|cx| DiffState::new([file], cx).with_mode(mode));
                cx.new(|_| View { state })
            },
        )
        .expect("打开测试窗口")
    });
    // 第一帧量出可视区宽度,之后的帧才按可视区布局
    for _ in 0..3 {
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }
    cx.update_window(window, |_, window, _| {
        window
            .find_all("source")
            .into_iter()
            .filter(|s| s.visible())
            .map(|s| s.bounds())
            .collect()
    })
    .unwrap()
}

#[gpui::test]
fn split_columns_fit_the_viewport_with_long_lines(cx: &mut TestAppContext) {
    let bounds = source_bounds(cx, DiffMode::Split);
    assert!(!bounds.is_empty());
    // 右半边有新版本的代码区,且所有代码区都没有伸出窗口
    assert!(
        bounds
            .iter()
            .any(|b| b.left() >= px(400.) && b.left() < px(800.)),
        "新版本一栏应在可视区右半边:{bounds:?}"
    );
    assert!(
        bounds.iter().all(|b| b.right() <= px(800.)),
        "代码区应在栏内裁剪:{bounds:?}"
    );
}

#[gpui::test]
fn unified_code_area_is_clipped_to_the_viewport(cx: &mut TestAppContext) {
    let bounds = source_bounds(cx, DiffMode::Unified);
    assert!(!bounds.is_empty());
    assert!(
        bounds.iter().all(|b| b.right() <= px(800.)),
        "代码区应在可视区内裁剪,行号栏才不会被长行带着滚走:{bounds:?}"
    );
}

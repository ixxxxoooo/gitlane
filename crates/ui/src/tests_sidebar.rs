//! 侧栏四个面板:展开的面板平分剩余高度,收起的只剩标题行。

use crate::tests::{TIMEOUT, fixture, open, repo};
use gpui::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui::{AppContext as _, Pixels, TestAppContext, px};

fn heights(handle: gpui::WindowHandle<gpui::base::Root>, cx: &mut TestAppContext) -> Vec<Pixels> {
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        (0..4)
            .map(|i| {
                window
                    .find(format!("sidebar-panel-{i}"))
                    .bounds()
                    .size
                    .height
            })
            .collect()
    })
    .unwrap()
}

fn click(handle: gpui::WindowHandle<gpui::base::Root>, id: &'static str, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn close(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() < 1.5
}

#[gpui::test]
async fn open_panels_split_the_height_equally(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let r = repo(&root, cx);
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).data.is_some()
    })
    .await;

    // 默认四个都展开:四等分
    let h = heights(handle, cx);
    assert!(h.iter().all(|x| close(*x, h[0])), "四等分:{h:?}");

    // 收起“远程”:它只剩标题行,其余三个等分
    click(handle, "远程", cx);
    let h = heights(handle, cx);
    assert!(h[1] < px(40.), "收起的面板只剩标题行:{h:?}");
    assert!(close(h[0], h[2]) && close(h[2], h[3]), "三等分:{h:?}");

    // 只留“本地分支”:另外三个都是标题行,它占满剩余高度
    click(handle, "标签", cx);
    click(handle, "Stash", cx);
    let h = heights(handle, cx);
    assert!(h[1] < px(40.) && h[2] < px(40.) && h[3] < px(40.), "{h:?}");
    assert!(h[0] > px(300.), "本地分支占满剩余高度:{h:?}");
}

fn side_heights(
    handle: gpui::WindowHandle<gpui::base::Root>,
    keys: &[&str],
    cx: &mut TestAppContext,
) -> Vec<Pixels> {
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        keys.iter()
            .map(|k| {
                window
                    .find(format!("side-section-{k}"))
                    .bounds()
                    .size
                    .height
            })
            .collect()
    })
    .unwrap()
}

#[gpui::test]
async fn right_panels_split_the_height_equally(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let r = repo(&root, cx);
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.data.is_some() && m.status.is_some()
    })
    .await;

    // 默认选中 Working changes:未暂存与已暂存两个面板等分
    let h = side_heights(handle, &["unstaged", "staged"], cx);
    assert!(close(h[0], h[1]), "两等分:{h:?}");

    // 收起“未暂存”:它只剩标题行,已暂存占满剩余高度
    click(handle, "side-toggle-unstaged", cx);
    let h = side_heights(handle, &["unstaged", "staged"], cx);
    assert!(h[0] < px(40.) && h[1] > px(200.), "{h:?}");

    // 再展开:恢复等分
    click(handle, "side-toggle-unstaged", cx);
    let h = side_heights(handle, &["unstaged", "staged"], cx);
    assert!(close(h[0], h[1]), "恢复两等分:{h:?}");
}

#[gpui::test]
async fn staging_area_only_exists_on_working_changes(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.data.is_some() && m.status.is_some()
    })
    .await;
    let has = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .unwrap()
    };

    // 默认在 Working changes:有暂存区和提交框,没有提交详情
    assert!(has(cx, "side-section-unstaged") && has(cx, "commit"));
    assert!(!has(cx, "side-section-details"));

    // 选中历史提交:只有提交详情,没有暂存区和提交框
    let head = cx.update(|cx| {
        repo.read(cx)
            .data
            .as_ref()
            .unwrap()
            .refs
            .head
            .oid()
            .unwrap()
    });
    cx.update(|cx| repo.update(cx, |r, cx| r.select_commit(head, cx)));
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).detail.is_some()
    })
    .await;
    assert!(has(cx, "side-section-details"));
    assert!(!has(cx, "side-section-unstaged") && !has(cx, "side-section-staged"));
    assert!(!has(cx, "commit"), "历史提交上没有提交按钮");

    // 在历史提交上按 ⌘L:切回 Working changes,提交框出现
    cx.update_window(handle.into(), |_, window, cx| {
        window.dispatch_action(Box::new(crate::actions::FocusCommitMessage), cx)
    })
    .unwrap();
    cx.run_until_parked();
    assert!(has(cx, "commit") && has(cx, "side-section-unstaged"));
}

#[gpui::test]
async fn commit_box_stays_at_the_bottom_when_panels_collapse(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let r = repo(&root, cx);
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.data.is_some() && m.status.is_some()
    })
    .await;
    let bottoms = |cx: &mut TestAppContext| {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            (
                window.find("side-panel").bounds().bottom(),
                window.find("commit-box").bounds().bottom(),
            )
        })
        .unwrap()
    };
    let (panel, boxed) = bottoms(cx);
    assert!(
        close(panel, boxed),
        "默认时提交框在最底部:{panel:?} {boxed:?}"
    );

    // 两个面板都收起:提交框仍然在最底部,而不是跟在标题行后面
    click(handle, "side-toggle-unstaged", cx);
    click(handle, "side-toggle-staged", cx);
    let (panel, boxed) = bottoms(cx);
    assert!(
        close(panel, boxed),
        "收起后提交框仍在最底部:{panel:?} {boxed:?}"
    );
}

#[gpui::test]
async fn diff_height_follows_ratio_and_can_be_maximized(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let r = repo(&root, cx);
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.data.is_some() && m.status.is_some()
    })
    .await;
    // 打开一个文件的 diff
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("u-main.rs", cx);
    })
    .unwrap();
    let r = repo(&root, cx);
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).diff.is_some()
    })
    .await;

    let diff_h = |cx: &mut TestAppContext| {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            f32::from(window.find("diff-panel").bounds().size.height)
        })
        .unwrap()
    };
    // 比例 0.5 与 0.25:diff 高度跟着变,且 0.25 时更矮
    let ws = cx.update(|cx| root.read(cx).workspace().clone());
    cx.update(|cx| ws.update(cx, |w, cx| w.update_settings(cx, |s| s.diff_ratio = 0.5)));
    let half = diff_h(cx);
    cx.update(|cx| ws.update(cx, |w, cx| w.update_settings(cx, |s| s.diff_ratio = 0.25)));
    let quarter = diff_h(cx);
    assert!(
        quarter < half * 0.7,
        "比例变小 diff 变矮:{half} -> {quarter}"
    );

    // 撑满:提交图消失,diff 比原来高;再点一次还原
    let has_graph = |cx: &mut TestAppContext| {
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.try_find("working-changes").is_some()
        })
        .unwrap()
    };
    assert!(has_graph(cx));
    click(handle, "maximize-diff", cx);
    assert!(!has_graph(cx), "撑满时不显示提交图");
    let full = diff_h(cx);
    assert!(full > half, "撑满后更高:{full} vs {half}");
    click(handle, "maximize-diff", cx);
    assert!(has_graph(cx), "再点一次还原");

    // 撑满后按 Esc 也能还原;没撑满时 Esc 什么都不改
    click(handle, "maximize-diff", cx);
    assert!(!has_graph(cx));
    let esc = |cx: &mut TestAppContext| {
        cx.update_window(handle.into(), |_, window, cx| {
            window.dispatch_action(Box::new(crate::actions::ExitMaximizedDiff), cx)
        })
        .unwrap();
        cx.run_until_parked();
    };
    esc(cx);
    assert!(has_graph(cx), "Esc 还原");
    esc(cx);
    assert!(has_graph(cx), "没撑满时 Esc 不改变界面");
}

//! 提交框的 AI 生成:没配置好时提示并打开设置,不进入“生成中”。

use crate::tests::{TIMEOUT, fixture, open, repo};
use gpui::test::TestAppContextExt as _;
use gpui::{AppContext as _, TestAppContext};

#[gpui::test]
async fn generating_without_a_key_opens_settings_instead_of_calling_out(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).data.is_some()
    })
    .await;
    assert!(cx.update(|cx| root.read(cx).settings_window().is_none()));

    let view = cx.update(|cx| root.read(cx).active_view(cx).expect("active view"));
    let v = view.clone();
    cx.update_window(handle.into(), |_, window, cx| {
        v.update(cx, |v, cx| v.generate_message(window, cx))
    })
    .unwrap();
    cx.run_until_parked();

    cx.update(|cx| {
        assert!(!view.read(cx).ai_busy, "没配置好不应进入生成中");
        assert!(
            root.read(cx).settings_window().is_some(),
            "应打开设置让用户去配置"
        );
    });
}

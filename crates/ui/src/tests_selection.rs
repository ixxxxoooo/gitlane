//! 切换提交时 diff 的行为:换了提交就不保留上一个提交里看的文件,再切回来也不自动打开。

use crate::tests::{TIMEOUT, fixture, open, repo};
use gitcore::Git;
use gpui::TestAppContext;
use gpui::test::TestAppContextExt as _;

#[gpui::test]
async fn switching_commits_does_not_keep_or_reopen_a_file(cx: &mut TestAppContext) {
    let dir = fixture();
    // 再提交一次,凑出两个提交;两个提交各只改一个文件
    let git = Git::new(dir.path());
    git.run(&["add", "-A"]).unwrap();
    git.run(&["commit", "-qm", "第二个提交"]).unwrap();
    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).data.is_some()
    })
    .await;
    let ids: Vec<_> = cx.update(|cx| {
        let h = &repo.read(cx).data.as_ref().unwrap().history;
        h.commits.iter().map(|c| c.id).collect()
    });
    assert!(ids.len() >= 2);

    // 选中第一个提交并等详情加载完:不应该自动打开任何文件
    cx.update(|cx| repo.update(cx, |r, cx| r.select_commit(ids[0], cx)));
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).detail.is_some()
    })
    .await;
    cx.run_until_parked();
    assert!(
        cx.update(|cx| repo.read(cx).diff.is_none()),
        "选中提交后不自动打开文件"
    );

    // 手动打开一个文件,切到另一个提交:diff 关掉
    let path = cx.update(|cx| repo.read(cx).detail.as_ref().unwrap().files[0].path.clone());
    cx.update(|cx| {
        repo.update(cx, |r, cx| {
            r.open_diff(gitcore::diff::DiffTarget::Commit(ids[0]), path, cx)
        })
    });
    assert!(cx.update(|cx| repo.read(cx).diff.is_some()));
    cx.update(|cx| repo.update(cx, |r, cx| r.select_commit(ids[1], cx)));
    cx.run_until_parked();
    assert!(
        cx.update(|cx| repo.read(cx).diff.is_none()),
        "换提交后关掉 diff"
    );

    // 切回第一个提交:不应该又把刚才那个文件打开
    cx.update(|cx| repo.update(cx, |r, cx| r.select_commit(ids[0], cx)));
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).detail.is_some()
    })
    .await;
    cx.run_until_parked();
    assert!(
        cx.update(|cx| repo.read(cx).diff.is_none()),
        "切回来后需要重新打开文件"
    );
}

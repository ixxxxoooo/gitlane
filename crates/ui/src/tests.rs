//! UI 集成测试:在无头窗口里跑真实视图,用临时仓库走完“看改动 → 暂存 → 提交 → 撤销”主线。

use crate::root::GitlaneWindow;
use app::{RepoModel, Selection, Settings, Workspace};
use gitcore::Git;
use gpui::base::Root;
use gpui::test::{TestAppContextExt as _, TestWindowExt as _};
use gpui::{
    AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds, WindowHandle,
    WindowOptions, px, size,
};
use std::time::Duration;

pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let git = Git::new(dir.path());
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.name", "UI Test"],
        vec!["config", "user.email", "ui@example.com"],
        vec!["config", "commit.gpgsign", "false"],
    ] {
        git.run(&args).unwrap();
    }
    std::fs::write(
        dir.path().join("main.rs"),
        "fn main() {\n    let x = 1;\n}\n",
    )
    .unwrap();
    git.run(&["add", "-A"]).unwrap();
    git.run(&["commit", "-qm", "初始提交"]).unwrap();
    std::fs::write(
        dir.path().join("main.rs"),
        "fn main() {\n    let x = 2;\n}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("notes.txt"), "笔记\n").unwrap();
    dir
}

pub(crate) fn open(
    cx: &mut TestAppContext,
    path: &std::path::Path,
) -> (WindowHandle<Root>, Entity<GitlaneWindow>) {
    cx.update(|cx| {
        gpui::init(cx);
        diffview::init(cx);
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
                let ws = cx.new(|cx| Workspace::new(Settings::default(), true, cx));
                cx.new(|cx| GitlaneWindow::new(ws, vec![path], window, cx))
            },
        )
        .expect("open test window");
        (window.downcast::<Root>().expect("Root"), root)
    })
}

pub(crate) fn repo(root: &Entity<GitlaneWindow>, cx: &mut TestAppContext) -> Entity<RepoModel> {
    cx.update(|cx| root.read(cx).workspace().read(cx).repos[0].clone())
}

#[gpui::test]
async fn stage_commit_and_undo_through_the_ui(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.data.is_some() && m.status.is_some() && m.busy.is_none()
    })
    .await;
    cx.update(|cx| {
        let m = repo.read(cx);
        assert_eq!(m.data.as_ref().unwrap().history.commits.len(), 1);
        assert_eq!(m.status.as_ref().unwrap().unstaged().count(), 2);
    });

    // 点 Working changes,再点未暂存的 main.rs 打开 diff
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("working-changes", cx);
        window.click("u-main.rs", cx);
    })
    .unwrap();
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx)
            .diff
            .as_ref()
            .and_then(|d| d.file.as_ref())
            .is_some_and(|f| f.hunks.len() == 1)
    })
    .await;
    cx.update(|cx| assert_eq!(repo.read(cx).selection, Selection::WorkingChanges));

    // 全部暂存
    cx.update_window(handle.into(), |_, window, cx| window.click("stage-all", cx))
        .unwrap();
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.busy.is_none() && m.status.as_ref().is_some_and(|s| s.staged().count() == 2)
    })
    .await;

    // 填提交信息并提交
    cx.update_window(handle.into(), |_, window, cx| {
        let view = root.read(cx).active_view(cx).unwrap();
        view.update(cx, |v, cx| {
            v.message
                .update(cx, |m, cx| m.set_value("界面测试提交", window, cx))
        });
        window.render_frame(cx);
        window.click("commit", cx);
    })
    .unwrap();
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.busy.is_none()
            && m.data
                .as_ref()
                .is_some_and(|d| d.history.commits.len() == 2)
    })
    .await;
    cx.update(|cx| {
        let m = repo.read(cx);
        assert_eq!(
            m.data.as_ref().unwrap().history.commits[0].summary,
            "界面测试提交"
        );
        assert!(m.status.as_ref().unwrap().is_clean());
        assert!(m.undo.peek_undo().is_some());
    });

    // 撤销提交:回到一个提交,改动仍在暂存区
    cx.update(|cx| repo.update(cx, |m, cx| m.undo(cx)));
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.busy.is_none()
            && m.data
                .as_ref()
                .is_some_and(|d| d.history.commits.len() == 1)
    })
    .await;
    cx.update(|cx| {
        let m = repo.read(cx);
        assert_eq!(m.status.as_ref().unwrap().staged().count(), 2);
        assert!(m.undo.peek_redo().is_some());
    });
    // 提交时清空的提交信息随撤销放回输入框
    cx.update(|cx| {
        let view = root.read(cx).active_view(cx).unwrap();
        assert_eq!(view.read(cx).message.read(cx).value(), "界面测试提交");
    });
}

#[gpui::test]
async fn cmd_w_closes_the_window_not_the_tab(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).data.is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.dispatch_action(Box::new(crate::actions::OpenSettings), cx)
    })
    .unwrap();
    cx.run_until_parked();
    let settings = cx.update(|cx| root.read(cx).settings_window().expect("设置窗口已打开"));
    let ws = cx.update(|cx| root.read(cx).workspace().clone());

    // 设置窗口里 ⌘W 只关设置窗口
    cx.simulate_keystrokes(settings.into(), "cmd-w");
    cx.run_until_parked();
    assert!(cx.update(|cx| {
        !cx.windows()
            .iter()
            .any(|w| w.window_id() == settings.window_id())
    }));
    assert!(cx.update(|cx| {
        cx.windows()
            .iter()
            .any(|w| w.window_id() == handle.window_id())
    }));

    // 主窗口里 ⌘W 关闭整个窗口(连带设置窗口),标签不受影响
    cx.update_window(handle.into(), |_, window, cx| {
        window.dispatch_action(Box::new(crate::actions::OpenSettings), cx)
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.simulate_keystrokes(handle.into(), "cmd-w");
    cx.run_until_parked();
    assert!(
        cx.update(|cx| cx.windows().is_empty()),
        "主窗口和设置窗口都已关闭"
    );
    assert_eq!(cx.update(|cx| ws.read(cx).repos.len()), 1, "关窗口不关标签");
}

#[gpui::test]
async fn line_level_staging_through_the_ui(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).status.is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("u-main.rs", cx);
    })
    .unwrap();
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx)
            .diff
            .as_ref()
            .and_then(|d| d.file.as_ref())
            .is_some()
    })
    .await;

    // 组件拿到了这个文件(并排 / 统一、高亮、选区都由组件负责)
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let view = root.read(cx).active_view(cx).unwrap();
        let v = view.read(cx);
        assert_eq!(v.diff_state.read(cx).files().len(), 1, "diff 已交给组件");
        assert!(v.diff_cache.is_some());
    })
    .unwrap();

    // 选中 hunk 里的新增行(“let x = 2;”),只暂存这一行
    cx.update_window(handle.into(), |_, window, cx| {
        let view = root.read(cx).active_view(cx).unwrap();
        view.update(cx, |v, cx| {
            let file = repo.read(cx).diff.as_ref().unwrap().file.clone().unwrap();
            let added = file.hunks[0]
                .lines
                .iter()
                .position(|l| l.kind == gitcore::diff::LineKind::Added)
                .unwrap();
            v.selected_lines.insert((0, added));
            cx.notify();
        });
        window.render_frame(cx);
        window.click("stage-lines", cx);
    })
    .unwrap();
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.busy.is_none() && m.status.as_ref().is_some_and(|s| s.staged().count() == 1)
    })
    .await;
    // 只暂存了新增行、没暂存删除行:索引里两行都在
    let index = Git::new(dir.path()).run(&["show", ":main.rs"]).unwrap();
    assert_eq!(index, "fn main() {\n    let x = 1;\n    let x = 2;\n}\n");
}

#[gpui::test]
async fn scan_directory_and_add_selected(cx: &mut TestAppContext) {
    // 扫描根目录下放三个仓库:opened(已打开)、alpha、group/beta
    let root = tempfile::tempdir().unwrap();
    let mk = |rel: &str| {
        let p = root.path().join(rel);
        std::fs::create_dir_all(&p).unwrap();
        Git::new(&p).run(&["init", "-q", "-b", "main"]).unwrap();
        p
    };
    let opened = mk("opened");
    mk("alpha");
    mk("group/beta");
    std::fs::create_dir_all(root.path().join("not-a-repo")).unwrap();

    let (handle, win) = open(cx, &opened);
    let root_path = root.path().to_path_buf();
    cx.update_window(handle.into(), |_, window, cx| {
        win.update(cx, |w, cx| w.open_scan_dialog(root_path, window, cx));
    })
    .unwrap();
    cx.wait_for(handle.into(), TIMEOUT, |window, _| {
        window.try_find(("scan-row", 2usize)).is_some()
    })
    .await;

    // 列表按路径排序:alpha、group/beta、opened;已打开的不默认勾选
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        // 取消勾选 alpha,只添加 group/beta
        window.click(("scan-row", 0usize), cx);
        window.click("scan-add", cx);
    })
    .unwrap();
    let ws = cx.update(|cx| win.read(cx).workspace().clone());
    let w = ws.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        w.read(cx).repos.len() == 2
    })
    .await;
    cx.update(|cx| {
        let names: Vec<String> = ws
            .read(cx)
            .repos
            .iter()
            .map(|r| r.read(cx).name.clone())
            .collect();
        assert_eq!(names, vec!["opened", "beta"]);
        assert_eq!(ws.read(cx).active, 1, "新添加的仓库成为当前标签");
    });
}

#[gpui::test]
async fn scan_rows_toggle_from_checkbox_and_label(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    for name in ["a", "b"] {
        let p = root.path().join(name);
        std::fs::create_dir_all(&p).unwrap();
        Git::new(&p).run(&["init", "-q"]).unwrap();
    }
    let opened = fixture();
    let (handle, win) = open(cx, opened.path());
    let rp = root.path().to_path_buf();
    cx.update_window(handle.into(), |_, window, cx| {
        win.update(cx, |w, cx| w.open_scan_dialog(rp, window, cx));
    })
    .unwrap();
    cx.wait_for(handle.into(), TIMEOUT, |window, _| {
        window.try_find(("scan-row", 1usize)).is_some()
    })
    .await;
    let label = |handle: WindowHandle<Root>, cx: &mut TestAppContext| {
        cx.update_window(handle.into(), |_, window, _| {
            window.find("scan-add").label().map(str::to_string)
        })
        .unwrap()
    };
    assert_eq!(label(handle, cx).as_deref(), Some("添加 2 个仓库"));
    // 点复选框本身:只切换一次(之前会被行的点击再切回去)
    cx.update_window(handle.into(), |_, window, cx| {
        window.click(("scan-check", 0usize), cx)
    })
    .unwrap();
    assert_eq!(label(handle, cx).as_deref(), Some("添加 1 个仓库"));
    // 点同一行的文字:切回来
    cx.update_window(handle.into(), |_, window, cx| {
        window.click(("scan-row", 0usize), cx)
    })
    .unwrap();
    assert_eq!(label(handle, cx).as_deref(), Some("添加 2 个仓库"));
}

#[gpui::test]
async fn sidebar_renders_only_visible_refs(cx: &mut TestAppContext) {
    let dir = fixture();
    let git = Git::new(dir.path());
    let head = git.run(&["rev-parse", "HEAD"]).unwrap();
    let mut input = String::new();
    for i in 0..400 {
        input.push_str(&format!("create refs/heads/b{i:03} {}", head));
    }
    git.run_with(
        &["update-ref", "--stdin"],
        Some(input.as_bytes()),
        &gitcore::Cancel::new(),
    )
    .unwrap();
    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).data.is_some()
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("ref:refs/heads/b000").is_some(),
            "首屏的分支已渲染"
        );
        assert!(
            window.try_find("ref:refs/heads/b399").is_none(),
            "屏幕外的分支不渲染"
        );
    })
    .unwrap();
}

#[gpui::test]
async fn settings_window_opens_once_and_changes_reach_open_repos(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).data.is_some()
    })
    .await;

    let open_settings = |cx: &mut TestAppContext| {
        cx.update_window(handle.into(), |_, window, cx| {
            window.dispatch_action(Box::new(crate::actions::OpenSettings), cx)
        })
        .unwrap();
        cx.run_until_parked();
        cx.update(|cx| root.read(cx).settings_window().expect("设置窗口已打开"))
    };
    let first = open_settings(cx);
    let windows = cx.update(|cx| cx.windows().len());
    assert_eq!(open_settings(cx), first, "再次打开只是激活已有窗口");
    assert_eq!(cx.update(|cx| cx.windows().len()), windows);
    cx.update_window(first.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("settings-root").is_some());
    })
    .unwrap();

    // 设置窗口改的是工作区设置:提交上限与上下文行数同步到已打开的仓库,主题切到跟随系统不出错
    let ws = cx.update(|cx| root.read(cx).workspace().clone());
    cx.update(|cx| {
        ws.update(cx, |ws, cx| {
            ws.update_settings(cx, |s| {
                s.history_limit = 10_000;
                s.diff_context_lines = 10;
                s.theme = app::ThemePreference::System;
            })
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let m = repo.read(cx);
        assert_eq!(m.history_limit, 10_000);
        assert_eq!(m.diff_context, 10);
    });
    cx.update_window(handle.into(), |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui::test]
async fn global_git_config_is_written_in_the_background(cx: &mut TestAppContext) {
    use gitcore::config::{ConfigChange, ConfigFile, PullMode};
    let dir = fixture();
    let cfg_dir = tempfile::tempdir().unwrap();
    let cfg_path = cfg_dir.path().join("gitconfig");
    let (handle, root) = open(cx, dir.path());
    let ws = cx.update(|cx| root.read(cx).workspace().clone());
    let path = cfg_path.clone();
    cx.update(|cx| {
        ws.update(cx, |ws, cx| {
            ws.set_git_config_file(ConfigFile::at(path));
            ws.load_git_config(cx);
        })
    });
    let w = ws.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        w.read(cx).git_config.is_some()
    })
    .await;

    cx.update(|cx| {
        ws.update(cx, |ws, cx| {
            ws.set_git_config(ConfigChange::UserName("界面测试".into()), cx);
            ws.set_git_config(ConfigChange::Pull(PullMode::Rebase), cx);
        })
    });
    // 界面先按新值显示
    cx.update(|cx| {
        let c = ws.read(cx).git_config.clone().unwrap();
        assert_eq!(c.user_name, "界面测试");
        assert_eq!(c.pull, PullMode::Rebase);
    });
    let file = ConfigFile::at(&cfg_path);
    for _ in 0..200 {
        cx.run_until_parked();
        if file.load().unwrap().pull == PullMode::Rebase {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let on_disk = file.load().unwrap();
    assert_eq!(on_disk.user_name, "界面测试");
    assert_eq!(on_disk.pull, PullMode::Rebase);
    cx.update(|cx| assert!(ws.read(cx).git_config_error.is_none()));
}

#[gpui::test]
async fn auto_fetch_runs_quietly_and_respects_the_interval(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        let m = r.read(cx);
        m.data.is_some() && m.busy.is_none()
    })
    .await;
    let notices = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let _sub = cx.update(|cx| {
        let notices = notices.clone();
        cx.subscribe(&repo, move |_, e: &app::RepoEvent, _| {
            notices.borrow_mut().push(format!("{e:?}"))
        })
    });
    let ws = cx.update(|cx| root.read(cx).workspace().clone());

    // 关闭时不执行
    cx.update(|cx| ws.update(cx, |ws, cx| ws.auto_fetch_tick(cx)));
    cx.update(|cx| assert!(repo.read(cx).busy.is_none()));

    cx.update(|cx| {
        ws.update(cx, |ws, cx| {
            ws.update_settings(cx, |s| s.auto_fetch_minutes = 5);
            ws.auto_fetch_tick(cx);
        })
    });
    cx.update(|cx| {
        let label = repo.read(cx).busy.as_ref().map(|b| b.label.clone());
        assert_eq!(label.as_deref(), Some("自动 Fetch"));
    });
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx).busy.is_none()
    })
    .await;
    assert!(
        notices.borrow().is_empty(),
        "自动 fetch 不弹提示:{:?}",
        notices.borrow()
    );

    // 刚 fetch 过,间隔未到不会再执行
    cx.update(|cx| ws.update(cx, |ws, cx| ws.auto_fetch_tick(cx)));
    cx.update(|cx| assert!(repo.read(cx).busy.is_none()));
}

#[gpui::test]
async fn forget_recent_repo_from_the_plus_menu(cx: &mut TestAppContext) {
    let dir = fixture();
    let (handle, win) = open(cx, dir.path());
    let ws = cx.update(|cx| win.read(cx).workspace().clone());
    cx.update(|cx| {
        assert_eq!(
            ws.read(cx).recent_repos().len(),
            1,
            "打开的仓库进入最近列表"
        )
    });

    // 打开“+”菜单,点第一行的移除按钮
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("open-repo", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.wait_for(handle.into(), TIMEOUT, |window, _| {
        window.try_find(("forget-recent", 0usize)).is_some()
    })
    .await;
    cx.run_until_parked();
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        window.click(("forget-recent", 0usize), cx)
    })
    .unwrap();
    cx.wait_for(handle.into(), TIMEOUT, |window, _| {
        window.try_find(("forget-recent", 0usize)).is_none()
    })
    .await;

    cx.update(|cx| {
        let ws = ws.read(cx);
        assert!(ws.recent_repos().is_empty(), "移除后最近列表为空");
        assert_eq!(ws.repos.len(), 1, "已打开的标签不受影响");
    });
}

#[gpui::test]
async fn aborting_a_merge_asks_first(cx: &mut TestAppContext) {
    use gpui::component::WindowExt as _;
    let dir = fixture();
    let git = Git::new(dir.path());
    git.run(&["checkout", "-q", "--", "main.rs"]).unwrap();
    git.run(&["checkout", "-q", "-b", "topic"]).unwrap();
    std::fs::write(dir.path().join("main.rs"), "topic\n").unwrap();
    git.run(&["commit", "-qam", "topic"]).unwrap();
    git.run(&["checkout", "-q", "main"]).unwrap();
    std::fs::write(dir.path().join("main.rs"), "main\n").unwrap();
    git.run(&["commit", "-qam", "main"]).unwrap();
    assert!(git.run(&["merge", "topic"]).is_err(), "制造冲突");

    let (handle, root) = open(cx, dir.path());
    let repo = repo(&root, cx);
    let r = repo.clone();
    cx.wait_for(handle.into(), TIMEOUT, move |_, cx| {
        r.read(cx)
            .status
            .as_ref()
            .is_some_and(|s| s.state == gitcore::RepoState::Merge)
    })
    .await;
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_action(Box::new(crate::actions::AbortOperation), cx);
    })
    .unwrap();
    cx.run_until_parked();
    // 中止会丢掉已解决的冲突且无法撤销:先弹确认框,不直接执行
    cx.update_window(handle.into(), |_, window, cx| {
        assert!(window.has_active_dialog(cx));
    })
    .unwrap();
    assert!(dir.path().join(".git/MERGE_HEAD").exists());
}

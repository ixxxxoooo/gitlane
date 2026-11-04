//! 用临时仓库做的集成测试:读路径、暂存、提交、撤销。

use gitcore::diff::{DEFAULT_CONTEXT_LINES, DiffTarget, file_diff};
use gitcore::ops::{CommitOptions, ResetMode};
use gitcore::patch::PatchDirection;
use gitcore::undo::{self, UndoStack};
use gitcore::{ChangeKind, Git, Head, RefKind, Repo};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
    git: Git,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path());
        git.run(&["init", "-q", "-b", "main"]).unwrap();
        git.run(&["config", "user.name", "Tester"]).unwrap();
        git.run(&["config", "user.email", "t@example.com"]).unwrap();
        git.run(&["config", "commit.gpgsign", "false"]).unwrap();
        Self { dir, git }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, name: &str, content: &str) {
        let p = self.path().join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.path().join(name)).unwrap()
    }

    fn commit_file(&self, name: &str, content: &str, msg: &str) {
        self.write(name, content);
        self.git.stage_paths(&[name]).unwrap();
        self.git.commit(msg, &CommitOptions::default()).unwrap();
    }

    fn repo(&self) -> Repo {
        Repo::open(self.path()).unwrap()
    }

    fn index_content(&self, name: &str) -> String {
        self.git.run(&["show", &format!(":{name}")]).unwrap()
    }
}

#[test]
fn history_and_refs() {
    let f = Fixture::new();
    f.commit_file("a.txt", "1\n", "first");
    f.git.create_branch("feature/x", "HEAD", true).unwrap();
    f.commit_file("b.txt", "2\n", "on feature");
    f.git.checkout("main").unwrap();
    f.commit_file("a.txt", "1\n2\n", "on main");
    f.git.merge("feature/x").unwrap();
    f.git.create_tag("v1", "HEAD", None).unwrap();

    let mut repo = f.repo();
    let h = repo.history(1000).unwrap();
    assert_eq!(h.commits.len(), 4);
    assert_eq!(h.commits[0].parents.len(), 2, "合并提交在最上面");
    assert!(h.parent_rows.iter().flatten().all(|r| *r != u32::MAX));
    let layout = graph::layout(&h.parent_rows);
    assert_eq!(layout.max_columns(), 2);

    let refs = repo.refs().unwrap();
    assert!(matches!(refs.head, Head::Branch { ref name, .. } if name == "main"));
    let locals: Vec<_> = refs
        .refs
        .iter()
        .filter(|r| r.kind == RefKind::Local)
        .map(|r| r.short.as_str())
        .collect();
    assert_eq!(locals, vec!["feature/x", "main"]);
    assert!(
        refs.refs
            .iter()
            .any(|r| r.kind == RefKind::Tag && r.short == "v1")
    );

    let detail = repo.commit_detail(h.commits[3].id).unwrap();
    assert_eq!(detail.info.summary, "first");
    assert_eq!(detail.files.len(), 1);
    assert_eq!(detail.files[0].kind, ChangeKind::Added);
}

#[test]
fn unborn_repo_status_and_first_commit() {
    let f = Fixture::new();
    let repo = f.repo();
    assert!(matches!(repo.head().unwrap(), Head::Unborn { .. }));
    assert_eq!(repo.history(10).unwrap().commits.len(), 0);

    f.write("new.txt", "hello\n");
    let st = repo.status().unwrap();
    assert_eq!(st.entries[0].unstaged, Some(ChangeKind::Untracked));

    f.git.stage_paths(&["new.txt"]).unwrap();
    assert_eq!(
        repo.status().unwrap().entries[0].staged,
        Some(ChangeKind::Added)
    );
    f.git.unstage_paths(&["new.txt"]).unwrap();
    assert_eq!(repo.status().unwrap().entries[0].staged, None);
}

#[test]
fn line_level_stage_and_unstage() {
    let f = Fixture::new();
    f.commit_file("a.txt", "one\ntwo\nthree\nfour\n", "base");
    f.write("a.txt", "one\nTWO\nthree\nFOUR\n");
    let repo = f.repo();

    // 只暂存 two → TWO 这一对
    let d = file_diff(
        &repo,
        DiffTarget::Unstaged,
        "a.txt",
        false,
        DEFAULT_CONTEXT_LINES,
    )
    .unwrap();
    assert_eq!(d.hunks.len(), 1);
    let lines = &d.hunks[0].lines;
    let sel: BTreeSet<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.text == "two" || l.text == "TWO")
        .map(|(i, _)| i)
        .collect();
    f.git
        .apply_selection(&d, 0, Some(&sel), PatchDirection::Stage)
        .unwrap();
    assert_eq!(f.index_content("a.txt"), "one\nTWO\nthree\nfour\n");
    assert_eq!(f.read("a.txt"), "one\nTWO\nthree\nFOUR\n", "工作区不变");

    // 再暂存剩余部分,然后只取消暂存 four → FOUR
    f.git.stage_paths(&["a.txt"]).unwrap();
    let staged = file_diff(
        &repo,
        DiffTarget::Staged,
        "a.txt",
        false,
        DEFAULT_CONTEXT_LINES,
    )
    .unwrap();
    let lines = &staged.hunks[0].lines;
    let sel: BTreeSet<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.text == "four" || l.text == "FOUR")
        .map(|(i, _)| i)
        .collect();
    f.git
        .apply_selection(&staged, 0, Some(&sel), PatchDirection::Unstage)
        .unwrap();
    assert_eq!(f.index_content("a.txt"), "one\nTWO\nthree\nfour\n");
}

#[test]
fn context_lines_setting_keeps_line_staging_working() {
    let f = Fixture::new();
    let lines = |five: &str, fifteen: &str| -> String {
        (1..=20)
            .map(|i| match i {
                5 => format!("{five}\n"),
                15 => format!("{fifteen}\n"),
                _ => format!("{i}\n"),
            })
            .collect()
    };
    let base = lines("5", "15");
    f.commit_file("a.txt", &base, "base");
    f.write("a.txt", &lines("five", "fifteen"));
    let repo = f.repo();

    // 默认 3 行上下文时两处改动相隔 10 行,是两个 hunk;10 行上下文合成一个
    let d = file_diff(&repo, DiffTarget::Unstaged, "a.txt", false, 3).unwrap();
    assert_eq!(d.hunks.len(), 2);
    assert_eq!(d.hunks[0].lines.len(), 8);
    let wide = file_diff(&repo, DiffTarget::Unstaged, "a.txt", false, 10).unwrap();
    assert_eq!(wide.hunks.len(), 1);

    // 0 会被当成 1:补丁仍能定位,只暂存 5 → five
    let narrow = file_diff(&repo, DiffTarget::Unstaged, "a.txt", false, 0).unwrap();
    assert_eq!(narrow.hunks[0].lines.len(), 4);
    let sel: BTreeSet<usize> = narrow.hunks[0]
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.text == "5" || l.text == "five")
        .map(|(i, _)| i)
        .collect();
    f.git
        .apply_selection(&narrow, 0, Some(&sel), PatchDirection::Stage)
        .unwrap();
    assert_eq!(f.index_content("a.txt"), lines("five", "15"));
}

#[test]
fn partial_stage_of_new_file() {
    let f = Fixture::new();
    f.commit_file("base.txt", "x\n", "base");
    f.write("n.txt", "a\nb\n");
    let repo = f.repo();
    let d = file_diff(
        &repo,
        DiffTarget::Unstaged,
        "n.txt",
        false,
        DEFAULT_CONTEXT_LINES,
    )
    .unwrap();
    assert!(d.is_new);
    let sel: BTreeSet<usize> = [0].into();
    f.git
        .apply_selection(&d, 0, Some(&sel), PatchDirection::Stage)
        .unwrap();
    assert_eq!(f.index_content("n.txt"), "a\n");
}

#[test]
fn discard_hunk_in_worktree() {
    let f = Fixture::new();
    f.commit_file("a.txt", "one\ntwo\n", "base");
    f.write("a.txt", "one\nTWO\n");
    let repo = f.repo();
    let d = file_diff(
        &repo,
        DiffTarget::Unstaged,
        "a.txt",
        false,
        DEFAULT_CONTEXT_LINES,
    )
    .unwrap();
    f.git
        .apply_selection(&d, 0, None, PatchDirection::Discard)
        .unwrap();
    assert_eq!(f.read("a.txt"), "one\ntwo\n");
}

#[test]
fn undo_redo_commit() {
    let f = Fixture::new();
    f.commit_file("a.txt", "1\n", "first");
    let repo = f.repo();
    let base = repo.head().unwrap().oid().unwrap();

    f.write("a.txt", "2\n");
    f.git.stage_paths(&["a.txt"]).unwrap();
    let mut stack = UndoStack::default();
    let entry = undo::record_refs(&repo, "提交", ResetMode::Soft, false, || {
        f.git.commit("second", &CommitOptions::default())
    })
    .unwrap();
    let committed = repo.head().unwrap().oid().unwrap();
    stack.push(entry);

    let e = stack.take_undo().unwrap();
    undo::undo(&repo, &e).unwrap();
    stack.did_undo(e);
    assert_eq!(repo.head().unwrap().oid(), Some(base));
    // soft 撤销:改动仍在暂存区
    assert_eq!(f.index_content("a.txt"), "2\n");

    let e = stack.take_redo().unwrap();
    undo::redo(&repo, &e).unwrap();
    stack.did_redo(e);
    assert_eq!(repo.head().unwrap().oid(), Some(committed));
    assert!(repo.status().unwrap().is_clean());
}

#[test]
fn undo_first_commit_returns_to_unborn() {
    let f = Fixture::new();
    let repo = f.repo();
    f.write("a.txt", "1\n");
    f.git.stage_paths(&["a.txt"]).unwrap();
    let e = undo::record_refs(&repo, "提交", ResetMode::Soft, false, || {
        f.git.commit("first", &CommitOptions::default())
    })
    .unwrap();
    undo::undo(&repo, &e).unwrap();
    assert!(matches!(repo.head().unwrap(), Head::Unborn { .. }));
    assert_eq!(
        repo.status().unwrap().entries[0].staged,
        Some(ChangeKind::Added)
    );
}

#[test]
fn undo_checkout_and_delete_branch() {
    let f = Fixture::new();
    f.commit_file("a.txt", "1\n", "first");
    f.git.create_branch("dev", "HEAD", false).unwrap();
    f.commit_file("a.txt", "2\n", "second on main");
    let repo = f.repo();

    let e = undo::record_refs(&repo, "检出 dev", ResetMode::Keep, false, || {
        f.git.checkout("dev")
    })
    .unwrap();
    assert_eq!(f.read("a.txt"), "1\n");
    undo::undo(&repo, &e).unwrap();
    assert_eq!(repo.head().unwrap().branch(), Some("main"));
    assert_eq!(f.read("a.txt"), "2\n");

    let dev_oid = f.git.run(&["rev-parse", "dev"]).unwrap();
    let e = undo::record_refs(&repo, "删除 dev", ResetMode::Keep, false, || {
        f.git.delete_branch("dev", true)
    })
    .unwrap();
    assert!(f.git.run(&["rev-parse", "--verify", "-q", "dev"]).is_err());
    undo::undo(&repo, &e).unwrap();
    assert_eq!(f.git.run(&["rev-parse", "dev"]).unwrap(), dev_oid);
}

#[test]
fn undo_create_and_checkout_branch() {
    let f = Fixture::new();
    f.commit_file("a.txt", "1\n", "first");
    let repo = f.repo();
    let e = undo::record_refs(&repo, "新建分支", ResetMode::Keep, false, || {
        f.git.create_branch("topic", "HEAD", true)
    })
    .unwrap();
    assert_eq!(repo.head().unwrap().branch(), Some("topic"));
    undo::undo(&repo, &e).unwrap();
    assert_eq!(repo.head().unwrap().branch(), Some("main"));
    assert!(
        f.git
            .run(&["rev-parse", "--verify", "-q", "topic"])
            .is_err()
    );
}

#[test]
fn undo_hard_reset_restores_work_in_progress() {
    let f = Fixture::new();
    f.commit_file("a.txt", "1\n", "first");
    f.commit_file("a.txt", "2\n", "second");
    f.write("a.txt", "dirty\n");
    let repo = f.repo();
    let head = repo.head().unwrap().oid().unwrap();

    let e = undo::record_refs(&repo, "hard reset", ResetMode::Hard, true, || {
        f.git.reset("HEAD~1", ResetMode::Hard)
    })
    .unwrap();
    assert_eq!(f.read("a.txt"), "1\n");
    undo::undo(&repo, &e).unwrap();
    assert_eq!(repo.head().unwrap().oid(), Some(head));
    assert_eq!(f.read("a.txt"), "dirty\n");
}

#[test]
fn undo_discard() {
    let f = Fixture::new();
    f.commit_file("a.txt", "1\n", "first");
    f.write("a.txt", "changed\n");
    f.write("untracked.txt", "u\n");
    let repo = f.repo();
    let e = undo::record_files(&repo, "丢弃", &["a.txt", "untracked.txt"], || {
        f.git.discard_paths(&["a.txt"], &["untracked.txt"])
    })
    .unwrap();
    assert_eq!(f.read("a.txt"), "1\n");
    assert!(!f.path().join("untracked.txt").exists());

    undo::undo(&repo, &e).unwrap();
    assert_eq!(f.read("a.txt"), "changed\n");
    assert_eq!(f.read("untracked.txt"), "u\n");

    undo::redo(&repo, &e).unwrap();
    assert_eq!(f.read("a.txt"), "1\n");
    assert!(!f.path().join("untracked.txt").exists());
}

#[test]
fn noop_entries_are_not_pushed() {
    let f = Fixture::new();
    f.commit_file("a.txt", "1\n", "first");
    let repo = f.repo();
    let e = undo::record_refs(&repo, "无变化", ResetMode::Keep, false, || Ok(())).unwrap();
    let mut stack = UndoStack::default();
    stack.push(e);
    assert!(stack.peek_undo().is_none());
}

#[test]
fn cancel_kills_running_command() {
    let f = Fixture::new();
    let cancel = gitcore::Cancel::new();
    cancel.cancel();
    // 已取消的标记会在首次轮询时结束进程
    let r = f.git.run_with(
        &["hash-object", "--stdin"],
        Some(&vec![b'a'; 1 << 20]),
        &cancel,
    );
    assert!(matches!(r, Err(gitcore::Error::Cancelled)) || r.is_ok());
}

#[test]
fn status_carries_line_stats_for_staged_unstaged_and_untracked() {
    let f = Fixture::new();
    f.write("a.txt", "1\n2\n3\n");
    f.write("b.txt", "x\ny\n");
    f.git.run(&["add", "-A"]).unwrap();
    f.git.run(&["commit", "-qm", "init"]).unwrap();

    // a.txt:暂存一处新增,再在工作区删一行;b.txt 只在工作区改一行;c.txt 未跟踪 3 行
    f.write("a.txt", "1\n2\n3\n4\n");
    f.git.run(&["add", "a.txt"]).unwrap();
    f.write("a.txt", "1\n2\n4\n");
    f.write("b.txt", "x\nz\n");
    f.write("c.txt", "p\nq\nr\n");

    let st = Repo::open(f.path()).unwrap().status().unwrap();
    let get = |name: &str| st.entries.iter().find(|e| e.path == name).unwrap();
    assert_eq!(get("a.txt").staged_lines, Some((1, 0)));
    assert_eq!(get("a.txt").unstaged_lines, Some((0, 1)));
    assert_eq!(get("b.txt").unstaged_lines, Some((1, 1)));
    assert_eq!(get("b.txt").staged_lines, None, "没有暂存改动就没有统计");
    assert_eq!(
        get("c.txt").unstaged_lines,
        Some((3, 0)),
        "未跟踪文件整个算新增"
    );
}

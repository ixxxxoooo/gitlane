//! 单个仓库的状态 Entity。视图只读取这里的状态、只调用这里的方法;
//! 所有 git 读写都在后台执行器里完成,结果写回后 `cx.notify()` 触发重绘。

use crate::command::{self, Command};
use futures::StreamExt as _;
use gitcore::diff::{DiffTarget, FileDiff, LARGE_DIFF_LINES, file_diff, side_texts};
use gitcore::highlight::{DiffSyntax, highlight_diff};
use gitcore::undo::{self, UndoEntry, UndoStack};
use gitcore::watch::{Change, RepoWatcher};
use gitcore::{Cancel, CommitDetail, Error, History, Oid, Refs, Repo, RepoStatus};
use gpui::{AppContext as _, Context, EventEmitter, Task};
use graph::GraphLayout;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 文件变化的防抖时间:外部工具批量写文件时只刷新一次。
const WATCH_DEBOUNCE: Duration = Duration::from_millis(300);
/// 语法高亮读取完整文件的大小上限,更大的文件只按 hunk 高亮。
const SYNTAX_MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

/// 历史 + 图布局 + 引用,一次加载、整体替换。
pub struct RepoData {
    pub history: History,
    pub graph: GraphLayout,
    pub refs: Refs,
    /// 提交 → 指向它的引用在 `refs.refs` 中的下标。
    pub badges: HashMap<Oid, Vec<usize>>,
    pub load_time: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    WorkingChanges,
    Commit(Oid),
}

pub struct DiffState {
    pub target: DiffTarget,
    pub path: String,
    /// `None` 表示加载中。
    pub file: Option<Arc<FileDiff>>,
    /// 与 `file` 同时到达;不认识的语言或超大 diff 为 `None`。
    pub syntax: Option<Arc<DiffSyntax>>,
    pub error: Option<String>,
}

pub struct Busy {
    pub label: String,
    /// 可取消的任务(联网操作)才有。
    pub cancel: Option<Cancel>,
    pub started: Instant,
    /// 后台自动执行(自动 fetch):成功不提示,失败只记日志。
    pub quiet: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Success,
    Error,
}

#[derive(Clone, Debug)]
pub enum RepoEvent {
    Notice {
        kind: NoticeKind,
        message: String,
    },
    /// 提交成功,UI 清空提交信息框。
    Committed,
}

enum Job {
    Command { cmd: Command, quiet: bool },
    Undo,
    Redo,
}

pub struct RepoModel {
    pub path: PathBuf,
    pub name: String,
    pub data: Option<Arc<RepoData>>,
    pub status: Option<Arc<RepoStatus>>,
    pub selection: Selection,
    pub detail: Option<Arc<CommitDetail>>,
    pub diff: Option<DiffState>,
    pub undo: UndoStack,
    pub busy: Option<Busy>,
    pub load_error: Option<String>,
    pub search_query: String,
    pub search_hits: Vec<usize>,
    pub history_limit: usize,
    /// diff 上下文行数(来自设置)。
    pub diff_context: u32,
    /// 上次成功 fetch / pull 的时间,自动 fetch 据此判断是否到期。
    last_fetch: Option<Instant>,
    queue: VecDeque<Job>,
    full_loading: bool,
    full_pending: bool,
    status_loading: bool,
    status_pending: bool,
    detail_gen: u64,
    diff_gen: u64,
    _watcher: Option<RepoWatcher>,
    _watch_task: Option<Task<()>>,
}

impl EventEmitter<RepoEvent> for RepoModel {}

impl RepoModel {
    /// 用已打开的仓库创建状态。调用方先在前台 `Repo::open` 校验路径(很快),数据加载在后台。
    pub fn new(
        repo: Repo,
        history_limit: usize,
        diff_context: u32,
        watch: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let workdir = repo.workdir().to_path_buf();
        let name = repo.name();

        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Change>();
        let watcher = if watch {
            match RepoWatcher::new(&workdir, repo.git_dir(), move |c| {
                let _ = tx.unbounded_send(c);
            }) {
                Ok(w) => Some(w),
                Err(e) => {
                    log::warn!("{e}");
                    None
                }
            }
        } else {
            None
        };
        let watch_task = cx.spawn(async move |this, cx| {
            while let Some(change) = rx.next().await {
                let mut full = change == Change::Repository;
                cx.background_executor().timer(WATCH_DEBOUNCE).await;
                while let Ok(c) = rx.try_recv() {
                    full |= c == Change::Repository;
                }
                if this
                    .update(cx, |this, cx| this.on_fs_change(full, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut model = Self {
            path: workdir,
            name,
            data: None,
            status: None,
            selection: Selection::WorkingChanges,
            detail: None,
            diff: None,
            undo: UndoStack::default(),
            busy: None,
            load_error: None,
            search_query: String::new(),
            search_hits: Vec::new(),
            history_limit,
            diff_context,
            last_fetch: None,
            queue: VecDeque::new(),
            full_loading: false,
            full_pending: false,
            status_loading: false,
            status_pending: false,
            detail_gen: 0,
            diff_gen: 0,
            _watcher: watcher,
            _watch_task: Some(watch_task),
        };
        model.refresh(true, cx);
        model
    }

    pub fn head_label(&self) -> String {
        self.data
            .as_ref()
            .map(|d| d.refs.head.label())
            .unwrap_or_else(|| "…".into())
    }

    fn notice(&self, kind: NoticeKind, message: impl Into<String>, cx: &mut Context<Self>) {
        cx.emit(RepoEvent::Notice {
            kind,
            message: message.into(),
        });
    }

    // ---- 刷新 ----

    fn on_fs_change(&mut self, full: bool, cx: &mut Context<Self>) {
        // 命令执行期间自己的写操作也会触发监听,命令结束时会统一刷新
        if self.busy.is_some() {
            return;
        }
        self.refresh(full, cx);
    }

    /// `full` 为 true 时重新加载历史、图与引用;否则只刷新 status。
    ///
    /// 同一类加载同时只跑一个:进行中再收到请求只记一个“待刷新”,当前这次结束后再跑一次。
    /// 这样外部工具连续写文件时不会并发出多份全量加载互相抢 CPU。
    pub fn refresh(&mut self, full: bool, cx: &mut Context<Self>) {
        let path = self.path.clone();
        if full {
            if self.full_loading {
                self.full_pending = true;
                return;
            }
            self.full_loading = true;
            let limit = self.history_limit;
            let task = cx.background_spawn(async move { load_full(&path, limit) });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.full_loading = false;
                    match result {
                        Ok((data, status)) => {
                            this.load_error = None;
                            this.apply_data(data, cx);
                            this.apply_status(status, cx);
                        }
                        Err(e) => this.load_error = Some(e.to_string()),
                    }
                    if std::mem::take(&mut this.full_pending) {
                        this.refresh(true, cx);
                    }
                    cx.notify();
                });
            })
            .detach();
        } else {
            if self.status_loading {
                self.status_pending = true;
                return;
            }
            self.status_loading = true;
            let task = cx.background_spawn(async move { Repo::open(&path)?.status() });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    this.status_loading = false;
                    match result {
                        Ok(status) => this.apply_status(status, cx),
                        Err(e) => this.load_error = Some(e.to_string()),
                    }
                    if std::mem::take(&mut this.status_pending) {
                        this.refresh(false, cx);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    fn apply_data(&mut self, data: RepoData, cx: &mut Context<Self>) {
        let data = Arc::new(data);
        // 选中的提交不在新历史里(例如被 reset 掉)时,回到 HEAD
        if let Selection::Commit(oid) = self.selection
            && !data.history.row_of.contains_key(&oid)
        {
            match data.refs.head.oid() {
                Some(head) => self.select_commit(head, cx),
                None => self.select_working(cx),
            }
        }
        self.data = Some(data);
        self.update_search();
    }

    fn apply_status(&mut self, status: RepoStatus, cx: &mut Context<Self>) {
        self.status = Some(Arc::new(status));
        // 工作区 diff 跟着刷新;文件已不在对应列表里就关掉
        if let Some(diff) = &self.diff
            && matches!(diff.target, DiffTarget::Staged | DiffTarget::Unstaged)
        {
            let still_there = self.status.as_ref().is_some_and(|s| {
                s.entries.iter().any(|e| {
                    e.path == diff.path
                        && match diff.target {
                            DiffTarget::Staged => e.staged.is_some(),
                            _ => e.unstaged.is_some(),
                        }
                })
            });
            if still_there {
                let (target, path) = (diff.target, diff.path.clone());
                self.load_diff(target, path, false, true, cx);
            } else {
                self.diff = None;
            }
        }
    }

    // ---- 设置变化 ----

    /// 提交上限变化后按新上限重新加载历史。
    pub fn set_history_limit(&mut self, limit: usize, cx: &mut Context<Self>) {
        if self.history_limit != limit {
            self.history_limit = limit;
            self.refresh(true, cx);
        }
    }

    /// 上下文行数变化后重新加载正在显示的 diff。
    pub fn set_diff_context(&mut self, lines: u32, cx: &mut Context<Self>) {
        if self.diff_context == lines {
            return;
        }
        self.diff_context = lines;
        if let Some(d) = &self.diff {
            let (target, path) = (d.target, d.path.clone());
            self.load_diff(target, path, false, true, cx);
        }
    }

    // ---- 选择 ----

    pub fn select_working(&mut self, cx: &mut Context<Self>) {
        self.selection = Selection::WorkingChanges;
        self.detail = None;
        self.detail_gen += 1;
        if self
            .diff
            .as_ref()
            .is_some_and(|d| matches!(d.target, DiffTarget::Commit(_)))
        {
            self.diff = None;
        }
        cx.notify();
    }

    pub fn select_commit(&mut self, oid: Oid, cx: &mut Context<Self>) {
        if self.selection == Selection::Commit(oid) && self.detail.is_some() {
            return;
        }
        self.selection = Selection::Commit(oid);
        self.diff = None;
        self.detail = None;
        self.detail_gen += 1;
        let generation = self.detail_gen;
        let path = self.path.clone();
        let task = cx.background_spawn(async move { Repo::open(&path)?.commit_detail(oid) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.detail_gen {
                    return;
                }
                match result {
                    Ok(detail) => {
                        // 只改了一个文件时直接打开它的 diff
                        if let [only] = detail.files.as_slice() {
                            let p = only.path.clone();
                            this.load_diff(DiffTarget::Commit(oid), p, false, false, cx);
                        }
                        this.detail = Some(Arc::new(detail));
                    }
                    Err(e) => this.notice(NoticeKind::Error, e.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn open_diff(&mut self, target: DiffTarget, path: String, cx: &mut Context<Self>) {
        self.load_diff(target, path, false, false, cx);
    }

    /// 大文件“仍然显示”。
    pub fn open_full_diff(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = &self.diff {
            let (target, path) = (d.target, d.path.clone());
            self.load_diff(target, path, true, true, cx);
        }
    }

    /// `keep_old` 为 true 时加载期间保留旧内容,避免刷新时闪烁。
    fn load_diff(
        &mut self,
        target: DiffTarget,
        path: String,
        force_full: bool,
        keep_old: bool,
        cx: &mut Context<Self>,
    ) {
        self.diff_gen += 1;
        let generation = self.diff_gen;
        let old = self
            .diff
            .take()
            .filter(|d| keep_old && d.path == path && d.target == target);
        let (file, syntax) = old.map(|d| (d.file, d.syntax)).unwrap_or_default();
        let had_old = file.is_some();
        self.diff = Some(DiffState {
            target,
            path: path.clone(),
            file,
            syntax,
            error: None,
        });
        let repo_path = self.path.clone();
        let p = path.clone();
        let context = self.diff_context;
        let task = cx.background_spawn({
            let repo_path = repo_path.clone();
            async move {
                let repo = Repo::open(&repo_path)?;
                file_diff(&repo, target, &p, force_full, context)
            }
        });
        cx.spawn(async move |this, cx| {
            let file = match task.await {
                Ok(f) => Arc::new(f),
                Err(e) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(d) = this.diff.as_mut().filter(|_| generation == this.diff_gen)
                        {
                            d.error = Some(e.to_string());
                            cx.notify();
                        }
                    });
                    return;
                }
            };
            let set = |file: Arc<FileDiff>, syntax: Option<DiffSyntax>, cx: &mut gpui::AsyncApp| {
                let _ = this.update(cx, |this, cx| {
                    if let Some(d) = this.diff.as_mut().filter(|_| generation == this.diff_gen) {
                        d.file = Some(file);
                        d.syntax = syntax.map(Arc::new);
                        cx.notify();
                    }
                });
            };
            // 高亮比 diff 慢得多(大文件要几百毫秒):首次打开先显示无高亮的 diff;
            // 刷新时旧内容还在,等高亮算完一起替换,避免闪一下无高亮的版本
            if !had_old {
                set(file.clone(), None, cx);
            }
            let f = file.clone();
            let syntax = cx
                .background_executor()
                .spawn(async move { load_syntax(&repo_path, target, &f) })
                .await;
            set(file, syntax, cx);
        })
        .detach();
        cx.notify();
    }

    pub fn close_diff(&mut self, cx: &mut Context<Self>) {
        self.diff = None;
        self.diff_gen += 1;
        cx.notify();
    }

    // ---- 搜索 ----

    pub fn set_search(&mut self, query: String, cx: &mut Context<Self>) {
        if query == self.search_query {
            return;
        }
        self.search_query = query;
        self.update_search();
        cx.notify();
    }

    fn update_search(&mut self) {
        self.search_hits = match (&self.data, self.search_query.trim().is_empty()) {
            (Some(d), false) => crate::search::search(&d.history.commits, &self.search_query),
            _ => Vec::new(),
        };
    }

    // ---- 命令 ----

    pub fn run(&mut self, cmd: Command, cx: &mut Context<Self>) {
        self.queue.push_back(Job::Command { cmd, quiet: false });
        self.pump(cx);
    }

    /// 自动 fetch:距上次 fetch 超过 `interval` 且仓库空闲时才执行,不打断用户正在排队的操作。
    pub fn auto_fetch(&mut self, interval: Duration, cx: &mut Context<Self>) {
        if self.busy.is_some()
            || !self.queue.is_empty()
            || self.last_fetch.is_some_and(|t| t.elapsed() < interval)
        {
            return;
        }
        self.queue.push_back(Job::Command {
            cmd: Command::Fetch,
            quiet: true,
        });
        self.pump(cx);
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.queue.push_back(Job::Undo);
        self.pump(cx);
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.queue.push_back(Job::Redo);
        self.pump(cx);
    }

    pub fn cancel_running(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = self.busy.as_ref().and_then(|b| b.cancel.as_ref()) {
            c.cancel();
        }
        cx.notify();
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let Some(job) = self.queue.pop_front() else {
            return;
        };
        let path = self.path.clone();
        match job {
            Job::Command { cmd, quiet } => {
                let cancel = Cancel::new();
                self.busy = Some(Busy {
                    label: if quiet {
                        format!("自动 {}", cmd.label())
                    } else {
                        cmd.label()
                    },
                    cancel: cmd.is_network().then(|| cancel.clone()),
                    started: Instant::now(),
                    quiet,
                });
                let c = cmd.clone();
                let task = cx.background_spawn(async move { command::execute(&path, &c, &cancel) });
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    let _ = this.update(cx, |this, cx| this.finish_command(cmd, result, cx));
                })
                .detach();
            }
            Job::Undo | Job::Redo => {
                let is_undo = matches!(job, Job::Undo);
                let entry = if is_undo {
                    self.undo.take_undo()
                } else {
                    self.undo.take_redo()
                };
                let Some(entry) = entry else {
                    let msg = if is_undo {
                        "没有可撤销的操作"
                    } else {
                        "没有可重做的操作"
                    };
                    self.notice(NoticeKind::Info, msg, cx);
                    return self.pump(cx);
                };
                self.busy = Some(Busy {
                    label: format!("{}:{}", if is_undo { "撤销" } else { "重做" }, entry.label),
                    cancel: None,
                    started: Instant::now(),
                    quiet: false,
                });
                let e = entry.clone();
                let task = cx.background_spawn(async move {
                    let repo = Repo::open(&path)?;
                    if is_undo {
                        undo::undo(&repo, &e)
                    } else {
                        undo::redo(&repo, &e)
                    }
                });
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    let _ =
                        this.update(cx, |this, cx| this.finish_undo(entry, is_undo, result, cx));
                })
                .detach();
            }
        }
        cx.notify();
    }

    fn finish_command(
        &mut self,
        cmd: Command,
        result: gitcore::Result<Option<UndoEntry>>,
        cx: &mut Context<Self>,
    ) {
        let (elapsed, quiet) = self
            .busy
            .take()
            .map(|b| (b.started.elapsed(), b.quiet))
            .unwrap_or_default();
        log::info!("{} 完成,用时 {:?}", cmd.label(), elapsed);
        match result {
            Ok(entry) => {
                if let Some(e) = entry {
                    self.undo.push(e);
                }
                if matches!(cmd, Command::Fetch | Command::Pull) {
                    self.last_fetch = Some(Instant::now());
                }
                if matches!(cmd, Command::Commit { .. }) {
                    cx.emit(RepoEvent::Committed);
                }
                if cmd.is_network() && !quiet {
                    self.notice(NoticeKind::Success, format!("{} 完成", cmd.label()), cx);
                }
            }
            Err(e) if quiet => {
                // 自动 fetch 失败(断网、凭据过期)不弹提示,下个周期再试;记下时间避免每分钟重试
                self.last_fetch = Some(Instant::now());
                log::warn!("自动 fetch 失败:{e}");
            }
            Err(Error::Cancelled) => {
                self.queue.clear();
                self.notice(NoticeKind::Info, format!("{} 已取消", cmd.label()), cx);
            }
            Err(e) => {
                // 一个命令失败后不再继续执行排队的命令,避免连锁错误
                self.queue.clear();
                self.notice(NoticeKind::Error, e.to_string(), cx);
            }
        }
        self.refresh(cmd.touches_history(), cx);
        self.pump(cx);
        cx.notify();
    }

    fn finish_undo(
        &mut self,
        entry: UndoEntry,
        is_undo: bool,
        result: gitcore::Result<()>,
        cx: &mut Context<Self>,
    ) {
        self.busy = None;
        match result {
            Ok(()) => {
                let msg = format!(
                    "已{}:{}",
                    if is_undo { "撤销" } else { "重做" },
                    entry.label
                );
                if is_undo {
                    self.undo.did_undo(entry);
                } else {
                    self.undo.did_redo(entry);
                }
                self.notice(NoticeKind::Success, msg, cx);
            }
            Err(e) => {
                if is_undo {
                    self.undo.restore_undo(entry);
                } else {
                    self.undo.restore_redo(entry);
                }
                self.queue.clear();
                self.notice(NoticeKind::Error, format!("撤销失败:{e}"), cx);
            }
        }
        self.refresh(true, cx);
        self.pump(cx);
        cx.notify();
    }
}

fn load_full(path: &Path, limit: usize) -> gitcore::Result<(RepoData, RepoStatus)> {
    let start = Instant::now();
    let mut repo = Repo::open(path)?;
    let history = repo.history(limit)?;
    let graph = graph::layout(&history.parent_rows);
    let refs = repo.refs()?;
    let badges = refs.by_target();
    let status = repo.status()?;
    let load_time = start.elapsed();
    log::info!("加载 {} 个提交用时 {:?}", history.commits.len(), load_time);
    Ok((
        RepoData {
            history,
            graph,
            refs,
            badges,
            load_time,
        },
        status,
    ))
}

/// 后台计算 diff 的语法高亮;二进制、超大 diff 或不认识的语言返回 `None`。
fn load_syntax(repo_path: &Path, target: DiffTarget, file: &FileDiff) -> Option<DiffSyntax> {
    if file.binary || file.hunks.is_empty() || file.line_count() > LARGE_DIFF_LINES {
        return None;
    }
    let started = Instant::now();
    let repo = Repo::open(repo_path).ok()?;
    let (old, new) = side_texts(&repo, target, file, SYNTAX_MAX_FILE_BYTES);
    let syntax = highlight_diff(file, old.as_deref(), new.as_deref());
    log::debug!("高亮 {} 用时 {:?}", file.path, started.elapsed());
    syntax
}

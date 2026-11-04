//! 窗口级状态:多仓库标签与设置。

use crate::repo_model::RepoModel;
use crate::settings::{Settings, ThemePreference};
use gitcore::config::{ConfigChange, ConfigFile, GlobalConfig};
use gitcore::{Git, GitVersion};
use gpui::{AppContext as _, Context, Entity, Task};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 自动 fetch 的检查周期;每个仓库是否到期由它自己的上次 fetch 时间决定。
const AUTO_FETCH_TICK: Duration = Duration::from_secs(60);

pub struct Workspace {
    pub repos: Vec<Entity<RepoModel>>,
    pub active: usize,
    pub settings: Settings,
    /// 启动时检测到的 git 问题(未安装或版本过旧)。
    pub git_warning: Option<String>,
    /// 全局 git 配置,打开设置窗口时加载;`None` 表示还没加载完。
    pub git_config: Option<GlobalConfig>,
    /// 读写全局 git 配置失败的原因,显示在设置窗口里。
    pub git_config_error: Option<String>,
    /// 全局 git 配置文件;ephemeral 模式下为 `None`,测试不会碰用户的 `~/.gitconfig`。
    git_config_file: Option<ConfigFile>,
    /// 测试用:不写设置文件、不监听文件变化。
    ephemeral: bool,
    _auto_fetch: Option<Task<()>>,
}

impl Workspace {
    /// 正式启动:读取本地设置并恢复上次打开的仓库。
    pub fn load(cx: &mut Context<Self>) -> Self {
        Self::new(Settings::load(), false, cx)
    }

    /// `ephemeral` 为 true 时不持久化设置、不启动文件监听(用于测试)。
    pub fn new(settings: Settings, ephemeral: bool, cx: &mut Context<Self>) -> Self {
        let mut ws = Self {
            repos: Vec::new(),
            active: 0,
            settings,
            git_warning: None,
            git_config: None,
            git_config_error: None,
            git_config_file: (!ephemeral).then(ConfigFile::global),
            ephemeral,
            _auto_fetch: (!ephemeral).then(|| Self::start_auto_fetch(cx)),
        };
        if ws.settings.restore_tabs {
            for path in ws.settings.open_repos.clone() {
                if let Err(e) = ws.open_repo(&path, cx) {
                    log::warn!("恢复仓库 {} 失败:{e}", path.display());
                }
            }
        }
        ws.active = 0;
        ws.check_git(cx);
        ws
    }

    fn start_auto_fetch(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AUTO_FETCH_TICK).await;
                if this
                    .update(cx, |this, cx| this.auto_fetch_tick(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    /// 对每个到期且空闲的仓库发起一次静默 fetch。
    pub fn auto_fetch_tick(&mut self, cx: &mut Context<Self>) {
        let minutes = self.settings.auto_fetch_minutes;
        if minutes == 0 {
            return;
        }
        let interval = Duration::from_secs(u64::from(minutes) * 60);
        for repo in &self.repos {
            repo.update(cx, |r, cx| r.auto_fetch(interval, cx));
        }
    }

    fn check_git(&mut self, cx: &mut Context<Self>) {
        let task = cx.background_spawn(async { Git::version() });
        cx.spawn(async move |this, cx| {
            let warning = match task.await {
                Ok(v) if v < GitVersion::MIN_SUPPORTED => Some(format!(
                    "系统 git 版本 {v} 过旧,建议升级到 {} 以上",
                    GitVersion::MIN_SUPPORTED
                )),
                Ok(_) => None,
                Err(e) => Some(e.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                this.git_warning = warning;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn active_repo(&self) -> Option<&Entity<RepoModel>> {
        self.repos.get(self.active)
    }

    /// 打开仓库;已经打开的直接切换过去。
    pub fn open_repo(&mut self, path: &Path, cx: &mut Context<Self>) -> Result<(), String> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self
            .repos
            .iter()
            .position(|r| r.read(cx).path == canonical || r.read(cx).path == path)
        {
            self.active = i;
            cx.notify();
            return Ok(());
        }
        let repo = gitcore::Repo::open(path).map_err(|e| e.to_string())?;
        let repo_path = repo.workdir().to_path_buf();
        let limit = self.settings.history_limit;
        let context = self.settings.diff_context_lines;
        let watch = !self.ephemeral;
        let entity = cx.new(|cx| RepoModel::new(repo, limit, context, watch, cx));
        self.repos.push(entity);
        self.active = self.repos.len() - 1;
        self.settings.remember_recent(repo_path);
        self.persist(cx);
        cx.notify();
        Ok(())
    }

    /// 某个路径是否已经在标签里打开。
    pub fn is_open(&self, path: &Path, cx: &gpui::App) -> bool {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.repos.iter().any(|r| {
            let p = &r.read(cx).path;
            *p == canonical || p.canonicalize().is_ok_and(|c| c == canonical)
        })
    }

    /// 批量打开(扫描目录后添加)。返回打开失败的路径与原因;最后一个成功打开的仓库成为当前标签。
    pub fn open_repos(
        &mut self,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> Vec<(PathBuf, String)> {
        paths
            .iter()
            .filter_map(|p| self.open_repo(p, cx).err().map(|e| (p.clone(), e)))
            .collect()
    }

    pub fn close_repo(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.repos.len() {
            self.repos.remove(index);
            if self.active >= self.repos.len() {
                self.active = self.repos.len().saturating_sub(1);
            }
            self.persist(cx);
            cx.notify();
        }
    }

    pub fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.repos.len() {
            self.active = index;
            cx.notify();
        }
    }

    pub fn set_theme(&mut self, theme: ThemePreference, cx: &mut Context<Self>) {
        self.update_settings(cx, |s| s.theme = theme);
    }

    pub fn set_split_diff(&mut self, split: bool, cx: &mut Context<Self>) {
        self.update_settings(cx, |s| s.split_diff = split);
    }

    /// 修改设置并保存;影响已打开仓库的项(提交上限、diff 上下文)同步下去。
    pub fn update_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings);
        let (limit, context) = (
            self.settings.history_limit,
            self.settings.diff_context_lines,
        );
        for repo in &self.repos {
            repo.update(cx, |r, cx| {
                r.set_history_limit(limit, cx);
                r.set_diff_context(context, cx);
            });
        }
        self.persist(cx);
        cx.notify();
    }

    /// 测试用:把全局 git 配置指向临时文件。
    pub fn set_git_config_file(&mut self, file: ConfigFile) {
        self.git_config_file = Some(file);
    }

    /// 在后台读取全局 git 配置。
    pub fn load_git_config(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.git_config_file.clone() else {
            self.git_config = Some(GlobalConfig::default());
            return;
        };
        let task = cx.background_spawn(async move { file.load() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(c) => {
                        this.git_config = Some(c);
                        this.git_config_error = None;
                    }
                    Err(e) => this.git_config_error = Some(format!("读取全局 git 配置失败:{e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 修改一项全局 git 配置:界面先按新值显示,后台写入失败时显示错误并重新读取。
    pub fn set_git_config(&mut self, change: ConfigChange, cx: &mut Context<Self>) {
        if let Some(c) = self.git_config.as_mut() {
            match &change {
                ConfigChange::UserName(v) => c.user_name = v.trim().to_string(),
                ConfigChange::UserEmail(v) => c.user_email = v.trim().to_string(),
                ConfigChange::DefaultBranch(v) => c.default_branch = v.trim().to_string(),
                ConfigChange::Pull(m) => c.pull = *m,
                ConfigChange::GpgSign(on) => c.gpg_sign = *on,
            }
        }
        cx.notify();
        let Some(file) = self.git_config_file.clone() else {
            return;
        };
        let task = cx.background_spawn(async move { file.apply(&change) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.git_config_error = None;
                    cx.notify();
                }
                Err(e) => {
                    this.git_config_error = Some(format!("写入全局 git 配置失败:{e}"));
                    this.load_git_config(cx);
                }
            });
        })
        .detach();
    }

    pub fn recent_repos(&self) -> &[PathBuf] {
        &self.settings.recent_repos
    }

    fn persist(&mut self, cx: &mut Context<Self>) {
        if self.ephemeral {
            return;
        }
        self.settings.open_repos = self.repos.iter().map(|r| r.read(cx).path.clone()).collect();
        let settings = self.settings.clone();
        cx.background_spawn(async move { settings.save() }).detach();
    }
}

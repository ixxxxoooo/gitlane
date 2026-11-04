//! 本地设置,存放在 `~/Library/Application Support/Gitlane/settings.json`。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    Light,
    Dark,
    /// 跟随系统外观。
    System,
}

/// “在终端中打开”使用的终端应用。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalApp {
    #[default]
    Terminal,
    #[serde(rename = "iterm")]
    ITerm,
    Warp,
    Ghostty,
}

impl TerminalApp {
    pub const ALL: [TerminalApp; 4] = [
        TerminalApp::Terminal,
        TerminalApp::ITerm,
        TerminalApp::Warp,
        TerminalApp::Ghostty,
    ];

    /// `open -a` 用的应用名。
    pub fn app_name(self) -> &'static str {
        match self {
            TerminalApp::Terminal => "Terminal",
            TerminalApp::ITerm => "iTerm",
            TerminalApp::Warp => "Warp",
            TerminalApp::Ghostty => "Ghostty",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemePreference,
    /// 上次退出时打开的仓库标签,启动时恢复。
    pub open_repos: Vec<PathBuf>,
    pub recent_repos: Vec<PathBuf>,
    /// 提交图最多加载的提交数。
    pub history_limit: usize,
    /// diff 视图默认并排显示。
    pub split_diff: bool,
    /// diff 上下文行数。
    pub diff_context_lines: u32,
    /// 启动时恢复上次打开的仓库标签。
    pub restore_tabs: bool,
    /// 后台自动 fetch 的间隔(分钟),0 表示关闭。
    pub auto_fetch_minutes: u32,
    pub terminal: TerminalApp,
    /// 启动时(之后每 3 小时)检查更新,发现新版本后台下载并校验;重启更新由用户确认。
    pub auto_update_check: bool,
    /// 联网获取作者头像(GitHub 账号头像或 Gravatar),关闭后只显示首字母。
    pub fetch_avatars: bool,
    /// 左侧引用树宽度(逻辑像素),拖动分隔条调整;窗口缩放不改变它。
    pub sidebar_width: f32,
    /// 右侧详情与暂存区宽度。
    pub side_panel_width: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::Light,
            open_repos: Vec::new(),
            recent_repos: Vec::new(),
            history_limit: 100_000,
            split_diff: false,
            diff_context_lines: gitcore::diff::DEFAULT_CONTEXT_LINES,
            restore_tabs: true,
            auto_fetch_minutes: 0,
            terminal: TerminalApp::Terminal,
            auto_update_check: true,
            fetch_avatars: true,
            sidebar_width: 240.,
            side_panel_width: 360.,
        }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library/Application Support/Gitlane")
                .join("settings.json"),
        )
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match serde_json::to_vec_pretty(self) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&path, bytes) {
                    log::warn!("保存设置失败:{e}");
                }
            }
            Err(e) => log::warn!("序列化设置失败:{e}"),
        }
    }

    pub fn remember_recent(&mut self, path: PathBuf) {
        self.recent_repos.retain(|p| *p != path);
        self.recent_repos.insert(0, path);
        self.recent_repos.truncate(20);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_and_missing_fields_are_tolerated() {
        let s: Settings = serde_json::from_str(r#"{"theme":"dark","future_field":1}"#).unwrap();
        assert_eq!(s.theme, ThemePreference::Dark);
        assert_eq!(s.history_limit, 100_000);
    }

    #[test]
    fn settings_file_from_before_the_settings_window_still_loads() {
        let old = r#"{"theme":"light","open_repos":["/a"],"recent_repos":[],"history_limit":5000,"split_diff":true}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.history_limit, 5000);
        assert!(s.split_diff);
        assert!(s.restore_tabs, "旧设置没有这个键,默认恢复标签");
        assert_eq!(s.diff_context_lines, 3);
        assert_eq!(s.auto_fetch_minutes, 0);
        assert_eq!(s.terminal, TerminalApp::Terminal);
        assert!(s.auto_update_check, "旧设置没有这个键,默认自动检查更新");
        assert!(s.fetch_avatars, "旧设置没有这个键,默认联网获取头像");
    }

    #[test]
    fn new_enum_values_round_trip() {
        let s = Settings {
            theme: ThemePreference::System,
            terminal: TerminalApp::ITerm,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""theme":"system""#), "{json}");
        assert!(json.contains(r#""terminal":"iterm""#), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.theme, ThemePreference::System);
        assert_eq!(back.terminal, TerminalApp::ITerm);
    }

    #[test]
    fn recent_list_is_deduplicated() {
        let mut s = Settings::default();
        s.remember_recent("/a".into());
        s.remember_recent("/b".into());
        s.remember_recent("/a".into());
        assert_eq!(
            s.recent_repos,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }
}

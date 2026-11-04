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

/// 配色方案:在亮色 / 暗色之外,整个软件的底色与强调色。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePalette {
    /// 组件库自带的配色。
    #[default]
    Default,
    Indigo,
    Ocean,
    Forest,
    Amber,
    Rose,
    Violet,
    Graphite,
}

impl ThemePalette {
    pub const ALL: [ThemePalette; 8] = [
        ThemePalette::Default,
        ThemePalette::Indigo,
        ThemePalette::Ocean,
        ThemePalette::Forest,
        ThemePalette::Amber,
        ThemePalette::Rose,
        ThemePalette::Violet,
        ThemePalette::Graphite,
    ];

    /// 设置里显示的名称。
    pub fn label(self) -> &'static str {
        match self {
            ThemePalette::Default => "默认",
            ThemePalette::Indigo => "靛蓝",
            ThemePalette::Ocean => "海洋",
            ThemePalette::Forest => "森林",
            ThemePalette::Amber => "琥珀",
            ThemePalette::Rose => "玫瑰",
            ThemePalette::Violet => "紫罗兰",
            ThemePalette::Graphite => "石墨",
        }
    }
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

/// 窗口的显示方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

/// 窗口位置与大小(逻辑像素)。最大化 / 全屏时 `x y width height` 是还原后的位置与大小。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub mode: WindowMode,
}

impl WindowState {
    /// 校验并修正上次保存的窗口:数值非法返回 `None`;大小不小于 `min`;
    /// 窗口与任何一块屏幕的重叠都太小(显示器拔掉、分辨率变了)时返回 `None`,调用方改用默认位置。
    ///
    /// 参数:`displays` 现有屏幕的 `(x, y, 宽, 高)`;`min` 最小 `(宽, 高)`。
    pub fn fit(&self, displays: &[(f32, f32, f32, f32)], min: (f32, f32)) -> Option<Self> {
        let finite = [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite());
        if !finite || self.width <= 0. || self.height <= 0. {
            return None;
        }
        let fixed = Self {
            width: self.width.max(min.0),
            height: self.height.max(min.1),
            ..*self
        };
        // 至少 120x60 的区域落在某块屏幕里,标题栏才够得着
        let visible = displays.iter().any(|&(dx, dy, dw, dh)| {
            let w = (fixed.x + fixed.width).min(dx + dw) - fixed.x.max(dx);
            let h = (fixed.y + fixed.height).min(dy + dh) - fixed.y.max(dy);
            w >= 120. && h >= 60.
        });
        visible.then_some(fixed)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemePreference,
    /// 配色方案;与 `theme`(亮 / 暗 / 跟随系统)组合使用。
    pub palette: ThemePalette,
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
    /// 主窗口上次的位置、大小与显示方式,启动时恢复。
    pub window: Option<WindowState>,
    /// 设置窗口上次的位置与大小。
    pub settings_window: Option<WindowState>,
    /// 上次选中的仓库标签。
    pub active_repo: Option<PathBuf>,
    /// 左侧引用树里收起的分区与分组(如 `远程`、`local:feature`)。
    pub sidebar_collapsed: Vec<String>,
    /// 右栏里收起的面板(`details` / `unstaged` / `staged`)。
    pub side_collapsed: Vec<String>,
    /// diff 是否撑满中间栏。
    pub diff_maximized: bool,
    /// diff 区域占中间栏高度的比例,拖动提交图与 diff 之间的分隔条调整。
    pub diff_ratio: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemePreference::Light,
            palette: ThemePalette::Default,
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
            window: None,
            settings_window: None,
            active_repo: None,
            sidebar_collapsed: Vec::new(),
            side_collapsed: Vec::new(),
            diff_maximized: false,
            diff_ratio: 0.5,
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
        assert_eq!(s.diff_ratio, 0.5, "旧设置没有这个键,diff 默认占一半高度");
        assert_eq!(s.palette, ThemePalette::Default, "旧设置没有配色,用默认");
        assert!(s.window.is_none() && s.active_repo.is_none() && !s.diff_maximized);
        assert!(s.sidebar_collapsed.is_empty() && s.side_collapsed.is_empty());
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

    const SCREEN: [(f32, f32, f32, f32); 1] = [(0., 0., 1920., 1080.)];

    fn win(x: f32, y: f32, w: f32, h: f32) -> WindowState {
        WindowState {
            x,
            y,
            width: w,
            height: h,
            mode: WindowMode::Windowed,
        }
    }

    #[test]
    fn saved_window_is_restored_only_when_reachable() {
        let min = (960., 600.);
        assert_eq!(
            win(100., 80., 1400., 900.).fit(&SCREEN, min),
            Some(win(100., 80., 1400., 900.))
        );
        // 比最小尺寸小:放大到最小
        assert_eq!(
            win(0., 0., 300., 200.).fit(&SCREEN, min),
            Some(win(0., 0., 960., 600.))
        );
        // 显示器拔掉了:窗口整个在屏幕外,丢弃
        assert_eq!(win(3000., 100., 1400., 900.).fit(&SCREEN, min), None);
        // 只露出一个角(不到 120x60)也丢弃;露出足够的部分保留
        assert_eq!(win(1900., 1070., 1400., 900.).fit(&SCREEN, min), None);
        assert!(win(1500., 100., 1400., 900.).fit(&SCREEN, min).is_some());
        // 另一块屏幕上的窗口在多屏下有效
        let two = [(0., 0., 1920., 1080.), (1920., 0., 1920., 1080.)];
        assert!(win(2500., 100., 1400., 900.).fit(&two, min).is_some());
        // 非法数值
        assert_eq!(win(f32::NAN, 0., 1400., 900.).fit(&SCREEN, min), None);
        assert_eq!(win(0., 0., 0., 900.).fit(&SCREEN, min), None);
    }

    #[test]
    fn layout_fields_round_trip() {
        let s = Settings {
            window: Some(WindowState {
                mode: WindowMode::Maximized,
                ..win(10., 20., 1300., 800.)
            }),
            active_repo: Some("/a".into()),
            sidebar_collapsed: vec!["远程".into(), "local:feature".into()],
            side_collapsed: vec!["staged".into()],
            diff_maximized: true,
            ..Settings::default()
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.window, s.window);
        assert_eq!(back.active_repo, s.active_repo);
        assert_eq!(back.sidebar_collapsed, s.sidebar_collapsed);
        assert_eq!(back.side_collapsed, s.side_collapsed);
        assert!(back.diff_maximized);
    }
}

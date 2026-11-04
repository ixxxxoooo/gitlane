//! 全部动作与默认快捷键。菜单、命令面板和快捷键共用同一套动作。

use gpui::{App, KeyBinding, actions};

actions!(
    gitlane,
    [
        Quit,
        OpenRepository,
        ScanDirectory,
        CloseTab,
        NextTab,
        PrevTab,
        CommandPalette,
        Refresh,
        Fetch,
        Pull,
        Push,
        ForcePush,
        NewBranch,
        Stash,
        StashPop,
        Undo,
        Redo,
        Commit,
        StageAll,
        UnstageAll,
        FocusSearch,
        FocusCommitMessage,
        SelectNextCommit,
        SelectPrevCommit,
        ToggleTheme,
        ToggleSplitDiff,
        AbortOperation,
        OpenSettings,
        OpenInTerminal,
        RevealInFinder,
        CheckForUpdates,
        InstallUpdate,
    ]
);

/// 仓库视图的键盘上下文;输入框有自己更深的上下文,所以在输入框里 ⌘Z 仍是文本撤销。
pub const REPO_CONTEXT: &str = "RepoView";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", OpenRepository, None),
        KeyBinding::new("cmd-shift-o", ScanDirectory, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("cmd-k", CommandPalette, None),
        KeyBinding::new("cmd-shift-p", CommandPalette, None),
        KeyBinding::new("cmd-r", Refresh, None),
        KeyBinding::new("cmd-shift-t", ToggleTheme, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-alt-t", OpenInTerminal, None),
        KeyBinding::new("cmd-alt-r", RevealInFinder, None),
        KeyBinding::new("cmd-f", FocusSearch, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-l", FocusCommitMessage, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-enter", Commit, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-z", Undo, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-shift-z", Redo, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-shift-f", Fetch, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-shift-l", Pull, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-shift-k", Push, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-shift-b", NewBranch, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-shift-s", Stash, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-shift-a", StageAll, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-shift-u", UnstageAll, Some(REPO_CONTEXT)),
        KeyBinding::new("cmd-\\", ToggleSplitDiff, Some(REPO_CONTEXT)),
        KeyBinding::new("down", SelectNextCommit, Some("CommitGraph")),
        KeyBinding::new("up", SelectPrevCommit, Some("CommitGraph")),
        KeyBinding::new("j", SelectNextCommit, Some("CommitGraph")),
        KeyBinding::new("k", SelectPrevCommit, Some("CommitGraph")),
    ]);
}

/// 命令面板里可搜索的动作:(名称, 快捷键提示, 动作)。
pub fn palette_entries() -> Vec<(&'static str, &'static str, Box<dyn gpui::Action>)> {
    vec![
        ("Fetch:从所有远程获取", "⌘⇧F", Box::new(Fetch)),
        ("Pull:拉取当前分支", "⌘⇧L", Box::new(Pull)),
        ("Push:推送当前分支", "⌘⇧K", Box::new(Push)),
        ("Force push(--force-with-lease)", "", Box::new(ForcePush)),
        ("新建分支", "⌘⇧B", Box::new(NewBranch)),
        ("Stash:储藏当前改动", "⌘⇧S", Box::new(Stash)),
        ("Stash pop:恢复最近的储藏", "", Box::new(StashPop)),
        ("暂存全部", "⌘⇧A", Box::new(StageAll)),
        ("取消暂存全部", "⌘⇧U", Box::new(UnstageAll)),
        ("提交", "⌘↵", Box::new(Commit)),
        ("撤销上一步操作", "⌘Z", Box::new(Undo)),
        ("重做", "⌘⇧Z", Box::new(Redo)),
        (
            "中止进行中的 merge / rebase / cherry-pick",
            "",
            Box::new(AbortOperation),
        ),
        ("刷新", "⌘R", Box::new(Refresh)),
        ("打开仓库…", "⌘O", Box::new(OpenRepository)),
        ("扫描目录,批量添加仓库…", "⌘⇧O", Box::new(ScanDirectory)),
        ("关闭当前标签", "⌘W", Box::new(CloseTab)),
        ("下一个标签", "⌃⇥", Box::new(NextTab)),
        ("切换亮色 / 暗色主题", "⌘⇧T", Box::new(ToggleTheme)),
        ("设置…", "⌘,", Box::new(OpenSettings)),
        ("在终端中打开当前仓库", "⌥⌘T", Box::new(OpenInTerminal)),
        ("在 Finder 中显示当前仓库", "⌥⌘R", Box::new(RevealInFinder)),
        ("切换统一 / 并排 diff", "⌘\\", Box::new(ToggleSplitDiff)),
        ("搜索提交", "⌘F", Box::new(FocusSearch)),
        ("聚焦提交信息", "⌘L", Box::new(FocusCommitMessage)),
        ("检查更新…", "", Box::new(CheckForUpdates)),
    ]
}

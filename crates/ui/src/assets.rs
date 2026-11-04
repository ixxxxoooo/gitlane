//! 应用资源:gpui-kit 默认图标集 + 额外用到的 Lucide 图标。

use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

gpui::assets::icon_assets!(
    ExtraIcons,
    [
        Archive,
        ArrowDownToLine,
        ArrowUpFromLine,
        Briefcase,
        Clock,
        File,
        FolderGit2,
        FileArchive,
        FileBraces,
        FileCode,
        FileCog,
        FileDiff,
        FileImage,
        FileLock,
        FileSpreadsheet,
        FileStack,
        FileTerminal,
        FileText,
        GitBranch,
        GitCommitHorizontal,
        GitGraph,
        Hash,
        Laptop,
        Maximize2,
        Minimize2,
        Minus,
        Plus,
        Sparkles,
        Tag,
    ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

//! 按文件名 / 扩展名选图标与颜色,用于改动文件列表和 diff 标题栏。
//! Lucide 里没有各语言的品牌图标,所以同类图标靠颜色区分语言。

use crate::colors::file_tint;
use gpui::component::{Icon, Theme};
use gpui::prelude::*;

/// 文件类别:决定图标形状和颜色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Rust,
    TypeScript,
    JavaScript,
    Vue,
    Python,
    Go,
    /// C / C++ / C# / Java / Kotlin / Swift 等编译型语言。
    Native,
    Markup,
    Style,
    Json,
    Config,
    Doc,
    Image,
    Shell,
    Lock,
    Sheet,
    Archive,
    Sql,
    Diff,
    Other,
}

impl FileKind {
    /// 按路径判断类别。参数:`path` 仓库内路径或文件名(不区分大小写)。
    pub fn of(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
        // 先看整名:没有扩展名,或扩展名说明不了用途的文件
        match name.as_str() {
            "cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" => {
                return Self::Lock;
            }
            "dockerfile" | "makefile" | ".gitignore" | ".gitattributes" | ".editorconfig" => {
                return Self::Config;
            }
            "license" | "readme" => return Self::Doc,
            _ => {}
        }
        match name.rsplit_once('.').map_or("", |(_, e)| e) {
            "rs" => Self::Rust,
            "ts" | "tsx" => Self::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "vue" => Self::Vue,
            "py" => Self::Python,
            "go" => Self::Go,
            "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "java" | "kt" | "swift" | "rb" | "php"
            | "lua" => Self::Native,
            "html" | "htm" => Self::Markup,
            "css" | "scss" | "less" => Self::Style,
            "json" | "jsonc" | "json5" => Self::Json,
            "toml" | "yml" | "yaml" | "ini" | "conf" | "cfg" | "env" | "plist" => Self::Config,
            "md" | "mdx" | "txt" | "rst" | "log" => Self::Doc,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "icns" | "bmp" => Self::Image,
            "sh" | "zsh" | "bash" | "fish" => Self::Shell,
            "lock" => Self::Lock,
            "csv" | "tsv" | "xlsx" => Self::Sheet,
            "zip" | "tar" | "gz" | "tgz" | "dmg" | "7z" => Self::Archive,
            "sql" | "db" | "sqlite" => Self::Sql,
            "diff" | "patch" => Self::Diff,
            _ => Self::Other,
        }
    }

    fn icon_name(self) -> gpui::assets::IconName {
        use gpui::assets::IconName as I;
        match self {
            Self::Rust
            | Self::TypeScript
            | Self::JavaScript
            | Self::Vue
            | Self::Python
            | Self::Go
            | Self::Native
            | Self::Markup
            | Self::Style => I::FileCode,
            Self::Json => I::FileBraces,
            Self::Config => I::FileCog,
            Self::Doc => I::FileText,
            Self::Image => I::FileImage,
            Self::Shell => I::FileTerminal,
            Self::Lock => I::FileLock,
            Self::Sheet => I::FileSpreadsheet,
            Self::Archive => I::FileArchive,
            Self::Sql => I::FileStack,
            Self::Diff => I::FileDiff,
            Self::Other => I::File,
        }
    }

    /// 图标色相(度);`None` 表示不上色,用主题的弱化文字色。
    fn hue(self) -> Option<f32> {
        Some(match self {
            Self::Rust => 22.,
            Self::TypeScript => 210.,
            Self::JavaScript => 50.,
            Self::Vue => 150.,
            Self::Python => 195.,
            Self::Go => 180.,
            Self::Native => 280.,
            Self::Markup => 12.,
            Self::Style => 262.,
            Self::Json => 40.,
            Self::Config => 235.,
            Self::Image => 320.,
            Self::Shell => 135.,
            Self::Sql => 170.,
            Self::Sheet => 140.,
            Self::Archive => 30.,
            Self::Diff => 200.,
            Self::Doc | Self::Lock | Self::Other => return None,
        })
    }
}

/// 文件对应的图标(形状 + 颜色)。参数:`path` 仓库内路径或文件名;`theme` 当前主题。
pub fn file_icon(path: &str, theme: &Theme) -> Icon {
    let kind = FileKind::of(path);
    let color = kind.hue().map_or(theme.muted_foreground, |h| {
        file_tint(h, theme.mode.is_dark())
    });
    Icon::from(kind.icon_name()).text_color(color)
}

#[cfg(test)]
mod tests {
    use super::FileKind;

    #[test]
    fn maps_paths_to_kinds() {
        let cases = [
            ("src/main.rs", FileKind::Rust),
            ("a/b/App.tsx", FileKind::TypeScript),
            ("x.mjs", FileKind::JavaScript),
            ("Home.vue", FileKind::Vue),
            ("t.py", FileKind::Python),
            ("main.go", FileKind::Go),
            ("a.cpp", FileKind::Native),
            ("index.html", FileKind::Markup),
            ("a.scss", FileKind::Style),
            ("package.json", FileKind::Json),
            (".github/workflows/ci.yml", FileKind::Config),
            ("Cargo.toml", FileKind::Config),
            ("README.md", FileKind::Doc),
            ("x.PNG", FileKind::Image),
            ("run.sh", FileKind::Shell),
            ("Cargo.lock", FileKind::Lock),
            ("a.csv", FileKind::Sheet),
            ("a.dmg", FileKind::Archive),
            ("q.sql", FileKind::Sql),
            ("x.patch", FileKind::Diff),
            ("Dockerfile", FileKind::Config),
            ("a/b/noext", FileKind::Other),
        ];
        for (path, kind) in cases {
            assert_eq!(FileKind::of(path), kind, "{path}");
        }
    }
}

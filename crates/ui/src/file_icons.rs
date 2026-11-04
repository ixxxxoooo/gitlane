//! 按文件名 / 扩展名选图标,用于改动文件列表和 diff 标题栏。

use gpui::component::Icon;

/// 文件对应的图标。参数:`path` 仓库内路径或文件名。返回:Lucide 文件类图标,认不出的用通用文件图标。
pub fn file_icon(path: &str) -> Icon {
    use gpui::assets::IconName as I;
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    // 先看整名(没有扩展名或扩展名不能说明用途的文件)
    let by_name = match name.as_str() {
        "cargo.lock" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" => Some(I::FileLock),
        "dockerfile" | "makefile" | ".gitignore" | ".gitattributes" | ".editorconfig" => {
            Some(I::FileCog)
        }
        "license" | "readme" => Some(I::FileText),
        _ => None,
    };
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    let icon = by_name.unwrap_or(match ext {
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "vue" | "py" | "go" | "java" | "kt"
        | "swift" | "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "rb" | "php" | "lua" | "html"
        | "css" | "scss" | "less" => I::FileCode,
        "json" | "jsonc" | "json5" => I::FileBraces,
        "toml" | "yml" | "yaml" | "ini" | "conf" | "cfg" | "env" | "plist" => I::FileCog,
        "md" | "mdx" | "txt" | "rst" | "log" => I::FileText,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "icns" | "bmp" => I::FileImage,
        "sh" | "zsh" | "bash" | "fish" => I::FileTerminal,
        "lock" => I::FileLock,
        "csv" | "tsv" | "xlsx" => I::FileSpreadsheet,
        "zip" | "tar" | "gz" | "tgz" | "dmg" | "7z" => I::FileArchive,
        "sql" | "db" | "sqlite" => I::FileStack,
        "diff" | "patch" => I::FileDiff,
        _ => I::File,
    });
    Icon::from(icon)
}

#[cfg(test)]
mod tests {
    use super::file_icon;

    #[test]
    fn known_and_unknown_files_get_an_icon() {
        // 不比较具体图标(Icon 没有相等性),只保证各类路径都不会 panic
        for p in [
            "src/main.rs",
            "Cargo.lock",
            ".github/workflows/ci.yml",
            "README.md",
            "a/b/noext",
            "Dockerfile",
            "x.PNG",
        ] {
            let _ = file_icon(p);
        }
    }
}

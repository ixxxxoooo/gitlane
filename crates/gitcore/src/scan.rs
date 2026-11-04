//! 扫描目录下的 Git 仓库,供“扫描目录”批量添加使用。

use std::path::{Path, PathBuf};

/// 默认最多向下找几层目录。
pub const DEFAULT_MAX_DEPTH: usize = 4;

/// 这些目录里不会有用户自己的仓库,且往往文件极多,直接跳过。
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "build",
    "dist",
    "vendor",
    "Pods",
    "DerivedData",
];

/// 递归查找 `root` 下的 Git 工作区(含 `.git` 目录或 `.git` 文件的目录)。
///
/// - 找到仓库后不再进入它的子目录(子模块、嵌套仓库不单独列出);
/// - 跳过隐藏目录、符号链接与 `SKIP_DIRS`;
/// - `root` 本身是仓库时只返回它自己。
///
/// 结果按路径排序。读不了的目录静默跳过。
pub fn find_repos(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, 0, max_depth, &mut found);
    found.sort();
    found
}

fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

fn walk(dir: &Path, depth: usize, max_depth: usize, found: &mut Vec<PathBuf>) {
    if is_repo(dir) {
        found.push(dir.to_path_buf());
        return;
    }
    if depth >= max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ty) = entry.file_type() else { continue };
        if !ty.is_dir() || ty.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }
        walk(&entry.path(), depth + 1, max_depth, found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn mkrepo(p: &Path) {
        fs::create_dir_all(p.join(".git")).unwrap();
    }

    #[test]
    fn finds_nested_repos_and_stops_at_repo_roots() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        mkrepo(&r.join("a"));
        mkrepo(&r.join("group/b"));
        mkrepo(&r.join("a/sub")); // 仓库内部的嵌套仓库不列出
        mkrepo(&r.join("node_modules/pkg"));
        mkrepo(&r.join(".hidden/c"));
        mkrepo(&r.join("x/y/z/w/too-deep"));
        fs::create_dir_all(r.join("plain/dir")).unwrap();
        // 链接式 worktree:.git 是文件
        fs::create_dir_all(r.join("wt")).unwrap();
        fs::write(r.join("wt/.git"), "gitdir: /elsewhere").unwrap();

        let found: Vec<String> = find_repos(r, DEFAULT_MAX_DEPTH)
            .iter()
            .map(|p| p.strip_prefix(r).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(found, vec!["a", "group/b", "wt"]);
    }

    #[test]
    fn root_itself_is_a_repo() {
        let t = tempfile::tempdir().unwrap();
        mkrepo(t.path());
        mkrepo(&t.path().join("inner"));
        assert_eq!(find_repos(t.path(), 3), vec![t.path().to_path_buf()]);
    }

    #[test]
    fn missing_root_returns_empty() {
        assert!(find_repos(Path::new("/definitely/not/here"), 3).is_empty());
    }
}

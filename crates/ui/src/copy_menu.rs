//! 右键菜单里的“复制”项:分支名、文件路径、diff 行文字都走同一个 `CopyText` 动作。

use crate::actions::CopyText;
use gpui::component::menu::{PopupMenu, PopupMenuItem};

/// 一个“复制 `text`”的菜单项,点击后写入剪贴板并提示“已复制”。
pub fn copy_item(label: &'static str, text: String) -> PopupMenuItem {
    PopupMenuItem::new(label)
        .on_click(move |_, window, cx| window.dispatch_action(Box::new(CopyText(text.clone())), cx))
}

/// 文件的复制菜单:路径(相对仓库)、文件名、绝对路径。
///
/// 参数:`rel` 仓库内相对路径;`abs` 绝对路径(仓库目录拼上 `rel`)。
pub fn file_menu(menu: PopupMenu, rel: &str, abs: String) -> PopupMenu {
    let name = rel.rsplit('/').next().unwrap_or(rel).to_string();
    menu.item(copy_item("复制路径", rel.to_string()))
        .item(copy_item("复制文件名", name))
        .item(copy_item("复制绝对路径", abs))
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_name_is_the_last_path_segment() {
        // 与 file_menu 里取文件名的写法一致
        let name = |rel: &str| rel.rsplit('/').next().unwrap_or(rel).to_string();
        assert_eq!(name("crates/ui/src/main.rs"), "main.rs");
        assert_eq!(name("README.md"), "README.md");
    }
}

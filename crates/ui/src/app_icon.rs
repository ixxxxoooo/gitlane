//! Dock 图标跟随界面主题切换浅色 / 深色版。
//!
//! 打包的 `.icns` 只有浅色版,运行中由这里覆盖;开发实例(裸二进制)没有包图标,也靠这里显示。
//! 图标源文件与生成脚本在仓库根目录的 `assets/icon/`。

#[cfg(target_os = "macos")]
mod mac {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    use std::cell::Cell;

    const ICON_LIGHT: &[u8] = include_bytes!("../../../assets/icon/app-light.png");
    const ICON_DARK: &[u8] = include_bytes!("../../../assets/icon/app-dark.png");

    thread_local! {
        /// 当前 Dock 图标是否深色,避免重复设置。
        static DOCK_DARK: Cell<Option<bool>> = const { Cell::new(None) };
    }

    fn image(bytes: &[u8]) -> Option<Retained<NSImage>> {
        use objc2::AnyThread;
        let data = NSData::with_bytes(bytes);
        NSImage::initWithData(NSImage::alloc(), &data)
    }

    pub fn set_dock_icon(dark: bool) {
        // 测试跑的是无头窗口,不碰真实的 NSApplication
        if cfg!(test) {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if DOCK_DARK.get() == Some(dark) {
            return;
        }
        if let Some(img) = image(if dark { ICON_DARK } else { ICON_LIGHT }) {
            let app = NSApplication::sharedApplication(mtm);
            // SAFETY: 在主线程调用,图片在调用期间有效,AppKit 会自行持有
            unsafe { app.setApplicationIconImage(Some(&img)) };
            DOCK_DARK.set(Some(dark));
        }
    }
}

/// 按主题切换 Dock 图标。
pub fn set_dock_icon(dark: bool) {
    #[cfg(target_os = "macos")]
    mac::set_dock_icon(dark);
    #[cfg(not(target_os = "macos"))]
    let _ = dark;
}

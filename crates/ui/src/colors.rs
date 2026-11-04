//! 提交图与 diff 的颜色。基础颜色来自 gpui-component 主题,这里只补充专用颜色。

use gpui::{Hsla, hsla};

/// 泳道调色板:亮暗主题下都清晰可辨的 8 种颜色。
pub fn lane_color(index: u16, dark: bool) -> Hsla {
    const HUES: [f32; 8] = [210., 145., 30., 280., 0., 180., 50., 320.];
    let h = HUES[index as usize % HUES.len()] / 360.;
    if dark {
        hsla(h, 0.65, 0.62, 1.)
    } else {
        hsla(h, 0.7, 0.45, 1.)
    }
}

/// 文件类型图标的颜色:给定色相(度),亮暗主题下都清晰,饱和度适中不抢文字。
pub fn file_tint(hue: f32, dark: bool) -> Hsla {
    let h = hue / 360.;
    if dark {
        hsla(h, 0.55, 0.65, 1.)
    } else {
        hsla(h, 0.6, 0.42, 1.)
    }
}

pub struct DiffColors {
    pub added_bg: Hsla,
    pub added_word: Hsla,
    pub deleted_bg: Hsla,
    pub deleted_word: Hsla,
    /// 改动行行号栏,比行底色略深,扫一眼就能看出改动范围。
    pub added_gutter: Hsla,
    pub deleted_gutter: Hsla,
    pub added_sign: Hsla,
    pub deleted_sign: Hsla,
    pub hunk_bg: Hsla,
    pub hunk_fg: Hsla,
    pub selected_bg: Hsla,
    pub added_mark: Hsla,
    pub deleted_mark: Hsla,
}

fn hex(rgb: u32) -> Hsla {
    gpui::rgb(rgb).into()
}

fn hexa(rgb: u32, alpha: f32) -> Hsla {
    hex(rgb).opacity(alpha)
}

/// 亮色取自参考截图(浅粉 / 浅绿行底,字符级改动用更深的同色),暗色是同一色相的半透明版本。
pub fn diff_colors(dark: bool) -> DiffColors {
    if dark {
        DiffColors {
            added_bg: hexa(0x2ea043, 0.15),
            added_word: hexa(0x2ea043, 0.40),
            deleted_bg: hexa(0xf85149, 0.15),
            deleted_word: hexa(0xf85149, 0.40),
            added_gutter: hexa(0x2ea043, 0.25),
            deleted_gutter: hexa(0xf85149, 0.25),
            added_sign: hex(0x56d364),
            deleted_sign: hex(0xf47067),
            hunk_bg: hexa(0x388bfd, 0.12),
            hunk_fg: hex(0x8b949e),
            selected_bg: hexa(0x388bfd, 0.35),
            added_mark: hex(0x3fb950),
            deleted_mark: hex(0xf85149),
        }
    } else {
        DiffColors {
            added_bg: hex(0xe8f6ef),
            added_word: hex(0xc0e6d4),
            deleted_bg: hex(0xfbe8e6),
            deleted_word: hex(0xf4c1b8),
            added_gutter: hex(0xd3eee0),
            deleted_gutter: hex(0xf8d5d0),
            added_sign: hex(0x1a7f37),
            deleted_sign: hex(0xcf222e),
            hunk_bg: hex(0xeef4fb),
            hunk_fg: hex(0x57606a),
            selected_bg: hexa(0x0969da, 0.22),
            added_mark: hex(0x2da44e),
            deleted_mark: hex(0xe5534b),
        }
    }
}

/// 语法高亮名(见 `gitcore::highlight`)对应的文字颜色,`None` 用默认前景色。
/// 亮色贴近参考截图(VS Code Light+:蓝色关键字、暗红字符串、绿色注释),暗色对应 Dark+。
pub fn syntax_color(name: &str, dark: bool) -> Option<Hsla> {
    let rgb = match (name, dark) {
        ("keyword" | "boolean" | "variable.special", false) => 0x0000ff,
        ("keyword" | "boolean" | "variable.special", true) => 0x569cd6,
        ("string" | "text.literal", false) => 0xa31515,
        ("string" | "text.literal", true) => 0xce9178,
        ("string.escape", false) => 0xee0000,
        ("string.escape", true) => 0xd7ba7d,
        ("string.regex", false) => 0x811f3f,
        ("string.regex", true) => 0xd16969,
        ("comment", false) => 0x008000,
        ("comment", true) => 0x6a9955,
        ("number", false) => 0x098658,
        ("number", true) => 0xb5cea8,
        ("constant", false) => 0x0070c1,
        ("constant", true) => 0x4fc1ff,
        ("type", false) => 0x267f99,
        ("type", true) => 0x4ec9b0,
        ("function", false) => 0x795e26,
        ("function", true) => 0xdcdcaa,
        ("tag" | "title", false) => 0x800000,
        ("tag" | "title", true) => 0x569cd6,
        ("attribute", false) => 0xe50000,
        ("attribute", true) => 0x9cdcfe,
        ("link_uri", false) => 0x0451a5,
        ("link_uri", true) => 0x3794ff,
        _ => return None,
    };
    Some(hex(rgb))
}

/// 头像底色:按名字散列到泳道色,同一作者颜色固定。
pub fn avatar_color(name: &str, dark: bool) -> Hsla {
    let h = name
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    lane_color((h % 8) as u16, dark)
}

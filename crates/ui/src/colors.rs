//! 提交图与 diff 的颜色。基础颜色来自 gpui-component 主题,这里只补充专用颜色。

use gpui::{Hsla, Rgba, hsla};

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

/// 在 `base` 上叠 `over`,`t` 是 `over` 的占比(0..1),结果不透明。
/// 列表高亮用它算实色,避免半透明色盖住提交图里“空心节点”的底色。
pub fn mix(base: Hsla, over: Hsla, t: f32) -> Hsla {
    let (a, b) = (Rgba::from(base), Rgba::from(over));
    Hsla::from(Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.,
    })
}

/// 标签徽章的 (背景, 文字, 边框):柔和的琥珀色,不用高饱和的纯黄。
pub fn tag_badge(dark: bool) -> (Hsla, Hsla, Hsla) {
    let hue = 34. / 360.;
    if dark {
        (
            hsla(hue, 0.8, 0.55, 0.16),
            hsla(hue, 0.85, 0.72, 1.),
            hsla(hue, 0.8, 0.55, 0.38),
        )
    } else {
        (
            hsla(hue, 0.9, 0.5, 0.12),
            hsla(hue, 0.85, 0.3, 1.),
            hsla(hue, 0.85, 0.45, 0.35),
        )
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

fn hex(rgb: u32) -> Hsla {
    gpui::rgb(rgb).into()
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

/// diff 改动行的配色:亮色是浅粉 / 浅绿整行底色,只在改动不多的行里用更深的同色标出改动字符;
/// 暗色是同一色相的半透明版本。
pub fn diff_palette(dark: bool) -> diffview::DiffPalette {
    let a = |rgb: u32, alpha: f32| hex(rgb).opacity(alpha);
    if dark {
        diffview::DiffPalette {
            added_line: a(0x2ea043, 0.15),
            deleted_line: a(0xf85149, 0.15),
            added_word: a(0x2ea043, 0.40),
            deleted_word: a(0xf85149, 0.40),
            added_sign: hex(0x56d364),
            deleted_sign: hex(0xf47067),
        }
    } else {
        diffview::DiffPalette {
            added_line: hex(0xe8f6ef),
            deleted_line: hex(0xfbe8e6),
            added_word: hex(0xc0e6d4),
            deleted_word: hex(0xf4c1b8),
            added_sign: hex(0x1a7f37),
            deleted_sign: hex(0xcf222e),
        }
    }
}

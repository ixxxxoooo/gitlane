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

pub struct DiffColors {
    pub added_bg: Hsla,
    pub added_word: Hsla,
    pub deleted_bg: Hsla,
    pub deleted_word: Hsla,
    pub hunk_bg: Hsla,
    pub selected_bg: Hsla,
    pub added_mark: Hsla,
    pub deleted_mark: Hsla,
}

pub fn diff_colors(dark: bool) -> DiffColors {
    if dark {
        DiffColors {
            added_bg: hsla(140. / 360., 0.45, 0.22, 0.55),
            added_word: hsla(140. / 360., 0.55, 0.35, 0.8),
            deleted_bg: hsla(0., 0.45, 0.25, 0.55),
            deleted_word: hsla(0., 0.6, 0.4, 0.8),
            hunk_bg: hsla(210. / 360., 0.3, 0.25, 0.5),
            selected_bg: hsla(210. / 360., 0.8, 0.5, 0.35),
            added_mark: hsla(140. / 360., 0.6, 0.5, 1.),
            deleted_mark: hsla(0., 0.7, 0.6, 1.),
        }
    } else {
        DiffColors {
            added_bg: hsla(140. / 360., 0.6, 0.9, 1.),
            added_word: hsla(140. / 360., 0.6, 0.75, 1.),
            deleted_bg: hsla(0., 0.8, 0.94, 1.),
            deleted_word: hsla(0., 0.8, 0.85, 1.),
            hunk_bg: hsla(210. / 360., 0.6, 0.95, 1.),
            selected_bg: hsla(210. / 360., 0.9, 0.6, 0.3),
            added_mark: hsla(140. / 360., 0.6, 0.4, 1.),
            deleted_mark: hsla(0., 0.7, 0.5, 1.),
        }
    }
}

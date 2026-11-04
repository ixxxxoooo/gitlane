//! 配色方案:在组件库的亮色 / 暗色主题之上,按“色相 + 饱和度”生成整套底色与强调色。
//! 每个方案都同时有亮色与暗色两套;`apply` 要在 `Theme::change` 之后调用(`change` 会把颜色重置回注册的主题)。

use crate::colors::mix;
use app::ThemePalette;
use gpui::component::Theme;
use gpui::{App, Hsla, hsla};

/// 一个方案的参数:底色色相与饱和度、强调色色相与饱和度(色相 0..360,饱和度 0..1)。
struct Spec {
    surface_hue: f32,
    surface_sat: f32,
    primary_hue: f32,
    primary_sat: f32,
}

fn spec(palette: ThemePalette) -> Option<Spec> {
    let (sh, ss, ph, ps) = match palette {
        ThemePalette::Default => return None,
        ThemePalette::Indigo => (232., 0.30, 234., 0.62),
        ThemePalette::Ocean => (196., 0.34, 190., 0.68),
        ThemePalette::Forest => (150., 0.22, 146., 0.46),
        ThemePalette::Amber => (36., 0.40, 30., 0.80),
        ThemePalette::Rose => (345., 0.28, 342., 0.66),
        ThemePalette::Violet => (268., 0.28, 266., 0.56),
        // 石墨:底色几乎是中性灰,只有强调色是蓝
        ThemePalette::Graphite => (220., 0.07, 212., 0.66),
    };
    Some(Spec {
        surface_hue: sh,
        surface_sat: ss,
        primary_hue: ph,
        primary_sat: ps,
    })
}

fn hs(h: f32, s: f32, l: f32) -> Hsla {
    hsla(h / 360., s.clamp(0., 1.), l, 1.)
}

/// 把方案套到当前全局主题上(亮 / 暗取当前模式)。所有方案——包括“默认”——的列表悬停与选中
/// 底色都比组件库默认更醒目。
pub fn apply(palette: ThemePalette, cx: &mut App) {
    Theme::update(cx, |theme| {
        let dark = theme.mode.is_dark();
        if let Some(sp) = spec(palette) {
            set_surfaces(&mut theme.colors, &sp, dark);
        }
        let c = &mut theme.colors;
        let (hover, active) = if dark { (0.16, 0.28) } else { (0.11, 0.20) };
        c.list_hover = mix(c.background, c.primary, hover);
        c.list_active = mix(c.background, c.primary, active);
        c.list_active_border = c.primary;
    });
}

/// 设置底色、边框、文字与强调色。
fn set_surfaces(c: &mut gpui::component::ThemeColor, sp: &Spec, dark: bool) {
    let (sh, ss, ph, ps) = (
        sp.surface_hue,
        sp.surface_sat,
        sp.primary_hue,
        sp.primary_sat,
    );
    // 由浅到深(亮色)/ 由深到浅(暗色)的几层底色
    let (bg, side, bar, panel, hover, border, fg, muted_fg, popover) = if dark {
        (
            hs(sh, ss * 0.8, 0.105),
            hs(sh, ss * 0.85, 0.125),
            hs(sh, ss * 0.9, 0.145),
            hs(sh, ss, 0.17),
            hs(sh, ss, 0.2),
            hs(sh, ss, 0.23),
            hs(sh, 0.12, 0.92),
            hs(sh, 0.1, 0.62),
            hs(sh, ss * 0.9, 0.135),
        )
    } else {
        (
            hs(sh, ss * 0.5, 0.99),
            hs(sh, ss * 0.85, 0.96),
            hs(sh, ss * 0.9, 0.945),
            hs(sh, ss, 0.925),
            hs(sh, ss, 0.9),
            hs(sh, ss, 0.86),
            hs(sh, 0.25, 0.14),
            hs(sh, 0.12, 0.42),
            hs(sh, ss * 0.4, 0.995),
        )
    };
    let (primary, primary_hover, primary_active) = if dark {
        (hs(ph, ps, 0.62), hs(ph, ps, 0.67), hs(ph, ps, 0.57))
    } else {
        (hs(ph, ps, 0.48), hs(ph, ps, 0.43), hs(ph, ps, 0.38))
    };
    let on_primary = if dark {
        hs(ph, 0.3, 0.08)
    } else {
        hs(0., 0., 1.)
    };

    c.background = bg;
    c.foreground = fg;
    c.muted = panel;
    c.muted_foreground = muted_fg;
    c.border = border;
    c.input = border;
    c.popover = popover;
    c.popover_foreground = fg;
    c.secondary = panel;
    c.secondary_hover = hover;
    c.secondary_active = border;
    c.secondary_foreground = fg;
    c.accent = mix(bg, primary, 0.14);
    c.accent_foreground = fg;
    c.sidebar = side;
    c.sidebar_foreground = fg;
    c.sidebar_border = border;
    c.sidebar_accent = mix(side, primary, 0.16);
    c.sidebar_accent_foreground = fg;
    c.title_bar = bar;
    c.title_bar_border = border;
    c.tab_bar = bar;
    c.tab = bar;
    c.tab_active = bg;
    c.tab_active_foreground = fg;
    c.tab_foreground = muted_fg;
    c.list = bg;
    c.list_even = bg;
    c.list_head = side;
    c.primary = primary;
    c.primary_hover = primary_hover;
    c.primary_active = primary_active;
    c.primary_foreground = on_primary;
    c.button_primary = primary;
    c.button_primary_hover = primary_hover;
    c.button_primary_active = primary_active;
    c.button_primary_foreground = on_primary;
    c.ring = primary;
    c.link = primary;
    c.link_hover = primary_hover;
    c.link_active = primary_active;
    c.selection = hsla(ph / 360., ps, if dark { 0.62 } else { 0.48 }, 0.28);
}

#[cfg(test)]
mod tests {
    use super::apply;
    use app::ThemePalette;
    use gpui::TestAppContext;
    use gpui::component::{ActiveTheme as _, Theme, ThemeMode};

    #[gpui::test]
    fn every_palette_applies_in_both_modes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui::component::init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                let mut seen = Vec::new();
                for palette in ThemePalette::ALL {
                    Theme::change(mode, None, cx);
                    apply(palette, cx);
                    let t = cx.theme();
                    // 悬停与选中底色不同,且都和背景有区别
                    assert_ne!(t.list_hover, t.background, "{palette:?} {mode:?}");
                    assert_ne!(t.list_active, t.list_hover, "{palette:?} {mode:?}");
                    seen.push((t.background, t.primary));
                }
                // 各个方案的底色 / 强调色互不相同(默认以外都有自己的配色)
                for i in 1..seen.len() {
                    for j in 0..i {
                        assert_ne!(seen[i], seen[j], "{mode:?} 第 {i} 个与第 {j} 个方案相同");
                    }
                }
            }
        });
    }
}

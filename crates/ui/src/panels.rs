//! 左右栏宽度:拖动分隔条手动调整,窗口缩放时保持不变(只有中间区域伸缩)。
//!
//! 用户设定的宽度存在设置里;窗口太窄放不下时只在显示上临时收窄,窗口变宽后自动恢复,
//! 设定值本身不变。

use gpui::prelude::*;
use gpui::{CursorStyle, Div, EmptyView, Hsla, Stateful, div, px};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelSide {
    /// 左侧引用树。
    Left,
    /// 右侧详情与暂存区。
    Right,
}

/// 提交图与 diff 之间的横向分隔条拖拽数据。
#[derive(Clone, Copy, Debug)]
pub struct DiffDrag;

/// diff 区域高度占中间栏的比例范围:上下都至少留出一部分。
pub const DIFF_RATIO_RANGE: (f32, f32) = (0.15, 0.85);
pub const DIFF_RATIO_DEFAULT: f32 = 0.5;

/// 把比例限制在合理范围内;非法值(NaN、旧设置里的 0)回到默认值。
pub fn clamp_diff_ratio(ratio: f32) -> f32 {
    if !ratio.is_finite() || ratio <= 0. {
        return DIFF_RATIO_DEFAULT;
    }
    ratio.clamp(DIFF_RATIO_RANGE.0, DIFF_RATIO_RANGE.1)
}

/// 拖动时由鼠标位置算出 diff 的高度比例。`from_bottom` 是鼠标到中间栏底边的距离,`total` 是中间栏高度。
pub fn dragged_diff_ratio(from_bottom: f32, total: f32) -> f32 {
    if total <= 0. {
        return DIFF_RATIO_DEFAULT;
    }
    clamp_diff_ratio(from_bottom / total)
}

/// 横向分隔条:一条细线,悬停或拖动时高亮。线贴在下边缘,紧挨下面有底色的 diff 标题栏,
/// 不会在两者之间留出一条空隙而显得是两条线。
pub fn row_handle(id: &'static str, line: Hsla, hover: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(HANDLE_WIDTH))
        .w_full()
        .flex_none()
        .flex()
        .flex_col()
        .justify_end()
        .cursor(CursorStyle::ResizeRow)
        .group(id)
        .child(
            div()
                .h(px(1.))
                .w_full()
                .bg(line)
                .group_hover(id, |s| s.h(px(2.)).bg(hover)),
        )
        .on_drag(DiffDrag, |_, _, _, cx| cx.new(|_| EmptyView))
}

/// 拖动中的分隔条,作为 GPUI 拖拽数据。
#[derive(Clone, Copy, Debug)]
pub struct PanelDrag(pub PanelSide);

pub const LEFT_DEFAULT: f32 = 240.;
pub const RIGHT_DEFAULT: f32 = 360.;
const LEFT_RANGE: (f32, f32) = (160., 520.);
const RIGHT_RANGE: (f32, f32) = (260., 720.);
/// 中间提交图至少保留的宽度。
pub const MIN_CENTER: f32 = 360.;
/// 分隔条的可拖动宽度。
pub const HANDLE_WIDTH: f32 = 5.;

impl PanelSide {
    pub fn default_width(self) -> f32 {
        match self {
            PanelSide::Left => LEFT_DEFAULT,
            PanelSide::Right => RIGHT_DEFAULT,
        }
    }

    /// 把设定宽度限制在合理范围内;非法值(NaN、旧设置里的 0)回到默认值。
    pub fn clamp(self, width: f32) -> f32 {
        let (min, max) = match self {
            PanelSide::Left => LEFT_RANGE,
            PanelSide::Right => RIGHT_RANGE,
        };
        if !width.is_finite() || width <= 0. {
            return self.default_width();
        }
        width.clamp(min, max)
    }

    fn min(self) -> f32 {
        match self {
            PanelSide::Left => LEFT_RANGE.0,
            PanelSide::Right => RIGHT_RANGE.0,
        }
    }
}

/// 根据总宽度算出实际显示的左右栏宽度。
///
/// 放得下时原样返回设定值(窗口缩放不影响左右栏);放不下时先收窄右栏、再收窄左栏,
/// 各自不小于最小宽度。
pub fn layout(total: f32, left: f32, right: f32) -> (f32, f32) {
    let (mut l, mut r) = (PanelSide::Left.clamp(left), PanelSide::Right.clamp(right));
    let handles = HANDLE_WIDTH * 2.;
    let mut overflow = l + r + handles + MIN_CENTER - total;
    if overflow > 0. {
        let cut = overflow.min(r - PanelSide::Right.min());
        r -= cut;
        overflow -= cut;
    }
    if overflow > 0. {
        let cut = overflow.min(l - PanelSide::Left.min());
        l -= cut;
    }
    (l, r)
}

/// 拖动时由鼠标位置算出新的设定宽度。`offset` 是鼠标到容器左边(Left)或右边(Right)的距离,
/// `other` 是另一侧当前显示的宽度,保证中间区域不被挤到 `MIN_CENTER` 以下。
pub fn dragged_width(side: PanelSide, offset: f32, total: f32, other: f32) -> f32 {
    let room = total - other - HANDLE_WIDTH * 2. - MIN_CENTER;
    side.clamp(offset.min(room.max(side.min())))
}

/// 竖向分隔条:一条细线,悬停或拖动时高亮。双击恢复默认宽度由调用方处理。
///
/// `fill` 是分隔条自身留白区域的底色:左侧栏的底色和中间区域不同,留白不填同色就会在侧栏边缘
/// 和细线之间多出一道空隙,看起来像两条线;所以左侧栏的分隔条填侧栏底色、线贴在右边缘。
pub fn handle(
    id: &'static str,
    side: PanelSide,
    line: Hsla,
    hover: Hsla,
    fill: Option<Hsla>,
) -> Stateful<Div> {
    let at_end = side == PanelSide::Left;
    div()
        .id(id)
        .w(px(HANDLE_WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .when(at_end, |d| d.justify_end())
        .when(!at_end, |d| d.justify_center())
        .when_some(fill, |d, c| d.bg(c))
        .cursor(CursorStyle::ResizeColumn)
        .group(id)
        .child(
            div()
                .w(px(1.))
                .h_full()
                .bg(line)
                .group_hover(id, |s| s.w(px(2.)).bg(hover)),
        )
        .on_drag(PanelDrag(side), |_, _, _, cx| cx.new(|_| EmptyView))
}

#[cfg(test)]
mod tests {
    use super::{
        LEFT_DEFAULT, MIN_CENTER, PanelSide, RIGHT_DEFAULT, clamp_diff_ratio, dragged_diff_ratio,
        dragged_width, layout,
    };

    #[test]
    fn widths_stay_fixed_while_window_resizes() {
        // 窗口从 1400 拉到 2400,左右栏都不变,多出来的都给中间
        assert_eq!(layout(1400., 300., 400.), (300., 400.));
        assert_eq!(layout(2400., 300., 400.), (300., 400.));
    }

    #[test]
    fn narrow_window_shrinks_right_first_then_left_but_keeps_setting() {
        // 960 宽:300 + 400 + 10 + 360 = 1070,超出 110,先从右栏扣
        assert_eq!(layout(960., 300., 400.), (300., 290.));
        // 更窄:右栏到最小 260 后再扣左栏
        let (l, r) = layout(800., 300., 400.);
        assert_eq!(r, 260.);
        assert_eq!(l, 800. - 260. - 10. - MIN_CENTER);
        // 极窄时两边都停在最小值
        assert_eq!(layout(300., 300., 400.), (160., 260.));
    }

    #[test]
    fn invalid_settings_fall_back_to_defaults() {
        assert_eq!(PanelSide::Left.clamp(0.), LEFT_DEFAULT);
        assert_eq!(PanelSide::Right.clamp(f32::NAN), RIGHT_DEFAULT);
        assert_eq!(PanelSide::Left.clamp(10_000.), 520.);
    }

    #[test]
    fn dragging_respects_range_and_center_room() {
        assert_eq!(dragged_width(PanelSide::Left, 300., 1600., 360.), 300.);
        assert_eq!(dragged_width(PanelSide::Left, 50., 1600., 360.), 160.);
        // 中间至少留 MIN_CENTER:1200 - 360 - 10 - 360 = 470
        assert_eq!(dragged_width(PanelSide::Left, 500., 1200., 360.), 470.);
        assert_eq!(dragged_width(PanelSide::Right, 900., 2400., 240.), 720.);
    }

    #[test]
    fn diff_ratio_is_clamped_and_follows_the_mouse() {
        assert_eq!(clamp_diff_ratio(0.), 0.5);
        assert_eq!(clamp_diff_ratio(f32::NAN), 0.5);
        assert_eq!(clamp_diff_ratio(0.01), 0.15);
        assert_eq!(clamp_diff_ratio(0.99), 0.85);
        // 鼠标离底边 300px、中间栏 1000px 高:diff 占 30%
        assert_eq!(dragged_diff_ratio(300., 1000.), 0.3);
        assert_eq!(dragged_diff_ratio(990., 1000.), 0.85);
        assert_eq!(dragged_diff_ratio(5., 0.), 0.5);
    }
}

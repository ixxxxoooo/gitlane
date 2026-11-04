//! 提交图单元格绘制:每行一个 canvas,只画这一行的线段和节点。
//! 布局数据由后台线程预先算好(graph crate),这里只做坐标换算,滚动时不分配大块内存。

use crate::colors::lane_color;
use gpui::{
    App, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Point, Styled, Window, canvas, point, px,
    quad,
};
use graph::{Node, Segment, SegmentKind};

pub const LANE_WIDTH: f32 = 14.;
const NODE_RADIUS: f32 = 4.;
const LINE_WIDTH: f32 = 1.6;

pub fn graph_width(columns: u16) -> Pixels {
    px(LANE_WIDTH * columns.max(1) as f32 + 6.)
}

/// `head` 为 true 时节点画得更大,标出 HEAD 所在提交。
pub fn graph_cell(
    segments: Vec<Segment>,
    node: Node,
    columns: u16,
    dark: bool,
    background: Hsla,
    head: bool,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            let x = |col: u16| bounds.origin.x + px(3. + LANE_WIDTH * col as f32 + LANE_WIDTH / 2.);
            let top = bounds.origin.y;
            let mid = bounds.origin.y + bounds.size.height / 2.;
            let bottom = bounds.origin.y + bounds.size.height;

            for s in &segments {
                let color = lane_color(s.color, dark);
                let (from, to): (Point<Pixels>, Point<Pixels>) = match s.kind {
                    SegmentKind::Through => (point(x(s.from), top), point(x(s.from), bottom)),
                    SegmentKind::Top => (point(x(s.from), top), point(x(s.to), mid)),
                    SegmentKind::Bottom => (point(x(s.from), mid), point(x(s.to), bottom)),
                };
                let mut b = PathBuilder::stroke(px(LINE_WIDTH));
                b.move_to(from);
                if from.x == to.x {
                    b.line_to(to);
                } else {
                    // 斜线用曲线:在起点所在列先竖直走,再弯向目标列
                    let ctrl = match s.kind {
                        SegmentKind::Top => point(from.x, to.y),
                        _ => point(to.x, from.y),
                    };
                    b.curve_to(to, ctrl);
                }
                if let Ok(path) = b.build() {
                    window.paint_path(path, color);
                }
            }

            let r = px(if head { NODE_RADIUS + 1.5 } else { NODE_RADIUS });
            let c = point(x(node.column), mid);
            let color = lane_color(node.color, dark);
            let node_bounds = Bounds::new(point(c.x - r, c.y - r), gpui::size(r * 2., r * 2.));
            // 合并提交画空心点
            let fill = if node.is_merge { background } else { color };
            window.paint_quad(quad(
                node_bounds,
                r,
                fill,
                px(if node.is_merge || head { 2. } else { 0. }),
                if head && !node.is_merge {
                    background
                } else {
                    color
                },
                Default::default(),
            ));
            if head && !node.is_merge {
                // HEAD:实心点外加一圈
                let rr = r + px(2.);
                window.paint_quad(quad(
                    Bounds::new(point(c.x - rr, c.y - rr), gpui::size(rr * 2., rr * 2.)),
                    rr,
                    gpui::transparent_black(),
                    px(1.5),
                    color,
                    Default::default(),
                ));
            }
        },
    )
    .w(graph_width(columns))
    .h_full()
}

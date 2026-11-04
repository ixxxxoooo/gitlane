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

/// 虚线的线段长度与间隔(像素)。
const DASH: f32 = 3.;
const GAP: f32 = 3.;

/// 把 `y0..y1` 切成虚线的 (起点, 终点) 段,`y1 <= y0` 时没有线段。
pub fn dash_spans(y0: f32, y1: f32) -> Vec<(f32, f32)> {
    let mut spans = Vec::new();
    let mut y = y0;
    while y < y1 {
        spans.push((y, (y + DASH).min(y1)));
        y += DASH + GAP;
    }
    spans
}

/// 画一条竖直虚线。
fn paint_dashed_vline(window: &mut Window, x: Pixels, y0: Pixels, y1: Pixels, color: Hsla) {
    for (a, b) in dash_spans(f32::from(y0), f32::from(y1)) {
        let mut p = PathBuilder::stroke(px(LINE_WIDTH));
        p.move_to(point(x, px(a)));
        p.line_to(point(x, px(b)));
        if let Ok(path) = p.build() {
            window.paint_path(path, color);
        }
    }
}

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
    link_up: bool,
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
            // 上面紧挨着“工作区改动”行时,用虚线把它和 HEAD 节点连起来
            if link_up {
                paint_dashed_vline(window, c.x, top, c.y - r - px(1.), color.opacity(0.7));
            }
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

/// “工作区改动”行的图单元格:在 HEAD 所在列画虚线圆点节点,并从节点向下画虚线,
/// 与下一行 HEAD 提交的节点连起来。
///
/// 参数:`column`、`color` 是 HEAD 节点的列与颜色序号;`columns` 决定单元格宽度。
pub fn working_cell(column: u16, color: u16, columns: u16, dark: bool) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            let cx = bounds.origin.x + px(3. + LANE_WIDTH * column as f32 + LANE_WIDTH / 2.);
            let mid = bounds.origin.y + bounds.size.height / 2.;
            let bottom = bounds.origin.y + bounds.size.height;
            let line = lane_color(color, dark);
            // 虚线圆:一圈小圆点
            let ring = NODE_RADIUS + 1.;
            let dots = 10;
            for k in 0..dots {
                let a = std::f32::consts::TAU * k as f32 / dots as f32;
                let d = point(cx + px(ring * a.cos()), mid + px(ring * a.sin()));
                let r = px(1.1);
                window.paint_quad(quad(
                    Bounds::new(point(d.x - r, d.y - r), gpui::size(r * 2., r * 2.)),
                    r,
                    line,
                    px(0.),
                    line,
                    Default::default(),
                ));
            }
            paint_dashed_vline(window, cx, mid + px(ring + 2.), bottom, line.opacity(0.7));
        },
    )
    .w(graph_width(columns))
    .h_full()
}

#[cfg(test)]
mod tests {
    use super::dash_spans;

    #[test]
    fn dashes_cover_the_range_with_gaps() {
        assert!(dash_spans(10., 10.).is_empty());
        assert!(dash_spans(10., 5.).is_empty());
        // 0..12:段 0-3、6-9,下一段从 12 开始,不再画
        assert_eq!(dash_spans(0., 12.), vec![(0., 3.), (6., 9.)]);
        // 末尾不足一段时截到终点
        assert_eq!(dash_spans(0., 7.), vec![(0., 3.), (6., 7.)]);
    }
}

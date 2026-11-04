//! 提交图泳道布局。
//!
//! 输入是按拓扑顺序(子提交在前)排列的提交,每个提交给出父提交在同一列表中的下标;
//! 输出是每一行的节点列和需要绘制的线段。纯函数、不依赖 git 与 UI,方便基准测试。
//!
//! 每一行在纵向上分成上下两半:
//! - 上半段:从行顶部的某一列连到行中点的某一列(来自子提交的线);
//! - 下半段:从行中点连到行底部(去往父提交的线);
//! - 贯穿段:与本行节点无关的泳道,从顶到底竖直穿过。
//!
//! 泳道一旦分配就不平移,空出的槽位会被后续分支复用,这样滚动时线条不会左右跳动。

use std::ops::Range;

/// 线段在一行内的位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SegmentKind {
    /// 行顶 `from` 列 → 行中点 `to` 列。
    Top,
    /// 行中点 `from` 列 → 行底 `to` 列。
    Bottom,
    /// 行顶到行底,`from == to`。
    Through,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Segment {
    pub kind: SegmentKind,
    pub from: u16,
    pub to: u16,
    /// 颜色序号,由调用方映射到调色板。
    pub color: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub column: u16,
    pub color: u16,
    /// 是否为合并提交(多于一个父提交),UI 可以画成空心点。
    pub is_merge: bool,
}

#[derive(Clone, Copy, Debug)]
struct Lane {
    /// 这条泳道等待的提交下标。
    target: u32,
    color: u16,
}

/// 布局结果。线段按行连续存放,`row_segments(i)` 取第 i 行。
#[derive(Clone, Debug, Default)]
pub struct GraphLayout {
    nodes: Vec<Node>,
    segments: Vec<Segment>,
    row_offsets: Vec<u32>,
    max_columns: u16,
}

impl GraphLayout {
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, row: usize) -> Node {
        self.nodes[row]
    }

    pub fn row_segments(&self, row: usize) -> &[Segment] {
        let start = self.row_offsets[row] as usize;
        let end = self
            .row_offsets
            .get(row + 1)
            .map_or(self.segments.len(), |e| *e as usize);
        &self.segments[start..end]
    }

    /// 图中出现过的最大列数,用于决定图区宽度。
    pub fn max_columns(&self) -> u16 {
        self.max_columns
    }

    /// 某个可见范围内的最大列数,用于按可见区收窄图宽。
    pub fn columns_in(&self, rows: Range<usize>) -> u16 {
        let rows = rows.start.min(self.len())..rows.end.min(self.len());
        rows.map(|r| {
            let seg_max = self
                .row_segments(r)
                .iter()
                .map(|s| s.from.max(s.to))
                .max()
                .unwrap_or(0);
            seg_max.max(self.nodes[r].column) + 1
        })
        .max()
        .unwrap_or(0)
    }
}

/// 增量布局器:可以分批 `push`,适合在后台线程边读提交边布局。
#[derive(Default)]
pub struct GraphBuilder {
    lanes: Vec<Option<Lane>>,
    next_color: u16,
    layout: GraphLayout,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加下一行。`row` 必须等于已经 push 的行数,`parents` 中的下标必须大于 `row`;
    /// 不在列表中的父提交(例如被截断的历史)直接传 `u32::MAX` 或省略即可。
    pub fn push(&mut self, parents: &[u32]) {
        let row = self.layout.nodes.len() as u32;
        self.layout
            .row_offsets
            .push(self.layout.segments.len() as u32);

        // 1. 找出所有在等本提交的泳道;第一个作为节点列,其余在上半段汇入。
        let mut node_col: Option<usize> = None;
        let mut node_color = 0;
        for (col, lane) in self.lanes.iter_mut().enumerate() {
            let Some(l) = lane else { continue };
            if l.target != row {
                continue;
            }
            match node_col {
                None => {
                    node_col = Some(col);
                    node_color = l.color;
                    self.layout.segments.push(Segment {
                        kind: SegmentKind::Top,
                        from: col as u16,
                        to: col as u16,
                        color: l.color,
                    });
                }
                Some(nc) => {
                    self.layout.segments.push(Segment {
                        kind: SegmentKind::Top,
                        from: col as u16,
                        to: nc as u16,
                        color: l.color,
                    });
                    *lane = None;
                }
            }
        }
        let node_col = match node_col {
            Some(c) => c,
            None => {
                // 分支头:没有子提交指向它,分配新泳道。
                node_color = self.alloc_color();
                self.alloc_lane(Lane {
                    target: row,
                    color: node_color,
                })
            }
        };

        // 2. 与本行无关的泳道竖直贯穿。
        for (col, lane) in self.lanes.iter().enumerate() {
            if col == node_col {
                continue;
            }
            if let Some(l) = lane {
                self.layout.segments.push(Segment {
                    kind: SegmentKind::Through,
                    from: col as u16,
                    to: col as u16,
                    color: l.color,
                });
            }
        }

        // 3. 父提交:第一个父提交总是沿用节点列,保证主线竖直不漂移;如果别的泳道也在等它,
        //    两条线会在父提交那一行汇合。其余父提交优先连到已有泳道,否则在右侧新开泳道。
        let parents: Vec<u32> = parents.iter().copied().filter(|p| *p != u32::MAX).collect();
        self.lanes[node_col] = None;
        for (i, &parent) in parents.iter().enumerate() {
            debug_assert!(parent > row, "父提交必须排在子提交之后");
            let (col, color) = if i == 0 {
                self.lanes[node_col] = Some(Lane {
                    target: parent,
                    color: node_color,
                });
                (node_col, node_color)
            } else if let Some(col) = self
                .lanes
                .iter()
                .position(|l| l.is_some_and(|l| l.target == parent))
            {
                (col, self.lanes[col].unwrap().color)
            } else {
                let color = self.alloc_color();
                let col = self.alloc_lane_after(
                    Lane {
                        target: parent,
                        color,
                    },
                    node_col,
                );
                (col, color)
            };
            self.layout.segments.push(Segment {
                kind: SegmentKind::Bottom,
                from: node_col as u16,
                to: col as u16,
                color,
            });
        }

        while self.lanes.last().is_some_and(|l| l.is_none()) {
            self.lanes.pop();
        }

        let width = self.lanes.len().max(node_col + 1) as u16;
        self.layout.max_columns = self.layout.max_columns.max(width);
        self.layout.nodes.push(Node {
            column: node_col as u16,
            color: node_color,
            is_merge: parents.len() > 1,
        });
    }

    pub fn layout(&self) -> &GraphLayout {
        &self.layout
    }

    pub fn finish(self) -> GraphLayout {
        self.layout
    }

    fn alloc_color(&mut self) -> u16 {
        let c = self.next_color;
        self.next_color = self.next_color.wrapping_add(1);
        c
    }

    fn alloc_lane(&mut self, lane: Lane) -> usize {
        if let Some(col) = self.lanes.iter().position(|l| l.is_none()) {
            self.lanes[col] = Some(lane);
            col
        } else {
            self.lanes.push(Some(lane));
            self.lanes.len() - 1
        }
    }

    /// 合并进来的分支尽量放在节点右侧,让主线保持在左边。
    fn alloc_lane_after(&mut self, lane: Lane, after: usize) -> usize {
        if let Some(off) = self.lanes[after + 1..].iter().position(|l| l.is_none()) {
            let col = after + 1 + off;
            self.lanes[col] = Some(lane);
            col
        } else {
            self.lanes.push(Some(lane));
            self.lanes.len() - 1
        }
    }
}

/// 一次性布局全部提交。
pub fn layout<P: AsRef<[u32]>>(parents: &[P]) -> GraphLayout {
    let mut b = GraphBuilder::new();
    for p in parents {
        b.push(p.as_ref());
    }
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use SegmentKind::*;

    fn seg(kind: SegmentKind, from: u16, to: u16) -> (SegmentKind, u16, u16) {
        (kind, from, to)
    }

    fn shape(l: &GraphLayout, row: usize) -> Vec<(SegmentKind, u16, u16)> {
        l.row_segments(row)
            .iter()
            .map(|s| (s.kind, s.from, s.to))
            .collect()
    }

    #[test]
    fn linear_history_stays_in_one_column() {
        let l = layout(&[vec![1], vec![2], vec![]]);
        assert_eq!(l.len(), 3);
        for r in 0..3 {
            assert_eq!(l.node(r).column, 0);
        }
        assert_eq!(shape(&l, 0), vec![seg(Bottom, 0, 0)]);
        assert_eq!(shape(&l, 1), vec![seg(Top, 0, 0), seg(Bottom, 0, 0)]);
        assert_eq!(shape(&l, 2), vec![seg(Top, 0, 0)]);
        assert_eq!(l.max_columns(), 1);
    }

    #[test]
    fn merge_opens_lane_on_the_right_and_closes_at_fork_point() {
        // 0: merge(1, 2)   1: main   2: feature   3: base
        let l = layout(&[vec![1, 2], vec![3], vec![3], vec![]]);
        assert!(l.node(0).is_merge);
        assert_eq!(shape(&l, 0), vec![seg(Bottom, 0, 0), seg(Bottom, 0, 1)]);
        assert_eq!(l.node(1).column, 0);
        assert_eq!(
            shape(&l, 1),
            vec![seg(Top, 0, 0), seg(Through, 1, 1), seg(Bottom, 0, 0)]
        );
        assert_eq!(l.node(2).column, 1);
        // feature 沿自己的泳道向下,在分叉点(第 3 行)汇入主线
        assert_eq!(
            shape(&l, 2),
            vec![seg(Top, 1, 1), seg(Through, 0, 0), seg(Bottom, 1, 1)]
        );
        assert_eq!(l.node(3).column, 0);
        assert_eq!(shape(&l, 3), vec![seg(Top, 0, 0), seg(Top, 1, 0)]);
        assert_eq!(l.max_columns(), 2);
    }

    #[test]
    fn two_branch_heads_with_common_parent() {
        // 0: a -> 2   1: b -> 2   2: root
        let l = layout(&[vec![2], vec![2], vec![]]);
        assert_eq!(l.node(0).column, 0);
        assert_eq!(l.node(1).column, 1);
        assert_eq!(shape(&l, 1), vec![seg(Through, 0, 0), seg(Bottom, 1, 1)]);
        assert_eq!(l.node(2).column, 0);
        assert_eq!(shape(&l, 2), vec![seg(Top, 0, 0), seg(Top, 1, 0)]);
    }

    #[test]
    fn freed_lanes_are_reused() {
        // 两个先后结束的侧分支应复用同一列
        // 0: m1(1,2) 1: x->3 2: s1->3 3: m2(4,5) 4: y->6 5: s2->6 6: root
        let l = layout(&[
            vec![1, 2],
            vec![3],
            vec![3],
            vec![4, 5],
            vec![6],
            vec![6],
            vec![],
        ]);
        assert_eq!(l.node(2).column, 1);
        assert_eq!(l.node(5).column, 1);
        assert_eq!(l.max_columns(), 2);
    }

    #[test]
    fn mainline_does_not_drift_after_many_merges() {
        // 反复从主线分出又合回的历史,列数应保持常数
        let mut parents = Vec::new();
        for i in 0..300u32 {
            if i % 3 == 0 {
                parents.push(vec![i + 1, i + 2]);
            } else {
                parents.push(vec![i + 1]);
            }
        }
        parents.push(vec![]);
        let l = layout(&parents);
        assert!(l.max_columns() <= 2, "max_columns = {}", l.max_columns());
        assert!((0..l.len()).step_by(3).all(|r| l.node(r).column == 0));
    }

    #[test]
    fn missing_parents_are_ignored() {
        let l = layout(&[vec![u32::MAX]]);
        assert_eq!(l.len(), 1);
        assert!(l.row_segments(0).is_empty());
    }

    #[test]
    fn incremental_push_matches_batch() {
        let parents = vec![vec![1, 2], vec![3], vec![3], vec![]];
        let batch = layout(&parents);
        let mut b = GraphBuilder::new();
        for p in &parents {
            b.push(p);
        }
        let inc = b.finish();
        for r in 0..parents.len() {
            assert_eq!(batch.row_segments(r), inc.row_segments(r));
            assert_eq!(batch.node(r), inc.node(r));
        }
    }

    #[test]
    fn columns_in_visible_range() {
        let l = layout(&[vec![1, 2], vec![3], vec![3], vec![]]);
        assert_eq!(l.columns_in(0..4), 2);
        // 第 3 行有从第 1 列汇入的线
        assert_eq!(l.columns_in(3..4), 2);
        assert_eq!(layout(&[vec![]]).columns_in(0..1), 1);
        assert_eq!(l.columns_in(10..20), 0);
    }
}

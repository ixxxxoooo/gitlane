//! 提交图布局基准:生成带分支与合并的合成历史,测量全量布局耗时。
//!
//! 运行:`cargo run -p graph --release --example bench_layout -- 50000`

use std::time::Instant;

fn synthetic_history(n: u32) -> Vec<Vec<u32>> {
    // 每 7 个提交从主线分出一个侧分支,侧分支长 1–5 个提交后合并回主线。
    let mut parents = Vec::with_capacity(n as usize);
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for i in 0..n {
        let next = i + 1;
        if next >= n {
            parents.push(vec![]);
        } else if i % 7 == 0 && i + 8 < n {
            let side = next + 1 + (rand() % 5) as u32;
            parents.push(vec![next, side.min(n - 1)]);
        } else {
            parents.push(vec![next]);
        }
    }
    parents
}

fn main() {
    let n: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_000);
    let history = synthetic_history(n);
    let runs = 5;
    let mut best = f64::MAX;
    let mut layout = graph::GraphLayout::default();
    for _ in 0..runs {
        let t = Instant::now();
        layout = graph::layout(&history);
        best = best.min(t.elapsed().as_secs_f64());
    }
    let segs: usize = (0..layout.len())
        .map(|r| layout.row_segments(r).len())
        .sum();
    println!(
        "{n} 个提交:最佳 {:.2} ms({runs} 次),最大列数 {},线段 {segs}",
        best * 1000.0,
        layout.max_columns()
    );
}

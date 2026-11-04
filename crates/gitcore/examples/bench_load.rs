//! 读路径分段计时:`cargo run -p gitcore --release --example bench_load -- /path/to/repo`

use std::time::Instant;

fn rss_mb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .unwrap_or(0)
        / 1024
}

fn main() {
    let path = std::env::args().nth(1).expect("用法:bench_load <仓库路径>");
    println!("启动 RSS {} MB", rss_mb());
    if std::env::var_os("NO_CACHE").is_some() {
        git2::opts::enable_caching(false);
    }
    let t = Instant::now();
    let mut repo = gitcore::Repo::open(&path).unwrap();
    println!("open     {:>8.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let h = repo.history(100_000).unwrap();
    println!(
        "history  {:>8.1} ms({} 个提交)",
        t.elapsed().as_secs_f64() * 1e3,
        h.commits.len()
    );
    let t = Instant::now();
    let refs = repo.refs().unwrap();
    println!(
        "refs     {:>8.1} ms({} 个引用)",
        t.elapsed().as_secs_f64() * 1e3,
        refs.refs.len()
    );
    let t = Instant::now();
    let st = repo.status().unwrap();
    println!(
        "status   {:>8.1} ms({} 个条目)",
        t.elapsed().as_secs_f64() * 1e3,
        st.entries.len()
    );
    println!("加载后 RSS {} MB", rss_mb());
    drop(repo);
    println!("关闭仓库后 RSS {} MB(历史数据仍在内存)", rss_mb());
    drop(h);
}

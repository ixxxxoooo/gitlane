//! 头像逻辑的单元测试:只用临时目录,不联网(命中缓存或标记时 `fetch` 不会发请求)。
//!
//! @author ixxxxoooo

use super::*;

#[test]
fn github_noreply_emails_give_the_login() {
    assert_eq!(
        github_login("12345+Octo-Cat@users.noreply.github.com").as_deref(),
        Some("octo-cat")
    );
    assert_eq!(
        github_login("octocat@users.noreply.github.com").as_deref(),
        Some("octocat")
    );
    assert_eq!(github_login("me@example.com"), None);
    assert_eq!(github_login("@users.noreply.github.com"), None);
    assert_eq!(github_login("a b@users.noreply.github.com"), None);
}

#[test]
fn candidates_prefer_github_then_gravatar() {
    let urls = candidate_urls("1+octo@users.noreply.github.com");
    assert_eq!(urls[0], "https://github.com/octo.png?size=64");
    assert!(urls[1].starts_with("https://gravatar.com/avatar/") && urls[1].ends_with("&d=404"));
    // 普通邮箱只有 Gravatar;大小写与首尾空格不影响哈希
    let a = candidate_urls("Me@Example.com ");
    let b = candidate_urls("me@example.com");
    assert_eq!(a.len(), 1);
    assert_eq!(a, b);
}

#[test]
fn cache_hits_and_negative_markers_skip_the_network() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    assert_eq!(cached(dir, "x@example.com"), None, "没有缓存");

    let key = sha256("x@example.com").unwrap();
    std::fs::write(dir.join(format!("{key}.img")), b"png").unwrap();
    let hit = cached(dir, "X@Example.com").unwrap().unwrap();
    assert_eq!(fetch(dir, "x@example.com"), Some(hit), "命中缓存不联网");

    let none_key = sha256("y@example.com").unwrap();
    std::fs::write(dir.join(format!("{none_key}.none")), b"").unwrap();
    assert_eq!(cached(dir, "y@example.com"), Some(None), "负缓存期内不重试");
    assert_eq!(fetch(dir, "y@example.com"), None);
}

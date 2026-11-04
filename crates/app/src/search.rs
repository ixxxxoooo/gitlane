//! 提交搜索:按提交信息、作者或 SHA 前缀匹配,不区分大小写。

use gitcore::CommitInfo;

pub fn matches(commit: &CommitInfo, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let q = q.to_lowercase();
    if q.len() >= 4
        && q.chars().all(|c| c.is_ascii_hexdigit())
        && commit.id.to_string().starts_with(&q)
    {
        return true;
    }
    commit.summary.to_lowercase().contains(&q)
        || commit.author.to_lowercase().contains(&q)
        || commit.email.to_lowercase().contains(&q)
}

/// 返回匹配的行号。
pub fn search(commits: &[CommitInfo], query: &str) -> Vec<usize> {
    commits
        .iter()
        .enumerate()
        .filter(|(_, c)| matches(c, query))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitcore::Oid;

    fn c(summary: &str, author: &str, sha: &str) -> CommitInfo {
        CommitInfo {
            id: Oid::from_str(sha).unwrap(),
            parents: vec![],
            author: author.into(),
            email: format!("{author}@example.com"),
            time: 0,
            tz_offset: 0,
            summary: summary.into(),
        }
    }

    #[test]
    fn search_by_summary_author_sha() {
        let list = vec![
            c(
                "修复登录",
                "Alice",
                "abcdef1234567890abcdef1234567890abcdef12",
            ),
            c(
                "Add graph",
                "Bob",
                "1234567890abcdef1234567890abcdef12345678",
            ),
        ];
        assert_eq!(search(&list, "登录"), vec![0]);
        assert_eq!(search(&list, "bob"), vec![1]);
        assert_eq!(search(&list, "abcdef1"), vec![0]);
        assert_eq!(search(&list, "GRAPH"), vec![1]);
        assert_eq!(search(&list, ""), vec![0, 1]);
    }
}

//! 时间与文本格式化。

/// 相对时间:一周内显示“x 分钟前”,更早显示日期。
pub fn relative_time(unix: i64, now: i64) -> String {
    let d = now - unix;
    match d {
        i64::MIN..60 => "刚刚".into(),
        60..3600 => format!("{} 分钟前", d / 60),
        3600..86400 => format!("{} 小时前", d / 3600),
        86400..604800 => format!("{} 天前", d / 86400),
        _ => date(unix, 0),
    }
}

/// 日期 YYYY-MM-DD,`tz_offset` 为分钟。
pub fn date(unix: i64, tz_offset: i32) -> String {
    let (y, m, d) = civil_from_days((unix + tz_offset as i64 * 60).div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// 完整时间 YYYY-MM-DD HH:MM ±HHMM。
pub fn datetime(unix: i64, tz_offset: i32) -> String {
    let local = unix + tz_offset as i64 * 60;
    let secs = local.rem_euclid(86400);
    let sign = if tz_offset < 0 { '-' } else { '+' };
    let tz = tz_offset.abs();
    format!(
        "{} {:02}:{:02} {sign}{:02}{:02}",
        date(unix, tz_offset),
        secs / 3600,
        secs % 3600 / 60,
        tz / 60,
        tz % 60
    )
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Howard Hinnant 的 days → civil 算法。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 简单的子序列模糊匹配,命令面板使用。
pub fn fuzzy_match(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    let mut chars = text.chars();
    query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|q| chars.any(|c| c == q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date(0, 0), "1970-01-01");
        assert_eq!(date(1_700_000_000, 0), "2023-11-14");
        assert_eq!(datetime(1_700_000_000, 480), "2023-11-15 06:13 +0800");
        assert_eq!(relative_time(100, 130), "刚刚");
        assert_eq!(relative_time(0, 7200), "2 小时前");
    }

    #[test]
    fn fuzzy() {
        assert!(fuzzy_match("Force push", "fpush"));
        assert!(fuzzy_match("新建分支", "分支"));
        assert!(!fuzzy_match("Fetch", "push"));
    }
}

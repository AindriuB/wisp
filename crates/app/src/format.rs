//! Pure display helpers for the response pane.

use std::time::Duration;

/// Maximum number of body bytes shown in the response pane.
pub const BODY_LIMIT: usize = 64 * 1024;

/// Format a byte count as B, KB or MB.
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < MB && (b / KB * 10.0).round() < 10240.0 {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / MB)
    }
}

/// Status line for a completed request: `<status> <reason> · <ms> ms · <size>`.
pub fn completed_line(status: u16, reason: Option<&str>, total: Duration, size: u64) -> String {
    let head = match reason {
        Some(r) if !r.is_empty() => format!("{status} {r}"),
        _ => status.to_string(),
    };
    format!("{head} · {} ms · {}", total.as_millis(), format_size(size))
}

/// Lossy UTF-8 text of the first `BODY_LIMIT` bytes, with a marker if cut.
pub fn body_text(body: &[u8]) -> String {
    if body.len() <= BODY_LIMIT {
        return String::from_utf8_lossy(body).into_owned();
    }
    let mut text = String::from_utf8_lossy(&body[..BODY_LIMIT]).into_owned();
    text.push_str("\n\n[truncated: showing first 64 KiB]");
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_b_kb_and_mb_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1_048_575), "1.0 MB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.0 MB");
        assert_eq!(format_size(5_000_000), "4.8 MB");
    }

    #[test]
    fn status_line_shows_status_reason_time_and_size() {
        let d = Duration::from_millis(42);
        assert_eq!(
            completed_line(200, Some("OK"), d, 2048),
            "200 OK · 42 ms · 2.0 KB"
        );
        assert_eq!(completed_line(299, None, d, 5), "299 · 42 ms · 5 B");
    }

    #[test]
    fn bodies_over_the_limit_are_truncated_with_a_marker() {
        assert_eq!(body_text(b"hi"), "hi");
        let exact = vec![b'a'; BODY_LIMIT];
        assert!(!body_text(&exact).contains("truncated"));
        let big = vec![b'a'; BODY_LIMIT + 1];
        let t = body_text(&big);
        assert!(t.contains("truncated"));
        assert!(t.starts_with(&"a".repeat(BODY_LIMIT)));
        assert!(!t.starts_with(&"a".repeat(BODY_LIMIT + 1)));
    }
}

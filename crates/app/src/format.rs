//! Pure display helpers for the response pane.

use std::time::Duration;

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

/// Duration as `N ms` below one second, otherwise `N.NN s`.
pub fn format_duration(d: Duration) -> String {
    if d < Duration::from_secs(1) {
        format!("{} ms", d.as_millis())
    } else {
        format!("{:.2} s", d.as_secs_f64())
    }
}

/// `<status> <reason>`, or just the status when the reason is absent or empty.
pub fn status_text(status: u16, reason: Option<&str>) -> String {
    match reason {
        Some(r) if !r.is_empty() => format!("{status} {r}"),
        _ => status.to_string(),
    }
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
    fn durations_use_ms_then_seconds() {
        assert_eq!(format_duration(Duration::from_millis(0)), "0 ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999 ms");
        assert_eq!(format_duration(Duration::from_millis(1000)), "1.00 s");
        assert_eq!(format_duration(Duration::from_millis(1234)), "1.23 s");
    }

    #[test]
    fn status_text_with_and_without_reason() {
        assert_eq!(status_text(404, Some("Not Found")), "404 Not Found");
        assert_eq!(status_text(299, None), "299");
        assert_eq!(status_text(200, Some("")), "200");
    }
}

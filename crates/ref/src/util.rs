//! Small shared helpers.

use chrono::{Local, TimeZone};

/// `HH:MM:SS` local time; prefixed with the date when not today.
pub fn fmt_time(ms: i64) -> String {
    let Some(t) = Local.timestamp_millis_opt(ms).single() else { return String::new() };
    let now = Local::now();
    if t.date_naive() == now.date_naive() {
        t.format("%H:%M:%S").to_string()
    } else {
        t.format("%Y-%m-%d %H:%M:%S").to_string()
    }
}

/// Short relative/absolute date for lists.
pub fn fmt_date(ms: i64) -> String {
    let Some(t) = Local.timestamp_millis_opt(ms).single() else { return String::new() };
    let now = Local::now();
    if t.date_naive() == now.date_naive() {
        t.format("%H:%M").to_string()
    } else {
        t.format("%m-%d %H:%M").to_string()
    }
}

/// Human duration: `850ms`, `12.3s`, `2m05s`, `1h02m`.
pub fn fmt_dur(ms: i64) -> String {
    let ms = ms.max(0);
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else if ms < 3_600_000 {
        format!("{}m{:02}s", ms / 60_000, (ms / 1000) % 60)
    } else {
        format!("{}h{:02}m", ms / 3_600_000, (ms / 60_000) % 60)
    }
}

/// Compact token count: `950`, `12.3k`, `1.2M`.
pub fn fmt_tokens(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        let k = n as f64 / 1000.0;
        if k < 100.0 {
            format!("{k:.1}k")
        } else {
            format!("{k:.0}k")
        }
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

pub fn fmt_cost(c: f64) -> String {
    format!("${c:.2}")
}

pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn spinner(tick: u64) -> &'static str {
    SPINNER[(tick % SPINNER.len() as u64) as usize]
}

pub fn data_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("REF_DATA_DIR") {
        if !d.is_empty() {
            return d.into();
        }
    }
    dirs::data_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp")).join("ref")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(fmt_dur(850), "850ms");
        assert_eq!(fmt_dur(12_345), "12.3s");
        assert_eq!(fmt_dur(125_000), "2m05s");
        assert_eq!(fmt_tokens(12_345), "12.3k");
        assert_eq!(fmt_tokens(200_000), "200k");
    }

    #[test]
    fn time_today_has_no_date() {
        let s = fmt_time(ref_proto::now_ms());
        assert_eq!(s.len(), 8);
        let old = fmt_time(0);
        assert!(old.contains('-'));
    }
}

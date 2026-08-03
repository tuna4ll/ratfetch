//! Small helpers shared across the crate.

/// Levenshtein distance, used for "did you mean" hints in config errors.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];

    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The closest candidate to `input`, if one is close enough to be worth
/// suggesting.
pub fn suggest<'a, I: IntoIterator<Item = &'a str>>(input: &str, candidates: I) -> Option<&'a str> {
    let limit = (input.chars().count() / 2).clamp(2, 4);
    candidates
        .into_iter()
        .map(|c| {
            (
                c,
                edit_distance(&input.to_ascii_lowercase(), &c.to_ascii_lowercase()),
            )
        })
        .filter(|(_, d)| *d <= limit)
        .min_by_key(|(_, d)| *d)
        .map(|(c, _)| c)
}

/// Formats a byte count with binary prefixes (KiB, MiB, ...).
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// Formats a per-second byte rate.
pub fn human_rate(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec < 0.5 {
        return "0 B/s".to_string();
    }
    format!("{}/s", human_bytes(bytes_per_sec as u64))
}

/// Formats a duration in seconds as `3d 4h 12m`, dropping empty leading units.
pub fn human_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    let s = secs % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if days > 0 || hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if days > 0 || hours > 0 || mins > 0 {
        parts.push(format!("{mins}m"));
    }
    // Seconds are only interesting on a machine that booted minutes ago.
    if days == 0 && hours == 0 {
        parts.push(format!("{s}s"));
    }
    parts.join(" ")
}

/// Truncates to `width` display columns, appending `…` when it had to cut.
pub fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if s.chars().count() <= width {
        return s.to_string();
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("kernal", "kernel"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn suggest_picks_the_near_miss() {
        let items = ["kernel", "uptime", "memory"];
        assert_eq!(suggest("kernal", items), Some("kernel"));
        assert_eq!(suggest("zzzzzzzz", items), None);
    }

    #[test]
    fn bytes_are_scaled() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.00 KiB");
        assert_eq!(human_bytes(1536), "1.50 KiB");
        assert_eq!(human_bytes(20 * 1024 * 1024), "20.0 MiB");
        assert_eq!(human_bytes(512 * 1024 * 1024), "512 MiB");
    }

    #[test]
    fn uptime_drops_leading_zero_units() {
        assert_eq!(human_uptime(45), "45s");
        assert_eq!(human_uptime(3 * 3600 + 5 * 60), "3h 5m");
        assert_eq!(human_uptime(2 * 86400 + 3600), "2d 1h 0m");
    }

    #[test]
    fn truncate_marks_the_cut() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 4), "hel…");
        assert_eq!(truncate("hello", 0), "");
    }
}

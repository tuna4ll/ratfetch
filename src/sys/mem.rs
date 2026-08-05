//! Memory and swap, from `/proc/meminfo`.

use std::fs;

use super::{percent, split_kv};

/// A reading of RAM and swap, in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    /// Total minus `MemAvailable`, which is what "used" means to a user.
    pub used: u64,
    pub available: u64,
    pub cached: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

impl Memory {
    pub fn load() -> Self {
        let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        let mut total = 0;
        let mut free = 0;
        let mut available = None;
        let mut cached = 0;
        let mut buffers = 0;
        let mut reclaimable = 0;
        let mut swap_total = 0;
        let mut swap_free = 0;

        for line in text.lines() {
            let Some((key, value)) = split_kv(line) else {
                continue;
            };
            // Values are "<number> kB", except for a couple that are bare.
            let Some(kib) = value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
            else {
                continue;
            };
            let bytes = kib.saturating_mul(1024);
            match key {
                "MemTotal" => total = bytes,
                "MemFree" => free = bytes,
                "MemAvailable" => available = Some(bytes),
                "Cached" => cached = bytes,
                "Buffers" => buffers = bytes,
                "SReclaimable" => reclaimable = bytes,
                "SwapTotal" => swap_total = bytes,
                "SwapFree" => swap_free = bytes,
                _ => {}
            }
        }

        // Kernels before 3.14 have no MemAvailable; approximate it the way
        // the kernel's own estimate does.
        let available = available.unwrap_or(free + buffers + cached + reclaimable);
        let available = available.min(total);

        Self {
            total,
            used: total.saturating_sub(available),
            available,
            cached: cached + reclaimable,
            swap_total,
            swap_used: swap_total.saturating_sub(swap_free),
        }
    }

    pub fn percent(&self) -> f64 {
        percent(self.used, self.total)
    }

    pub fn swap_percent(&self) -> f64 {
        percent(self.swap_used, self.swap_total)
    }

    pub fn has_swap(&self) -> bool {
        self.swap_total > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
MemTotal:       32000000 kB
MemFree:         2000000 kB
MemAvailable:   24000000 kB
Buffers:          500000 kB
Cached:          8000000 kB
SReclaimable:     500000 kB
SwapTotal:       8000000 kB
SwapFree:        6000000 kB
";

    #[test]
    fn parses_meminfo() {
        let m = Memory::parse(SAMPLE);
        assert_eq!(m.total, 32_000_000 * 1024);
        assert_eq!(m.available, 24_000_000 * 1024);
        assert_eq!(m.used, 8_000_000 * 1024);
        assert_eq!(m.cached, 8_500_000 * 1024);
        assert_eq!(m.swap_total, 8_000_000 * 1024);
        assert_eq!(m.swap_used, 2_000_000 * 1024);
        assert!((m.percent() - 25.0).abs() < 1e-9);
        assert!((m.swap_percent() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn falls_back_when_mem_available_is_absent() {
        let text = "\
MemTotal:       1000 kB
MemFree:         100 kB
Buffers:          50 kB
Cached:          200 kB
SReclaimable:     50 kB
";
        let m = Memory::parse(text);
        assert_eq!(
            m.available,
            400 * 1024,
            "free + buffers + cached + reclaimable"
        );
        assert_eq!(m.used, 600 * 1024);
    }

    #[test]
    fn available_never_exceeds_total() {
        let m = Memory::parse("MemTotal: 100 kB\nMemAvailable: 500 kB\n");
        assert_eq!(m.available, 100 * 1024);
        assert_eq!(m.used, 0);
    }

    #[test]
    fn empty_input_is_all_zero() {
        let m = Memory::parse("");
        assert_eq!(m, Memory::default());
        assert_eq!(m.percent(), 0.0);
        assert!(!m.has_swap());
    }

    #[test]
    fn a_machine_with_no_swap_reports_none() {
        let m = Memory::parse("MemTotal: 100 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n");
        assert!(!m.has_swap());
        assert_eq!(m.swap_percent(), 0.0);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn reads_this_machine() {
        let m = Memory::load();
        assert!(m.total > 0);
        assert!(m.used <= m.total);
        assert!((0.0..=100.0).contains(&m.percent()));
    }
}

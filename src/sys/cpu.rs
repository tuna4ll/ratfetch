//! CPU identity from `/proc/cpuinfo` and utilisation from `/proc/stat`.

use std::collections::HashSet;
use std::fs;

use super::{read_trimmed, read_u64, split_kv};

/// Facts about the processor that do not change while running.
#[derive(Debug, Clone, Default)]
pub struct CpuInfo {
    pub model: String,
    /// Distinct physical cores, where the kernel reports enough to tell.
    pub cores: usize,
    /// Logical CPUs, i.e. the number of `cpuN` lines in `/proc/stat`.
    pub threads: usize,
    /// Maximum clock in MHz, when the driver exposes one.
    pub max_mhz: Option<f64>,
}

impl CpuInfo {
    pub fn load() -> Self {
        let text = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();

        let mut model = String::new();
        let mut physical_ids: HashSet<(String, String)> = HashSet::new();
        let mut threads = 0usize;
        let mut socket = String::new();

        for line in text.lines() {
            let Some((key, value)) = split_kv(line) else {
                continue;
            };
            match key {
                // x86 uses "model name"; arm64 has "Model name" or nothing at
                // all, in which case "Hardware" is the best available label.
                "model name" | "Model name" | "cpu model" if model.is_empty() => {
                    model = value.to_string();
                }
                "Hardware" | "Model" if model.is_empty() => model = value.to_string(),
                "processor" => threads += 1,
                "physical id" => socket = value.to_string(),
                "core id" => {
                    physical_ids.insert((socket.clone(), value.to_string()));
                }
                _ => {}
            }
        }

        if model.is_empty() {
            model = read_trimmed("/sys/firmware/devicetree/base/model")
                .unwrap_or_else(|| "Unknown CPU".to_string());
        }

        if threads == 0 {
            threads = count_online_cpus();
        }
        let cores = if physical_ids.is_empty() {
            threads
        } else {
            physical_ids.len()
        };

        Self {
            model: tidy_model(&model),
            cores,
            threads,
            max_mhz: max_mhz(),
        }
    }

    /// `Model (8) @ 4.20 GHz`, the way the info row shows it.
    pub fn label(&self, current_mhz: Option<f64>) -> String {
        let mut s = self.model.clone();
        if self.threads > 0 {
            s.push_str(&format!(" ({})", self.threads));
        }
        if let Some(mhz) = current_mhz.or(self.max_mhz) {
            s.push_str(&format!(" @ {:.2} GHz", mhz / 1000.0));
        }
        s
    }
}

/// Strips the marketing noise vendors put in `model name`.
fn tidy_model(raw: &str) -> String {
    let mut s = raw.to_string();
    // Order matters: the compound forms have to go before their pieces, or
    // stripping "(TM)" first would leave a bare "Core" behind.
    for junk in [
        "(R)",
        "(r)",
        "Core(TM) ",
        "Core(tm) ",
        "(TM)",
        "(tm)",
        "(C)",
        "CPU ",
        "Processor ",
        "Technologies, Inc ",
        " with Radeon Graphics",
        " 16-Core",
        " 12-Core",
        " 8-Core",
        " 6-Core",
        " 4-Core",
        " Dual-Core",
        " Quad-Core",
    ] {
        s = s.replace(junk, "");
    }
    // Collapse the whitespace the replacements leave behind.
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn count_online_cpus() -> usize {
    // SAFETY: sysconf only reads a static system parameter.
    let n = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    if n > 0 {
        n as usize
    } else {
        1
    }
}

/// Highest advertised clock, in MHz.
fn max_mhz() -> Option<f64> {
    // cpufreq reports kHz; the value is per-policy, so take the highest.
    let mut best: Option<f64> = None;
    if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu/cpufreq") {
        for entry in entries.flatten() {
            if let Some(khz) = read_u64(entry.path().join("cpuinfo_max_freq")) {
                let mhz = khz as f64 / 1000.0;
                best = Some(best.map_or(mhz, |b: f64| b.max(mhz)));
            }
        }
    }
    if best.is_some() {
        return best;
    }

    // Without cpufreq, `/proc/cpuinfo` still carries a current clock.
    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    text.lines()
        .filter_map(|l| split_kv(l).filter(|(k, _)| *k == "cpu MHz"))
        .filter_map(|(_, v)| v.parse::<f64>().ok())
        .fold(None, |acc: Option<f64>, v| {
            Some(acc.map_or(v, |a| a.max(v)))
        })
}

/// Current average clock across the online CPUs, in MHz.
fn current_mhz() -> Option<f64> {
    let mut sum = 0.0;
    let mut n = 0usize;
    if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("cpu") || !name[3..].chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if let Some(khz) = read_u64(entry.path().join("cpufreq/scaling_cur_freq")) {
                sum += khz as f64 / 1000.0;
                n += 1;
            }
        }
    }
    if n > 0 {
        return Some(sum / n as f64);
    }

    let text = fs::read_to_string("/proc/cpuinfo").ok()?;
    let values: Vec<f64> = text
        .lines()
        .filter_map(|l| split_kv(l).filter(|(k, _)| *k == "cpu MHz"))
        .filter_map(|(_, v)| v.parse().ok())
        .collect();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

/// Utilisation over the interval between the last two samples.
#[derive(Debug, Clone, Default)]
pub struct CpuUsage {
    /// Average busy percentage, 0..=100.
    pub total: f64,
    /// Busy percentage per logical CPU.
    pub per_core: Vec<f64>,
    pub freq_mhz: Option<f64>,
}

/// A raw `/proc/stat` row: total jiffies and the idle share of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Times {
    total: u64,
    idle: u64,
}

impl Times {
    /// Parses the numbers after a `cpu` label.
    fn parse(fields: &str) -> Self {
        let values: Vec<u64> = fields
            .split_whitespace()
            .filter_map(|f| f.parse().ok())
            .collect();
        // user nice system idle iowait irq softirq steal guest guest_nice
        let total: u64 = values.iter().sum();
        let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
        Self { total, idle }
    }

    /// Busy percentage between two readings.
    fn busy_since(self, prev: Self) -> f64 {
        let total = self.total.saturating_sub(prev.total);
        let idle = self.idle.saturating_sub(prev.idle);
        if total == 0 {
            return 0.0;
        }
        let busy = total.saturating_sub(idle) as f64 / total as f64 * 100.0;
        busy.clamp(0.0, 100.0)
    }
}

/// Turns successive `/proc/stat` readings into percentages.
pub struct Sampler {
    prev_total: Option<Times>,
    prev_cores: Vec<Times>,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            prev_total: None,
            prev_cores: Vec::new(),
        }
    }

    pub fn sample(&mut self) -> CpuUsage {
        let Ok(text) = fs::read_to_string("/proc/stat") else {
            return CpuUsage::default();
        };

        let mut total_now = Times::default();
        let mut cores_now = Vec::new();

        for line in text.lines() {
            let Some(rest) = line.strip_prefix("cpu") else {
                break;
            };
            match rest.split_once(char::is_whitespace) {
                // The aggregate row is `cpu` with no index.
                Some(("", fields)) => total_now = Times::parse(fields),
                Some((idx, fields)) if idx.chars().all(|c| c.is_ascii_digit()) => {
                    cores_now.push(Times::parse(fields));
                }
                _ => break,
            }
        }

        let total = match self.prev_total {
            Some(prev) => total_now.busy_since(prev),
            None => 0.0,
        };
        let per_core = if self.prev_cores.len() == cores_now.len() {
            cores_now
                .iter()
                .zip(&self.prev_cores)
                .map(|(now, prev)| now.busy_since(*prev))
                .collect()
        } else {
            // Core count changed (or this is the first sample); report zeroes
            // rather than nonsense until the next tick.
            vec![0.0; cores_now.len()]
        };

        self.prev_total = Some(total_now);
        self.prev_cores = cores_now;

        CpuUsage {
            total,
            per_core,
            freq_mhz: current_mhz(),
        }
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_parse_and_subtract() {
        let a = Times::parse(" 100 0 50 800 50 0 0 0 0 0");
        assert_eq!(a.total, 1000);
        assert_eq!(a.idle, 850);

        let b = Times::parse(" 200 0 100 1600 100 0 0 0 0 0");
        // 1000 more jiffies, 850 of them idle => 15% busy.
        assert!((b.busy_since(a) - 15.0).abs() < 1e-9);
    }

    #[test]
    fn identical_readings_are_zero_percent() {
        let a = Times::parse(" 1 2 3 4 5 6 7 8");
        assert_eq!(a.busy_since(a), 0.0);
    }

    #[test]
    fn counters_going_backwards_do_not_panic() {
        let big = Times::parse(" 500 0 0 500 0");
        let small = Times::parse(" 100 0 0 100 0");
        assert_eq!(small.busy_since(big), 0.0);
    }

    #[test]
    fn model_names_are_tidied() {
        assert_eq!(
            tidy_model("AMD Ryzen 7 5800X 8-Core Processor "),
            "AMD Ryzen 7 5800X"
        );
        assert_eq!(
            tidy_model("Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz"),
            "Intel i7-9750H @ 2.60GHz"
        );
    }

    #[test]
    fn label_includes_thread_count() {
        let info = CpuInfo {
            model: "Test CPU".into(),
            cores: 4,
            threads: 8,
            max_mhz: Some(4200.0),
        };
        assert_eq!(info.label(None), "Test CPU (8) @ 4.20 GHz");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn reads_this_machine() {
        let info = CpuInfo::load();
        assert!(info.threads >= 1);
        assert!(!info.model.is_empty());

        let mut s = Sampler::new();
        let first = s.sample();
        assert_eq!(first.total, 0.0, "no previous sample to compare against");
        let second = s.sample();
        assert!((0.0..=100.0).contains(&second.total));
        assert_eq!(
            second.per_core.len(),
            info.threads.max(second.per_core.len())
        );
    }
}

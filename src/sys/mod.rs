//! Reading the machine's state out of `/proc`, `/sys` and a few libc calls.
//!
//! Nothing here shells out on the hot path. Facts that cannot change while the
//! program runs are collected once into [`Static`]; everything else is
//! re-read each tick into [`Dynamic`] by a [`Sampler`], which keeps the
//! previous reading so it can turn counters into rates.

pub mod battery;
pub mod cpu;
pub mod desktop;
pub mod disk;
pub mod disk_io;
pub mod gpu;
pub mod host;
pub mod mem;
pub mod net;
pub mod os;
pub mod packages;
pub mod procs;
pub mod temp;
pub mod time;

use std::fs;
use std::path::Path;
use std::time::Instant;

use crate::config::model::{Config, Disks as DiskCfg, Network as NetCfg};

/// Facts that are read once at startup.
#[derive(Debug, Clone, Default)]
pub struct Static {
    /// The id used to pick a logo, e.g. `arch`.
    pub distro_id: String,
    pub os_name: String,
    pub kernel: String,
    pub hostname: String,
    pub username: String,
    pub host_model: String,
    pub cpu: cpu::CpuInfo,
    pub gpus: Vec<gpu::Gpu>,
    pub shell: String,
    pub terminal: String,
    pub de: String,
    pub wm: String,
    pub resolution: String,
    pub locale: String,
    pub packages: String,
    pub init: String,
}

impl Static {
    /// Collects everything that does not change while the program runs.
    pub fn collect() -> Self {
        let release = os::Release::load();
        let env = desktop::Environment::detect();

        Self {
            distro_id: release.logo_id(),
            os_name: release.pretty_name(),
            kernel: os::kernel(),
            hostname: os::hostname(),
            username: os::username(),
            host_model: host::model(),
            cpu: cpu::CpuInfo::load(),
            gpus: gpu::list(),
            shell: env.shell,
            terminal: env.terminal,
            de: env.de,
            wm: env.wm,
            resolution: desktop::resolution(),
            locale: os::locale(),
            packages: packages::summary(),
            init: os::init_system(),
        }
    }
}

/// Everything re-read on each tick.
#[derive(Debug, Clone, Default)]
pub struct Dynamic {
    pub uptime: u64,
    pub cpu: cpu::CpuUsage,
    pub mem: mem::Memory,
    pub disks: Vec<disk::Disk>,
    pub disk_io: Vec<disk_io::DeviceIo>,
    pub nets: Vec<net::Interface>,
    pub battery: Option<battery::Battery>,
    pub temps: Vec<temp::Sensor>,
    pub load: [f64; 3],
    pub procs: Vec<procs::Process>,
    pub proc_total: usize,
    pub thread_total: usize,
    pub users: usize,
    pub local_ip: String,
}

impl Dynamic {
    /// The filesystem behind the `disk` meter and info row.
    pub fn primary_disk<'a>(&'a self, cfg: &DiskCfg) -> Option<&'a disk::Disk> {
        self.disks
            .iter()
            .find(|d| d.mount == cfg.primary)
            .or_else(|| self.disks.first())
    }

    /// The interface behind the network graph.
    pub fn primary_net<'a>(&'a self, cfg: &NetCfg) -> Option<&'a net::Interface> {
        if cfg.primary != "auto" {
            return self.nets.iter().find(|n| n.name == cfg.primary);
        }
        // "Busiest" means most bytes moved since boot, which is a stable
        // choice frame to frame, unlike picking on instantaneous rate.
        self.nets
            .iter()
            .max_by_key(|n| n.rx_bytes.saturating_add(n.tx_bytes))
    }

    /// Total receive and transmit rates across the shown interfaces.
    pub fn net_rates(&self) -> (f64, f64) {
        self.nets
            .iter()
            .fold((0.0, 0.0), |(rx, tx), n| (rx + n.rx_rate, tx + n.tx_rate))
    }
}

/// Produces [`Dynamic`] snapshots, remembering the previous reading so that
/// counters can be turned into per-second rates.
pub struct Sampler {
    cpu: cpu::Sampler,
    net: net::Sampler,
    disk_io: disk_io::Sampler,
    procs: procs::Sampler,
    last: Option<Instant>,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            cpu: cpu::Sampler::new(),
            net: net::Sampler::new(),
            disk_io: disk_io::Sampler::new(),
            procs: procs::Sampler::new(),
            last: None,
        }
    }

    /// Takes a reading.
    ///
    /// The first call has no previous sample to subtract from, so rates and
    /// CPU percentages come back as zero; that resolves itself one tick later.
    pub fn sample(&mut self, cfg: &Config) -> Dynamic {
        let now = Instant::now();
        let elapsed = self
            .last
            .map(|t| now.duration_since(t).as_secs_f64())
            .unwrap_or(0.0);
        self.last = Some(now);

        let nets = self.net.sample(elapsed, &cfg.network);
        let local_ip = net::local_ip(&nets);

        Dynamic {
            uptime: os::uptime(),
            cpu: self.cpu.sample(),
            mem: mem::Memory::load(),
            disks: disk::list(&cfg.disks),
            disk_io: self.disk_io.sample(elapsed),
            nets,
            battery: battery::load(),
            temps: temp::list(),
            load: os::load_average(),
            procs: self.procs.sample(elapsed, &cfg.processes),
            proc_total: self.procs.last_total,
            thread_total: self.procs.last_threads,
            users: os::logged_in_users(),
            local_ip,
        }
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------- utilities --

/// Reads a file, trimming trailing whitespace. `None` on any error, because
/// every one of these paths is optional on some kernel or container.
pub(crate) fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Reads a file containing a single integer.
pub(crate) fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

/// Reads an environment variable, treating empty as unset.
pub(crate) fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// A percentage of `total`, guarding against a zero denominator.
pub(crate) fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64) * 100.0
    }
}

/// Splits a `key: value` line from `/proc`.
pub(crate) fn split_kv(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(':')?;
    Some((k.trim(), v.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_handles_zero_total() {
        assert_eq!(percent(0, 0), 0.0);
        assert_eq!(percent(5, 0), 0.0);
        assert_eq!(percent(1, 4), 25.0);
    }

    #[test]
    fn kv_lines_split_on_the_first_colon() {
        assert_eq!(
            split_kv("MemTotal:  32768 kB"),
            Some(("MemTotal", "32768 kB"))
        );
        assert_eq!(
            split_kv("model name : Ryzen: 9"),
            Some(("model name", "Ryzen: 9"))
        );
        assert_eq!(split_kv("no colon here"), None);
    }

    #[test]
    fn missing_files_are_not_errors() {
        assert_eq!(read_trimmed("/definitely/not/here"), None);
        assert_eq!(read_u64("/definitely/not/here"), None);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_full_sample_succeeds_on_this_machine() {
        let cfg = Config::default();
        let mut s = Sampler::new();
        let _ = s.sample(&cfg);
        let d = s.sample(&cfg);
        assert!(d.uptime > 0, "uptime should be readable");
        assert!(d.mem.total > 0, "memory should be readable");
        assert!(d.cpu.total >= 0.0 && d.cpu.total <= 100.0);
    }
}

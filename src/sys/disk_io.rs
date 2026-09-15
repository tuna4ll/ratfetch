//! Block-device throughput from `/proc/diskstats`.

use std::collections::{HashMap, HashSet};
use std::fs;

/// Live I/O for one whole block device.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceIo {
    pub name: String,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_rate: f64,
    pub write_rate: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Counters {
    read_sectors: u64,
    write_sectors: u64,
}

fn parse(text: &str) -> HashMap<String, Counters> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let name = fields.get(2)?.to_string();
            let read_sectors = fields.get(5)?.parse().ok()?;
            let write_sectors = fields.get(9)?.parse().ok()?;
            Some((
                name,
                Counters {
                    read_sectors,
                    write_sectors,
                },
            ))
        })
        .collect()
}

fn whole_devices() -> HashSet<String> {
    let Ok(entries) = fs::read_dir("/sys/block") else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| {
            !name.starts_with("loop") && !name.starts_with("ram") && !name.starts_with("zram")
        })
        .collect()
}

/// Turns monotonic sector counters into byte rates.
pub struct Sampler {
    prev: HashMap<String, Counters>,
}

impl Sampler {
    pub fn new() -> Self {
        Self {
            prev: HashMap::new(),
        }
    }

    pub fn sample(&mut self, elapsed: f64) -> Vec<DeviceIo> {
        let text = fs::read_to_string("/proc/diskstats").unwrap_or_default();
        let counters = parse(&text);
        let wanted = whole_devices();
        let mut out = Vec::new();

        for (name, current) in &counters {
            if !wanted.is_empty() && !wanted.contains(name) {
                continue;
            }
            // Linux diskstats sectors are always expressed as 512-byte units.
            const SECTOR: u64 = 512;
            let (read_rate, write_rate) = match self.prev.get(name) {
                Some(previous) if elapsed > 0.0 => (
                    current.read_sectors.saturating_sub(previous.read_sectors) as f64
                        * SECTOR as f64
                        / elapsed,
                    current.write_sectors.saturating_sub(previous.write_sectors) as f64
                        * SECTOR as f64
                        / elapsed,
                ),
                _ => (0.0, 0.0),
            };
            out.push(DeviceIo {
                name: name.clone(),
                read_bytes: current.read_sectors.saturating_mul(SECTOR),
                write_bytes: current.write_sectors.saturating_mul(SECTOR),
                read_rate,
                write_rate,
            });
        }

        self.prev = counters;
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

pub fn total_rate(devices: &[DeviceIo]) -> (f64, f64) {
    devices.iter().fold((0.0, 0.0), |(read, write), device| {
        (read + device.read_rate, write + device.write_rate)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATS: &str = "259 0 nvme0n1 10 0 200 0 20 0 400 0 0 0 0 0 0 0 0\n\
                         259 1 nvme0n1p1 2 0 40 0 3 0 60 0 0 0 0 0 0 0 0\n";

    #[test]
    fn parses_sector_counters() {
        let parsed = parse(STATS);
        assert_eq!(parsed["nvme0n1"].read_sectors, 200);
        assert_eq!(parsed["nvme0n1"].write_sectors, 400);
    }

    #[test]
    fn totals_device_rates() {
        let devices = vec![
            DeviceIo {
                read_rate: 10.0,
                write_rate: 20.0,
                ..Default::default()
            },
            DeviceIo {
                read_rate: 2.0,
                write_rate: 3.0,
                ..Default::default()
            },
        ];
        assert_eq!(total_rate(&devices), (12.0, 23.0));
    }
}

//! Temperature sensors from `/sys/class/hwmon`.

use std::fs;

use super::{read_trimmed, read_u64};

/// One temperature reading.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sensor {
    /// The chip, e.g. `k10temp` or `coretemp`.
    pub chip: String,
    /// The individual input's label, e.g. `Tctl` or `Package id 0`.
    pub label: String,
    pub celsius: f64,
}

impl Sensor {
    /// Whether this looks like the CPU package sensor.
    pub fn is_cpu(&self) -> bool {
        const CHIPS: [&str; 6] = [
            "k10temp",
            "coretemp",
            "zenpower",
            "cpu_thermal",
            "k8temp",
            "acpitz",
        ];
        const LABELS: [&str; 4] = ["tctl", "tdie", "package id 0", "cpu"];

        CHIPS.contains(&self.chip.as_str())
            || LABELS
                .iter()
                .any(|l| self.label.to_ascii_lowercase().contains(l))
    }
}

/// Every readable sensor, sorted by chip then label.
pub fn list() -> Vec<Sensor> {
    let Ok(hwmons) = fs::read_dir("/sys/class/hwmon") else {
        return Vec::new();
    };

    let mut out = Vec::new();

    for hwmon in hwmons.flatten() {
        let dir = hwmon.path();
        let chip = read_trimmed(dir.join("name")).unwrap_or_else(|| "unknown".to_string());

        let Ok(files) = fs::read_dir(&dir) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name();
            let name = name.to_string_lossy();
            // Inputs are named tempN_input; their labels are tempN_label.
            let Some(index) = name
                .strip_prefix("temp")
                .and_then(|n| n.strip_suffix("_input"))
            else {
                continue;
            };

            // Millidegrees Celsius.
            let Some(milli) = read_u64(file.path()) else {
                continue;
            };
            let celsius = milli as f64 / 1000.0;
            // Disconnected sensors read as absurd values.
            if !(-50.0..=150.0).contains(&celsius) {
                continue;
            }

            let label = read_trimmed(dir.join(format!("temp{index}_label")))
                .unwrap_or_else(|| format!("temp{index}"));

            out.push(Sensor {
                chip: chip.clone(),
                label,
                celsius,
            });
        }
    }

    out.sort_by(|a, b| a.chip.cmp(&b.chip).then(a.label.cmp(&b.label)));
    out
}

/// The CPU temperature, if any sensor looks like one.
pub fn cpu(sensors: &[Sensor]) -> Option<f64> {
    sensors.iter().find(|s| s.is_cpu()).map(|s| s.celsius)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_sensors_are_recognised_by_chip_or_label() {
        let s = Sensor {
            chip: "k10temp".into(),
            label: "Tctl".into(),
            celsius: 45.0,
        };
        assert!(s.is_cpu());

        let s = Sensor {
            chip: "coretemp".into(),
            label: "Core 0".into(),
            celsius: 45.0,
        };
        assert!(s.is_cpu());

        let s = Sensor {
            chip: "nvme".into(),
            label: "Composite".into(),
            celsius: 40.0,
        };
        assert!(!s.is_cpu());
    }

    #[test]
    fn cpu_picks_the_first_matching_sensor() {
        let sensors = vec![
            Sensor {
                chip: "nvme".into(),
                label: "Composite".into(),
                celsius: 40.0,
            },
            Sensor {
                chip: "k10temp".into(),
                label: "Tctl".into(),
                celsius: 55.5,
            },
        ];
        assert_eq!(cpu(&sensors), Some(55.5));
        assert_eq!(cpu(&[]), None);
    }

    #[test]
    fn listing_never_panics() {
        // Containers have no hwmon at all; that must be an empty list, not a
        // failure.
        let sensors = list();
        assert!(sensors.iter().all(|s| (-50.0..=150.0).contains(&s.celsius)));
    }
}

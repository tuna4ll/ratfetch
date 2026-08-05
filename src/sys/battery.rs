//! Battery state from `/sys/class/power_supply`.

use std::fs;
use std::path::Path;

use super::{read_trimmed, read_u64};

/// A battery's charge and what it is doing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Battery {
    pub name: String,
    /// Charge percentage, 0..=100.
    pub percent: f64,
    /// `Charging`, `Discharging`, `Full`, ...
    pub status: String,
    /// Current draw in watts, when the driver reports it.
    pub watts: Option<f64>,
    /// Battery health as a percentage of its design capacity.
    pub health: Option<f64>,
}

impl Battery {
    pub fn is_charging(&self) -> bool {
        self.status.eq_ignore_ascii_case("charging")
    }

    /// A short label such as `87% (Charging, 21.4 W)`.
    pub fn label(&self) -> String {
        let mut s = format!("{:.0}%", self.percent);
        let mut detail = Vec::new();
        if !self.status.is_empty() && !self.status.eq_ignore_ascii_case("unknown") {
            detail.push(self.status.clone());
        }
        if let Some(w) = self.watts.filter(|w| *w > 0.01) {
            detail.push(format!("{w:.1} W"));
        }
        if !detail.is_empty() {
            s.push_str(&format!(" ({})", detail.join(", ")));
        }
        s
    }
}

/// The first battery the kernel exposes, or `None` on a desktop.
pub fn load() -> Option<Battery> {
    list().into_iter().next()
}

/// Every battery, in name order.
pub fn list() -> Vec<Battery> {
    let Ok(entries) = fs::read_dir("/sys/class/power_supply") else {
        return Vec::new();
    };

    let mut out: Vec<Battery> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            // Mains adapters live in the same directory; only batteries have
            // a "Battery" type.
            let kind = read_trimmed(path.join("type"))?;
            if !kind.eq_ignore_ascii_case("battery") {
                return None;
            }
            read(&path, entry.file_name().to_string_lossy().as_ref())
        })
        .collect();

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn read(path: &Path, name: &str) -> Option<Battery> {
    let percent = match read_u64(path.join("capacity")) {
        Some(p) => p as f64,
        // Some drivers only expose the raw charge counters.
        None => {
            let now = charge_now(path)?;
            let full = charge_full(path)?;
            if full == 0 {
                return None;
            }
            now as f64 / full as f64 * 100.0
        }
    };

    // energy_* is in µWh and power_now in µW; charge_* is in µAh with a
    // separate voltage. Watts are only reported when the units line up.
    let watts = read_u64(path.join("power_now"))
        .map(|uw| uw as f64 / 1_000_000.0)
        .or_else(|| {
            let ua = read_u64(path.join("current_now"))? as f64;
            let uv = read_u64(path.join("voltage_now"))? as f64;
            Some(ua * uv / 1e12)
        });

    let health = match (charge_full(path), charge_full_design(path)) {
        (Some(full), Some(design)) if design > 0 => Some(full as f64 / design as f64 * 100.0),
        _ => None,
    };

    Some(Battery {
        name: name.to_string(),
        percent: percent.clamp(0.0, 100.0),
        status: read_trimmed(path.join("status")).unwrap_or_else(|| "Unknown".to_string()),
        watts,
        health,
    })
}

fn charge_now(path: &Path) -> Option<u64> {
    read_u64(path.join("energy_now")).or_else(|| read_u64(path.join("charge_now")))
}

fn charge_full(path: &Path) -> Option<u64> {
    read_u64(path.join("energy_full")).or_else(|| read_u64(path.join("charge_full")))
}

fn charge_full_design(path: &Path) -> Option<u64> {
    read_u64(path.join("energy_full_design")).or_else(|| read_u64(path.join("charge_full_design")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_reads_naturally() {
        let b = Battery {
            name: "BAT0".into(),
            percent: 87.4,
            status: "Charging".into(),
            watts: Some(21.42),
            health: None,
        };
        assert_eq!(b.label(), "87% (Charging, 21.4 W)");
        assert!(b.is_charging());
    }

    #[test]
    fn label_omits_what_is_missing() {
        let b = Battery {
            percent: 50.0,
            status: "Unknown".into(),
            ..Default::default()
        };
        assert_eq!(b.label(), "50%");

        let b = Battery {
            percent: 50.0,
            status: "Full".into(),
            ..Default::default()
        };
        assert_eq!(b.label(), "50% (Full)");
    }

    #[test]
    fn a_negligible_draw_is_not_shown() {
        let b = Battery {
            percent: 100.0,
            status: "Full".into(),
            watts: Some(0.0),
            ..Default::default()
        };
        assert_eq!(b.label(), "100% (Full)");
    }

    #[test]
    fn machines_without_a_battery_report_none() {
        // On a desktop this is None; on a laptop it is Some. Either way the
        // call must not panic or block.
        let _ = load();
    }
}

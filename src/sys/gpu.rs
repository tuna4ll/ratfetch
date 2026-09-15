//! Graphics adapters, identified through `/sys/class/drm` and the system's
//! `pci.ids` database when one is installed.

use std::fs;
use std::path::Path;

use super::read_trimmed;

/// A graphics adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gpu {
    pub card: String,
    pub vendor: String,
    pub model: String,
    /// The kernel driver bound to it, e.g. `amdgpu`.
    pub driver: String,
}

/// Live telemetry exposed by a DRM driver's sysfs nodes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuStats {
    pub card: String,
    pub usage_percent: Option<f64>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub temperature_celsius: Option<f64>,
    pub power_watts: Option<f64>,
}

impl Gpu {
    /// `AMD Radeon RX 7900 XTX`, or the best available approximation.
    pub fn label(&self) -> String {
        match (self.vendor.is_empty(), self.model.is_empty()) {
            (false, false) => format!("{} {}", self.vendor, self.model),
            (true, false) => self.model.clone(),
            (false, true) => self.vendor.clone(),
            (true, true) => "Unknown GPU".to_string(),
        }
    }
}

/// The vendors worth naming without consulting `pci.ids`.
fn vendor_name(id: u16) -> Option<&'static str> {
    Some(match id {
        0x10de => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        0x1af4 => "Red Hat",
        0x15ad => "VMware",
        0x1234 => "QEMU",
        0x5143 => "Qualcomm",
        0x13b5 => "ARM",
        0x1d17 => "Zhaoxin",
        _ => return None,
    })
}

/// Every adapter the kernel exposes.
pub fn list() -> Vec<Gpu> {
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };

    let db = PciIds::load();
    let mut out: Vec<Gpu> = Vec::new();

    let mut cards: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // cardN is the device itself; cardN-HDMI-A-1 is a connector.
            name.starts_with("card") && !name.contains('-')
        })
        .collect();
    cards.sort_by_key(|e| e.file_name());

    for card in cards {
        let device = card.path().join("device");
        let Some(mut gpu) = read_card(&device, db.as_ref()) else {
            continue;
        };
        gpu.card = card.file_name().to_string_lossy().into_owned();
        if !out
            .iter()
            .any(|known| known.vendor == gpu.vendor && known.model == gpu.model)
        {
            out.push(gpu);
        }
    }

    out
}

fn read_card(device: &Path, db: Option<&PciIds>) -> Option<Gpu> {
    let vendor_id = read_hex_id(device.join("vendor"))?;
    let device_id = read_hex_id(device.join("device"));

    let driver = fs::read_link(device.join("driver"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();

    let vendor = vendor_name(vendor_id)
        .map(str::to_string)
        .or_else(|| db.and_then(|d| d.vendor(vendor_id).map(str::to_string)))
        .unwrap_or_else(|| format!("{vendor_id:04x}"));

    let model = device_id
        .and_then(|id| db.and_then(|d| d.device(vendor_id, id)).map(str::to_string))
        .or_else(|| device_id.map(|id| format!("{id:04x}")))
        .unwrap_or_default();

    Some(Gpu {
        card: String::new(),
        vendor,
        model,
        driver,
    })
}

/// Reads every live field the active kernel drivers expose.
pub fn stats() -> Vec<GpuStats> {
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let card = entry.file_name().to_string_lossy().into_owned();
        if !card.starts_with("card") || card.contains('-') {
            continue;
        }
        let device = entry.path().join("device");
        let usage_percent = read_trimmed(device.join("gpu_busy_percent"))
            .and_then(|value| value.parse::<f64>().ok())
            .map(|value| value.clamp(0.0, 100.0));
        let memory_used = super::read_u64(device.join("mem_info_vram_used"));
        let memory_total = super::read_u64(device.join("mem_info_vram_total"));

        let mut temperature_celsius = None;
        let mut power_watts = None;
        if let Ok(hwmons) = fs::read_dir(device.join("hwmon")) {
            for hwmon in hwmons.flatten() {
                if temperature_celsius.is_none() {
                    temperature_celsius = super::read_u64(hwmon.path().join("temp1_input"))
                        .map(|milli| milli as f64 / 1000.0)
                        .filter(|value| (-50.0..=150.0).contains(value));
                }
                if power_watts.is_none() {
                    power_watts = super::read_u64(hwmon.path().join("power1_average"))
                        .or_else(|| super::read_u64(hwmon.path().join("power1_input")))
                        .map(|microwatts| microwatts as f64 / 1_000_000.0);
                }
            }
        }

        if usage_percent.is_some()
            || memory_used.is_some()
            || memory_total.is_some()
            || temperature_celsius.is_some()
            || power_watts.is_some()
        {
            out.push(GpuStats {
                card,
                usage_percent,
                memory_used,
                memory_total,
                temperature_celsius,
                power_watts,
            });
        }
    }
    out.sort_by(|a, b| a.card.cmp(&b.card));
    out
}

/// Reads a `0x10de`-style sysfs id.
fn read_hex_id(path: impl AsRef<Path>) -> Option<u16> {
    let text = read_trimmed(path)?;
    u16::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}

/// The subset of `pci.ids` needed to name an adapter.
struct PciIds {
    text: String,
}

impl PciIds {
    fn load() -> Option<Self> {
        for path in [
            "/usr/share/hwdata/pci.ids",
            "/usr/share/misc/pci.ids",
            "/usr/share/pci.ids",
        ] {
            if let Ok(text) = fs::read_to_string(path) {
                return Some(Self { text });
            }
        }
        None
    }

    fn vendor(&self, id: u16) -> Option<&str> {
        let prefix = format!("{id:04x}");
        self.text.lines().find_map(|line| {
            let rest = line.strip_prefix(&prefix)?;
            // Vendor rows start at column zero; device rows are indented.
            rest.strip_prefix("  ").map(str::trim)
        })
    }

    /// Device names are indented one tab under their vendor.
    fn device(&self, vendor: u16, device: u16) -> Option<&str> {
        let vendor_prefix = format!("{vendor:04x}  ");
        let device_prefix = format!("\t{device:04x}  ");

        let mut lines = self
            .text
            .lines()
            .skip_while(|l| !l.starts_with(&vendor_prefix));
        lines.next()?;

        for line in lines {
            // A new vendor block means the device was not listed.
            if !line.starts_with('\t') && !line.starts_with('#') && !line.trim().is_empty() {
                return None;
            }
            if let Some(name) = line.strip_prefix(&device_prefix) {
                return Some(name.trim());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDS: &str = "\
# comment
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t744c  Navi 31 [Radeon RX 7900 XT/7900 XTX]
\t164e  Raphael
10de  NVIDIA Corporation
\t2484  GA104 [GeForce RTX 3070]
";

    fn db() -> PciIds {
        PciIds {
            text: IDS.to_string(),
        }
    }

    #[test]
    fn looks_up_vendors_and_devices() {
        let db = db();
        assert_eq!(db.vendor(0x10de), Some("NVIDIA Corporation"));
        assert_eq!(db.device(0x10de, 0x2484), Some("GA104 [GeForce RTX 3070]"));
        assert_eq!(
            db.device(0x1002, 0x744c),
            Some("Navi 31 [Radeon RX 7900 XT/7900 XTX]")
        );
    }

    #[test]
    fn a_device_is_not_found_under_the_wrong_vendor() {
        let db = db();
        assert_eq!(
            db.device(0x1002, 0x2484),
            None,
            "belongs to NVIDIA, not AMD"
        );
        assert_eq!(db.device(0xffff, 0x0001), None);
        assert_eq!(db.vendor(0xffff), None);
    }

    #[test]
    fn known_vendors_get_a_short_name() {
        assert_eq!(vendor_name(0x10de), Some("NVIDIA"));
        assert_eq!(vendor_name(0x1002), Some("AMD"));
        assert_eq!(vendor_name(0x8086), Some("Intel"));
        assert_eq!(vendor_name(0xabcd), None);
    }

    #[test]
    fn label_degrades_gracefully() {
        let g = Gpu {
            card: "card0".into(),
            vendor: "AMD".into(),
            model: "Navi 31".into(),
            driver: "amdgpu".into(),
        };
        assert_eq!(g.label(), "AMD Navi 31");

        let g = Gpu {
            vendor: "AMD".into(),
            ..Default::default()
        };
        assert_eq!(g.label(), "AMD");

        assert_eq!(Gpu::default().label(), "Unknown GPU");
    }

    #[test]
    fn listing_never_panics() {
        // Headless machines and containers have no /sys/class/drm.
        let _ = list();
        let _ = stats();
    }
}

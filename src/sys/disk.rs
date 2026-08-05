//! Mounted filesystems, from `/proc/mounts` plus `statvfs`.

use std::collections::HashSet;
use std::ffi::CString;
use std::fs;

use super::percent;
use crate::config::model::Disks as DiskCfg;

/// One mounted filesystem.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Disk {
    pub device: String,
    pub mount: String,
    pub fstype: String,
    pub total: u64,
    pub used: u64,
    pub available: u64,
}

impl Disk {
    pub fn percent(&self) -> f64 {
        percent(self.used, self.total)
    }
}

/// A line of `/proc/mounts`, before `statvfs` fills in the sizes.
struct MountEntry {
    device: String,
    mount: String,
    fstype: String,
}

/// Parses `/proc/mounts`, decoding the octal escapes the kernel uses for
/// spaces and tabs in paths.
fn parse_mounts(text: &str) -> Vec<MountEntry> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let device = unescape(fields.next()?);
            let mount = unescape(fields.next()?);
            let fstype = fields.next()?.to_string();
            Some(MountEntry {
                device,
                mount,
                fstype,
            })
        })
        .collect()
}

/// `\040` and friends become the characters they stand for.
fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        // Exactly three octal digits, or a literal backslash.
        let digits: String = chars.clone().take(3).collect();
        match u8::from_str_radix(&digits, 8) {
            Ok(byte) if digits.len() == 3 => {
                for _ in 0..3 {
                    chars.next();
                }
                out.push(byte as char);
            }
            _ => out.push('\\'),
        }
    }
    out
}

/// Sizes for a mount point, or `None` if it cannot be stat'ed.
// The statvfs fields are u64 on 64-bit Linux but narrower elsewhere, so the
// casts are load-bearing on other targets even where clippy sees them as
// redundant here.
#[allow(clippy::unnecessary_cast)]
fn usage(mount: &str) -> Option<(u64, u64, u64)> {
    let path = CString::new(mount).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: statvfs writes only into the buffer we pass and reports failure.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };

    // f_frsize is the fragment size the block counts are expressed in.
    let block = if stat.f_frsize > 0 {
        stat.f_frsize
    } else {
        stat.f_bsize
    } as u64;
    let total = stat.f_blocks as u64 * block;
    // f_bavail excludes the root reserve, which is what `df` shows a user.
    let available = stat.f_bavail as u64 * block;
    let used = total.saturating_sub(stat.f_bfree as u64 * block);
    Some((total, used, available))
}

/// Lists the filesystems a config asks for.
pub fn list(cfg: &DiskCfg) -> Vec<Disk> {
    let text = fs::read_to_string("/proc/mounts").unwrap_or_default();
    collect(&parse_mounts(&text), cfg, usage)
}

/// The filtering half of [`list`], with `statvfs` injected so it can be tested.
fn collect(
    entries: &[MountEntry],
    cfg: &DiskCfg,
    usage: impl Fn(&str) -> Option<(u64, u64, u64)>,
) -> Vec<Disk> {
    let min_bytes = cfg.min_size_mib.saturating_mul(1024 * 1024);
    let mut seen_devices = HashSet::new();
    let mut out = Vec::new();

    for entry in entries {
        // An explicit list overrides every other filter.
        let explicit = !cfg.mounts.is_empty();
        if explicit {
            if !cfg.mounts.iter().any(|m| m == &entry.mount) {
                continue;
            }
        } else {
            if cfg.hide_types.iter().any(|t| t == &entry.fstype) {
                continue;
            }
            // Pseudo-filesystems have no backing device path.
            if !entry.device.starts_with('/') && !entry.fstype.starts_with("fuse") {
                continue;
            }
            // Bind mounts and btrfs subvolumes repeat a device; keep the first.
            if !seen_devices.insert(entry.device.clone()) {
                continue;
            }
        }

        let Some((total, used, available)) = usage(&entry.mount) else {
            continue;
        };
        if total == 0 || (!explicit && total < min_bytes) {
            continue;
        }

        out.push(Disk {
            device: entry.device.clone(),
            mount: entry.mount.clone(),
            fstype: entry.fstype.clone(),
            total,
            used,
            available,
        });
    }

    // Root first, then by mount point, so the list is stable between ticks.
    out.sort_by(|a, b| {
        (a.mount != "/")
            .cmp(&(b.mount != "/"))
            .then(a.mount.cmp(&b.mount))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTS: &str = "\
proc /proc proc rw,nosuid 0 0
/dev/nvme0n1p2 / ext4 rw,relatime 0 0
/dev/nvme0n1p1 /boot vfat rw,relatime 0 0
/dev/nvme0n1p2 /home ext4 rw,relatime 0 0
tmpfs /run tmpfs rw,nosuid 0 0
/dev/sda1 /mnt/my\\040disk ext4 rw 0 0
";

    fn cfg() -> DiskCfg {
        DiskCfg::default()
    }

    /// Every mount reports 100 GiB total, 40 GiB used.
    fn fake_usage(_: &str) -> Option<(u64, u64, u64)> {
        let gib = 1024 * 1024 * 1024;
        Some((100 * gib, 40 * gib, 60 * gib))
    }

    #[test]
    fn parses_mount_lines() {
        let entries = parse_mounts(MOUNTS);
        assert_eq!(entries.len(), 6);
        assert_eq!(entries[1].device, "/dev/nvme0n1p2");
        assert_eq!(entries[1].mount, "/");
        assert_eq!(entries[1].fstype, "ext4");
    }

    #[test]
    fn octal_escapes_are_decoded() {
        assert_eq!(unescape("/mnt/my\\040disk"), "/mnt/my disk");
        assert_eq!(unescape("/plain/path"), "/plain/path");
        assert_eq!(unescape("trailing\\"), "trailing\\");
    }

    #[test]
    fn pseudo_filesystems_and_duplicates_are_dropped() {
        let disks = collect(&parse_mounts(MOUNTS), &cfg(), fake_usage);
        let mounts: Vec<&str> = disks.iter().map(|d| d.mount.as_str()).collect();
        assert_eq!(mounts, vec!["/", "/boot", "/mnt/my disk"]);
        assert!(!mounts.contains(&"/proc"), "pseudo filesystems are hidden");
        assert!(
            !mounts.contains(&"/home"),
            "a repeated device is shown once"
        );
    }

    #[test]
    fn root_sorts_first() {
        let disks = collect(&parse_mounts(MOUNTS), &cfg(), fake_usage);
        assert_eq!(disks[0].mount, "/");
    }

    #[test]
    fn an_explicit_list_bypasses_the_filters() {
        let mut c = cfg();
        c.mounts = vec!["/proc".to_string(), "/home".to_string()];
        let disks = collect(&parse_mounts(MOUNTS), &c, fake_usage);
        let mounts: Vec<&str> = disks.iter().map(|d| d.mount.as_str()).collect();
        assert_eq!(mounts, vec!["/home", "/proc"]);
    }

    #[test]
    fn tiny_filesystems_are_skipped() {
        let mut c = cfg();
        c.min_size_mib = 1024;
        let small = |_: &str| Some((10 * 1024 * 1024, 0, 10 * 1024 * 1024));
        assert!(collect(&parse_mounts(MOUNTS), &c, small).is_empty());
    }

    #[test]
    fn unstattable_mounts_are_skipped_not_fatal() {
        let disks = collect(&parse_mounts(MOUNTS), &cfg(), |_| None);
        assert!(disks.is_empty());
    }

    #[test]
    fn percentage_is_used_over_total() {
        let d = Disk {
            total: 200,
            used: 50,
            ..Default::default()
        };
        assert_eq!(d.percent(), 25.0);
        assert_eq!(Disk::default().percent(), 0.0);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn reads_this_machine() {
        let disks = list(&cfg());
        assert!(
            !disks.is_empty(),
            "at least the root filesystem should show"
        );
        assert!(disks.iter().all(|d| d.total > 0));
    }
}

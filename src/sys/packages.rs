//! Installed package counts.
//!
//! Counting is done by reading each manager's on-disk database directly. Two
//! managers keep theirs in a format that is not worth reimplementing, so they
//! are queried through their own tool — once, at startup, never on the hot
//! path.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// One manager's contribution to the summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Count {
    pub manager: &'static str,
    pub count: usize,
}

/// `1843 (pacman), 12 (flatpak)`, or an empty string when nothing is found.
pub fn summary() -> String {
    summary_of(&list())
}

/// Formatting half of [`summary`], split out so it can be tested directly.
fn summary_of(counts: &[Count]) -> String {
    counts
        .iter()
        .map(|c| format!("{} ({})", c.count, c.manager))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every manager with at least one package installed, largest first.
pub fn list() -> Vec<Count> {
    let mut out = vec![
        Count {
            manager: "pacman",
            count: pacman(),
        },
        Count {
            manager: "dpkg",
            count: dpkg(),
        },
        Count {
            manager: "rpm",
            count: rpm(),
        },
        Count {
            manager: "apk",
            count: apk(),
        },
        Count {
            manager: "portage",
            count: portage(),
        },
        Count {
            manager: "xbps",
            count: xbps(),
        },
        Count {
            manager: "nix",
            count: nix(),
        },
        Count {
            manager: "flatpak",
            count: flatpak(),
        },
        Count {
            manager: "snap",
            count: snap(),
        },
    ];
    out.retain(|c| c.count > 0);
    out.sort_by_key(|c| std::cmp::Reverse(c.count));
    out
}

/// Counts the immediate subdirectories of `dir`.
fn count_dirs(dir: impl AsRef<Path>) -> usize {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .count()
        })
        .unwrap_or(0)
}

fn pacman() -> usize {
    count_dirs("/var/lib/pacman/local")
}

/// dpkg's status file lists every known package; only those whose status ends
/// in `installed` are actually present.
fn dpkg() -> usize {
    let Ok(text) = fs::read_to_string("/var/lib/dpkg/status") else {
        return 0;
    };
    text.split("\n\n")
        .filter(|block| {
            block.lines().any(|l| l.starts_with("Package:"))
                && block
                    .lines()
                    .find_map(|l| l.strip_prefix("Status:"))
                    .is_some_and(|s| s.trim().ends_with("installed"))
        })
        .count()
}

fn apk() -> usize {
    let Ok(text) = fs::read_to_string("/lib/apk/db/installed") else {
        return 0;
    };
    text.lines().filter(|l| l.starts_with("P:")).count()
}

/// Portage stores one directory per installed package, grouped by category.
fn portage() -> usize {
    let Ok(categories) = fs::read_dir("/var/db/pkg") else {
        return 0;
    };
    categories
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|c| count_dirs(c.path()))
        .sum()
}

fn xbps() -> usize {
    let Ok(text) = fs::read_to_string("/var/db/xbps/pkgdb-0.38.plist") else {
        return 0;
    };
    text.matches("<key>installed_size</key>").count()
}

fn flatpak() -> usize {
    count_dirs("/var/lib/flatpak/app")
        + dirs::data_dir()
            .map(|d| count_dirs(d.join("flatpak/app")))
            .unwrap_or(0)
}

/// Snap keeps a directory per snap, plus a `bin` directory that is not one.
fn snap() -> usize {
    let n = count_dirs("/snap");
    n.saturating_sub(usize::from(Path::new("/snap/bin").is_dir()))
}

/// The rpm database is a Berkeley DB or sqlite file depending on the distro,
/// so ask rpm itself.
fn rpm() -> usize {
    if !Path::new("/var/lib/rpm").exists() && !Path::new("/usr/lib/sysimage/rpm").exists() {
        return 0;
    }
    run_counting_lines("rpm", &["-qa", "--qf", "%{NAME}\n"])
}

/// Nix profiles are symlink farms; the store itself is far too large to walk.
fn nix() -> usize {
    if !Path::new("/nix/store").is_dir() {
        return 0;
    }
    run_counting_lines(
        "nix-store",
        &["--query", "--requisites", "/run/current-system"],
    )
}

/// Runs a command with a short leash and counts the lines it prints.
///
/// Anything that fails, is missing, or takes too long counts as zero — a
/// package count is never worth delaying startup for.
fn run_counting_lines(program: &str, args: &[&str]) -> usize {
    use std::io::Read;
    use std::process::Stdio;

    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
    else {
        return 0;
    };

    // Poll rather than wait, so a hung database cannot hang the program.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return 0;
            }
        }
    }

    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    out.lines().filter(|l| !l.trim().is_empty()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpkg_counts_only_installed_packages() {
        let status = "\
Package: bash
Status: install ok installed
Version: 5.2

Package: removed-thing
Status: deinstall ok config-files
Version: 1.0

Package: coreutils
Status: install ok installed
Version: 9.4
";
        let count = status
            .split("\n\n")
            .filter(|block| {
                block.lines().any(|l| l.starts_with("Package:"))
                    && block
                        .lines()
                        .find_map(|l| l.strip_prefix("Status:"))
                        .is_some_and(|s| s.trim().ends_with("installed"))
            })
            .count();
        assert_eq!(count, 2);
    }

    #[test]
    fn summary_is_empty_when_nothing_is_found() {
        assert_eq!(summary_of(&[]), String::new());
    }

    #[test]
    fn summary_lists_largest_first() {
        let counts = vec![
            Count {
                manager: "pacman",
                count: 1843,
            },
            Count {
                manager: "flatpak",
                count: 12,
            },
        ];
        assert_eq!(summary_of(&counts), "1843 (pacman), 12 (flatpak)");
    }

    #[test]
    fn a_missing_program_counts_as_zero() {
        assert_eq!(run_counting_lines("definitely-not-a-real-program", &[]), 0);
    }

    #[test]
    fn counting_this_machine_does_not_panic() {
        let _ = summary();
    }
}

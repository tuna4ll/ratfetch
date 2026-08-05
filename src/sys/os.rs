//! Distribution, kernel and account facts.

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs;

use super::{env, read_trimmed};

/// The parsed contents of `/etc/os-release`.
#[derive(Debug, Clone, Default)]
pub struct Release {
    fields: HashMap<String, String>,
}

impl Release {
    /// Reads `/etc/os-release`, falling back to the `/usr/lib` copy that
    /// stateless systems ship.
    pub fn load() -> Self {
        for path in ["/etc/os-release", "/usr/lib/os-release"] {
            if let Ok(text) = fs::read_to_string(path) {
                return Self::parse(&text);
            }
        }
        Self::default()
    }

    fn parse(text: &str) -> Self {
        let mut fields = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            // Values may be bare, or single- or double-quoted.
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            fields.insert(key.trim().to_string(), value.to_string());
        }
        Self { fields }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// The name shown in the info table, e.g. `Arch Linux x86_64`.
    pub fn pretty_name(&self) -> String {
        let name = self
            .get("PRETTY_NAME")
            .or_else(|| self.get("NAME"))
            .unwrap_or("Linux")
            .to_string();
        match arch() {
            Some(arch) => format!("{name} {arch}"),
            None => name,
        }
    }

    /// The id used to select a logo.
    ///
    /// `ID` is preferred, but `ID_LIKE` is a useful fallback for the many
    /// derivatives that have no art of their own.
    pub fn logo_id(&self) -> String {
        if let Some(id) = self.get("ID") {
            if crate::logo::find(id).is_some() {
                return id.to_string();
            }
        }
        if let Some(likes) = self.get("ID_LIKE") {
            for like in likes.split_whitespace() {
                if crate::logo::find(like).is_some() {
                    return like.to_string();
                }
            }
        }
        self.get("ID").unwrap_or("linux").to_string()
    }
}

/// `uname -m`, via `/proc/sys/kernel/arch` where the kernel exposes it.
fn arch() -> Option<String> {
    if let Some(a) = read_trimmed("/proc/sys/kernel/arch") {
        return Some(a);
    }
    let mut buf = std::mem::MaybeUninit::<libc::utsname>::uninit();
    // SAFETY: uname fills the buffer it is handed and reports failure.
    let ok = unsafe { libc::uname(buf.as_mut_ptr()) } == 0;
    if !ok {
        return None;
    }
    let uts = unsafe { buf.assume_init() };
    Some(c_str(&uts.machine))
}

/// The running kernel release, e.g. `6.9.3-arch1-1`.
pub fn kernel() -> String {
    read_trimmed("/proc/sys/kernel/osrelease").unwrap_or_else(|| "unknown".to_string())
}

/// The system's hostname.
pub fn hostname() -> String {
    read_trimmed("/proc/sys/kernel/hostname")
        .or_else(|| read_trimmed("/etc/hostname"))
        .unwrap_or_else(|| "localhost".to_string())
}

/// The current user's login name.
pub fn username() -> String {
    if let Some(u) = env("USER").or_else(|| env("LOGNAME")) {
        return u;
    }
    // SAFETY: getpwuid returns a pointer into a static buffer, or null.
    unsafe {
        let pw = libc::getpwuid(libc::geteuid());
        if !pw.is_null() && !(*pw).pw_name.is_null() {
            if let Ok(name) = CStr::from_ptr((*pw).pw_name).to_str() {
                return name.to_string();
            }
        }
    }
    "user".to_string()
}

/// Seconds since boot.
pub fn uptime() -> u64 {
    if let Some(text) = read_trimmed("/proc/uptime") {
        if let Some(first) = text.split_whitespace().next() {
            if let Ok(secs) = first.parse::<f64>() {
                return secs as u64;
            }
        }
    }
    0
}

/// The 1, 5 and 15 minute load averages.
pub fn load_average() -> [f64; 3] {
    let mut out = [0.0; 3];
    if let Some(text) = read_trimmed("/proc/loadavg") {
        for (slot, field) in out.iter_mut().zip(text.split_whitespace()) {
            *slot = field.parse().unwrap_or(0.0);
        }
    }
    out
}

/// The active locale.
pub fn locale() -> String {
    env("LC_ALL")
        .or_else(|| env("LC_MESSAGES"))
        .or_else(|| env("LANG"))
        .or_else(|| {
            // Fall back to the system default when the shell exports nothing.
            let text = fs::read_to_string("/etc/locale.conf").ok()?;
            text.lines()
                .find_map(|l| l.trim().strip_prefix("LANG=").map(str::to_string))
                .map(|v| v.trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "C".to_string())
}

/// The name of PID 1.
pub fn init_system() -> String {
    read_trimmed("/proc/1/comm").unwrap_or_else(|| "unknown".to_string())
}

/// How many distinct users have a login session.
///
/// Read from `/run/systemd/sessions` where available, since parsing `utmp`
/// portably is more trouble than the number is worth.
pub fn logged_in_users() -> usize {
    let Ok(entries) = fs::read_dir("/run/systemd/sessions") else {
        return 1;
    };
    let mut uids = std::collections::HashSet::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        if let Ok(text) = fs::read_to_string(entry.path()) {
            if let Some(uid) = text.lines().find_map(|l| l.strip_prefix("UID=")) {
                uids.insert(uid.to_string());
            }
        }
    }
    uids.len().max(1)
}

/// Turns a NUL-terminated C char array into a String.
fn c_str(buf: &[libc::c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_handles_quoting_and_comments() {
        let r = Release::parse(
            r#"
# a comment

NAME="Arch Linux"
PRETTY_NAME="Arch Linux"
ID=arch
ID_LIKE='archlinux'
BUILD_ID=rolling
EMPTY=
"#,
        );
        assert_eq!(r.get("NAME"), Some("Arch Linux"));
        assert_eq!(r.get("ID"), Some("arch"));
        assert_eq!(r.get("ID_LIKE"), Some("archlinux"));
        assert_eq!(r.get("EMPTY"), None, "empty values read as absent");
        assert_eq!(r.get("MISSING"), None);
    }

    #[test]
    fn logo_id_prefers_id_then_id_like() {
        let r = Release::parse("ID=arch\n");
        assert_eq!(r.logo_id(), "arch");

        // A derivative with no art of its own falls back to what it is like.
        let r = Release::parse("ID=some-unpackaged-distro\nID_LIKE=debian\n");
        assert_eq!(r.logo_id(), "debian");

        // Nothing usable at all still yields the raw id, and the logo layer
        // turns that into the "unknown" art.
        let r = Release::parse("ID=nothing-matches-this\n");
        assert_eq!(r.logo_id(), "nothing-matches-this");

        assert_eq!(Release::default().logo_id(), "linux");
    }

    #[test]
    fn pretty_name_falls_back_to_name() {
        let r = Release::parse("NAME=\"Debian GNU/Linux\"\n");
        assert!(r.pretty_name().starts_with("Debian GNU/Linux"));
        assert!(Release::default().pretty_name().starts_with("Linux"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn reads_this_machine() {
        assert!(uptime() > 0);
        assert!(!kernel().is_empty());
        assert!(!hostname().is_empty());
        assert!(!username().is_empty());
        assert!(logged_in_users() >= 1);
        let load = load_average();
        assert!(load.iter().all(|l| *l >= 0.0));
    }
}

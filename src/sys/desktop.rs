//! Shell, terminal, desktop environment, window manager and display modes.

use std::fs;
use std::path::Path;

use super::{env, read_trimmed};

/// What the user is sitting in front of.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    pub shell: String,
    pub terminal: String,
    pub de: String,
    pub wm: String,
}

impl Environment {
    pub fn detect() -> Self {
        Self {
            shell: shell(),
            terminal: terminal(),
            de: de(),
            wm: wm(),
        }
    }
}

/// The login shell, by name.
fn shell() -> String {
    // $SHELL is the login shell, which is what a fetch tool is expected to
    // report even when run from a subshell.
    env("SHELL")
        .and_then(|p| {
            Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Terminal emulators recognised by process name.
const TERMINALS: [&str; 26] = [
    "alacritty",
    "kitty",
    "wezterm-gui",
    "wezterm",
    "foot",
    "footclient",
    "ghostty",
    "gnome-terminal-server",
    "konsole",
    "xfce4-terminal",
    "mate-terminal",
    "lxterminal",
    "terminator",
    "tilix",
    "urxvt",
    "urxvtd",
    "rxvt",
    "st",
    "xterm",
    "termite",
    "contour",
    "rio",
    "hyper",
    "tabby",
    "qterminal",
    "deepin-terminal",
];

/// Names that are shells or multiplexers rather than terminals, so the walk
/// up the process tree keeps going past them.
const TRANSPARENT: [&str; 12] = [
    "bash", "zsh", "fish", "sh", "dash", "ksh", "tcsh", "nu", "elvish", "tmux", "screen",
    "ratfetch",
];

/// The terminal emulator this process is running inside.
fn terminal() -> String {
    // Some emulators announce themselves and save the walk entirely.
    if let Some(program) = env("TERM_PROGRAM") {
        return program;
    }

    // Walk up the process tree until something looks like an emulator.
    let mut pid = unsafe { libc::getppid() };
    for _ in 0..12 {
        if pid <= 1 {
            break;
        }
        let Some(name) = read_trimmed(format!("/proc/{pid}/comm")) else {
            break;
        };

        if TERMINALS.contains(&name.as_str()) {
            return tidy_terminal(&name);
        }
        if !TRANSPARENT.contains(&name.as_str()) && !name.starts_with("login") {
            // An unrecognised non-shell ancestor is most likely the terminal.
            if pid != unsafe { libc::getppid() } {
                return tidy_terminal(&name);
            }
        }

        let Some(parent) = ppid(pid) else { break };
        pid = parent;
    }

    // Inside a bare TTY there is no emulator; $TERM is the honest answer.
    env("TERM").unwrap_or_else(|| "unknown".to_string())
}

fn tidy_terminal(name: &str) -> String {
    match name {
        "gnome-terminal-server" => "gnome-terminal".to_string(),
        "wezterm-gui" => "wezterm".to_string(),
        "footclient" => "foot".to_string(),
        "urxvtd" => "urxvt".to_string(),
        other => other.to_string(),
    }
}

/// The parent of a pid, from field four of `/proc/<pid>/stat`.
fn ppid(pid: libc::pid_t) -> Option<libc::pid_t> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The comm field is parenthesised and may contain spaces, so parsing has
    // to resume after the last ')'.
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// The desktop environment.
fn de() -> String {
    if let Some(current) = env("XDG_CURRENT_DESKTOP") {
        // The variable is colon-separated and often prefixed, e.g. "ubuntu:GNOME".
        if let Some(last) = current.split(':').next_back().filter(|s| !s.is_empty()) {
            return tidy_de(last);
        }
    }
    if let Some(session) = env("DESKTOP_SESSION") {
        let name = Path::new(&session)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(session);
        return tidy_de(&name);
    }
    String::new()
}

fn tidy_de(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "gnome" => "GNOME".to_string(),
        "kde" | "plasma" | "plasma5" | "plasma6" => "KDE Plasma".to_string(),
        "xfce" | "xfce4" => "Xfce".to_string(),
        "lxqt" => "LXQt".to_string(),
        "mate" => "MATE".to_string(),
        "cinnamon" | "x-cinnamon" => "Cinnamon".to_string(),
        "budgie" | "budgie-desktop" => "Budgie".to_string(),
        "deepin" => "Deepin".to_string(),
        "pantheon" => "Pantheon".to_string(),
        "cosmic" => "COSMIC".to_string(),
        _ => name.to_string(),
    }
}

/// Window managers and Wayland compositors recognised by process name.
const WMS: [&str; 30] = [
    "hyprland",
    "sway",
    "river",
    "niri",
    "wayfire",
    "labwc",
    "cosmic-comp",
    "kwin_wayland",
    "kwin_x11",
    "kwin",
    "mutter",
    "gnome-shell",
    "xfwm4",
    "openbox",
    "i3",
    "bspwm",
    "dwm",
    "awesome",
    "qtile",
    "xmonad",
    "herbstluftwm",
    "spectrwm",
    "leftwm",
    "berry",
    "icewm",
    "fluxbox",
    "jwm",
    "marco",
    "muffin",
    "weston",
];

/// The window manager or compositor.
fn wm() -> String {
    // Wayland compositors set this themselves, and it is authoritative.
    if let Some(name) =
        env("XDG_SESSION_DESKTOP").filter(|n| WMS.contains(&n.to_ascii_lowercase().as_str()))
    {
        return tidy_wm(&name);
    }
    if env("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return "Hyprland".to_string();
    }
    if env("SWAYSOCK").is_some() {
        return "sway".to_string();
    }

    // Otherwise look for a known compositor among the running processes.
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(pid) = name
                .to_str()
                .filter(|n| n.bytes().all(|b| b.is_ascii_digit()))
            else {
                continue;
            };
            let Some(comm) = read_trimmed(format!("/proc/{pid}/comm")) else {
                continue;
            };
            if WMS.contains(&comm.to_ascii_lowercase().as_str()) {
                return tidy_wm(&comm);
            }
        }
    }

    String::new()
}

fn tidy_wm(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "hyprland" => "Hyprland".to_string(),
        "kwin_wayland" | "kwin_x11" | "kwin" => "KWin".to_string(),
        "mutter" | "gnome-shell" => "Mutter".to_string(),
        "cosmic-comp" => "COSMIC".to_string(),
        _ => name.to_string(),
    }
}

/// Connected display modes, e.g. `2560x1440, 1920x1080`.
pub fn resolution() -> String {
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return String::new();
    };

    let mut modes: Vec<String> = Vec::new();
    let mut connectors: Vec<_> = entries.flatten().collect();
    connectors.sort_by_key(|e| e.file_name());

    for connector in connectors {
        let path = connector.path();
        // Only connectors that actually have something plugged in.
        if read_trimmed(path.join("status")).as_deref() != Some("connected") {
            continue;
        }
        // The first line of "modes" is the preferred, i.e. native, mode.
        if let Some(first) =
            read_trimmed(path.join("modes")).and_then(|m| m.lines().next().map(str::to_string))
        {
            if !modes.contains(&first) {
                modes.push(first);
            }
        }
    }

    modes.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_names_are_normalised() {
        assert_eq!(tidy_de("gnome"), "GNOME");
        assert_eq!(tidy_de("KDE"), "KDE Plasma");
        assert_eq!(tidy_de("xfce4"), "Xfce");
        assert_eq!(tidy_de("X-Cinnamon"), "Cinnamon");
        assert_eq!(tidy_de("Something Else"), "Something Else");
    }

    #[test]
    fn window_manager_names_are_normalised() {
        assert_eq!(tidy_wm("kwin_wayland"), "KWin");
        assert_eq!(tidy_wm("gnome-shell"), "Mutter");
        assert_eq!(tidy_wm("hyprland"), "Hyprland");
        assert_eq!(tidy_wm("dwm"), "dwm");
    }

    #[test]
    fn terminal_names_are_normalised() {
        assert_eq!(tidy_terminal("gnome-terminal-server"), "gnome-terminal");
        assert_eq!(tidy_terminal("wezterm-gui"), "wezterm");
        assert_eq!(tidy_terminal("kitty"), "kitty");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn ppid_walks_to_the_real_parent() {
        let me = std::process::id() as libc::pid_t;
        let parent = ppid(me).expect("this process has a parent");
        assert_eq!(parent, unsafe { libc::getppid() });
        assert_eq!(ppid(-1), None);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn detection_never_panics_or_blocks() {
        let e = Environment::detect();
        assert!(!e.shell.is_empty());
        assert!(!e.terminal.is_empty());
        let _ = resolution();
    }
}

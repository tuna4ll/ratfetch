//! The shape of `config.toml`.
//!
//! Every field carries a `#[serde(default)]`, so any layer may be a partial
//! document, and every struct is `deny_unknown_fields`, so typos are reported
//! instead of silently ignored.

use serde::{Deserialize, Serialize};

use super::color::ColorSpec;
use super::enums::{
    GraphKind, GraphStyle, InfoItem, LogoColorMode, LogoPosition, MeterKind, MeterStyle, PanelKind,
    ProcSort, Tab,
};
use super::keys::KeyBinding;

/// The fully resolved configuration.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub general: General,
    pub logo: Logo,
    pub theme: Theme,
    pub layout: Layout,
    pub info: Info,
    pub meters: Meters,
    pub graphs: Graphs,
    pub processes: Processes,
    pub disks: Disks,
    pub network: Network,
    pub keys: Keys,
}

// ---------------------------------------------------------------- general --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct General {
    /// How often the sampled values are refreshed, in milliseconds.
    pub interval_ms: u64,
    /// How many samples the graphs keep.
    pub history: usize,
    /// Render on the alternate screen, restoring the shell scrollback on exit.
    pub alt_screen: bool,
    /// React to mouse clicks and scrolling.
    pub mouse: bool,
    /// Show the key hint bar along the bottom.
    pub footer: bool,
    /// Show the tab strip along the top.
    pub tab_bar: bool,
    /// Which tabs exist, in order.
    pub tabs: Vec<Tab>,
    /// Tab shown at startup. Must appear in `tabs`.
    pub start_tab: Tab,
    /// Quit automatically after this many seconds. `0` disables it.
    pub exit_after: u64,
    /// Re-read the config file when it changes on disk.
    pub watch_config: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            interval_ms: 1000,
            history: 120,
            alt_screen: true,
            mouse: true,
            footer: true,
            tab_bar: true,
            tabs: vec![Tab::Overview, Tab::Processes, Tab::Disks, Tab::Network],
            start_tab: Tab::Overview,
            exit_after: 0,
            watch_config: true,
        }
    }
}

// ------------------------------------------------------------------- logo --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Logo {
    /// `auto` to detect the distro, a logo name such as `arch`, `none` to hide
    /// it, or `file:/path/to/art.txt` to load your own.
    pub source: String,
    /// Prefer the `*_small` variant when one exists.
    pub small: bool,
    /// Where the logo sits relative to the info table.
    pub position: LogoPosition,
    /// How the art is coloured.
    pub color_mode: LogoColorMode,
    /// Replace the logo's own palette. Empty keeps the vendored colours.
    pub colors: Vec<ColorSpec>,
    /// Blank columns kept between the art and the info table.
    pub padding: u16,
    /// Clamp the art's width. `0` means "as wide as the art is".
    pub max_width: u16,
    /// Clamp the art's height. `0` means "as tall as the art is".
    pub max_height: u16,
    /// Draw a border around the logo panel.
    pub border: bool,
}

impl Default for Logo {
    fn default() -> Self {
        Self {
            source: "auto".to_string(),
            small: false,
            position: LogoPosition::Left,
            color_mode: LogoColorMode::Logo,
            colors: Vec::new(),
            padding: 2,
            max_width: 0,
            max_height: 0,
            border: false,
        }
    }
}

// ------------------------------------------------------------------ theme --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Theme {
    /// A built-in theme name; see `ratfetch --list-themes`.
    pub name: String,
    /// Per-role overrides applied on top of the named theme.
    pub colors: Palette,
    /// Draw panel borders with rounded corners.
    pub rounded: bool,
    /// Draw panel borders at all.
    pub borders: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "catppuccin-mocha".to_string(),
            colors: Palette::default(),
            rounded: true,
            borders: true,
        }
    }
}

/// Every colour role the UI can paint. All optional: unset roles fall back to
/// the named theme.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Palette {
    pub background: Option<ColorSpec>,
    pub foreground: Option<ColorSpec>,
    pub accent: Option<ColorSpec>,
    pub muted: Option<ColorSpec>,
    pub border: Option<ColorSpec>,
    pub border_focus: Option<ColorSpec>,
    pub title: Option<ColorSpec>,
    pub key: Option<ColorSpec>,
    pub value: Option<ColorSpec>,
    pub good: Option<ColorSpec>,
    pub warn: Option<ColorSpec>,
    pub critical: Option<ColorSpec>,
    pub graph_primary: Option<ColorSpec>,
    pub graph_secondary: Option<ColorSpec>,
    pub tab_active: Option<ColorSpec>,
    pub tab_inactive: Option<ColorSpec>,
    pub footer: Option<ColorSpec>,
}

// ------------------------------------------------------------------ layout --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Layout {
    /// Panels stacked below the header, in order.
    pub panels: Vec<PanelKind>,
    /// Height of the logo + info header. `0` sizes it to the content.
    pub header_height: u16,
    /// Width of the logo column. `0` sizes it to the art.
    pub logo_width: u16,
    /// Fixed height per panel; `0` shares the leftover space evenly.
    pub panel_heights: PanelHeights,
    /// Outer margin around the whole frame.
    pub margin: u16,
    /// Fall back to a single stacked column below this terminal width.
    pub narrow_width: u16,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            panels: vec![PanelKind::Meters, PanelKind::Graphs],
            header_height: 0,
            logo_width: 0,
            panel_heights: PanelHeights::default(),
            margin: 0,
            narrow_width: 80,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct PanelHeights {
    pub meters: u16,
    pub graphs: u16,
    pub processes: u16,
    pub disks: u16,
    pub network: u16,
    pub colors: u16,
}

impl Default for PanelHeights {
    fn default() -> Self {
        Self {
            meters: 0,
            graphs: 0,
            processes: 0,
            disks: 0,
            network: 0,
            colors: 3,
        }
    }
}

// ------------------------------------------------------------------- info --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Info {
    /// Rows of the info table, in order.
    pub items: Vec<InfoItem>,
    /// Column width reserved for keys. `0` sizes it to the longest key.
    pub key_width: u16,
    /// Printed between key and value.
    pub separator: String,
    /// Text of the `separator` row, repeated to the table's width.
    pub rule: String,
    /// Show the `user@host` title row in bold.
    pub bold_title: bool,
    /// Hide rows whose value could not be determined.
    pub hide_empty: bool,
}

impl Default for Info {
    fn default() -> Self {
        Self {
            items: vec![
                InfoItem::Title,
                InfoItem::Separator,
                InfoItem::Os,
                InfoItem::Host,
                InfoItem::Kernel,
                InfoItem::Uptime,
                InfoItem::Packages,
                InfoItem::Shell,
                InfoItem::Terminal,
                InfoItem::De,
                InfoItem::Wm,
                InfoItem::Resolution,
                InfoItem::Cpu,
                InfoItem::Gpu,
                InfoItem::Memory,
                InfoItem::Swap,
                InfoItem::Disk,
                InfoItem::LocalIp,
                InfoItem::Battery,
                InfoItem::Locale,
            ],
            key_width: 0,
            separator: " ".to_string(),
            rule: "─".to_string(),
            bold_title: true,
            hide_empty: true,
        }
    }
}

// ----------------------------------------------------------------- meters --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Meters {
    pub items: Vec<MeterKind>,
    pub style: MeterStyle,
    /// Show the numeric percentage beside each bar.
    pub show_percent: bool,
    /// Show the raw figures (e.g. `7.2 GiB / 31.1 GiB`).
    pub show_values: bool,
    /// Percentage above which a meter turns `warn`.
    pub warn_at: u8,
    /// Percentage above which a meter turns `critical`.
    pub critical_at: u8,
    /// Column width reserved for meter labels.
    pub label_width: u16,
    /// Draw one meter per CPU core instead of a single average.
    pub per_core: bool,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            items: vec![
                MeterKind::Cpu,
                MeterKind::Memory,
                MeterKind::Swap,
                MeterKind::Disk,
            ],
            style: MeterStyle::Bar,
            show_percent: true,
            show_values: true,
            warn_at: 75,
            critical_at: 90,
            label_width: 8,
            per_core: false,
        }
    }
}

// ----------------------------------------------------------------- graphs --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Graphs {
    pub items: Vec<GraphKind>,
    pub style: GraphStyle,
    /// Show min/max/current in the graph title.
    pub show_stats: bool,
    /// Draw each graph inside its own bordered block.
    pub bordered: bool,
}

impl Default for Graphs {
    fn default() -> Self {
        Self {
            items: vec![GraphKind::Cpu, GraphKind::Memory, GraphKind::Network],
            style: GraphStyle::Sparkline,
            show_stats: true,
            bordered: true,
        }
    }
}

// -------------------------------------------------------------- processes --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Processes {
    /// Rows shown in the overview panel. The Processes tab always fills.
    pub count: usize,
    pub sort: ProcSort,
    /// Sort ascending instead of descending.
    pub ascending: bool,
    /// Show the full command line rather than just the process name.
    pub full_command: bool,
    /// Only show processes owned by the current user.
    pub only_mine: bool,
    /// Hide kernel threads.
    pub hide_kthreads: bool,
}

impl Default for Processes {
    fn default() -> Self {
        Self {
            count: 8,
            sort: ProcSort::Cpu,
            ascending: false,
            full_command: false,
            only_mine: false,
            hide_kthreads: true,
        }
    }
}

// ------------------------------------------------------------------ disks --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Disks {
    /// Mount points to show. Empty means "every real filesystem".
    pub mounts: Vec<String>,
    /// Filesystem types that are never interesting.
    pub hide_types: Vec<String>,
    /// Skip filesystems smaller than this many mebibytes.
    pub min_size_mib: u64,
    /// The mount whose usage feeds the `disk` meter and info row.
    pub primary: String,
}

impl Default for Disks {
    fn default() -> Self {
        Self {
            mounts: Vec::new(),
            hide_types: [
                "autofs",
                "bpf",
                "binfmt_misc",
                "cgroup",
                "cgroup2",
                "configfs",
                "debugfs",
                "devpts",
                "devtmpfs",
                "efivarfs",
                "fuse.gvfsd-fuse",
                "fusectl",
                "hugetlbfs",
                "mqueue",
                "proc",
                "pstore",
                "ramfs",
                "securityfs",
                "sysfs",
                "tracefs",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            min_size_mib: 64,
            primary: "/".to_string(),
        }
    }
}

// ---------------------------------------------------------------- network --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Network {
    /// Interfaces to show. Empty means "every interface that is up".
    pub interfaces: Vec<String>,
    /// Interface name prefixes that are never interesting.
    pub hide_prefixes: Vec<String>,
    /// Include the loopback interface.
    pub show_loopback: bool,
    /// The interface whose throughput feeds the network graph.
    pub primary: String,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            interfaces: Vec::new(),
            hide_prefixes: ["veth", "docker", "br-", "virbr", "tun", "tap"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            show_loopback: false,
            primary: "auto".to_string(),
        }
    }
}

// ------------------------------------------------------------------- keys --

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Keys {
    pub quit: Vec<KeyBinding>,
    pub reload: Vec<KeyBinding>,
    pub help: Vec<KeyBinding>,
    pub next_tab: Vec<KeyBinding>,
    pub prev_tab: Vec<KeyBinding>,
    pub scroll_down: Vec<KeyBinding>,
    pub scroll_up: Vec<KeyBinding>,
    pub sort_next: Vec<KeyBinding>,
    pub toggle_per_core: Vec<KeyBinding>,
    pub freeze: Vec<KeyBinding>,
}

impl Default for Keys {
    fn default() -> Self {
        let b = |specs: &[&str]| specs.iter().map(|s| s.parse().unwrap()).collect();
        Self {
            quit: b(&["q", "esc", "ctrl+c"]),
            reload: b(&["r"]),
            help: b(&["?", "f1"]),
            next_tab: b(&["tab", "right", "l"]),
            prev_tab: b(&["shift+tab", "left", "h"]),
            scroll_down: b(&["down", "j"]),
            scroll_up: b(&["up", "k"]),
            sort_next: b(&["s"]),
            toggle_per_core: b(&["c"]),
            freeze: b(&["space"]),
        }
    }
}

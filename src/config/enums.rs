//! The closed vocabularies used in the config file.
//!
//! Each one round-trips through a lowercase string, and a bad spelling comes
//! back with the list of accepted values plus a "did you mean" hint.

use std::fmt;
use std::str::FromStr;

/// Declares an enum whose TOML representation is a fixed set of strings.
///
/// Generates `ALL`, `as_str`, `FromStr`, `Display`, `Serialize` and
/// `Deserialize`, so adding a variant only means adding a line here.
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(
                $(#[$vmeta:meta])*
                $variant:ident => $lit:literal $(| $alias:literal)*
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )*
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [$name] = &[ $( $name::$variant, )* ];

            /// The canonical spelling.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( $name::$variant => $lit, )*
                }
            }

            /// Canonical spellings of every variant.
            pub fn names() -> Vec<&'static str> {
                Self::ALL.iter().map(|v| v.as_str()).collect()
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let key = s.trim().to_ascii_lowercase().replace(['-', ' '], "_");
                match key.as_str() {
                    $( $lit $(| $alias)* => Ok($name::$variant), )*
                    _ => {
                        let hint = match $crate::util::suggest(&key, Self::names()) {
                            Some(s) => format!(" (did you mean `{}`?)", s),
                            None => String::new(),
                        };
                        Err(format!(
                            "unknown {} `{}`{}; expected one of: {}",
                            stringify!($name),
                            s,
                            hint,
                            Self::names().join(", "),
                        ))
                    }
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

string_enum! {
    /// A top-level view.
    Tab {
        Overview  => "overview",
        Processes => "processes" | "procs",
        Disks     => "disks",
        Network   => "network" | "net",
    }
}

string_enum! {
    /// A stackable panel in the overview.
    PanelKind {
        Meters    => "meters",
        Graphs    => "graphs",
        Processes => "processes" | "procs",
        Disks     => "disks",
        Network   => "network" | "net",
        Colors    => "colors" | "palette",
    }
}

string_enum! {
    /// A row in the info table.
    InfoItem {
        Title      => "title",
        Separator  => "separator" | "rule",
        Blank      => "blank" | "break",
        Os         => "os" | "distro",
        Host       => "host" | "model",
        Kernel     => "kernel",
        Uptime     => "uptime",
        Packages   => "packages" | "pkgs",
        Shell      => "shell",
        Terminal   => "terminal" | "term",
        De         => "de",
        Wm         => "wm",
        Resolution => "resolution" | "display",
        Cpu        => "cpu",
        CpuTemp    => "cpu_temp" | "temperature" | "temp",
        Gpu        => "gpu",
        Memory     => "memory" | "ram",
        Swap       => "swap",
        Disk       => "disk",
        LocalIp    => "local_ip" | "ip",
        Battery    => "battery",
        LoadAvg    => "load_avg" | "load",
        Processes  => "processes" | "procs",
        Users      => "users",
        Locale     => "locale",
        DateTime   => "date_time" | "datetime" | "clock",
        Colors     => "colors" | "palette",
    }
}

string_enum! {
    /// A live percentage bar.
    MeterKind {
        Cpu     => "cpu",
        Memory  => "memory" | "ram",
        Swap    => "swap",
        Disk    => "disk",
        Battery => "battery",
        Load    => "load" | "load_avg",
    }
}

string_enum! {
    /// A time-series graph.
    GraphKind {
        Cpu     => "cpu",
        Memory  => "memory" | "ram",
        Swap    => "swap",
        Network => "network" | "net",
        Disk    => "disk" | "disk_io",
        Load    => "load" | "load_avg",
    }
}

string_enum! {
    /// How a meter is drawn.
    MeterStyle {
        Bar    => "bar",
        Line   => "line",
        Blocks => "blocks",
        Dots   => "dots",
    }
}

string_enum! {
    /// How a graph is drawn.
    GraphStyle {
        Sparkline => "sparkline",
        Braille   => "braille",
        Bars      => "bars",
    }
}

string_enum! {
    /// Where the logo sits.
    LogoPosition {
        Left  => "left",
        Right => "right",
        Top   => "top",
        None  => "none" | "off" | "hidden",
    }
}

string_enum! {
    /// How the logo art is coloured.
    LogoColorMode {
        /// Use the palette that ships with the art.
        Logo   => "logo" | "builtin",
        /// Paint the whole logo in the theme accent.
        Accent => "accent" | "theme",
        /// Paint the whole logo in the foreground colour.
        Mono   => "mono",
        /// Cycle the theme palette across the art's colour slots.
        Rainbow => "rainbow",
    }
}

string_enum! {
    /// The process table's sort column.
    ProcSort {
        Cpu    => "cpu",
        Memory => "memory" | "mem",
        Pid    => "pid",
        Name   => "name",
    }
}

impl ProcSort {
    /// The next column in the cycle, for the sort keybinding.
    pub fn next(self) -> Self {
        match self {
            Self::Cpu => Self::Memory,
            Self::Memory => Self::Pid,
            Self::Pid => Self::Name,
            Self::Name => Self::Cpu,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_round_trip() {
        for item in InfoItem::ALL {
            assert_eq!(item.as_str().parse::<InfoItem>().unwrap(), *item);
        }
        for tab in Tab::ALL {
            assert_eq!(tab.as_str().parse::<Tab>().unwrap(), *tab);
        }
    }

    #[test]
    fn aliases_and_case_are_accepted() {
        assert_eq!("RAM".parse::<InfoItem>().unwrap(), InfoItem::Memory);
        assert_eq!("date-time".parse::<InfoItem>().unwrap(), InfoItem::DateTime);
        assert_eq!("  Distro ".parse::<InfoItem>().unwrap(), InfoItem::Os);
        assert_eq!("net".parse::<Tab>().unwrap(), Tab::Network);
    }

    #[test]
    fn typos_get_a_suggestion() {
        let e = "kernal".parse::<InfoItem>().unwrap_err();
        assert!(e.contains("did you mean `kernel`"), "{e}");
        assert!(e.contains("expected one of"), "{e}");
    }

    #[test]
    fn sort_cycles_back_to_start() {
        let mut s = ProcSort::Cpu;
        for _ in 0..4 {
            s = s.next();
        }
        assert_eq!(s, ProcSort::Cpu);
    }
}

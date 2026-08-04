//! Built-in colour themes and the role table the UI actually paints with.
//!
//! A theme defines eight base colours; the seventeen UI roles are derived from
//! them, and `[theme.colors]` in the config can override any role afterwards.

use ratatui::style::Color;

use super::color::ColorSpec;
use super::model;

/// The base colours a theme has to supply.
struct Base {
    name: &'static str,
    bg: &'static str,
    fg: &'static str,
    muted: &'static str,
    accent: &'static str,
    alt: &'static str,
    good: &'static str,
    warn: &'static str,
    critical: &'static str,
}

/// Every theme `theme.name` accepts.
const BASES: &[Base] = &[
    Base {
        name: "catppuccin-mocha",
        bg: "#1e1e2e",
        fg: "#cdd6f4",
        muted: "#6c7086",
        accent: "#89b4fa",
        alt: "#cba6f7",
        good: "#a6e3a1",
        warn: "#f9e2af",
        critical: "#f38ba8",
    },
    Base {
        name: "catppuccin-latte",
        bg: "#eff1f5",
        fg: "#4c4f69",
        muted: "#9ca0b0",
        accent: "#1e66f5",
        alt: "#8839ef",
        good: "#40a02b",
        warn: "#df8e1d",
        critical: "#d20f39",
    },
    Base {
        name: "tokyonight",
        bg: "#1a1b26",
        fg: "#c0caf5",
        muted: "#565f89",
        accent: "#7aa2f7",
        alt: "#bb9af7",
        good: "#9ece6a",
        warn: "#e0af68",
        critical: "#f7768e",
    },
    Base {
        name: "gruvbox-dark",
        bg: "#282828",
        fg: "#ebdbb2",
        muted: "#928374",
        accent: "#83a598",
        alt: "#d3869b",
        good: "#b8bb26",
        warn: "#fabd2f",
        critical: "#fb4934",
    },
    Base {
        name: "gruvbox-light",
        bg: "#fbf1c7",
        fg: "#3c3836",
        muted: "#7c6f64",
        accent: "#076678",
        alt: "#8f3f71",
        good: "#79740e",
        warn: "#b57614",
        critical: "#9d0006",
    },
    Base {
        name: "nord",
        bg: "#2e3440",
        fg: "#d8dee9",
        muted: "#4c566a",
        accent: "#88c0d0",
        alt: "#b48ead",
        good: "#a3be8c",
        warn: "#ebcb8b",
        critical: "#bf616a",
    },
    Base {
        name: "dracula",
        bg: "#282a36",
        fg: "#f8f8f2",
        muted: "#6272a4",
        accent: "#bd93f9",
        alt: "#ff79c6",
        good: "#50fa7b",
        warn: "#f1fa8c",
        critical: "#ff5555",
    },
    Base {
        name: "everforest",
        bg: "#2d353b",
        fg: "#d3c6aa",
        muted: "#859289",
        accent: "#7fbbb3",
        alt: "#d699b6",
        good: "#a7c080",
        warn: "#dbbc7f",
        critical: "#e67e80",
    },
    Base {
        name: "rose-pine",
        bg: "#191724",
        fg: "#e0def4",
        muted: "#6e6a86",
        accent: "#9ccfd8",
        alt: "#c4a7e7",
        good: "#31748f",
        warn: "#f6c177",
        critical: "#eb6f92",
    },
    Base {
        name: "solarized-dark",
        bg: "#002b36",
        fg: "#93a1a1",
        muted: "#586e75",
        accent: "#268bd2",
        alt: "#6c71c4",
        good: "#859900",
        warn: "#b58900",
        critical: "#dc322f",
    },
    Base {
        name: "kanagawa",
        bg: "#1f1f28",
        fg: "#dcd7ba",
        muted: "#727169",
        accent: "#7e9cd8",
        alt: "#957fb8",
        good: "#98bb6c",
        warn: "#e6c384",
        critical: "#e46876",
    },
    // Uses only the 16 ANSI colours, so it inherits whatever the terminal is
    // set to. The safe choice over SSH and in TTYs without truecolour.
    Base {
        name: "ansi",
        bg: "default",
        fg: "default",
        muted: "bright_black",
        accent: "blue",
        alt: "magenta",
        good: "green",
        warn: "yellow",
        critical: "red",
    },
    // No colour at all, for screenshots and monochrome terminals.
    Base {
        name: "mono",
        bg: "default",
        fg: "default",
        muted: "bright_black",
        accent: "default",
        alt: "bright_black",
        good: "default",
        warn: "default",
        critical: "default",
    },
];

/// The colours the UI draws with, one per role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub background: ColorSpec,
    pub foreground: ColorSpec,
    pub accent: ColorSpec,
    pub muted: ColorSpec,
    pub border: ColorSpec,
    pub border_focus: ColorSpec,
    pub title: ColorSpec,
    pub key: ColorSpec,
    pub value: ColorSpec,
    pub good: ColorSpec,
    pub warn: ColorSpec,
    pub critical: ColorSpec,
    pub graph_primary: ColorSpec,
    pub graph_secondary: ColorSpec,
    pub tab_active: ColorSpec,
    pub tab_inactive: ColorSpec,
    pub footer: ColorSpec,
    /// Whether panel borders are drawn at all.
    pub borders: bool,
    /// Whether borders use rounded corners.
    pub rounded: bool,
}

impl Default for Theme {
    fn default() -> Self {
        resolve(&model::Theme::default())
    }
}

impl Theme {
    /// The colour a meter takes at `percent` full.
    pub fn level(&self, percent: f64, warn_at: u8, critical_at: u8) -> ColorSpec {
        if percent >= f64::from(critical_at) {
            self.critical
        } else if percent >= f64::from(warn_at) {
            self.warn
        } else {
            self.good
        }
    }
}

/// Whether `name` is a built-in theme.
pub fn exists(name: &str) -> bool {
    find(name).is_some()
}

/// Every built-in theme name.
pub fn names() -> Vec<&'static str> {
    BASES.iter().map(|b| b.name).collect()
}

/// Alias kept for the public API surface used by `--list-themes`.
pub static THEMES: &[&str] = &[
    "catppuccin-mocha",
    "catppuccin-latte",
    "tokyonight",
    "gruvbox-dark",
    "gruvbox-light",
    "nord",
    "dracula",
    "everforest",
    "rose-pine",
    "solarized-dark",
    "kanagawa",
    "ansi",
    "mono",
];

fn find(name: &str) -> Option<&'static Base> {
    let key = name.trim().to_ascii_lowercase().replace('_', "-");
    BASES.iter().find(|b| b.name == key)
}

/// Builds the role table for a config's theme section.
///
/// An unknown theme name falls back to the first built-in rather than failing;
/// [`super::validate`] rejects it earlier, so this only matters to callers
/// that build a `model::Theme` by hand.
pub fn resolve(cfg: &model::Theme) -> Theme {
    let base = find(&cfg.name).unwrap_or(&BASES[0]);
    let c = |s: &str| {
        s.parse::<ColorSpec>()
            .unwrap_or(ColorSpec::new(Color::Reset))
    };

    let bg = c(base.bg);
    let fg = c(base.fg);
    let muted = c(base.muted);
    let accent = c(base.accent);
    let alt = c(base.alt);

    let mut theme = Theme {
        background: bg,
        foreground: fg,
        accent,
        muted,
        border: muted,
        border_focus: accent,
        title: ColorSpec {
            color: accent.color,
            modifiers: accent.modifiers | ratatui::style::Modifier::BOLD,
        },
        key: accent,
        value: fg,
        good: c(base.good),
        warn: c(base.warn),
        critical: c(base.critical),
        graph_primary: accent,
        graph_secondary: alt,
        tab_active: accent,
        tab_inactive: muted,
        footer: muted,
        borders: cfg.borders,
        rounded: cfg.rounded,
    };

    // Per-role overrides from `[theme.colors]`.
    let o = &cfg.colors;
    macro_rules! apply {
        ($($field:ident),* $(,)?) => {
            $( if let Some(v) = o.$field { theme.$field = v; } )*
        };
    }
    apply!(
        background,
        foreground,
        accent,
        muted,
        border,
        border_focus,
        title,
        key,
        value,
        good,
        warn,
        critical,
        graph_primary,
        graph_secondary,
        tab_active,
        tab_inactive,
        footer,
    );

    theme
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_theme_resolves() {
        for name in names() {
            assert!(exists(name), "{name}");
            let cfg = model::Theme {
                name: name.to_string(),
                ..Default::default()
            };
            let _ = resolve(&cfg);
        }
    }

    #[test]
    fn the_public_list_matches_the_table() {
        assert_eq!(names(), THEMES.to_vec());
    }

    #[test]
    fn every_base_colour_is_parseable() {
        for b in BASES {
            for (role, s) in [
                ("bg", b.bg),
                ("fg", b.fg),
                ("muted", b.muted),
                ("accent", b.accent),
                ("alt", b.alt),
                ("good", b.good),
                ("warn", b.warn),
                ("critical", b.critical),
            ] {
                assert!(
                    s.parse::<ColorSpec>().is_ok(),
                    "{}: bad {role} `{s}`",
                    b.name
                );
            }
        }
    }

    #[test]
    fn names_are_matched_loosely() {
        assert!(exists("Catppuccin-Mocha"));
        assert!(exists("catppuccin_mocha"));
        assert!(exists("  nord  "));
        assert!(!exists("nordic"));
    }

    #[test]
    fn overrides_take_precedence() {
        let mut cfg = model::Theme::default();
        cfg.colors.accent = Some("#ff0000".parse().unwrap());
        let t = resolve(&cfg);
        assert_eq!(t.accent.color, Color::Rgb(255, 0, 0));
        // Roles derived from accent keep the theme's value unless overridden.
        assert_ne!(t.key.color, t.accent.color);
    }

    #[test]
    fn levels_step_at_the_thresholds() {
        let t = Theme::default();
        assert_eq!(t.level(10.0, 75, 90), t.good);
        assert_eq!(t.level(75.0, 75, 90), t.warn);
        assert_eq!(t.level(99.0, 75, 90), t.critical);
    }
}

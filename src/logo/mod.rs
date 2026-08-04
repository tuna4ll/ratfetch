//! The vendored ASCII logos and the renderer that colours them.
//!
//! Art files use fastfetch's convention: `$1` through `$9` switch to the
//! logo's Nth colour, `$$` is a literal dollar sign, and the colour in effect
//! carries across line breaks.

use std::fs;
use std::path::Path;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::config::color::ColorSpec;
use crate::config::enums::LogoColorMode;
use crate::config::theme::Theme;

/// One entry of the compiled-in logo table.
pub struct RawLogo {
    pub key: &'static str,
    pub names: &'static [&'static str],
    pub colors: &'static [&'static str],
    pub art: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/logo_data.rs"));

/// How many logos ship with the binary.
pub fn count() -> usize {
    LOGOS.len()
}

/// Every logo's canonical key, sorted.
pub fn keys() -> Vec<&'static str> {
    LOGOS.iter().map(|l| l.key).collect()
}

/// Every name and alias a logo answers to.
pub fn all_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = LOGOS.iter().flat_map(|l| l.names.iter().copied()).collect();
    names.sort_unstable_by_key(|n| n.to_ascii_lowercase());
    names.dedup();
    names
}

/// Art plus the palette it was drawn with.
#[derive(Debug, Clone)]
pub struct Logo {
    pub name: String,
    /// Raw art, markers included.
    pub art: String,
    /// The logo's own palette.
    pub colors: Vec<ColorSpec>,
}

/// Normalises a name the way `build.rs` normalised the alias table.
fn normalise(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Looks a logo up by any of its names, case- and punctuation-insensitively.
pub fn find(name: &str) -> Option<Logo> {
    let key = normalise(name);
    let idx = ALIASES
        .binary_search_by(|(a, _)| (*a).cmp(key.as_str()))
        .ok()?;
    Some(from_raw(&LOGOS[ALIASES[idx].1]))
}

fn from_raw(raw: &RawLogo) -> Logo {
    Logo {
        name: raw.names.first().copied().unwrap_or(raw.key).to_string(),
        art: raw.art.to_string(),
        colors: raw
            .colors
            .iter()
            .map(|c| c.parse().unwrap_or_default())
            .collect(),
    }
}

/// The fallback used when a distro has no art of its own.
pub fn unknown() -> Logo {
    find("unknown").unwrap_or_else(|| Logo {
        name: "unknown".to_string(),
        art: "  ?  ".to_string(),
        colors: vec![ColorSpec::default()],
    })
}

/// Resolves `logo.source` into art.
///
/// Accepts `none`, `auto` (resolved against `detected`), `file:<path>`, or a
/// logo name. Returns `None` when the logo is switched off.
pub fn resolve(source: &str, small: bool, detected: &str) -> Result<Option<Logo>, String> {
    let source = source.trim();

    if source.eq_ignore_ascii_case("none") || source.eq_ignore_ascii_case("off") {
        return Ok(None);
    }

    if let Some(path) = source.strip_prefix("file:") {
        let path = Path::new(path.trim());
        let art = fs::read_to_string(path)
            .map_err(|e| format!("cannot read logo file {}: {e}", path.display()))?;
        return Ok(Some(Logo {
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            art,
            colors: vec![ColorSpec::default()],
        }));
    }

    let wanted = if source.eq_ignore_ascii_case("auto") {
        detected
    } else {
        source
    };

    // A `_small` variant is preferred only when one actually exists.
    if small {
        if let Some(logo) = find(&format!("{wanted}_small")) {
            return Ok(Some(logo));
        }
    }

    if let Some(logo) = find(wanted) {
        return Ok(Some(logo));
    }

    if source.eq_ignore_ascii_case("auto") {
        // Detection produced something we have no art for; that is not an error.
        return Ok(Some(unknown()));
    }

    let hint = crate::util::suggest(wanted, all_names())
        .map(|s| format!(" (did you mean `{s}`?)"))
        .unwrap_or_default();
    Err(format!(
        "unknown logo `{source}`{hint}; try `ratfetch --list-logos`"
    ))
}

/// A single art line, split into runs that share a colour slot.
struct Run {
    slot: usize,
    text: String,
}

impl Logo {
    /// Art lines with the markers removed, for measuring.
    pub fn plain_lines(&self) -> Vec<String> {
        self.parse()
            .into_iter()
            .map(|runs| runs.into_iter().map(|r| r.text).collect())
            .collect()
    }

    /// Widest line, in display columns.
    pub fn width(&self) -> u16 {
        self.plain_lines()
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0)
            .min(u16::MAX as usize) as u16
    }

    /// Number of art lines.
    pub fn height(&self) -> u16 {
        self.parse().len().min(u16::MAX as usize) as u16
    }

    /// Splits the art into per-line colour runs.
    fn parse(&self) -> Vec<Vec<Run>> {
        let mut out = Vec::new();
        // fastfetch's colour state carries over line breaks.
        let mut slot = 0usize;

        for raw in self.art.lines() {
            let mut runs: Vec<Run> = Vec::new();
            let mut current = String::new();
            let mut chars = raw.chars().peekable();

            while let Some(ch) = chars.next() {
                if ch != '$' {
                    current.push(ch);
                    continue;
                }
                match chars.peek() {
                    Some('$') => {
                        chars.next();
                        current.push('$');
                    }
                    Some(d) if d.is_ascii_digit() && *d != '0' => {
                        let d = chars.next().unwrap();
                        if !current.is_empty() {
                            runs.push(Run {
                                slot,
                                text: std::mem::take(&mut current),
                            });
                        }
                        slot = d.to_digit(10).unwrap() as usize - 1;
                    }
                    // A lone `$` is literal.
                    _ => current.push('$'),
                }
            }
            if !current.is_empty() || runs.is_empty() {
                runs.push(Run {
                    slot,
                    text: current,
                });
            }
            out.push(runs);
        }
        out
    }

    /// Renders the art as styled lines.
    ///
    /// `overrides` replaces the logo's own palette when non-empty.
    pub fn render(
        &self,
        mode: LogoColorMode,
        theme: &Theme,
        overrides: &[ColorSpec],
    ) -> Vec<Line<'static>> {
        let palette: Vec<ColorSpec> = if !overrides.is_empty() {
            overrides.to_vec()
        } else if self.colors.is_empty() {
            vec![ColorSpec::default()]
        } else {
            self.colors.clone()
        };

        let rainbow = [
            theme.accent,
            theme.graph_secondary,
            theme.good,
            theme.warn,
            theme.critical,
            theme.foreground,
        ];

        let style_for = |slot: usize| -> Style {
            match mode {
                LogoColorMode::Mono => theme.foreground.fg(),
                LogoColorMode::Accent => theme.accent.fg(),
                LogoColorMode::Rainbow => rainbow[slot % rainbow.len()].fg(),
                LogoColorMode::Logo => {
                    // Slots beyond the palette wrap rather than vanish.
                    palette[slot % palette.len()].fg()
                }
            }
        };

        self.parse()
            .into_iter()
            .map(|runs| {
                Line::from(
                    runs.into_iter()
                        .map(|r| Span::styled(r.text, style_for(r.slot)))
                        .collect::<Vec<_>>(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_populated_and_sorted() {
        assert!(
            count() > 500,
            "expected the full fastfetch set, got {}",
            count()
        );
        assert!(
            ALIASES.windows(2).all(|w| w[0].0 < w[1].0),
            "alias table must be sorted"
        );
    }

    #[test]
    fn lookup_is_forgiving_about_spelling() {
        assert!(find("arch").is_some());
        assert!(find("Arch").is_some());
        assert!(find("ARCH").is_some());
        assert!(find("pop_os").is_some());
        assert!(find("Pop!_OS").is_some());
        assert!(find("definitely-not-a-distro").is_none());
    }

    #[test]
    fn aliases_reach_the_same_art() {
        let a = find("macOS").unwrap();
        let b = find("OSX").unwrap();
        assert_eq!(a.art, b.art);
    }

    #[test]
    fn markers_are_stripped_from_plain_text() {
        let logo = Logo {
            name: "t".into(),
            art: "$1ab$2cd\nef".into(),
            colors: vec![ColorSpec::default(); 2],
        };
        assert_eq!(
            logo.plain_lines(),
            vec!["abcd".to_string(), "ef".to_string()]
        );
        assert_eq!(logo.width(), 4);
        assert_eq!(logo.height(), 2);
    }

    #[test]
    fn double_dollar_is_a_literal() {
        let logo = Logo {
            name: "t".into(),
            art: "a$$b".into(),
            colors: vec![],
        };
        assert_eq!(logo.plain_lines(), vec!["a$b".to_string()]);
    }

    #[test]
    fn colour_state_carries_across_lines() {
        let logo = Logo {
            name: "t".into(),
            art: "$2xx\nyy".into(),
            colors: vec![ColorSpec::default(); 2],
        };
        let parsed = logo.parse();
        assert_eq!(parsed[0][0].slot, 1);
        assert_eq!(parsed[1][0].slot, 1, "second line keeps the colour");
    }

    #[test]
    fn every_logo_parses_and_has_art() {
        for raw in LOGOS.iter() {
            let logo = from_raw(raw);
            assert!(!logo.art.is_empty(), "{} has no art", raw.key);
            assert!(logo.width() > 0, "{} measured as zero width", raw.key);
            // Rendering must not panic on any vendored file.
            let _ = logo.render(LogoColorMode::Logo, &Theme::default(), &[]);
        }
    }

    #[test]
    fn resolve_handles_every_source_form() {
        assert!(resolve("none", false, "arch").unwrap().is_none());
        assert_eq!(
            resolve("auto", false, "arch").unwrap().unwrap().name,
            "arch"
        );
        assert!(resolve("nixos", false, "arch").unwrap().is_some());
        // Unknown detection falls back rather than failing.
        assert!(resolve("auto", false, "no-such-distro").unwrap().is_some());
        assert!(resolve("no-such-distro", false, "arch").is_err());
    }

    #[test]
    fn small_falls_back_when_there_is_no_small_variant() {
        let small = resolve("arch", true, "arch").unwrap().unwrap();
        assert!(small.height() < find("arch").unwrap().height());
        // AIX has no _small variant; asking for one must still work.
        assert!(resolve("aix", true, "aix").unwrap().is_some());
    }

    #[test]
    fn slots_beyond_the_palette_wrap() {
        let logo = Logo {
            name: "t".into(),
            art: "$9x".into(),
            colors: vec![ColorSpec::default()],
        };
        let lines = logo.render(LogoColorMode::Logo, &Theme::default(), &[]);
        assert_eq!(lines.len(), 1);
    }
}

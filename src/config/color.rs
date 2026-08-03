//! Colour values as they appear in config files and in the vendored logo
//! metadata.
//!
//! Accepted spellings:
//!
//! | form            | example        |
//! |-----------------|----------------|
//! | named           | `red`, `bright_blue` |
//! | terminal default| `default`      |
//! | 256-colour      | `256:141`      |
//! | hex truecolour  | `#89b4fa`      |
//! | rgb triple      | `rgb:137,180,250` |
//!
//! Any of them may carry a leading `bold ` / `dim ` / `italic ` modifier, e.g.
//! `bold #f38ba8`.

use std::fmt;
use std::str::FromStr;

use ratatui::style::{Color, Modifier, Style};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

/// A colour plus the text modifiers that were written alongside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorSpec {
    pub color: Color,
    pub modifiers: Modifier,
}

impl ColorSpec {
    pub const fn new(color: Color) -> Self {
        Self {
            color,
            modifiers: Modifier::empty(),
        }
    }

    /// The spec as a foreground style.
    pub fn fg(self) -> Style {
        Style::default().fg(self.color).add_modifier(self.modifiers)
    }

    /// The spec as a background style.
    pub fn bg(self) -> Style {
        Style::default().bg(self.color).add_modifier(self.modifiers)
    }
}

impl Default for ColorSpec {
    fn default() -> Self {
        Self::new(Color::Reset)
    }
}

impl From<Color> for ColorSpec {
    fn from(color: Color) -> Self {
        Self::new(color)
    }
}

/// Why a colour string could not be understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseColorError {
    input: String,
    reason: String,
}

impl fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid colour `{}`: {}", self.input, self.reason)
    }
}

impl std::error::Error for ParseColorError {}

impl FromStr for ColorSpec {
    type Err = ParseColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = |reason: &str| ParseColorError {
            input: s.to_string(),
            reason: reason.into(),
        };

        let mut modifiers = Modifier::empty();
        let mut rest = s.trim();

        // Peel off any leading modifier words.
        while let Some((word, tail)) = rest.split_once(char::is_whitespace) {
            let tail = tail.trim_start();
            match word.to_ascii_lowercase().as_str() {
                "bold" => modifiers |= Modifier::BOLD,
                "dim" => modifiers |= Modifier::DIM,
                "italic" => modifiers |= Modifier::ITALIC,
                "underline" | "underlined" => modifiers |= Modifier::UNDERLINED,
                _ => break,
            }
            rest = tail;
        }

        if rest.is_empty() {
            return Err(err("no colour after the modifiers"));
        }

        let color = if let Some(hex) = rest.strip_prefix('#') {
            parse_hex(hex).ok_or_else(|| err("expected 3 or 6 hex digits after `#`"))?
        } else if let Some(idx) = rest.strip_prefix("256:") {
            let n: u8 = idx
                .trim()
                .parse()
                .map_err(|_| err("expected a number in 0..=255 after `256:`"))?;
            Color::Indexed(n)
        } else if let Some(triple) = rest.strip_prefix("rgb:") {
            parse_rgb(triple)
                .ok_or_else(|| err("expected `rgb:R,G,B` with each part in 0..=255"))?
        } else {
            named(rest).ok_or_else(|| {
                err(&format!(
                    "unknown colour name{}",
                    match suggest_name(rest) {
                        Some(s) => format!(" (did you mean `{s}`?)"),
                        None => String::new(),
                    }
                ))
            })?
        };

        Ok(Self { color, modifiers })
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim();
    let expand = |c: u8| c * 17;
    match hex.len() {
        3 => {
            let mut it = hex.chars().map(|c| c.to_digit(16).map(|d| expand(d as u8)));
            Some(Color::Rgb(it.next()??, it.next()??, it.next()??))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

fn parse_rgb(triple: &str) -> Option<Color> {
    let mut parts = triple.split(',').map(|p| p.trim().parse::<u8>());
    let r = parts.next()?.ok()?;
    let g = parts.next()?.ok()?;
    let b = parts.next()?.ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Color::Rgb(r, g, b))
}

/// Names accepted for the 16 ANSI colours, plus `default`.
pub const NAMES: [(&str, Color); 19] = [
    ("default", Color::Reset),
    ("reset", Color::Reset),
    ("black", Color::Black),
    ("red", Color::Red),
    ("green", Color::Green),
    ("yellow", Color::Yellow),
    ("blue", Color::Blue),
    ("magenta", Color::Magenta),
    ("cyan", Color::Cyan),
    ("white", Color::Gray),
    ("gray", Color::DarkGray),
    ("bright_black", Color::DarkGray),
    ("bright_red", Color::LightRed),
    ("bright_green", Color::LightGreen),
    ("bright_yellow", Color::LightYellow),
    ("bright_blue", Color::LightBlue),
    ("bright_magenta", Color::LightMagenta),
    ("bright_cyan", Color::LightCyan),
    ("bright_white", Color::White),
];

fn named(s: &str) -> Option<Color> {
    let key = s.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    NAMES.iter().find(|(n, _)| *n == key).map(|(_, c)| *c)
}

/// Closest known colour name, for "did you mean" hints.
fn suggest_name(input: &str) -> Option<&'static str> {
    let key = input.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    NAMES
        .iter()
        .map(|(n, _)| (*n, crate::util::edit_distance(&key, n)))
        .filter(|(_, d)| *d <= 3)
        .min_by_key(|(_, d)| *d)
        .map(|(n, _)| n)
}

impl fmt::Display for ColorSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (flag, word) in [
            (Modifier::BOLD, "bold"),
            (Modifier::DIM, "dim"),
            (Modifier::ITALIC, "italic"),
            (Modifier::UNDERLINED, "underline"),
        ] {
            if self.modifiers.contains(flag) {
                write!(f, "{word} ")?;
            }
        }
        match self.color {
            Color::Rgb(r, g, b) => write!(f, "#{r:02x}{g:02x}{b:02x}"),
            Color::Indexed(i) => write!(f, "256:{i}"),
            other => {
                let name = NAMES
                    .iter()
                    .find(|(_, c)| *c == other)
                    .map(|(n, _)| *n)
                    .unwrap_or("default");
                f.write_str(name)
            }
        }
    }
}

impl<'de> Deserialize<'de> for ColorSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(de::Error::custom)
    }
}

impl Serialize for ColorSpec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_documented_form() {
        assert_eq!("red".parse::<ColorSpec>().unwrap().color, Color::Red);
        assert_eq!("default".parse::<ColorSpec>().unwrap().color, Color::Reset);
        assert_eq!(
            "256:141".parse::<ColorSpec>().unwrap().color,
            Color::Indexed(141)
        );
        assert_eq!(
            "#89b4fa".parse::<ColorSpec>().unwrap().color,
            Color::Rgb(0x89, 0xb4, 0xfa)
        );
        assert_eq!(
            "rgb:137,180,250".parse::<ColorSpec>().unwrap().color,
            Color::Rgb(137, 180, 250)
        );
        assert_eq!(
            "#abc".parse::<ColorSpec>().unwrap().color,
            Color::Rgb(0xaa, 0xbb, 0xcc)
        );
    }

    #[test]
    fn parses_modifiers() {
        let c: ColorSpec = "bold italic #ff0000".parse().unwrap();
        assert_eq!(c.color, Color::Rgb(255, 0, 0));
        assert!(c.modifiers.contains(Modifier::BOLD));
        assert!(c.modifiers.contains(Modifier::ITALIC));
    }

    #[test]
    fn rejects_nonsense_and_suggests() {
        assert!("#gg0000".parse::<ColorSpec>().is_err());
        assert!("256:999".parse::<ColorSpec>().is_err());
        assert!("rgb:1,2".parse::<ColorSpec>().is_err());
        let e = "bleu".parse::<ColorSpec>().unwrap_err().to_string();
        assert!(e.contains("did you mean `blue`"), "{e}");
    }

    #[test]
    fn display_round_trips() {
        for s in ["red", "256:141", "#89b4fa", "bold #ff0000"] {
            let c: ColorSpec = s.parse().unwrap();
            assert_eq!(c.to_string().parse::<ColorSpec>().unwrap(), c);
        }
    }
}

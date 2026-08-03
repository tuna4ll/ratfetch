//! Key bindings, written in config as `ctrl+c`, `shift+tab`, `q`, `f1`, ...

use std::fmt;
use std::str::FromStr;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One chord: a key plus the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    /// Whether a key event triggers this binding.
    ///
    /// A binding written as a bare uppercase letter (`G`) is matched without
    /// requiring an explicit shift modifier, because terminals already report
    /// the shifted character.
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        let mut wanted = self.modifiers;
        let mut got = ev.modifiers;

        if matches!(self.code, KeyCode::Char(c) if c.is_ascii_uppercase()) {
            wanted.remove(KeyModifiers::SHIFT);
            got.remove(KeyModifiers::SHIFT);
        }
        // Terminals do not consistently report these; ignore them entirely.
        let ignored = KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META;
        got.remove(ignored);
        wanted.remove(ignored);

        self.code == ev.code && wanted == got
    }
}

/// Names accepted for keys that are not a single character.
const SPECIAL: [(&str, KeyCode); 18] = [
    ("esc", KeyCode::Esc),
    ("escape", KeyCode::Esc),
    ("enter", KeyCode::Enter),
    ("return", KeyCode::Enter),
    ("tab", KeyCode::Tab),
    ("backtab", KeyCode::BackTab),
    ("space", KeyCode::Char(' ')),
    ("backspace", KeyCode::Backspace),
    ("delete", KeyCode::Delete),
    ("del", KeyCode::Delete),
    ("insert", KeyCode::Insert),
    ("home", KeyCode::Home),
    ("end", KeyCode::End),
    ("pageup", KeyCode::PageUp),
    ("pagedown", KeyCode::PageDown),
    ("up", KeyCode::Up),
    ("down", KeyCode::Down),
    ("left", KeyCode::Left),
];

impl FromStr for KeyBinding {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw = s.trim();
        if raw.is_empty() {
            return Err("empty key binding".to_string());
        }

        let mut modifiers = KeyModifiers::empty();

        // `+` separates modifiers from the key, but is also a key in its own
        // right, so a trailing `+` is the key rather than a separator.
        let (mods_str, key) = if raw == "+" {
            ("", "+")
        } else if let Some(head) = raw.strip_suffix('+') {
            (head.strip_suffix('+').unwrap_or(head), "+")
        } else {
            raw.rsplit_once('+').unwrap_or(("", raw))
        };

        let mods: Vec<&str> = mods_str
            .split('+')
            .filter(|m| !m.trim().is_empty())
            .collect();
        for m in &mods {
            match m.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "c" => modifiers |= KeyModifiers::CONTROL,
                "alt" | "meta" | "m" => modifiers |= KeyModifiers::ALT,
                "shift" | "s" => modifiers |= KeyModifiers::SHIFT,
                other => return Err(format!("unknown modifier `{other}` in `{raw}`")),
            }
        }

        let key_l = key.trim().to_ascii_lowercase();
        let code = if let Some((_, c)) = SPECIAL.iter().find(|(n, _)| *n == key_l) {
            *c
        } else if key_l == "right" {
            KeyCode::Right
        } else if let Some(n) = key_l.strip_prefix('f') {
            match n.parse::<u8>() {
                Ok(n @ 1..=12) => KeyCode::F(n),
                _ => single_char(key.trim(), raw)?,
            }
        } else {
            single_char(key.trim(), raw)?
        };

        // `shift+tab` is reported by terminals as BackTab.
        if code == KeyCode::Tab && modifiers.contains(KeyModifiers::SHIFT) {
            return Ok(Self {
                code: KeyCode::BackTab,
                modifiers: modifiers ^ KeyModifiers::SHIFT,
            });
        }

        Ok(Self { code, modifiers })
    }
}

fn single_char(key: &str, raw: &str) -> Result<KeyCode, String> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(KeyCode::Char(c)),
        _ => {
            let hint = crate::util::suggest(key, SPECIAL.iter().map(|(n, _)| *n))
                .map(|s| format!(" (did you mean `{s}`?)"))
                .unwrap_or_default();
            Err(format!("`{raw}` is not a key: expected one character, `f1`..`f12`, or a name such as `esc`{hint}"))
        }
    }
}

impl fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            f.write_str("ctrl+")?;
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            f.write_str("alt+")?;
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            f.write_str("shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("space"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::F(n) => write!(f, "f{n}"),
            KeyCode::BackTab => f.write_str("shift+tab"),
            KeyCode::Right => f.write_str("right"),
            other => {
                let name = SPECIAL
                    .iter()
                    .find(|(_, c)| *c == other)
                    .map(|(n, _)| *n)
                    .unwrap_or("?");
                f.write_str(name)
            }
        }
    }
}

impl<'de> Deserialize<'de> for KeyBinding {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Serialize for KeyBinding {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// Renders a binding list the way the footer and help screen show it.
pub fn render_list(bindings: &[KeyBinding]) -> String {
    bindings
        .iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn parses_plain_and_modified_keys() {
        assert_eq!("q".parse::<KeyBinding>().unwrap().code, KeyCode::Char('q'));
        assert_eq!("esc".parse::<KeyBinding>().unwrap().code, KeyCode::Esc);
        assert_eq!("f1".parse::<KeyBinding>().unwrap().code, KeyCode::F(1));
        assert_eq!(
            "space".parse::<KeyBinding>().unwrap().code,
            KeyCode::Char(' ')
        );

        let c = "ctrl+c".parse::<KeyBinding>().unwrap();
        assert_eq!(c.code, KeyCode::Char('c'));
        assert_eq!(c.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn shift_tab_becomes_backtab() {
        let b = "shift+tab".parse::<KeyBinding>().unwrap();
        assert_eq!(b.code, KeyCode::BackTab);
        assert_eq!(b.modifiers, KeyModifiers::empty());
    }

    #[test]
    fn literal_plus_is_a_key() {
        assert_eq!("+".parse::<KeyBinding>().unwrap().code, KeyCode::Char('+'));
        let b = "ctrl++".parse::<KeyBinding>().unwrap();
        assert_eq!(b.code, KeyCode::Char('+'));
        assert_eq!(b.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn matching_ignores_shift_for_uppercase_letters() {
        let b: KeyBinding = "G".parse().unwrap();
        assert!(b.matches(&ev(KeyCode::Char('G'), KeyModifiers::SHIFT)));
        assert!(b.matches(&ev(KeyCode::Char('G'), KeyModifiers::NONE)));
        assert!(!b.matches(&ev(KeyCode::Char('g'), KeyModifiers::NONE)));
    }

    #[test]
    fn matching_is_exact_about_ctrl() {
        let b: KeyBinding = "c".parse().unwrap();
        assert!(b.matches(&ev(KeyCode::Char('c'), KeyModifiers::NONE)));
        assert!(!b.matches(&ev(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn rejects_nonsense() {
        assert!("ctrl+".parse::<KeyBinding>().is_ok()); // literal plus
        assert!("hyper+x".parse::<KeyBinding>().is_err());
        assert!("nope".parse::<KeyBinding>().is_err());
        assert!("".parse::<KeyBinding>().is_err());
    }

    #[test]
    fn display_round_trips() {
        for s in ["q", "esc", "f5", "ctrl+c", "shift+tab", "space", "alt+x"] {
            let b: KeyBinding = s.parse().unwrap();
            assert_eq!(b.to_string().parse::<KeyBinding>().unwrap(), b, "{s}");
        }
    }
}

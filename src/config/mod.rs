//! Configuration loading.
//!
//! The effective config is built in layers, each one overriding the last:
//!
//! 1. the built-in defaults ([`Config::default`]),
//! 2. `/etc/ratfetch/config.toml`,
//! 3. `$XDG_CONFIG_HOME/ratfetch/config.toml` (or `~/.config/...`),
//! 4. the file named by `--config`, which replaces layers 2 and 3,
//! 5. `--set key=value` overrides from the command line.
//!
//! Every layer may be partial. Files are parsed twice on purpose: once on
//! their own, so that a typo is reported against the file and line it came
//! from, and once as part of the merged document that is finally deserialised.

pub mod color;
pub mod enums;
pub mod keys;
pub mod model;
pub mod theme;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Value;

pub use color::ColorSpec;
pub use enums::*;
pub use keys::KeyBinding;
pub use model::*;
pub use theme::{Theme as ResolvedTheme, THEMES};

/// The default config file, fully commented.
pub const TEMPLATE: &str = include_str!("../../assets/config.template.toml");

/// What went wrong while loading a config.
#[derive(Debug)]
pub enum ConfigError {
    /// A file could not be read.
    Io { path: PathBuf, source: io::Error },
    /// A file was not valid TOML, or did not match the schema.
    Parse { path: PathBuf, message: String },
    /// A `--set` argument was malformed.
    Override { arg: String, message: String },
    /// The document parsed, but the values do not make sense together.
    Invalid { message: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::Parse { path, message } => write!(f, "in {}:\n{message}", path.display()),
            Self::Override { arg, message } => write!(f, "in `--set {arg}`: {message}"),
            Self::Invalid { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// How to assemble the config.
#[derive(Debug, Default, Clone)]
pub struct LoadOptions {
    /// Use only this file, instead of the system and user paths.
    pub explicit: Option<PathBuf>,
    /// Skip every file and start from the built-in defaults.
    pub skip_files: bool,
    /// `key.path=value` pairs applied last.
    pub overrides: Vec<String>,
}

/// A config plus a record of where it came from.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub config: Config,
    /// Files that were read, in the order they were applied.
    pub sources: Vec<PathBuf>,
    /// Non-fatal complaints worth showing the user.
    pub warnings: Vec<String>,
}

/// `/etc/ratfetch/config.toml`.
pub fn system_path() -> PathBuf {
    PathBuf::from("/etc/ratfetch/config.toml")
}

/// `$XDG_CONFIG_HOME/ratfetch/config.toml`, falling back to `~/.config`.
pub fn user_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("ratfetch").join("config.toml"))
}

/// Builds the effective config.
pub fn load(opts: &LoadOptions) -> Result<Loaded, ConfigError> {
    let mut merged = Value::Table(Default::default());
    let mut sources = Vec::new();
    let mut warnings = Vec::new();

    if !opts.skip_files {
        let paths: Vec<PathBuf> = match &opts.explicit {
            // An explicit path is required to exist; the discovered ones are not.
            Some(p) => vec![p.clone()],
            None => [Some(system_path()), user_path()]
                .into_iter()
                .flatten()
                .collect(),
        };

        for path in paths {
            match fs::read_to_string(&path) {
                Ok(text) => {
                    let value = parse_layer(&path, &text)?;
                    merge(&mut merged, value);
                    sources.push(path);
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound && opts.explicit.is_none() => {}
                Err(source) => return Err(ConfigError::Io { path, source }),
            }
        }
    }

    for arg in &opts.overrides {
        let value = parse_override(arg)?;
        merge(&mut merged, value);
    }

    let config = Config::deserialize(merged.clone()).map_err(|e| ConfigError::Invalid {
        message: format!("config is not valid: {e}"),
    })?;

    let config = validate(config, &mut warnings)?;
    Ok(Loaded {
        config,
        sources,
        warnings,
    })
}

/// Parses one layer, reporting schema errors against this file.
fn parse_layer(path: &Path, text: &str) -> Result<Value, ConfigError> {
    // First pass: surface syntax and schema errors with a line and column.
    // Partial documents are fine because every field has a default.
    toml::from_str::<Config>(text).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;

    // Second pass: keep the raw tree so later layers can merge onto it.
    toml::from_str::<Value>(text).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

/// Turns `a.b.c=value` into a nested table.
fn parse_override(arg: &str) -> Result<Value, ConfigError> {
    let (path, raw) = arg.split_once('=').ok_or_else(|| ConfigError::Override {
        arg: arg.to_string(),
        message: "expected `key.path=value`".to_string(),
    })?;

    let path = path.trim();
    if path.is_empty() {
        return Err(ConfigError::Override {
            arg: arg.to_string(),
            message: "the key path is empty".to_string(),
        });
    }

    // Interpret the value as TOML so that numbers, booleans and arrays work;
    // anything else is taken literally as a string.
    let leaf = toml::from_str::<Value>(&format!("v = {raw}"))
        .ok()
        .and_then(|v| v.get("v").cloned())
        .unwrap_or_else(|| Value::String(raw.to_string()));

    let mut value = leaf;
    for key in path.split('.').rev() {
        if key.is_empty() {
            return Err(ConfigError::Override {
                arg: arg.to_string(),
                message: "the key path has an empty segment".to_string(),
            });
        }
        let mut table = toml::Table::new();
        table.insert(key.to_string(), value);
        value = Value::Table(table);
    }
    Ok(value)
}

/// Deep-merges `src` onto `dst`. Tables merge key by key; everything else,
/// arrays included, is replaced outright.
fn merge(dst: &mut Value, src: Value) {
    match (dst, src) {
        (Value::Table(d), Value::Table(s)) => {
            for (k, v) in s {
                match d.get_mut(&k) {
                    Some(existing) => merge(existing, v),
                    None => {
                        d.insert(k, v);
                    }
                }
            }
        }
        (d, s) => *d = s,
    }
}

/// Checks the values make sense together, clamping what can be clamped and
/// rejecting what cannot.
fn validate(mut c: Config, warnings: &mut Vec<String>) -> Result<Config, ConfigError> {
    let invalid = |message: String| ConfigError::Invalid { message };

    if c.general.interval_ms < 50 {
        warnings.push(format!(
            "general.interval_ms = {} is below the 50 ms floor; using 50",
            c.general.interval_ms
        ));
        c.general.interval_ms = 50;
    }
    if c.general.interval_ms > 600_000 {
        return Err(invalid(format!(
            "general.interval_ms = {} is more than 10 minutes",
            c.general.interval_ms
        )));
    }

    c.general.history = c.general.history.clamp(8, 4096);

    if c.general.tabs.is_empty() {
        return Err(invalid(
            "general.tabs is empty: there would be nothing to show".into(),
        ));
    }
    if !c.general.tabs.contains(&c.general.start_tab) {
        return Err(invalid(format!(
            "general.start_tab = \"{}\" is not in general.tabs ({})",
            c.general.start_tab,
            c.general
                .tabs
                .iter()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        )));
    }

    if !theme::exists(&c.theme.name) {
        let hint = crate::util::suggest(&c.theme.name, theme::names())
            .map(|s| format!(" (did you mean \"{s}\"?)"))
            .unwrap_or_default();
        return Err(invalid(format!(
            "theme.name = \"{}\" is not a built-in theme{hint}; available: {}",
            c.theme.name,
            theme::names().join(", "),
        )));
    }

    if c.meters.warn_at > 100 || c.meters.critical_at > 100 {
        return Err(invalid(
            "meters.warn_at and meters.critical_at are percentages (0-100)".into(),
        ));
    }
    if c.meters.warn_at > c.meters.critical_at {
        return Err(invalid(format!(
            "meters.warn_at ({}) is above meters.critical_at ({})",
            c.meters.warn_at, c.meters.critical_at
        )));
    }

    if c.logo.source.trim().is_empty() {
        return Err(invalid(
            "logo.source is empty; use \"auto\", a logo name, or \"none\"".into(),
        ));
    }

    if c.info.items.is_empty() && c.layout.panels.is_empty() {
        warnings
            .push("info.items and layout.panels are both empty; the overview will be blank".into());
    }

    c.processes.count = c.processes.count.clamp(1, 512);

    // Empty binding lists are legal — that is how a user disables an action —
    // but quitting must stay reachable somehow.
    if c.keys.quit.is_empty() {
        return Err(invalid(
            "keys.quit is empty; there would be no way to exit".into(),
        ));
    }

    Ok(c)
}

/// Writes the commented template, refusing to clobber an existing file.
pub fn write_template(path: &Path, force: bool) -> Result<(), ConfigError> {
    if path.exists() && !force {
        return Err(ConfigError::Invalid {
            message: format!(
                "{} already exists; pass --force to overwrite it",
                path.display()
            ),
        });
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, TEMPLATE).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_str(text: &str) -> Result<Config, ConfigError> {
        let mut merged = Value::Table(Default::default());
        merge(&mut merged, parse_layer(Path::new("test.toml"), text)?);
        let c = Config::deserialize(merged).map_err(|e| ConfigError::Invalid {
            message: e.to_string(),
        })?;
        validate(c, &mut Vec::new())
    }

    #[test]
    fn defaults_are_valid() {
        let c = validate(Config::default(), &mut Vec::new()).unwrap();
        assert_eq!(c, Config::default());
    }

    #[test]
    fn the_shipped_template_parses_and_validates() {
        let c = load_str(TEMPLATE).expect("template must load");
        // The template documents the defaults, so it must reproduce them.
        assert_eq!(c, Config::default());
    }

    #[test]
    fn partial_documents_keep_the_defaults() {
        let c = load_str("[general]\ninterval_ms = 250\n").unwrap();
        assert_eq!(c.general.interval_ms, 250);
        assert_eq!(c.general.history, Config::default().general.history);
        assert_eq!(c.logo, Config::default().logo);
    }

    #[test]
    fn later_layers_win_and_tables_merge() {
        let mut merged = Value::Table(Default::default());
        merge(
            &mut merged,
            toml::from_str("[general]\ninterval_ms = 100\nhistory = 60\n").unwrap(),
        );
        merge(
            &mut merged,
            toml::from_str("[general]\ninterval_ms = 500\n").unwrap(),
        );
        let c = Config::deserialize(merged).unwrap();
        assert_eq!(c.general.interval_ms, 500);
        assert_eq!(c.general.history, 60, "untouched keys survive the merge");
    }

    #[test]
    fn arrays_are_replaced_not_appended() {
        let mut merged = Value::Table(Default::default());
        merge(
            &mut merged,
            toml::from_str(
                r#"[info]
items = ["os", "kernel"]
"#,
            )
            .unwrap(),
        );
        merge(
            &mut merged,
            toml::from_str(
                r#"[info]
items = ["cpu"]
"#,
            )
            .unwrap(),
        );
        let c = Config::deserialize(merged).unwrap();
        assert_eq!(c.info.items, vec![InfoItem::Cpu]);
    }

    #[test]
    fn unknown_keys_are_rejected_with_a_location() {
        let e = load_str("[general]\ninterval_ms = 100\ninterval = 5\n").unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("interval"), "{msg}");
        assert!(msg.contains("test.toml"), "{msg}");
    }

    #[test]
    fn unknown_sections_are_rejected() {
        assert!(load_str("[generl]\ninterval_ms = 100\n").is_err());
    }

    #[test]
    fn bad_enum_values_explain_themselves() {
        let e = load_str("[info]\nitems = [\"kernal\"]\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains("did you mean `kernel`"), "{e}");
    }

    #[test]
    fn overrides_build_nested_tables() {
        let v = parse_override("general.interval_ms=250").unwrap();
        assert_eq!(v["general"]["interval_ms"].as_integer(), Some(250));

        let v = parse_override("logo.source=arch").unwrap();
        assert_eq!(
            v["logo"]["source"].as_str(),
            Some("arch"),
            "bare words are strings"
        );

        let v = parse_override(r#"info.items=["os","cpu"]"#).unwrap();
        assert_eq!(v["info"]["items"].as_array().unwrap().len(), 2);

        let v = parse_override("general.mouse=false").unwrap();
        assert_eq!(v["general"]["mouse"].as_bool(), Some(false));
    }

    #[test]
    fn malformed_overrides_are_rejected() {
        assert!(parse_override("no-equals-sign").is_err());
        assert!(parse_override("=5").is_err());
        assert!(parse_override("a..b=5").is_err());
    }

    #[test]
    fn interval_is_clamped_with_a_warning() {
        let mut warnings = Vec::new();
        let mut c = Config::default();
        c.general.interval_ms = 1;
        let c = validate(c, &mut warnings).unwrap();
        assert_eq!(c.general.interval_ms, 50);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn contradictory_values_are_errors() {
        assert!(load_str("[general]\ninterval_ms = 999999999\n").is_err());
        assert!(load_str("[general]\nstart_tab = \"disks\"\ntabs = [\"overview\"]\n").is_err());
        assert!(load_str("[meters]\nwarn_at = 95\ncritical_at = 80\n").is_err());
        assert!(load_str("[keys]\nquit = []\n").is_err());
        assert!(load_str("[theme]\nname = \"nope\"\n").is_err());
    }

    #[test]
    fn theme_typos_suggest_a_real_theme() {
        let e = load_str("[theme]\nname = \"catppuccin-moccha\"\n")
            .unwrap_err()
            .to_string();
        assert!(e.contains("did you mean"), "{e}");
    }
}

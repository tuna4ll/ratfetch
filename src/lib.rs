//! ratfetch — a live, always-updating system fetch for the terminal.
//!
//! The crate is split into four layers:
//!
//! - [`config`] parses and validates `config.toml`,
//! - [`sys`] reads `/proc` and `/sys` into plain data,
//! - [`app`] holds the sampled state and the event loop,
//! - [`ui`] draws it.

pub mod app;
pub mod cli;
pub mod config;
pub mod logo;
pub mod sys;
pub mod ui;
pub mod util;

/// The version reported by `--version` and the footer.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

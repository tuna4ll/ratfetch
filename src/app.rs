//! Application state and the event loop.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::config::enums::{ProcSort, Tab};
use crate::config::theme::{self, Theme};
use crate::config::{Config, LoadOptions, Loaded};
use crate::logo::Logo;
use crate::sys::{Dynamic, Sampler, Static};

/// A fixed-length series of samples, oldest first.
#[derive(Debug, Clone)]
pub struct Ring {
    data: VecDeque<f64>,
    capacity: usize,
}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: VecDeque::with_capacity(capacity),
            capacity: capacity.max(1),
        }
    }

    pub fn push(&mut self, value: f64) {
        if self.data.len() == self.capacity {
            self.data.pop_front();
        }
        self.data.push_back(value);
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Samples oldest to newest.
    pub fn values(&self) -> impl Iterator<Item = f64> + '_ {
        self.data.iter().copied()
    }

    pub fn last(&self) -> f64 {
        self.data.back().copied().unwrap_or(0.0)
    }

    pub fn max(&self) -> f64 {
        self.data.iter().copied().fold(0.0, f64::max)
    }

    pub fn min(&self) -> f64 {
        self.data
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min)
            .min(self.max())
    }

    /// The newest `n` samples, scaled to `0..=height` for a bar renderer.
    pub fn scaled(&self, n: usize, ceiling: f64) -> Vec<u64> {
        let ceiling = if ceiling > 0.0 { ceiling } else { 1.0 };
        self.data
            .iter()
            .rev()
            .take(n)
            .rev()
            .map(|v| ((v / ceiling).clamp(0.0, 1.0) * 1000.0) as u64)
            .collect()
    }

    /// Discards samples beyond a new capacity.
    pub fn resize(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        while self.data.len() > self.capacity {
            self.data.pop_front();
        }
    }
}

/// Every series the graphs can draw.
#[derive(Debug, Clone)]
pub struct History {
    pub cpu: Ring,
    pub memory: Ring,
    pub swap: Ring,
    pub disk: Ring,
    pub load: Ring,
    pub net_rx: Ring,
    pub net_tx: Ring,
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self {
            cpu: Ring::new(capacity),
            memory: Ring::new(capacity),
            swap: Ring::new(capacity),
            disk: Ring::new(capacity),
            load: Ring::new(capacity),
            net_rx: Ring::new(capacity),
            net_tx: Ring::new(capacity),
        }
    }

    fn each_mut(&mut self) -> [&mut Ring; 7] {
        [
            &mut self.cpu,
            &mut self.memory,
            &mut self.swap,
            &mut self.disk,
            &mut self.load,
            &mut self.net_rx,
            &mut self.net_tx,
        ]
    }

    pub fn resize(&mut self, capacity: usize) {
        for ring in self.each_mut() {
            ring.resize(capacity);
        }
    }

    /// Appends one reading from a snapshot.
    pub fn record(&mut self, d: &Dynamic, cfg: &Config) {
        self.cpu.push(d.cpu.total);
        self.memory.push(d.mem.percent());
        self.swap.push(d.mem.swap_percent());
        self.disk.push(
            d.primary_disk(&cfg.disks)
                .map(|p| p.percent())
                .unwrap_or(0.0),
        );
        self.load.push(d.load[0]);
        let (rx, tx) = d.net_rates();
        self.net_rx.push(rx);
        self.net_tx.push(tx);
    }
}

/// What a key or mouse event asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Reload,
    ToggleHelp,
    NextTab,
    PrevTab,
    ScrollUp,
    ScrollDown,
    SortNext,
    TogglePerCore,
    ToggleFreeze,
    ToggleInfoDetails,
    ToggleErrors,
    SearchProcesses,
    ClearProcessFilter,
    ToggleProcessDetails,
    ToggleProcessTree,
    SignalProcess,
    ConfirmSignal,
    CancelSignal,
    SelectTab(Tab),
}

/// A transient message shown in the footer.
#[derive(Debug, Clone)]
pub struct Status {
    pub text: String,
    pub is_error: bool,
    shown_at: Instant,
}

impl Status {
    fn new(text: impl Into<String>, is_error: bool) -> Self {
        Self {
            text: text.into(),
            is_error,
            shown_at: Instant::now(),
        }
    }

    fn expired(&self) -> bool {
        // Errors stay up long enough to read; confirmations do not linger.
        let life = if self.is_error { 8 } else { 3 };
        self.shown_at.elapsed() > Duration::from_secs(life)
    }
}

/// The whole running program.
pub struct App {
    pub config: Config,
    pub theme: Theme,
    pub statics: Static,
    pub dynamic: Dynamic,
    pub history: History,
    pub logo: Option<Logo>,
    pub tab: Tab,
    pub sort: ProcSort,
    pub per_core: bool,
    pub frozen: bool,
    pub show_help: bool,
    pub show_info_details: bool,
    pub show_errors: bool,
    pub process_searching: bool,
    pub process_query: String,
    pub process_tree: bool,
    pub show_process_details: bool,
    pub show_signal_confirm: bool,
    pub scroll: usize,
    pub status: Option<Status>,
    pub errors: VecDeque<String>,
    pub should_quit: bool,
    pub started: Instant,
    sampler: Sampler,
    load_options: LoadOptions,
    config_files: Vec<PathBuf>,
    config_stamp: Option<SystemTime>,
}

impl App {
    /// Builds the initial state from a loaded config.
    pub fn new(loaded: Loaded, load_options: LoadOptions) -> Self {
        let config = loaded.config;
        let theme = theme::resolve(&config.theme);
        let statics = Static::collect();

        // A bad logo name is worth saying out loud, but not worth refusing to
        // start over.
        let mut logo_error = None;
        let logo = match crate::logo::resolve(
            &config.logo.source,
            config.logo.small,
            &statics.distro_id,
        ) {
            Ok(logo) => logo,
            Err(e) => {
                logo_error = Some(e);
                Some(crate::logo::unknown())
            }
        };

        let mut app = Self {
            history: History::new(config.general.history),
            sort: config.processes.sort,
            per_core: config.meters.per_core,
            tab: config.general.start_tab,
            theme,
            statics,
            dynamic: Dynamic::default(),
            logo,
            frozen: false,
            show_help: false,
            show_info_details: false,
            show_errors: false,
            process_searching: false,
            process_query: String::new(),
            process_tree: false,
            show_process_details: false,
            show_signal_confirm: false,
            scroll: 0,
            status: None,
            errors: VecDeque::with_capacity(32),
            should_quit: false,
            started: Instant::now(),
            sampler: Sampler::new(),
            config_files: loaded.sources,
            config_stamp: None,
            load_options,
            config,
        };

        app.config_stamp = app.newest_config_stamp();
        for message in logo_error.into_iter().chain(loaded.warnings) {
            app.set_status(message, true);
        }
        app.tick();
        app
    }

    /// How long to wait between samples.
    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.config.general.interval_ms)
    }

    /// Takes a sample and records it, unless the display is frozen.
    pub fn tick(&mut self) {
        if self.frozen {
            return;
        }
        self.dynamic = self.sampler.sample(&self.config);
        // The live sort key can differ from the configured one.
        crate::sys::procs::sort(
            &mut self.dynamic.procs,
            self.sort,
            self.config.processes.ascending,
        );
        self.history.record(&self.dynamic, &self.config);
        let rows =
            crate::sys::procs::view(&self.dynamic.procs, &self.process_query, self.process_tree)
                .len();
        self.scroll = self.scroll.min(rows.saturating_sub(1));

        if self
            .status
            .as_ref()
            .is_some_and(|status| !status.is_error && status.expired())
        {
            self.status = None;
        }
        if self.config.general.watch_config {
            self.reload_if_changed();
        }
    }

    /// Whether the run has hit `general.exit_after`.
    pub fn timed_out(&self) -> bool {
        let limit = self.config.general.exit_after;
        limit > 0 && self.started.elapsed() >= Duration::from_secs(limit)
    }

    /// Maps a key event to an action using the configured bindings.
    pub fn action_for(&self, key: &KeyEvent) -> Action {
        // Key repeat and release events would otherwise fire twice.
        if key.kind != KeyEventKind::Press {
            return Action::None;
        }
        let k = &self.config.keys;
        let hit = |list: &[crate::config::KeyBinding]| list.iter().any(|b| b.matches(key));

        if self.show_signal_confirm {
            return match key.code {
                KeyCode::Char('y') => Action::ConfirmSignal,
                KeyCode::Char('n') => Action::CancelSignal,
                _ if hit(&k.quit) => Action::CancelSignal,
                _ => Action::None,
            };
        }
        if self.show_process_details && (hit(&k.process_details) || hit(&k.quit)) {
            return Action::ToggleProcessDetails;
        }

        // Help is modal: while it is open, any bound key closes it.
        if self.show_help && (hit(&k.help) || hit(&k.quit)) {
            return Action::ToggleHelp;
        }
        if self.show_info_details && (hit(&k.info_details) || hit(&k.quit)) {
            return Action::ToggleInfoDetails;
        }
        if self.show_errors && (hit(&k.errors) || hit(&k.quit)) {
            return Action::ToggleErrors;
        }

        if hit(&k.quit) {
            Action::Quit
        } else if hit(&k.help) {
            Action::ToggleHelp
        } else if hit(&k.reload) {
            Action::Reload
        } else if hit(&k.next_tab) {
            Action::NextTab
        } else if hit(&k.prev_tab) {
            Action::PrevTab
        } else if hit(&k.scroll_down) {
            Action::ScrollDown
        } else if hit(&k.scroll_up) {
            Action::ScrollUp
        } else if hit(&k.sort_next) {
            Action::SortNext
        } else if hit(&k.toggle_per_core) {
            Action::TogglePerCore
        } else if hit(&k.freeze) {
            Action::ToggleFreeze
        } else if hit(&k.info_details) {
            Action::ToggleInfoDetails
        } else if hit(&k.errors) {
            Action::ToggleErrors
        } else if self.tab == Tab::Processes && hit(&k.search_processes) {
            Action::SearchProcesses
        } else if self.tab == Tab::Processes && hit(&k.clear_process_filter) {
            Action::ClearProcessFilter
        } else if self.tab == Tab::Processes && hit(&k.process_details) {
            Action::ToggleProcessDetails
        } else if self.tab == Tab::Processes && hit(&k.process_tree) {
            Action::ToggleProcessTree
        } else if self.tab == Tab::Processes && hit(&k.signal_process) {
            Action::SignalProcess
        } else {
            Action::None
        }
    }

    /// Maps a mouse event to an action.
    pub fn action_for_mouse(&self, ev: &MouseEvent) -> Action {
        match ev.kind {
            MouseEventKind::ScrollDown => Action::ScrollDown,
            MouseEventKind::ScrollUp => Action::ScrollUp,
            MouseEventKind::Down(MouseButton::Left)
                if self.config.general.tab_bar
                    && ev.row == self.config.layout.margin
                    && self.config.general.tabs.len() > 1 =>
            {
                let mut column = self.config.layout.margin.saturating_add(1);
                for (index, tab) in self.config.general.tabs.iter().enumerate() {
                    if index > 0 {
                        column = column.saturating_add(3);
                    }
                    let end = column.saturating_add(tab.to_string().chars().count() as u16 + 2);
                    if ev.column >= column && ev.column < end {
                        return Action::SelectTab(*tab);
                    }
                    column = end;
                }
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Applies an action.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Quit => self.should_quit = true,
            Action::ToggleHelp => self.show_help = !self.show_help,
            Action::Reload => self.reload(),
            Action::NextTab => self.switch_tab(1),
            Action::PrevTab => self.switch_tab(-1),
            Action::ScrollDown => {
                let max = match self.tab {
                    Tab::Processes => crate::sys::procs::view(
                        &self.dynamic.procs,
                        &self.process_query,
                        self.process_tree,
                    )
                    .len()
                    .saturating_sub(1),
                    Tab::Disks => self.dynamic.disks.len().saturating_sub(1),
                    Tab::Network => self.dynamic.nets.len().saturating_sub(1),
                    Tab::Overview => 4095,
                };
                self.scroll = self.scroll.saturating_add(1).min(max);
            }
            Action::ScrollUp => self.scroll = self.scroll.saturating_sub(1),
            Action::SortNext => {
                self.sort = self.sort.next();
                crate::sys::procs::sort(
                    &mut self.dynamic.procs,
                    self.sort,
                    self.config.processes.ascending,
                );
                self.set_status(format!("sorting by {}", self.sort), false);
            }
            Action::TogglePerCore => {
                self.per_core = !self.per_core;
            }
            Action::ToggleFreeze => {
                self.frozen = !self.frozen;
                let word = if self.frozen { "frozen" } else { "running" };
                self.set_status(word, false);
            }
            Action::ToggleInfoDetails => {
                self.show_info_details = !self.show_info_details;
            }
            Action::ToggleErrors => {
                self.show_errors = !self.show_errors;
                if !self.show_errors && self.status.as_ref().is_some_and(|s| s.is_error) {
                    self.status = None;
                }
            }
            Action::SearchProcesses => {
                self.process_searching = true;
                self.scroll = 0;
            }
            Action::ClearProcessFilter => {
                self.process_query.clear();
                self.scroll = 0;
            }
            Action::ToggleProcessDetails => {
                if self.selected_process().is_some() {
                    self.show_process_details = !self.show_process_details;
                }
            }
            Action::ToggleProcessTree => {
                self.process_tree = !self.process_tree;
                self.scroll = 0;
                self.set_status(
                    if self.process_tree {
                        "process tree"
                    } else {
                        "sorted process list"
                    },
                    false,
                );
            }
            Action::SignalProcess => {
                self.show_signal_confirm = self.selected_process().is_some();
            }
            Action::ConfirmSignal => {
                let selected = self
                    .selected_process()
                    .map(|process| (process.pid, process.name.clone()));
                self.show_signal_confirm = false;
                if let Some((pid, name)) = selected {
                    // SAFETY: kill does not dereference pointers; it reports failure.
                    let result = unsafe { libc::kill(pid, libc::SIGTERM) };
                    if result == 0 {
                        self.set_status(format!("sent SIGTERM to {name} ({pid})"), false);
                    } else {
                        self.set_status(
                            format!(
                                "cannot signal {name} ({pid}): {}",
                                std::io::Error::last_os_error()
                            ),
                            true,
                        );
                    }
                }
            }
            Action::CancelSignal => self.show_signal_confirm = false,
            Action::SelectTab(tab) => {
                self.tab = tab;
                self.scroll = 0;
            }
        }
    }

    /// Handles editable process-search input before global key bindings.
    pub fn handle_text_input(&mut self, key: &KeyEvent) -> bool {
        if !self.process_searching || key.kind != KeyEventKind::Press {
            return false;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.process_searching = false,
            KeyCode::Backspace => {
                self.process_query.pop();
                self.scroll = 0;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.process_query.clear();
                self.scroll = 0;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.process_query.push(c);
                self.scroll = 0;
            }
            _ => {}
        }
        true
    }

    pub fn selected_process(&self) -> Option<&crate::sys::procs::Process> {
        crate::sys::procs::view(&self.dynamic.procs, &self.process_query, self.process_tree)
            .get(self.scroll)
            .map(|(process, _)| *process)
    }

    fn switch_tab(&mut self, delta: isize) {
        let tabs = &self.config.general.tabs;
        if tabs.is_empty() {
            return;
        }
        let current = tabs.iter().position(|t| *t == self.tab).unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(tabs.len() as isize) as usize;
        self.tab = tabs[next];
        // Each tab keeps its own list, so a carried-over offset would be wrong.
        self.scroll = 0;
    }

    /// Shows a message in the footer.
    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        let text = text.into();
        if is_error {
            if self.errors.len() == 32 {
                self.errors.pop_front();
            }
            self.errors.push_back(text.clone());
        }
        self.status = Some(Status::new(text, is_error));
    }

    /// Re-reads the config from disk, keeping the current one on failure.
    pub fn reload(&mut self) {
        match crate::config::load(&self.load_options) {
            Ok(loaded) => {
                let warnings = loaded.warnings.clone();
                self.adopt(loaded);
                match warnings.first() {
                    Some(w) => self.set_status(w.clone(), true),
                    None => self.set_status("config reloaded", false),
                }
            }
            Err(e) => {
                // Keeping the old config running is far friendlier than
                // exiting because of a typo saved mid-edit.
                let first_line = e.to_string().lines().next().unwrap_or_default().to_string();
                self.set_status(
                    format!("config error, keeping the old one: {first_line}"),
                    true,
                );
                // Do not retry on every tick until the file changes again.
                self.config_stamp = self.newest_config_stamp();
            }
        }
    }

    /// Replaces the live config with a freshly loaded one.
    fn adopt(&mut self, loaded: Loaded) {
        let config = loaded.config;
        self.theme = theme::resolve(&config.theme);
        self.history.resize(config.general.history);

        if !config.general.tabs.contains(&self.tab) {
            self.tab = config.general.start_tab;
            self.scroll = 0;
        }

        let logo_changed = config.logo.source != self.config.logo.source
            || config.logo.small != self.config.logo.small;
        if logo_changed {
            match crate::logo::resolve(
                &config.logo.source,
                config.logo.small,
                &self.statics.distro_id,
            ) {
                Ok(logo) => self.logo = logo,
                Err(e) => self.set_status(e, true),
            }
        }

        self.config = config;
        self.config_files = loaded.sources;
        self.config_stamp = self.newest_config_stamp();
    }

    /// The most recent modification time across the files that were read.
    fn newest_config_stamp(&self) -> Option<SystemTime> {
        self.config_files
            .iter()
            .filter_map(|p| p.metadata().ok()?.modified().ok())
            .max()
    }

    /// Reloads when a config file has been written since the last check.
    fn reload_if_changed(&mut self) {
        let stamp = self.newest_config_stamp();
        if stamp != self.config_stamp {
            self.config_stamp = stamp;
            // A file appearing for the first time counts as a change.
            self.reload();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_only_the_newest_samples() {
        let mut r = Ring::new(3);
        for v in [1.0, 2.0, 3.0, 4.0] {
            r.push(v);
        }
        assert_eq!(r.values().collect::<Vec<_>>(), vec![2.0, 3.0, 4.0]);
        assert_eq!(r.last(), 4.0);
        assert_eq!(r.max(), 4.0);
        assert_eq!(r.min(), 2.0);
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn an_empty_ring_reports_zero_not_infinity() {
        let r = Ring::new(4);
        assert!(r.is_empty());
        assert_eq!(r.last(), 0.0);
        assert_eq!(r.max(), 0.0);
        assert_eq!(r.min(), 0.0);
    }

    #[test]
    fn ring_scales_against_a_ceiling() {
        let mut r = Ring::new(4);
        for v in [0.0, 50.0, 100.0, 200.0] {
            r.push(v);
        }
        assert_eq!(r.scaled(4, 100.0), vec![0, 500, 1000, 1000]);
        assert_eq!(r.scaled(2, 100.0), vec![1000, 1000], "newest samples");
    }

    #[test]
    fn a_zero_ceiling_does_not_divide_by_zero() {
        let mut r = Ring::new(2);
        r.push(5.0);
        assert_eq!(r.scaled(2, 0.0), vec![1000]);
    }

    #[test]
    fn shrinking_a_ring_drops_the_oldest() {
        let mut r = Ring::new(5);
        for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
            r.push(v);
        }
        r.resize(2);
        assert_eq!(r.values().collect::<Vec<_>>(), vec![4.0, 5.0]);
        r.resize(0);
        assert_eq!(r.len(), 1, "capacity is clamped to at least one");
    }

    #[test]
    fn history_resizes_every_series() {
        let mut h = History::new(10);
        for _ in 0..10 {
            h.cpu.push(1.0);
            h.net_rx.push(2.0);
        }
        h.resize(3);
        assert_eq!(h.cpu.len(), 3);
        assert_eq!(h.net_rx.len(), 3);
    }

    #[test]
    fn status_messages_expire() {
        let mut s = Status::new("hi", false);
        assert!(!s.expired());
        s.shown_at = Instant::now() - Duration::from_secs(10);
        assert!(s.expired());

        // Errors get a longer life than confirmations.
        let mut e = Status::new("bad", true);
        e.shown_at = Instant::now() - Duration::from_secs(5);
        assert!(!e.expired());
    }
}

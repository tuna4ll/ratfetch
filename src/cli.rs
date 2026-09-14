//! Command line handling and the terminal loop.

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::app::{Action, App};
use crate::config::{self, LoadOptions};

/// A live, always-updating system fetch for the terminal.
#[derive(Debug, Parser)]
#[command(name = "ratfetch", version, about, long_about = None)]
pub struct Args {
    /// Read this config file instead of the system and user ones.
    #[arg(short, long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Ignore every config file and use the built-in defaults.
    #[arg(long)]
    pub no_config: bool,

    /// Override one setting, e.g. --set general.interval_ms=250.
    #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
    pub overrides: Vec<String>,

    /// Use this logo instead of the detected one.
    #[arg(short, long, value_name = "NAME")]
    pub logo: Option<String>,

    /// Use this theme.
    #[arg(short, long, value_name = "NAME")]
    pub theme: Option<String>,

    /// Milliseconds between samples.
    #[arg(short, long, value_name = "MS")]
    pub interval: Option<u64>,

    /// Draw one frame and exit, the way a classic fetch tool does.
    #[arg(short = '1', long)]
    pub once: bool,

    /// Quit after this many seconds.
    #[arg(long, value_name = "SECS")]
    pub exit_after: Option<u64>,

    /// Write a fully commented config file and exit.
    #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    pub generate_config: Option<String>,

    /// Overwrite the file that --generate-config would refuse to replace.
    #[arg(long)]
    pub force: bool,

    /// Validate the config, report what was found, and exit.
    #[arg(long)]
    pub check_config: bool,

    /// Print the effective config as TOML and exit.
    #[arg(long)]
    pub print_config: bool,

    /// List every bundled logo name and exit.
    #[arg(long)]
    pub list_logos: bool,

    /// List every built-in theme and exit.
    #[arg(long)]
    pub list_themes: bool,
}

impl Args {
    /// Turns the flags into config load options.
    ///
    /// The shorthand flags are applied as ordinary overrides, so there is only
    /// one code path that can change a setting.
    fn load_options(&self) -> LoadOptions {
        let mut overrides = self.overrides.clone();
        if let Some(logo) = &self.logo {
            overrides.push(format!("logo.source={logo}"));
        }
        if let Some(theme) = &self.theme {
            overrides.push(format!("theme.name={theme}"));
        }
        if let Some(ms) = self.interval {
            overrides.push(format!("general.interval_ms={ms}"));
        }
        if let Some(secs) = self.exit_after {
            overrides.push(format!("general.exit_after={secs}"));
        }
        LoadOptions {
            explicit: self.config.clone(),
            skip_files: self.no_config,
            overrides,
        }
    }
}

/// Entry point.
pub fn run() -> ExitCode {
    let args = Args::parse();
    match dispatch(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("ratfetch: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(args: &Args) -> Result<ExitCode, Box<dyn std::error::Error>> {
    // The informational modes never touch the terminal.
    if args.list_logos {
        let mut out = io::stdout().lock();
        for name in crate::logo::all_names() {
            writeln!(out, "{name}")?;
        }
        return Ok(ExitCode::SUCCESS);
    }

    if args.list_themes {
        let mut out = io::stdout().lock();
        for name in config::theme::names() {
            writeln!(out, "{name}")?;
        }
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(path) = &args.generate_config {
        let path = if path.is_empty() {
            config::user_path().ok_or("cannot determine a config directory")?
        } else {
            PathBuf::from(path)
        };
        config::write_template(&path, args.force)?;
        println!("wrote {}", path.display());
        return Ok(ExitCode::SUCCESS);
    }

    let options = args.load_options();

    if args.check_config {
        return Ok(check(&options));
    }

    let loaded = config::load(&options)?;

    if args.print_config {
        print!("{}", toml::to_string_pretty(&loaded.config)?);
        return Ok(ExitCode::SUCCESS);
    }

    for warning in &loaded.warnings {
        eprintln!("ratfetch: warning: {warning}");
    }

    let app = App::new(loaded, options);
    if args.once {
        draw_once(app)?;
    } else {
        run_loop(app)?;
    }
    Ok(ExitCode::SUCCESS)
}

/// `--check-config`: report what would be loaded, without starting the UI.
fn check(options: &LoadOptions) -> ExitCode {
    match config::load(options) {
        Ok(loaded) => {
            if loaded.sources.is_empty() {
                println!("no config file found; the built-in defaults are valid");
            } else {
                for path in &loaded.sources {
                    println!("ok  {}", path.display());
                }
            }
            for warning in &loaded.warnings {
                println!("warning: {warning}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ratfetch: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Restores the terminal. Safe to call more than once.
fn restore(alt_screen: bool, mouse: bool) {
    let mut out = io::stdout();
    if mouse {
        let _ = out.execute(DisableMouseCapture);
    }
    if alt_screen {
        let _ = out.execute(LeaveAlternateScreen);
    }
    let _ = disable_raw_mode();
    let _ = out.execute(crossterm::cursor::Show);
}

/// Draws a single frame inline and leaves it in the scrollback.
fn draw_once(mut app: App) -> io::Result<()> {
    // A time series needs more than one sample. In one-shot mode the graphs
    // would be empty boxes, so they are dropped and their rows go to the
    // header and the meters instead.
    app.config
        .layout
        .panels
        .retain(|p| *p != crate::config::enums::PanelKind::Graphs);

    // Inline viewports need a height up front. Ask the header what it wants,
    // then add what the panels need and the rows the chrome costs, so the
    // single frame is not cut short.
    let width = crossterm::terminal::size().map(|(w, _)| w).unwrap_or(80);
    let probe = ratatui::layout::Rect::new(0, 0, width, u16::MAX);
    let chrome = u16::from(app.config.general.tab_bar) + u16::from(app.config.general.footer);
    let height = crate::ui::header::preferred_height(&app, probe)
        .saturating_add(crate::ui::panels_min_height(&app))
        .saturating_add(chrome)
        .max(3);

    // An inline viewport has to ask the terminal where the cursor is, which
    // only a real tty can answer; piping to a file or into CI would otherwise
    // fail on the cursor-position query. A fixed viewport needs no such round
    // trip, so that is what a redirected stdout gets.
    let viewport = if io::stdout().is_terminal() {
        Viewport::Inline(height)
    } else {
        Viewport::Fixed(ratatui::layout::Rect::new(0, 0, width, height))
    };

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::with_options(backend, TerminalOptions { viewport })?;

    terminal.draw(|frame| crate::ui::draw(frame, &app))?;
    // Leave the cursor below the frame so the shell prompt does not overwrite it.
    println!();
    Ok(())
}

/// The main loop.
fn run_loop(mut app: App) -> io::Result<()> {
    if !io::stdout().is_terminal() {
        // Without a terminal there is nothing to drive; fall back to one frame.
        return draw_once(app);
    }

    let alt_screen = app.config.general.alt_screen;
    let mouse = app.config.general.mouse;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    if alt_screen {
        execute!(stdout, EnterAlternateScreen)?;
    }
    if mouse {
        execute!(stdout, EnableMouseCapture)?;
    }

    // A panic must not leave the terminal in raw mode.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore(alt_screen, mouse);
        previous_hook(info);
    }));

    let result = event_loop(&mut app);

    restore(alt_screen, mouse);
    let _ = std::panic::take_hook();
    result
}

fn event_loop(app: &mut App) -> io::Result<()> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let mut next_tick = Instant::now() + app.interval();

    loop {
        terminal.draw(|frame| crate::ui::draw(frame, app))?;

        if app.should_quit || app.timed_out() {
            return Ok(());
        }

        // Wait for input, but never past the next scheduled sample.
        let timeout = next_tick.saturating_duration_since(Instant::now());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if app.handle_text_input(&key) {
                        continue;
                    }
                    // Ctrl-C is honoured even if the user unbound quit.
                    if is_interrupt(&key) {
                        return Ok(());
                    }
                    let action = app.action_for(&key);
                    if action != Action::None {
                        app.apply(action);
                    }
                }
                Event::Mouse(ev) => {
                    let action = app.action_for_mouse(&ev);
                    if action != Action::None {
                        app.apply(action);
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if Instant::now() >= next_tick {
            app.tick();
            // Re-derive from now rather than adding, so a slow sample cannot
            // build up a backlog of immediate ticks.
            next_tick = Instant::now() + app.interval();
        }
    }
}

/// Raw mode swallows the terminal's own interrupt, so it is handled here.
fn is_interrupt(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_argument_parser_is_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn shorthand_flags_become_overrides() {
        let args = Args::parse_from([
            "ratfetch",
            "--logo",
            "arch",
            "--theme",
            "nord",
            "--interval",
            "250",
            "--exit-after",
            "5",
        ]);
        let options = args.load_options();
        assert!(options.overrides.contains(&"logo.source=arch".to_string()));
        assert!(options.overrides.contains(&"theme.name=nord".to_string()));
        assert!(options
            .overrides
            .contains(&"general.interval_ms=250".to_string()));
        assert!(options
            .overrides
            .contains(&"general.exit_after=5".to_string()));
    }

    #[test]
    fn explicit_overrides_come_before_the_shorthands() {
        let args = Args::parse_from(["ratfetch", "--set", "logo.source=nixos", "--logo", "arch"]);
        let options = args.load_options();
        // The shorthand is appended last, so it wins the merge.
        assert_eq!(
            options.overrides,
            vec![
                "logo.source=nixos".to_string(),
                "logo.source=arch".to_string()
            ]
        );
    }

    #[test]
    fn no_config_skips_files() {
        let args = Args::parse_from(["ratfetch", "--no-config"]);
        assert!(args.load_options().skip_files);
    }

    #[test]
    fn generate_config_takes_an_optional_path() {
        let args = Args::parse_from(["ratfetch", "--generate-config"]);
        assert_eq!(args.generate_config.as_deref(), Some(""));

        let args = Args::parse_from(["ratfetch", "--generate-config", "/tmp/c.toml"]);
        assert_eq!(args.generate_config.as_deref(), Some("/tmp/c.toml"));

        let args = Args::parse_from(["ratfetch"]);
        assert_eq!(args.generate_config, None);
    }

    #[test]
    fn ctrl_c_is_always_an_interrupt() {
        assert!(is_interrupt(&KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_interrupt(&KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::NONE
        )));
    }
}

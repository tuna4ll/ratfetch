//! The logo beside the info table — the part that looks like a fetch tool.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use crate::config::enums::{InfoItem, LogoPosition};
use crate::config::theme::Theme;
use crate::sys::{temp, time};
use crate::util::{human_bytes, human_rate, human_uptime, truncate};

/// One row of the info table, before it is laid out.
struct Row {
    key: &'static str,
    value: String,
}

/// Builds the info rows the config asked for.
///
/// Rows whose value is unavailable come back with an empty value, and are
/// dropped later when `info.hide_empty` is set.
fn rows(app: &App) -> Vec<Option<Row>> {
    let s = &app.statics;
    let d = &app.dynamic;
    let cfg = &app.config;

    let row = |key: &'static str, value: String| Some(Row { key, value });

    cfg.info
        .items
        .iter()
        .map(|item| match item {
            // Structural rows carry no key/value pair.
            InfoItem::Title | InfoItem::Separator | InfoItem::Blank | InfoItem::Colors => None,

            InfoItem::Os => row("OS", s.os_name.clone()),
            InfoItem::Host => row("Host", s.host_model.clone()),
            InfoItem::Kernel => row("Kernel", s.kernel.clone()),
            InfoItem::Uptime => row("Uptime", human_uptime(d.uptime)),
            InfoItem::Packages => row("Packages", s.packages.clone()),
            InfoItem::Shell => row("Shell", s.shell.clone()),
            InfoItem::Terminal => row("Terminal", s.terminal.clone()),
            InfoItem::De => row("DE", s.de.clone()),
            InfoItem::Wm => row("WM", s.wm.clone()),
            InfoItem::Resolution => row("Display", s.resolution.clone()),

            InfoItem::Cpu => row("CPU", s.cpu.label(d.cpu.freq_mhz)),
            InfoItem::CpuTemp => row(
                "Temp",
                temp::cpu(&d.temps)
                    .map(|c| format!("{c:.1} °C"))
                    .unwrap_or_default(),
            ),
            InfoItem::Gpu => row(
                "GPU",
                s.gpus
                    .iter()
                    .map(|g| g.label())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),

            InfoItem::Memory => row(
                "Memory",
                format!(
                    "{} / {} ({:.0}%)",
                    human_bytes(d.mem.used),
                    human_bytes(d.mem.total),
                    d.mem.percent()
                ),
            ),
            InfoItem::Swap => row(
                "Swap",
                if d.mem.has_swap() {
                    format!(
                        "{} / {} ({:.0}%)",
                        human_bytes(d.mem.swap_used),
                        human_bytes(d.mem.swap_total),
                        d.mem.swap_percent()
                    )
                } else {
                    String::new()
                },
            ),
            InfoItem::Disk => row(
                "Disk",
                d.primary_disk(&cfg.disks)
                    .map(|disk| {
                        format!(
                            "{} / {} ({:.0}%) [{}]",
                            human_bytes(disk.used),
                            human_bytes(disk.total),
                            disk.percent(),
                            disk.fstype
                        )
                    })
                    .unwrap_or_default(),
            ),

            InfoItem::LocalIp => row("Local IP", d.local_ip.clone()),
            InfoItem::Battery => row(
                "Battery",
                d.battery.as_ref().map(|b| b.label()).unwrap_or_default(),
            ),
            InfoItem::LoadAvg => row(
                "Load",
                format!("{:.2} {:.2} {:.2}", d.load[0], d.load[1], d.load[2]),
            ),
            InfoItem::Processes => row(
                "Processes",
                format!("{} ({} threads)", d.proc_total, d.thread_total),
            ),
            InfoItem::Users => row("Users", d.users.to_string()),
            InfoItem::Locale => row("Locale", s.locale.clone()),
            InfoItem::DateTime => row("Date", time::DateTime::now().pretty()),
        })
        .collect()
}

/// The eight colour pairs a fetch tool traditionally prints.
fn palette_line(theme: &Theme) -> Line<'static> {
    use ratatui::style::Color;
    const SWATCH: &str = "███";
    let colors = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
    ];
    Line::from(
        colors
            .iter()
            .map(|c| Span::styled(SWATCH, ratatui::style::Style::default().fg(*c)))
            .collect::<Vec<_>>(),
    )
    .style(theme.background.bg())
}

/// Renders the info table as styled lines.
pub fn info_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let cfg = &app.config.info;
    let theme = &app.theme;
    let built = rows(app);

    // The key column is as wide as the widest key that survives filtering.
    let key_width = match cfg.key_width {
        0 => built
            .iter()
            .flatten()
            .filter(|r| !cfg.hide_empty || !r.value.is_empty())
            .map(|r| r.key.chars().count())
            .max()
            .unwrap_or(0),
        n => n as usize,
    };

    let mut out = Vec::new();

    for (item, row) in app.config.info.items.iter().zip(built) {
        match item {
            InfoItem::Title => {
                let title = format!("{}@{}", app.statics.username, app.statics.hostname);
                let style = if cfg.bold_title {
                    theme
                        .title
                        .fg()
                        .add_modifier(ratatui::style::Modifier::BOLD)
                } else {
                    theme.title.fg()
                };
                out.push(Line::from(Span::styled(title, style)));
            }
            InfoItem::Separator => {
                let title_len =
                    app.statics.username.chars().count() + app.statics.hostname.chars().count() + 1;
                let rule = if cfg.rule.is_empty() {
                    "─"
                } else {
                    cfg.rule.as_str()
                };
                let count = title_len.max(1).min(width.max(1) as usize);
                out.push(Line::from(Span::styled(
                    rule.repeat(count),
                    theme.muted.fg(),
                )));
            }
            InfoItem::Blank => out.push(Line::from("")),
            InfoItem::Colors => out.push(palette_line(theme)),
            _ => {
                let Some(row) = row else { continue };
                if cfg.hide_empty && row.value.trim().is_empty() {
                    continue;
                }
                let key = format!("{:<key_width$}", row.key);
                // Long values are cut rather than wrapped, so each row stays
                // one line and the table lines up with the logo beside it.
                let room =
                    (width as usize).saturating_sub(key_width + cfg.separator.chars().count());
                out.push(Line::from(vec![
                    Span::styled(key, theme.key.fg()),
                    Span::styled(cfg.separator.clone(), theme.muted.fg()),
                    Span::styled(truncate(&row.value, room), theme.value.fg()),
                ]));
            }
        }
    }

    out
}

/// Full-width information for the details overlay.
pub fn detail_lines(app: &App) -> Vec<Line<'static>> {
    info_lines(app, u16::MAX)
}

/// A one-line summary of live load, shown under the info table when there is
/// room for it.
pub fn live_line(app: &App) -> Line<'static> {
    let theme = &app.theme;
    let d = &app.dynamic;
    let (rx, tx) = d.net_rates();

    let part = |label: &'static str, value: String, color: crate::config::ColorSpec| {
        vec![
            Span::styled(label, theme.muted.fg()),
            Span::styled(value, color.fg()),
            Span::styled("  ", theme.muted.fg()),
        ]
    };

    let cpu_color = theme.level(
        d.cpu.total,
        app.config.meters.warn_at,
        app.config.meters.critical_at,
    );
    let mem_color = theme.level(
        d.mem.percent(),
        app.config.meters.warn_at,
        app.config.meters.critical_at,
    );

    let mut spans = Vec::new();
    spans.extend(part("cpu ", format!("{:.0}%", d.cpu.total), cpu_color));
    spans.extend(part("mem ", format!("{:.0}%", d.mem.percent()), mem_color));
    spans.extend(part("↓", human_rate(rx), theme.good));
    spans.extend(part("↑", human_rate(tx), theme.graph_secondary));
    Line::from(spans)
}

/// Whether the logo is shown at all.
fn logo_visible(app: &App) -> bool {
    app.logo.is_some() && app.config.logo.position != LogoPosition::None
}

/// Whether the logo goes above the info table rather than beside it.
///
/// `inner_width` is the space inside the header's border. A terminal narrower
/// than `layout.narrow_width` stacks them, so the values stay readable instead
/// of being squeezed into a sliver of column.
fn is_stacked(app: &App, inner_width: u16) -> bool {
    logo_visible(app)
        && (app.config.logo.position == LogoPosition::Top
            || inner_width < app.config.layout.narrow_width)
}

/// How tall the header wants to be.
pub fn preferred_height(app: &App, area: Rect) -> u16 {
    // Borders cost two columns and two rows when they are drawn.
    let chrome = if app.theme.borders { 2 } else { 0 };
    let inner_width = area.width.saturating_sub(chrome);

    let logo_height = if logo_visible(app) {
        clamp_logo_height(
            app.logo.as_ref().expect("logo_visible checked it"),
            &app.config,
        )
    } else {
        0
    };

    let content = if is_stacked(app, inner_width) {
        let info_height = info_lines(app, inner_width).len() as u16;
        logo_height.saturating_add(info_height)
    } else {
        let info_width = inner_width.saturating_sub(logo_width(app, inner_width));
        let info_height = info_lines(app, info_width).len() as u16;
        logo_height.max(info_height)
    };

    content.saturating_add(chrome).min(area.height)
}

fn clamp_logo_height(logo: &crate::logo::Logo, cfg: &crate::config::Config) -> u16 {
    match cfg.logo.max_height {
        0 => logo.height(),
        n => logo.height().min(n),
    }
}

/// Columns the logo column occupies inside `available`, padding included.
fn logo_width(app: &App, available: u16) -> u16 {
    let Some(logo) = &app.logo else { return 0 };
    if !logo_visible(app) || is_stacked(app, available) {
        return 0;
    }

    let natural = match app.config.layout.logo_width {
        0 => logo.width().saturating_add(app.config.logo.padding),
        n => n,
    };
    let natural = match app.config.logo.max_width {
        0 => natural,
        n => natural.min(n.saturating_add(app.config.logo.padding)),
    };
    // Never let the art squeeze the info table out entirely.
    natural.min(available.saturating_sub(available / 3))
}

/// Draws the header.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let block = super::panel(&app.theme, "");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if !logo_visible(app) {
        info(frame, inner, app);
        return;
    }

    if is_stacked(app, inner.width) {
        let logo = app.logo.as_ref().expect("logo_visible checked it");
        let logo_height = clamp_logo_height(logo, &app.config);
        // A logo is decoration; on a short split pane the facts take priority.
        if inner.height < logo_height.saturating_add(6) {
            info(frame, inner, app);
            return;
        }
        // The logo never takes so much height that the table has no room.
        let height =
            clamp_logo_height(logo, &app.config).min(inner.height.saturating_sub(inner.height / 3));
        let chunks =
            Layout::vertical([Constraint::Length(height), Constraint::Min(0)]).split(inner);
        logo_widget(frame, chunks[0], app);
        info(frame, chunks[1], app);
        return;
    }

    let width = logo_width(app, inner.width);
    let chunks = match app.config.logo.position {
        LogoPosition::Right => {
            let chunks =
                Layout::horizontal([Constraint::Min(0), Constraint::Length(width)]).split(inner);
            [chunks[1], chunks[0]]
        }
        _ => {
            let chunks =
                Layout::horizontal([Constraint::Length(width), Constraint::Min(0)]).split(inner);
            [chunks[0], chunks[1]]
        }
    };

    logo_widget(frame, chunks[0], app);
    info(frame, chunks[1], app);
}

fn logo_widget(frame: &mut Frame, area: Rect, app: &App) {
    let Some(logo) = &app.logo else { return };
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lines = logo.render(
        app.config.logo.color_mode,
        &app.theme,
        &app.config.logo.colors,
    );
    let area = if app.config.logo.border {
        let block = super::panel(&app.theme, "");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    } else {
        area
    };

    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.scroll.min(u16::MAX as usize) as u16, 0))
            .style(super::surface(&app.theme)),
        area,
    );
}

fn info(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut lines = info_lines(app, area.width);

    // Fill any spare row under the table with the live summary, so the header
    // is never a block of static text.
    if lines.len() + 2 <= area.height as usize {
        lines.push(Line::from(""));
        lines.push(live_line(app));
    }

    frame.render_widget(
        Paragraph::new(lines).style(super::surface(&app.theme)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, LoadOptions, Loaded};

    fn app_with(config: Config) -> App {
        App::new(
            Loaded {
                config,
                sources: Vec::new(),
                warnings: Vec::new(),
            },
            LoadOptions {
                skip_files: true,
                ..Default::default()
            },
        )
    }

    #[test]
    fn structural_rows_have_no_key_value_pair() {
        let app = app_with(Config::default());
        let built = rows(&app);
        assert_eq!(built.len(), app.config.info.items.len());
        // The default list starts with title and separator.
        assert!(built[0].is_none());
        assert!(built[1].is_none());
    }

    #[test]
    fn every_info_item_can_be_built() {
        let mut config = Config::default();
        config.info.items = crate::config::enums::InfoItem::ALL.to_vec();
        let app = app_with(config);
        let lines = info_lines(&app, 80);
        // Structural rows always render; value rows may be hidden when empty.
        assert!(!lines.is_empty());
    }

    #[test]
    fn empty_values_are_dropped_when_asked() {
        let mut config = Config::default();
        config.info.items = vec![InfoItem::Battery];
        config.info.hide_empty = true;
        let hidden = app_with(config.clone());

        config.info.hide_empty = false;
        let shown = app_with(config);

        // On a desktop the battery row is empty; on a laptop both render it.
        assert!(info_lines(&hidden, 80).len() <= info_lines(&shown, 80).len());
    }

    #[test]
    fn the_header_asks_for_a_sane_height() {
        let app = app_with(Config::default());
        let area = Rect::new(0, 0, 100, 40);
        let h = preferred_height(&app, area);
        assert!(h > 0 && h <= area.height);
    }

    #[test]
    fn the_logo_never_takes_the_whole_width() {
        let app = app_with(Config::default());
        for width in [80u16, 100, 200] {
            assert!(logo_width(&app, width) < width, "at {width} columns");
        }
    }

    #[test]
    fn a_hidden_logo_takes_no_columns() {
        let mut config = Config::default();
        config.logo.position = LogoPosition::None;
        let app = app_with(config);
        assert_eq!(logo_width(&app, 100), 0);
        assert!(!is_stacked(&app, 40), "a hidden logo has nothing to stack");
    }

    #[test]
    fn a_narrow_terminal_stacks_the_logo_above_the_table() {
        let app = app_with(Config::default());
        assert!(is_stacked(&app, 60), "below narrow_width");
        assert!(!is_stacked(&app, 120), "above narrow_width");
        // Stacked means the logo claims no columns of its own.
        assert_eq!(logo_width(&app, 60), 0);
    }

    #[test]
    fn stacking_asks_for_room_for_both_halves() {
        let app = app_with(Config::default());
        let tall = Rect::new(0, 0, 60, 200);
        let side_by_side = Rect::new(0, 0, 120, 200);

        let logo_height = app.logo.as_ref().unwrap().height();
        // Stacked, the header needs the logo and the table, not the taller one.
        assert!(preferred_height(&app, tall) > logo_height);
        assert!(preferred_height(&app, tall) > preferred_height(&app, side_by_side));
    }
}

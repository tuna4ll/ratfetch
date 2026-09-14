//! Drawing the frame.

pub mod chrome;
pub mod graphs;
pub mod header;
pub mod meters;
pub mod tables;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, BorderType, Borders};
use ratatui::Frame;

use crate::app::App;
use crate::config::enums::{PanelKind, Tab};
use crate::config::theme::Theme;

/// Draws the whole frame.
pub fn draw(frame: &mut Frame, app: &App) {
    let mut area = frame.area();

    // Paint the theme's background across the terminal, so a themed page does
    // not show the host colour through the gaps.
    frame.render_widget(Block::default().style(app.theme.background.bg()), area);

    let margin = app.config.layout.margin;
    area = shrink(area, margin);
    if area.width < 8 || area.height < 3 {
        // Nothing sensible fits; leave the background and stop.
        return;
    }

    let show_tabs = app.config.general.tab_bar && app.config.general.tabs.len() > 1;
    let chunks = Layout::vertical([
        Constraint::Length(u16::from(show_tabs)),
        Constraint::Min(1),
        Constraint::Length(u16::from(app.config.general.footer)),
    ])
    .split(area);

    if show_tabs {
        chrome::tab_bar(frame, chunks[0], app);
    }
    body(frame, chunks[1], app);
    if app.config.general.footer {
        chrome::footer(frame, chunks[2], app);
    }

    if app.show_help {
        chrome::help(frame, area, app);
    } else if app.show_info_details {
        chrome::info_details(frame, area, app);
    } else if app.show_errors {
        chrome::errors(frame, area, app);
    }
}

fn body(frame: &mut Frame, area: Rect, app: &App) {
    match app.tab {
        Tab::Overview => overview(frame, area, app),
        Tab::Processes => tables::processes(frame, area, app, app.scroll, true),
        Tab::Disks => tables::disks(frame, area, app),
        Tab::Network => tables::network(frame, area, app),
    }
}

/// The logo and info header, with the configured panels stacked underneath.
fn overview(frame: &mut Frame, area: Rect, app: &App) {
    // On a short, narrow terminal graphs collapse into a few meaningless cells.
    // Keep the information and meters reachable and let the richer graph view
    // return as soon as the terminal grows.
    let compact = area.width < app.config.layout.narrow_width && area.height < 30;
    let panels: Vec<_> = app
        .config
        .layout
        .panels
        .iter()
        .copied()
        .filter(|panel| !(compact && matches!(panel, PanelKind::Graphs | PanelKind::Colors)))
        .collect();

    let header_height = match app.config.layout.header_height {
        0 => header::preferred_height(app, area),
        n => n,
    };
    let header_height = if panels.is_empty() {
        // With no panels the header is free to use the whole area.
        area.height
    } else {
        // Otherwise it gives up whatever the panels need at minimum, so a tall
        // logo on a short terminal cannot squeeze them out of the frame.
        let panel_rows = panels.iter().map(|p| panel_min_height(app, *p)).sum();
        header_height.min(area.height.saturating_sub(panel_rows).max(1))
    };

    let chunks =
        Layout::vertical([Constraint::Length(header_height), Constraint::Min(0)]).split(area);

    header::draw(frame, chunks[0], app);

    let rest = chunks[1];
    if panels.is_empty() || rest.height == 0 {
        return;
    }

    let heights = &app.config.layout.panel_heights;
    let constraints: Vec<Constraint> = panels
        .iter()
        .map(|panel| match panel {
            PanelKind::Meters if heights.meters > 0 => Constraint::Length(heights.meters),
            // Bars have a natural size, unlike the panels that scroll, so an
            // unset height means "take what you need" rather than "fill".
            PanelKind::Meters => Constraint::Length(meters::natural_height(app)),
            PanelKind::Graphs if heights.graphs > 0 => Constraint::Length(heights.graphs),
            PanelKind::Processes if heights.processes > 0 => Constraint::Length(heights.processes),
            PanelKind::Disks if heights.disks > 0 => Constraint::Length(heights.disks),
            PanelKind::Network if heights.network > 0 => Constraint::Length(heights.network),
            PanelKind::Colors => Constraint::Length(heights.colors.max(1)),
            // A zero height means "share what is left".
            _ => Constraint::Fill(1),
        })
        .collect();

    let slots = Layout::vertical(constraints).split(rest);

    for (panel, slot) in panels.iter().zip(slots.iter()) {
        if slot.height == 0 {
            continue;
        }
        match panel {
            PanelKind::Meters => meters::draw(frame, *slot, app),
            PanelKind::Graphs => graphs::draw(frame, *slot, app),
            PanelKind::Processes => {
                tables::processes(frame, *slot, app, 0, false);
            }
            PanelKind::Disks => tables::disks(frame, *slot, app),
            PanelKind::Network => tables::network(frame, *slot, app),
            PanelKind::Colors => chrome::palette(frame, *slot, app),
        }
    }
}

/// The rows the configured panels need before they stop being useful.
///
/// The header is capped to whatever is left over, and `--once` uses it to size
/// its viewport so nothing is cut off.
pub fn panels_min_height(app: &App) -> u16 {
    app.config
        .layout
        .panels
        .iter()
        .map(|panel| panel_min_height(app, *panel))
        .fold(0u16, u16::saturating_add)
}

fn panel_min_height(app: &App, panel: PanelKind) -> u16 {
    let heights = &app.config.layout.panel_heights;
    const SCROLLING_MIN: u16 = 4;
    match panel {
        PanelKind::Meters if heights.meters > 0 => heights.meters,
        PanelKind::Meters => meters::natural_height(app),
        PanelKind::Graphs if heights.graphs > 0 => heights.graphs,
        PanelKind::Processes if heights.processes > 0 => heights.processes,
        PanelKind::Disks if heights.disks > 0 => heights.disks,
        PanelKind::Network if heights.network > 0 => heights.network,
        PanelKind::Colors => heights.colors.max(1),
        _ => SCROLLING_MIN,
    }
}

/// Insets a rect on all sides, without underflowing.
pub fn shrink(area: Rect, by: u16) -> Rect {
    let horizontal = by.saturating_mul(2);
    Rect {
        x: area.x.saturating_add(by),
        y: area.y.saturating_add(by),
        width: area.width.saturating_sub(horizontal),
        height: area.height.saturating_sub(horizontal),
    }
}

/// A panel block honouring the theme's border settings.
///
/// When borders are switched off the block is invisible, which keeps every
/// caller's inner-area arithmetic identical either way.
pub fn panel<'a>(theme: &Theme, title: &'a str) -> Block<'a> {
    let block = Block::default().style(theme.background.bg());
    if !theme.borders {
        return block;
    }
    let mut block = block
        .borders(Borders::ALL)
        .border_style(theme.border.fg())
        .border_type(if theme.rounded {
            BorderType::Rounded
        } else {
            BorderType::Plain
        });
    if !title.is_empty() {
        block = block.title(title).title_style(theme.title.fg());
    }
    block
}

/// The style used for a panel's own background.
pub fn surface(theme: &Theme) -> Style {
    theme.background.bg()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_never_underflows() {
        let area = Rect::new(0, 0, 4, 4);
        let small = shrink(area, 10);
        assert_eq!(small.width, 0);
        assert_eq!(small.height, 0);
    }

    #[test]
    fn shrink_insets_all_sides() {
        let area = Rect::new(0, 0, 10, 10);
        let inner = shrink(area, 2);
        assert_eq!((inner.x, inner.y, inner.width, inner.height), (2, 2, 6, 6));
    }
}

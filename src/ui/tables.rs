//! The process, disk and network tables.

use ratatui::layout::{Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::App;
use crate::util::{human_bytes, human_rate, truncate};

/// Clamps a scroll offset so the last page is the furthest it can go.
fn clamp_scroll(offset: usize, rows: usize, visible: usize) -> usize {
    offset.min(rows.saturating_sub(visible))
}

/// Draws a placeholder when a table has nothing to show.
fn empty(frame: &mut Frame, area: Rect, app: &App, message: &str) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            message.to_string(),
            app.theme.muted.fg(),
        )))
        .style(super::surface(&app.theme)),
        area,
    );
}

/// The process table.
///
/// `scroll` is ignored and the title simplified when `full` is false, which is
/// how the overview panel shows a short top-N list.
pub fn processes(frame: &mut Frame, area: Rect, app: &App, scroll: usize, full: bool) {
    let title = format!(
        " Processes — {} by {}{} ",
        app.dynamic.proc_total,
        app.sort,
        if app.config.processes.ascending {
            " ▲"
        } else {
            " ▼"
        }
    );
    let block = super::panel(&app.theme, &title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }
    if app.dynamic.procs.is_empty() {
        empty(frame, inner, app, "no processes visible");
        return;
    }

    // One row is spent on the header.
    let visible = inner.height.saturating_sub(1) as usize;
    if visible == 0 {
        return;
    }

    let limit = if full {
        visible
    } else {
        app.config.processes.count.min(visible)
    };
    let offset = if full {
        clamp_scroll(scroll, app.dynamic.procs.len(), visible)
    } else {
        0
    };

    let theme = &app.theme;
    let rows: Vec<Row> = app
        .dynamic
        .procs
        .iter()
        .skip(offset)
        .take(limit)
        .map(|p| {
            let cpu_color = theme.level(
                p.cpu,
                app.config.meters.warn_at,
                app.config.meters.critical_at,
            );
            Row::new(vec![
                Cell::from(p.pid.to_string()).style(theme.muted.fg()),
                Cell::from(p.display(app.config.processes.full_command).to_string())
                    .style(theme.value.fg()),
                Cell::from(format!("{:>5.1}", p.cpu)).style(cpu_color.fg()),
                Cell::from(human_bytes(p.memory)).style(theme.foreground.fg()),
                Cell::from(p.state.to_string()).style(theme.muted.fg()),
            ])
        })
        .collect();

    let header = Row::new(vec!["PID", "COMMAND", "CPU%", "MEM", "S"]).style(
        theme
            .accent
            .fg()
            .add_modifier(ratatui::style::Modifier::BOLD),
    );

    let widths = [
        Constraint::Length(7),
        Constraint::Fill(1),
        Constraint::Length(6),
        Constraint::Length(10),
        Constraint::Length(1),
    ];

    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .style(super::surface(theme)),
        inner,
    );
}

/// The filesystem table.
pub fn disks(frame: &mut Frame, area: Rect, app: &App) {
    let block = super::panel(&app.theme, " Filesystems ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }
    if app.dynamic.disks.is_empty() {
        empty(frame, inner, app, "no filesystems matched the filters");
        return;
    }

    let theme = &app.theme;
    let visible = inner.height.saturating_sub(1) as usize;
    let offset = clamp_scroll(app.scroll, app.dynamic.disks.len(), visible.max(1));

    let rows: Vec<Row> = app
        .dynamic
        .disks
        .iter()
        .skip(offset)
        .map(|d| {
            let percent = d.percent();
            let color = theme.level(
                percent,
                app.config.meters.warn_at,
                app.config.meters.critical_at,
            );
            Row::new(vec![
                Cell::from(truncate(&d.mount, 24)).style(theme.value.fg()),
                Cell::from(truncate(&d.device, 20)).style(theme.muted.fg()),
                Cell::from(d.fstype.clone()).style(theme.muted.fg()),
                Cell::from(human_bytes(d.used)).style(theme.foreground.fg()),
                Cell::from(human_bytes(d.total)).style(theme.foreground.fg()),
                Cell::from(human_bytes(d.available)).style(theme.foreground.fg()),
                Cell::from(format!("{percent:>5.1}%")).style(color.fg()),
            ])
        })
        .collect();

    let header = Row::new(vec![
        "MOUNT", "DEVICE", "TYPE", "USED", "SIZE", "FREE", "USE%",
    ])
    .style(
        theme
            .accent
            .fg()
            .add_modifier(ratatui::style::Modifier::BOLD),
    );

    let widths = [
        Constraint::Fill(2),
        Constraint::Fill(2),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(6),
    ];

    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .style(super::surface(theme)),
        inner,
    );
}

/// The interface table.
pub fn network(frame: &mut Frame, area: Rect, app: &App) {
    let block = super::panel(&app.theme, " Network ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }
    if app.dynamic.nets.is_empty() {
        empty(frame, inner, app, "no interfaces are up");
        return;
    }

    let theme = &app.theme;
    let visible = inner.height.saturating_sub(1) as usize;
    let offset = clamp_scroll(app.scroll, app.dynamic.nets.len(), visible.max(1));

    let rows: Vec<Row> = app
        .dynamic
        .nets
        .iter()
        .skip(offset)
        .map(|n| {
            let state_color = if n.is_up() { theme.good } else { theme.muted };
            Row::new(vec![
                Cell::from(n.name.clone()).style(theme.value.fg()),
                Cell::from(n.state.clone()).style(state_color.fg()),
                Cell::from(n.ipv4.clone().unwrap_or_else(|| "—".to_string()))
                    .style(theme.foreground.fg()),
                Cell::from(human_rate(n.rx_rate)).style(theme.good.fg()),
                Cell::from(human_rate(n.tx_rate)).style(theme.graph_secondary.fg()),
                Cell::from(human_bytes(n.rx_bytes)).style(theme.muted.fg()),
                Cell::from(human_bytes(n.tx_bytes)).style(theme.muted.fg()),
            ])
        })
        .collect();

    let header = Row::new(vec![
        "IFACE",
        "STATE",
        "IPV4",
        "↓ RATE",
        "↑ RATE",
        "↓ TOTAL",
        "↑ TOTAL",
    ])
    .style(
        theme
            .accent
            .fg()
            .add_modifier(ratatui::style::Modifier::BOLD),
    );

    let widths = [
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Length(16),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(11),
        Constraint::Length(11),
    ];

    frame.render_widget(
        Table::new(rows, widths)
            .header(header)
            .column_spacing(1)
            .style(super::surface(theme)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_stops_at_the_last_page() {
        // 100 rows, 10 visible: the furthest offset is 90.
        assert_eq!(clamp_scroll(0, 100, 10), 0);
        assert_eq!(clamp_scroll(50, 100, 10), 50);
        assert_eq!(clamp_scroll(95, 100, 10), 90);
        assert_eq!(clamp_scroll(9999, 100, 10), 90);
    }

    #[test]
    fn a_list_shorter_than_the_view_never_scrolls() {
        assert_eq!(clamp_scroll(5, 3, 10), 0);
        assert_eq!(clamp_scroll(5, 0, 10), 0);
    }
}

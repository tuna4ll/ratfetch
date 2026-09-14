//! Tab strip, footer, help overlay and the colour palette panel.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::config::keys::{render_list, KeyBinding};

/// The strip of tab names along the top.
pub fn tab_bar(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let mut spans = vec![Span::styled(" ", theme.background.bg())];

    for (i, tab) in app.config.general.tabs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", theme.muted.fg()));
        }
        let style = if *tab == app.tab {
            theme
                .tab_active
                .fg()
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            theme.tab_inactive.fg()
        };
        spans.push(Span::styled(format!(" {tab} "), style));
    }

    let left = Line::from(spans);
    let mut right = Line::from(vec![
        Span::styled(if app.frozen { "⏸ frozen " } else { "" }, theme.warn.fg()),
        Span::styled(format!("ratfetch {} ", crate::VERSION), theme.muted.fg()),
    ]);

    if left.width().saturating_add(right.width()) > area.width as usize {
        right = Line::from(Span::styled(
            if app.frozen { "⏸ frozen " } else { "" },
            theme.warn.fg(),
        ));
    }
    let chunks = Layout::horizontal([Constraint::Min(0), Constraint::Length(right.width() as u16)])
        .split(area);

    frame.render_widget(Paragraph::new(left).style(theme.background.bg()), chunks[0]);
    frame.render_widget(
        Paragraph::new(right)
            .alignment(Alignment::Right)
            .style(theme.background.bg()),
        chunks[1],
    );
}

/// A `key action` pair for the footer.
fn hint<'a>(bindings: &[KeyBinding], label: &'a str, app: &App) -> Vec<Span<'a>> {
    if bindings.is_empty() {
        return Vec::new();
    }
    // Only the first spelling is advertised; the rest still work.
    vec![
        Span::styled(
            bindings[0].to_string(),
            app.theme.accent.fg().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {label}  "), app.theme.footer.fg()),
    ]
}

/// The key hint bar along the bottom, or the current status message.
pub fn footer(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    if let Some(status) = &app.status {
        let color = if status.is_error {
            theme.critical
        } else {
            theme.good
        };
        let line = Line::from(vec![
            Span::styled(if status.is_error { " ! " } else { " ✓ " }, color.fg()),
            Span::styled(
                crate::util::truncate(&status.text, area.width.saturating_sub(3) as usize),
                color.fg(),
            ),
            Span::styled(
                if status.is_error { "  e details" } else { "" },
                theme.footer.fg(),
            ),
        ]);
        frame.render_widget(Paragraph::new(line).style(theme.background.bg()), area);
        return;
    }

    let k = &app.config.keys;
    let mut spans = vec![Span::styled(" ", theme.background.bg())];
    spans.extend(hint(&k.quit, "quit", app));
    spans.extend(hint(&k.help, "help", app));
    spans.extend(hint(&k.next_tab, "tab", app));
    spans.extend(hint(&k.sort_next, "sort", app));
    spans.extend(hint(
        &k.freeze,
        if app.frozen { "resume" } else { "freeze" },
        app,
    ));
    spans.extend(hint(&k.reload, "reload", app));
    if app.tab == crate::config::enums::Tab::Overview {
        spans.extend(hint(&k.info_details, "info", app));
    }
    spans.extend(hint(&k.scroll_down, "scroll", app));

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme.background.bg()),
        area,
    );
}

/// The eight normal and eight bright terminal colours.
pub fn palette(frame: &mut Frame, area: Rect, app: &App) {
    let block = super::panel(&app.theme, " Palette ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    const NORMAL: [Color; 8] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
    ];
    const BRIGHT: [Color; 8] = [
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];

    // Swatches share the width evenly, with at least one column each.
    let swatch = (inner.width as usize / 8).clamp(1, 6);
    let row = |colors: &[Color; 8]| {
        Line::from(
            colors
                .iter()
                .map(|c| Span::styled("█".repeat(swatch), Style::default().fg(*c)))
                .collect::<Vec<_>>(),
        )
    };

    let mut lines = vec![row(&NORMAL)];
    if inner.height > 1 {
        lines.push(row(&BRIGHT));
    }

    frame.render_widget(
        Paragraph::new(lines).style(super::surface(&app.theme)),
        inner,
    );
}

/// Centres a box of the given size inside `area`.
fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// The help overlay.
pub fn help(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let k = &app.config.keys;

    let entries: [(&str, &[KeyBinding]); 12] = [
        ("quit", &k.quit),
        ("this help", &k.help),
        ("reload config", &k.reload),
        ("next tab", &k.next_tab),
        ("previous tab", &k.prev_tab),
        ("scroll down", &k.scroll_down),
        ("scroll up", &k.scroll_up),
        ("cycle sort column", &k.sort_next),
        ("toggle per-core meters", &k.toggle_per_core),
        ("freeze / resume", &k.freeze),
        ("full system information", &k.info_details),
        ("error details", &k.errors),
    ];

    let key_column = entries
        .iter()
        .map(|(_, b)| render_list(b).chars().count())
        .max()
        .unwrap_or(0)
        .max(6);

    let mut lines = vec![Line::from(Span::styled(
        format!("ratfetch {}", crate::VERSION),
        theme.title.fg().add_modifier(Modifier::BOLD),
    ))];
    lines.push(Line::from(""));

    for (label, bindings) in entries {
        let keys = render_list(bindings);
        let keys = if keys.is_empty() {
            "—".to_string()
        } else {
            keys
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{keys:<key_column$}"), theme.accent.fg()),
            Span::styled("  ", theme.muted.fg()),
            Span::styled(label.to_string(), theme.value.fg()),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "theme {}  ·  {} logos bundled",
            app.config.theme.name,
            crate::logo::count()
        ),
        theme.muted.fg(),
    )));
    lines.push(Line::from(Span::styled(
        match crate::config::user_path() {
            Some(p) => format!("config {}", p.display()),
            None => "config path unavailable".to_string(),
        },
        theme.muted.fg(),
    )));

    let width = lines.iter().map(|l| l.width()).max().unwrap_or(20) as u16 + 4;
    let height = lines.len() as u16 + 2;
    let popup = centred(area, width, height);

    frame.render_widget(Clear, popup);
    let block = super::panel(theme, " Help ").border_style(theme.border_focus.fg());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(lines).style(super::surface(theme)),
        super::shrink(inner, 0),
    );
}

fn text_popup(frame: &mut Frame, area: Rect, app: &App, title: &str, lines: Vec<Line<'static>>) {
    let width = area.width.saturating_sub(4).min(100).max(1);
    let height = (lines.len() as u16 + 2)
        .min(area.height.saturating_sub(2))
        .max(1);
    let popup = centred(area, width, height);
    frame.render_widget(Clear, popup);
    let block = super::panel(&app.theme, title).border_style(app.theme.border_focus.fg());
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(super::surface(&app.theme)),
        inner,
    );
}

/// All system information without the overview's one-line truncation.
pub fn info_details(frame: &mut Frame, area: Rect, app: &App) {
    text_popup(
        frame,
        area,
        app,
        " Information — i/esc close ",
        super::header::detail_lines(app),
    );
}

/// Recent errors, retained after the footer notification is dismissed.
pub fn errors(frame: &mut Frame, area: Rect, app: &App) {
    let lines = if app.errors.is_empty() {
        vec![Line::from("No errors recorded.")]
    } else {
        app.errors
            .iter()
            .rev()
            .map(|error| Line::from(format!("! {error}")))
            .collect()
    };
    text_popup(frame, area, app, " Errors — e/esc close ", lines);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centring_keeps_the_box_inside() {
        let area = Rect::new(0, 0, 100, 40);
        let c = centred(area, 40, 10);
        assert_eq!((c.x, c.y, c.width, c.height), (30, 15, 40, 10));
    }

    #[test]
    fn an_oversized_box_is_clamped_to_the_area() {
        let area = Rect::new(0, 0, 10, 4);
        let c = centred(area, 100, 100);
        assert_eq!((c.x, c.y, c.width, c.height), (0, 0, 10, 4));
    }
}

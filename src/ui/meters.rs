//! The live percentage bars.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;
use crate::config::color::ColorSpec;
use crate::config::enums::{MeterKind, MeterStyle};
use crate::config::theme::Theme;
use crate::util::human_bytes;

/// One bar's worth of data.
struct Meter {
    label: String,
    percent: f64,
    /// The raw figures shown after the percentage, e.g. `7.2 GiB / 31.1 GiB`.
    detail: String,
}

/// The characters a style fills and pads with.
fn glyphs(style: MeterStyle) -> (&'static str, &'static str) {
    match style {
        MeterStyle::Bar => ("█", "░"),
        MeterStyle::Blocks => ("█", " "),
        MeterStyle::Line => ("━", "─"),
        MeterStyle::Dots => ("●", "·"),
    }
}

/// Eighth-block characters, for styles that can show a partial cell.
const PARTIAL: [&str; 8] = ["▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];

/// Builds the bar itself.
///
/// `Blocks` resolves to an eighth of a cell; the others fill whole cells.
fn bar(percent: f64, width: usize, style: MeterStyle) -> String {
    if width == 0 {
        return String::new();
    }
    let (full, empty) = glyphs(style);
    let fraction = (percent / 100.0).clamp(0.0, 1.0);

    if style == MeterStyle::Blocks {
        let eighths = (fraction * width as f64 * 8.0).round() as usize;
        let whole = eighths / 8;
        let remainder = eighths % 8;
        let mut out = full.repeat(whole.min(width));
        if whole < width && remainder > 0 {
            out.push_str(PARTIAL[remainder - 1]);
        }
        let drawn = whole.min(width) + usize::from(whole < width && remainder > 0);
        out.push_str(&empty.repeat(width.saturating_sub(drawn)));
        return out;
    }

    let filled = (fraction * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", full.repeat(filled), empty.repeat(width - filled))
}

/// Collects the meters the config asked for.
fn collect(app: &App) -> Vec<Meter> {
    let d = &app.dynamic;
    let cfg = &app.config;
    let mut out = Vec::new();

    for kind in &cfg.meters.items {
        match kind {
            MeterKind::Cpu => {
                if app.per_core && !d.cpu.per_core.is_empty() {
                    for (i, percent) in d.cpu.per_core.iter().enumerate() {
                        out.push(Meter {
                            label: format!("cpu{i}"),
                            percent: *percent,
                            detail: String::new(),
                        });
                    }
                } else {
                    let detail = match d.cpu.freq_mhz {
                        Some(mhz) => format!("{:.2} GHz", mhz / 1000.0),
                        None => String::new(),
                    };
                    out.push(Meter {
                        label: "CPU".into(),
                        percent: d.cpu.total,
                        detail,
                    });
                }
            }
            MeterKind::Memory => out.push(Meter {
                label: "Memory".into(),
                percent: d.mem.percent(),
                detail: format!("{} / {}", human_bytes(d.mem.used), human_bytes(d.mem.total)),
            }),
            MeterKind::Swap => {
                if d.mem.has_swap() {
                    out.push(Meter {
                        label: "Swap".into(),
                        percent: d.mem.swap_percent(),
                        detail: format!(
                            "{} / {}",
                            human_bytes(d.mem.swap_used),
                            human_bytes(d.mem.swap_total)
                        ),
                    });
                }
            }
            MeterKind::Disk => {
                if let Some(disk) = d.primary_disk(&cfg.disks) {
                    out.push(Meter {
                        label: "Disk".into(),
                        percent: disk.percent(),
                        detail: format!("{} / {}", human_bytes(disk.used), human_bytes(disk.total)),
                    });
                }
            }
            MeterKind::Battery => {
                if let Some(battery) = &d.battery {
                    out.push(Meter {
                        label: "Battery".into(),
                        percent: battery.percent,
                        detail: battery.status.clone(),
                    });
                }
            }
            MeterKind::Load => {
                // Load is not a percentage; scale it against the core count so
                // a fully-busy machine reads as 100%.
                let cores = app.statics.cpu.threads.max(1) as f64;
                out.push(Meter {
                    label: "Load".into(),
                    percent: (d.load[0] / cores * 100.0).clamp(0.0, 100.0),
                    detail: format!("{:.2} {:.2} {:.2}", d.load[0], d.load[1], d.load[2]),
                });
            }
            MeterKind::Gpu => {
                if let Some(percent) = d.gpus.iter().find_map(|gpu| gpu.usage_percent) {
                    let stats = d.gpus.iter().find(|gpu| gpu.usage_percent.is_some());
                    let detail = stats
                        .and_then(|gpu| match (gpu.memory_used, gpu.memory_total) {
                            (Some(used), Some(total)) => {
                                Some(format!("{} / {}", human_bytes(used), human_bytes(total)))
                            }
                            _ => gpu.temperature_celsius.map(|temp| format!("{temp:.0} °C")),
                        })
                        .unwrap_or_default();
                    out.push(Meter {
                        label: "GPU".into(),
                        percent,
                        detail,
                    });
                }
            }
        }
    }

    out
}

/// Renders one meter as a line that fits `width` exactly.
///
/// A narrow terminal loses the detail figures first, then the percentage,
/// then the label, so the bar itself survives as long as possible.
fn line(meter: &Meter, app: &App, width: u16, color: ColorSpec, theme: &Theme) -> Line<'static> {
    let cfg = &app.config.meters;
    let total = width as usize;

    let mut label_width = cfg.label_width as usize;
    let mut percent_text = if cfg.show_percent {
        format!(" {:>5.1}%", meter.percent)
    } else {
        String::new()
    };
    let mut detail_text = if cfg.show_values && !meter.detail.is_empty() {
        format!("  {}", meter.detail)
    } else {
        String::new()
    };

    // Everything that is not the bar: the padded label, both brackets, the
    // percentage and the detail.
    let fixed = |label: usize, percent: &str, detail: &str| {
        let label_cols = if label > 0 { label + 1 } else { 0 };
        label_cols + 2 + percent.chars().count() + detail.chars().count()
    };

    // A bar narrower than this is not worth the space it costs.
    const MIN_BAR: usize = 4;

    if fixed(label_width, &percent_text, &detail_text) + MIN_BAR > total {
        detail_text.clear();
    }
    if fixed(label_width, &percent_text, &detail_text) + MIN_BAR > total {
        percent_text.clear();
    }
    if fixed(label_width, &percent_text, &detail_text) + MIN_BAR > total {
        label_width = 0;
    }

    let used = fixed(label_width, &percent_text, &detail_text);
    if used > total {
        // Not even the brackets fit; show what little there is room for.
        return Line::from(Span::styled(
            crate::util::truncate(&meter.label, total),
            theme.foreground.fg(),
        ));
    }

    let label_part = if label_width > 0 {
        format!(
            "{:<label_width$} ",
            crate::util::truncate(&meter.label, label_width)
        )
    } else {
        String::new()
    };

    Line::from(vec![
        Span::styled(label_part, theme.foreground.fg()),
        Span::styled("[", theme.muted.fg()),
        Span::styled(bar(meter.percent, total - used, cfg.style), color.fg()),
        Span::styled("]", theme.muted.fg()),
        Span::styled(percent_text, color.fg()),
        Span::styled(detail_text, theme.muted.fg()),
    ])
}

/// Rows the panel needs to show every meter, borders included.
///
/// The layout uses this when `layout.panel_heights.meters` is left at zero, so
/// the bars take exactly the room they need and the graphs get the rest.
pub fn natural_height(app: &App) -> u16 {
    let rows = collect(app).len().min(u16::MAX as usize) as u16;
    let chrome = if app.theme.borders { 2 } else { 0 };
    rows.saturating_add(chrome).max(1)
}

/// Draws the meters panel.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let block = super::panel(&app.theme, " Load ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let cfg = &app.config.meters;
    let meters = collect(app);
    let lines: Vec<Line> = meters
        .iter()
        .take(inner.height as usize)
        .map(|m| {
            let color = app.theme.level(m.percent, cfg.warn_at, cfg.critical_at);
            line(m, app, inner.width, color, &app.theme)
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines).style(super::surface(&app.theme)),
        inner,
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
    fn a_bar_is_exactly_the_requested_width() {
        for style in MeterStyle::ALL {
            for percent in [0.0, 12.5, 50.0, 99.9, 100.0] {
                let b = bar(percent, 20, *style);
                assert_eq!(b.chars().count(), 20, "{style} at {percent}%");
            }
        }
    }

    #[test]
    fn bar_endpoints_are_empty_and_full() {
        assert_eq!(bar(0.0, 4, MeterStyle::Bar), "░░░░");
        assert_eq!(bar(100.0, 4, MeterStyle::Bar), "████");
        assert_eq!(bar(50.0, 4, MeterStyle::Bar), "██░░");
    }

    #[test]
    fn out_of_range_percentages_are_clamped() {
        assert_eq!(bar(-50.0, 4, MeterStyle::Bar), "░░░░");
        assert_eq!(bar(1000.0, 4, MeterStyle::Bar), "████");
        assert_eq!(bar(f64::NAN, 4, MeterStyle::Bar).chars().count(), 4);
    }

    #[test]
    fn a_zero_width_bar_is_empty_not_a_panic() {
        assert_eq!(bar(50.0, 0, MeterStyle::Bar), "");
    }

    #[test]
    fn blocks_style_uses_partial_cells() {
        // An eighth of one cell out of four.
        let b = bar(100.0 / 32.0, 4, MeterStyle::Blocks);
        assert!(b.starts_with('▏'), "expected a partial block, got {b:?}");
        assert_eq!(b.chars().count(), 4);
    }

    #[test]
    fn per_core_expands_into_one_meter_each() {
        let mut config = Config::default();
        config.meters.items = vec![MeterKind::Cpu];
        config.meters.per_core = true;
        let app = app_with(config);

        let meters = collect(&app);
        // One per logical CPU, or a single average if /proc/stat had no rows.
        assert!(!meters.is_empty());
        if app.dynamic.cpu.per_core.len() > 1 {
            assert_eq!(meters.len(), app.dynamic.cpu.per_core.len());
            assert_eq!(meters[0].label, "cpu0");
        }
    }

    #[test]
    fn a_meter_line_fits_its_width() {
        let app = app_with(Config::default());
        let meter = Meter {
            label: "CPU".into(),
            percent: 42.0,
            detail: "1.2 GHz".into(),
        };
        for width in [20u16, 40, 80, 200] {
            let l = line(&meter, &app, width, app.theme.good, &app.theme);
            assert!(
                l.width() <= width as usize,
                "width {width} produced {}",
                l.width()
            );
        }
    }

    #[test]
    fn a_very_narrow_meter_still_renders() {
        let app = app_with(Config::default());
        let meter = Meter {
            label: "CPU".into(),
            percent: 42.0,
            detail: String::new(),
        };
        let l = line(&meter, &app, 4, app.theme.good, &app.theme);
        assert!(l.width() > 0);
    }
}

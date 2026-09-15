//! The time-series graphs.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols::Marker;
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Sparkline};
use ratatui::Frame;

use crate::app::{App, Ring};
use crate::config::color::ColorSpec;
use crate::config::enums::{GraphKind, GraphStyle};
use crate::util::human_rate;

/// Everything needed to draw one graph.
struct Series<'a> {
    title: String,
    ring: &'a Ring,
    /// The value the graph's top edge represents.
    ceiling: f64,
    color: ColorSpec,
}

/// A graph's vertical scale.
///
/// Percentages are pinned to 0..100 so the shape is comparable between ticks;
/// rates have no natural maximum, so they track the window's own peak with a
/// floor that keeps an idle graph from looking busy.
fn ceiling_for(kind: GraphKind, ring: &Ring, app: &App) -> f64 {
    match kind {
        GraphKind::Cpu | GraphKind::Memory | GraphKind::Swap | GraphKind::Disk | GraphKind::Gpu => {
            100.0
        }
        GraphKind::Load => (app.statics.cpu.threads.max(1) as f64).max(ring.max()),
        GraphKind::Network | GraphKind::DiskIo => ring.max().max(64.0 * 1024.0),
    }
}

/// Builds the series the config asked for.
fn collect<'a>(app: &'a App) -> Vec<Series<'a>> {
    let h = &app.history;
    let theme = &app.theme;
    let show_stats = app.config.graphs.show_stats;

    let percent_title = |name: &str, ring: &Ring| {
        if show_stats {
            format!(" {name} {:.0}% (peak {:.0}%) ", ring.last(), ring.max())
        } else {
            format!(" {name} ")
        }
    };

    app.config
        .graphs
        .items
        .iter()
        .flat_map(|kind| match kind {
            GraphKind::Cpu => vec![Series {
                title: percent_title("CPU", &h.cpu),
                ring: &h.cpu,
                ceiling: ceiling_for(*kind, &h.cpu, app),
                color: theme.graph_primary,
            }],
            GraphKind::Memory => vec![Series {
                title: percent_title("Memory", &h.memory),
                ring: &h.memory,
                ceiling: ceiling_for(*kind, &h.memory, app),
                color: theme.graph_secondary,
            }],
            GraphKind::Gpu => vec![Series {
                title: percent_title("GPU", &h.gpu),
                ring: &h.gpu,
                ceiling: 100.0,
                color: theme.accent,
            }],
            GraphKind::Swap => vec![Series {
                title: percent_title("Swap", &h.swap),
                ring: &h.swap,
                ceiling: ceiling_for(*kind, &h.swap, app),
                color: theme.warn,
            }],
            GraphKind::Disk => vec![Series {
                title: percent_title("Disk use", &h.disk),
                ring: &h.disk,
                ceiling: ceiling_for(*kind, &h.disk, app),
                color: theme.accent,
            }],
            GraphKind::Load => vec![Series {
                title: if show_stats {
                    format!(" Load {:.2} (peak {:.2}) ", h.load.last(), h.load.max())
                } else {
                    " Load ".to_string()
                },
                ring: &h.load,
                ceiling: ceiling_for(*kind, &h.load, app),
                color: theme.critical,
            }],
            GraphKind::DiskIo => {
                let ceiling = h.disk_read.max().max(h.disk_write.max()).max(64.0 * 1024.0);
                vec![
                    Series {
                        title: if show_stats {
                            format!(
                                " read {} (peak {}) ",
                                human_rate(h.disk_read.last()),
                                human_rate(h.disk_read.max())
                            )
                        } else {
                            " read ".to_string()
                        },
                        ring: &h.disk_read,
                        ceiling,
                        color: theme.good,
                    },
                    Series {
                        title: if show_stats {
                            format!(
                                " write {} (peak {}) ",
                                human_rate(h.disk_write.last()),
                                human_rate(h.disk_write.max())
                            )
                        } else {
                            " write ".to_string()
                        },
                        ring: &h.disk_write,
                        ceiling,
                        color: theme.graph_secondary,
                    },
                ]
            }
            // Receive and transmit share a scale so the two are comparable.
            GraphKind::Network => {
                let ceiling = h.net_rx.max().max(h.net_tx.max()).max(64.0 * 1024.0);
                vec![
                    Series {
                        title: if show_stats {
                            format!(
                                " ↓ {} (peak {}) ",
                                human_rate(h.net_rx.last()),
                                human_rate(h.net_rx.max())
                            )
                        } else {
                            " ↓ rx ".to_string()
                        },
                        ring: &h.net_rx,
                        ceiling,
                        color: theme.good,
                    },
                    Series {
                        title: if show_stats {
                            format!(
                                " ↑ {} (peak {}) ",
                                human_rate(h.net_tx.last()),
                                human_rate(h.net_tx.max())
                            )
                        } else {
                            " ↑ tx ".to_string()
                        },
                        ring: &h.net_tx,
                        ceiling,
                        color: theme.graph_secondary,
                    },
                ]
            }
        })
        .collect()
}

/// Draws the graphs panel.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let series = collect(app);
    if series.is_empty() || area.height == 0 || area.width == 0 {
        return;
    }

    // Side by side while each graph still gets a useful width, stacked
    // otherwise.
    const MIN_COLUMN: u16 = 24;
    let columns = ((area.width / MIN_COLUMN) as usize).clamp(1, series.len());
    let rows = series.len().div_ceil(columns);

    let row_slots = Layout::vertical(vec![Constraint::Fill(1); rows]).split(area);

    for (row_index, chunk) in series.chunks(columns).enumerate() {
        let Some(row_area) = row_slots.get(row_index) else {
            break;
        };
        let column_slots =
            Layout::horizontal(vec![Constraint::Fill(1); chunk.len()]).split(*row_area);

        for (series, slot) in chunk.iter().zip(column_slots.iter()) {
            one(frame, *slot, app, series);
        }
    }
}

fn one(frame: &mut Frame, area: Rect, app: &App, series: &Series<'_>) {
    // A border costs two rows; below four there would be nothing left to plot,
    // so the title is dropped in favour of the data.
    let bordered = app.config.graphs.bordered && app.theme.borders && area.height >= 4;
    let (inner, block) = if bordered {
        let block = super::panel(&app.theme, "")
            .title(series.title.clone())
            .title_style(app.theme.title.fg());
        (block.inner(area), Some(block))
    } else {
        (area, None)
    };

    if let Some(block) = block {
        frame.render_widget(block, area);
    }
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    match app.config.graphs.style {
        GraphStyle::Braille => braille(frame, inner, app, series),
        GraphStyle::Sparkline | GraphStyle::Bars => {
            let data = series.ring.scaled(inner.width as usize, series.ceiling);
            let widget = Sparkline::default()
                .data(data)
                // scaled() normalises to a fixed 0..1000 range, so the graph
                // does not rescale itself every time the peak moves.
                .max(1000)
                .style(series.color.fg())
                .bar_set(if app.config.graphs.style == GraphStyle::Bars {
                    ratatui::symbols::bar::THREE_LEVELS
                } else {
                    ratatui::symbols::bar::NINE_LEVELS
                });
            frame.render_widget(widget, inner);
        }
    }
}

/// A line plot drawn with braille dots, for a smoother curve than bars give.
fn braille(frame: &mut Frame, area: Rect, app: &App, series: &Series<'_>) {
    let points: Vec<(f64, f64)> = series
        .ring
        .values()
        .enumerate()
        .map(|(i, v)| (i as f64, v))
        .collect();

    if points.len() < 2 {
        return;
    }

    let x_max = (points.len() - 1) as f64;
    let datasets = vec![Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(series.color.fg())
        .data(&points)];

    let chart = Chart::new(datasets)
        .style(super::surface(&app.theme))
        // The axes carry no labels: the title already says what the numbers
        // are, and the space is better spent on the curve.
        .x_axis(Axis::default().bounds([0.0, x_max]).style(Style::default()))
        .y_axis(Axis::default().bounds([0.0, series.ceiling.max(f64::EPSILON)]));

    frame.render_widget(chart, area);
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
    fn network_expands_into_two_series() {
        let mut config = Config::default();
        config.graphs.items = vec![GraphKind::Network];
        let app = app_with(config);
        assert_eq!(collect(&app).len(), 2);
    }

    #[test]
    fn percentage_graphs_are_pinned_to_a_hundred() {
        let app = app_with(Config::default());
        let ring = Ring::new(4);
        assert_eq!(ceiling_for(GraphKind::Cpu, &ring, &app), 100.0);
        assert_eq!(ceiling_for(GraphKind::Memory, &ring, &app), 100.0);
    }

    #[test]
    fn rate_graphs_have_a_floor_so_idle_looks_idle() {
        let app = app_with(Config::default());
        let ring = Ring::new(4);
        assert_eq!(ceiling_for(GraphKind::Network, &ring, &app), 64.0 * 1024.0);

        let mut busy = Ring::new(4);
        busy.push(10_000_000.0);
        assert_eq!(ceiling_for(GraphKind::Network, &busy, &app), 10_000_000.0);
    }

    #[test]
    fn load_scales_against_the_core_count() {
        let app = app_with(Config::default());
        let ring = Ring::new(4);
        let ceiling = ceiling_for(GraphKind::Load, &ring, &app);
        assert!(ceiling >= 1.0);
    }

    #[test]
    fn every_graph_kind_produces_a_series() {
        let mut config = Config::default();
        config.graphs.items = GraphKind::ALL.to_vec();
        let app = app_with(config);
        // Network and disk I/O each expand into two series.
        assert_eq!(collect(&app).len(), GraphKind::ALL.len() + 2);
    }
}

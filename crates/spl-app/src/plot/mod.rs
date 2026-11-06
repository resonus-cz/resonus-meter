//! GTK/Cairo rendering for the Log history plot and the RTA bars.

pub(crate) mod level_axis;
pub(crate) mod rta;
pub(crate) mod text;
pub(crate) mod ticks;

use crate::log_view::{LogPlotData, PlotRecord};
use crate::style::{Palette, Rgb};
use text::{Anchor, PX_PER_PT, draw_mark, draw_text};

/// Space left of the plot area for the level axis, in pixels.
const LEFT_PX: f64 = 48.0;
/// Space right of the plot area, in pixels.
const RIGHT_PX: f64 = 16.0;
/// Smallest distance between time ticks, in pixels.
const TICK_SPACING_PX: f64 = 70.0;
/// How far from a marker line a click still selects it, in pixels.
const MARKER_HIT_PX: f64 = 8.0;

/// Returns the id of the marker line nearest to `x_px`, if one is drawn
/// within [`MARKER_HIT_PX`] of it in a plot `width_px` wide.
pub(crate) fn marker_near(data: &LogPlotData, width_px: f64, x_px: f64) -> Option<u64> {
    let plot_width = (width_px - LEFT_PX - RIGHT_PX).max(1.0);
    let (start_s, end_s) = visible_time_range(data);
    let duration_s = (end_s - start_s).max(1.0);
    data.markers
        .iter()
        .filter(|marker| marker.t_s >= start_s && marker.t_s <= end_s)
        .map(|marker| {
            let marker_x = LEFT_PX + (marker.t_s - start_s) / duration_s * plot_width;
            (marker.id, (marker_x - x_px).abs())
        })
        .filter(|&(_, distance)| distance <= MARKER_HIT_PX)
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(id, _)| id)
}

/// Draws the Log plot of `data` across `width` × `height` in the current style.
#[allow(clippy::too_many_lines)]
pub(super) fn draw_log_plot(
    context: &gtk::cairo::Context,
    width: i32,
    height: i32,
    data: &LogPlotData,
) {
    let palette = crate::style::current();
    let width = f64::from(width);
    let height = f64::from(height);
    let left = LEFT_PX;
    let right = RIGHT_PX;
    let top = 22.0;
    let bottom = 40.0;
    let plot_width = (width - left - right).max(1.0);
    let plot_height = (height - top - bottom).max(1.0);
    let (start_s, end_s) = visible_time_range(data);
    let duration_s = (end_s - start_s).max(1.0);
    let (min_db, max_db) = (data.axis.bottom_db, data.axis.top_db);
    let level_span = (max_db - min_db).max(1.0);
    let x_for = |time_s: f64| left + (time_s - start_s) / duration_s * plot_width;
    let y_for = |level_db: f64| top + (max_db - level_db) / level_span * plot_height;

    palette.background.source(context);
    context.paint().unwrap_or(());

    context.set_line_width(palette.grid_width * PX_PER_PT);
    for tick in 0..=5 {
        let fraction = f64::from(tick) / 5.0;
        let y = top + fraction * plot_height;
        let level = max_db - fraction * level_span;
        palette.grid.source(context);
        context.move_to(left, y);
        context.line_to(left + plot_width, y);
        context.stroke().unwrap_or(());
        palette.axes.source(context);
        draw_text(
            context,
            palette,
            &format!("{level:.0}"),
            left - 5.0,
            y,
            Anchor::RIGHT_MIDDLE,
        );
    }
    // Round time steps (ticks.rs), about one per TICK_SPACING_PX.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max_ticks = ((plot_width / TICK_SPACING_PX) as usize).clamp(2, 10);
    for time_s in ticks::time_ticks(start_s, end_s, max_ticks) {
        let x = x_for(time_s);
        palette.grid.source(context);
        context.move_to(x, top);
        context.line_to(x, top + plot_height);
        context.stroke().unwrap_or(());
        palette.axes.source(context);
        let text = format_time_tick(time_s);
        draw_text(
            context,
            palette,
            &text,
            x,
            top + plot_height + 4.0,
            Anchor::CENTRE_TOP,
        );
    }
    palette.axes.source(context);
    context.set_line_width(1.0);
    context.move_to(left, top + plot_height);
    context.line_to(left + plot_width, top + plot_height);
    context.move_to(left, top);
    context.line_to(left, top + plot_height);
    context.stroke().unwrap_or(());

    for marker in data
        .markers
        .iter()
        .filter(|marker| marker.t_s >= start_s && marker.t_s <= end_s)
    {
        let x = x_for(marker.t_s);
        palette.neutral.source(context);
        context.set_line_width(1.0);
        context.set_dash(&[4.0, 3.0], 0.0);
        context.move_to(x, top);
        context.line_to(x, top + plot_height);
        context.stroke().unwrap_or(());
        context.set_dash(&[], 0.0);
        draw_text(
            context,
            palette,
            &marker.label,
            x + 3.0,
            top + 2.0,
            Anchor::LEFT_TOP,
        );
    }

    // A manual range may cut traces; keep them inside the plot area.
    context.save().unwrap_or(());
    context.rectangle(left, top, plot_width, plot_height);
    context.clip();
    let traces: [(bool, LevelOf); 3] = [
        (data.show_a, |record| record.laf_db),
        (data.show_c, |record| record.lcf_db),
        (data.show_z, |record| record.lzf_db),
    ];
    for (index, (shown, level)) in traces.into_iter().enumerate() {
        if shown {
            let trace = Trace {
                colour: trace_colour(palette, index),
                width: palette.line_width * PX_PER_PT,
            };
            draw_trace(
                context,
                &data.records,
                start_s,
                end_s,
                &x_for,
                &y_for,
                trace,
                level,
            );
        }
    }
    context.restore().unwrap_or(());

    palette.axes.source(context);
    draw_text(context, palette, "Lₚ", 3.0, 2.0, Anchor::LEFT_TOP);
    let centre = left + plot_width / 2.0;
    draw_text(
        context,
        palette,
        "t [s]",
        centre,
        height - 2.0,
        Anchor::CENTRE_BOTTOM,
    );
    if data.records.is_empty() {
        let text = "Waiting for the first completed log interval";
        draw_text(
            context,
            palette,
            text,
            left + 12.0,
            top + plot_height / 2.0,
            Anchor::LEFT_MIDDLE,
        );
    }
    draw_mark(context, palette, width - 3.0, height - 2.0);
}

/// The `index`-th trace colour of `palette` (the first three: A, C, Z).
pub(crate) fn trace_colour(palette: &Palette, index: usize) -> Rgb {
    palette.traces[index % palette.traces.len()]
}

/// Reads one trace's level from a record.
type LevelOf = fn(&PlotRecord) -> f64;

/// Colour and width (px) of one trace.
#[derive(Clone, Copy)]
struct Trace {
    colour: Rgb,
    width: f64,
}

#[allow(clippy::too_many_arguments)]
fn draw_trace(
    context: &gtk::cairo::Context,
    records: &[PlotRecord],
    start_s: f64,
    end_s: f64,
    x_for: &impl Fn(f64) -> f64,
    y_for: &impl Fn(f64) -> f64,
    trace: Trace,
    level: impl Fn(&PlotRecord) -> f64,
) {
    trace.colour.source(context);
    context.set_line_width(trace.width);
    let mut started = false;
    for record in records
        .iter()
        .filter(|record| record.t_s >= start_s && record.t_s <= end_s)
    {
        let value = level(record);
        if !value.is_finite() {
            continue;
        }
        if started && !record.after_pause {
            context.line_to(x_for(record.t_s), y_for(value));
        } else {
            context.move_to(x_for(record.t_s), y_for(value));
            started = true;
        }
    }
    if started {
        context.stroke().unwrap_or(());
    }
}

pub(super) fn visible_time_range(data: &LogPlotData) -> (f64, f64) {
    let end_s = data
        .records
        .last()
        .map_or(data.duration_s, |record| record.t_s.max(data.duration_s))
        .max(0.0);
    let start_s = data
        .window
        .duration_s()
        .map_or(0.0, |span_s| (end_s - span_s).max(0.0));
    (start_s, end_s.max(start_s + 1.0))
}

/// The lowest and highest level of the shown traces in the visible window.
pub(crate) fn visible_level_extent(data: &LogPlotData) -> Option<(f64, f64)> {
    let (start_s, end_s) = visible_time_range(data);
    let mut levels = data
        .records
        .iter()
        .filter(|record| record.t_s >= start_s && record.t_s <= end_s)
        .flat_map(|record| {
            [
                data.show_a.then_some(record.laf_db),
                data.show_c.then_some(record.lcf_db),
                data.show_z.then_some(record.lzf_db),
            ]
        })
        .flatten()
        .filter(|level| level.is_finite());
    let first = levels.next()?;
    Some(levels.fold((first, first), |(min_db, max_db), level| {
        (min_db.min(level), max_db.max(level))
    }))
}

fn format_time_tick(time_s: f64) -> String {
    if time_s >= 3_600.0 {
        format!("{:.1}h", time_s / 3_600.0)
    } else if time_s >= 60.0 {
        format!("{:.0}m", time_s / 60.0)
    } else {
        format!("{time_s:.0}s")
    }
}

#[cfg(test)]
mod tests {
    use super::{marker_near, visible_time_range};
    use crate::log_view::{LogPlotData, PlotMarker, TimeWindow};

    #[test]
    fn a_click_selects_the_nearest_marker_line_within_reach() {
        let marker = |id, t_s| PlotMarker {
            id,
            t_s,
            label: String::new(),
        };
        // 0–100 s over a 1 000 px plot area: 10 px per second, starting at 44 px.
        let data = LogPlotData {
            duration_s: 100.0,
            window: TimeWindow::All,
            markers: vec![marker(1, 0.0), marker(2, 50.0), marker(3, 51.0)],
            ..LogPlotData::default()
        };
        let width = 1_000.0 + 44.0 + 16.0;

        assert_eq!(marker_near(&data, width, 544.0), Some(2));
        assert_eq!(marker_near(&data, width, 553.0), Some(3));
        assert_eq!(marker_near(&data, width, 40.0), Some(1));
        assert_eq!(marker_near(&data, width, 300.0), None);
    }

    #[test]
    fn selected_time_window_ends_at_current_history_duration() {
        let data = LogPlotData {
            duration_s: 700.0,
            window: TimeWindow::TenMinutes,
            ..LogPlotData::default()
        };

        assert_eq!(visible_time_range(&data), (100.0, 700.0));
    }

    #[test]
    fn all_time_window_includes_the_session_start() {
        let data = LogPlotData {
            duration_s: 700.0,
            window: TimeWindow::All,
            ..LogPlotData::default()
        };

        assert_eq!(visible_time_range(&data), (0.0, 700.0));
    }
}

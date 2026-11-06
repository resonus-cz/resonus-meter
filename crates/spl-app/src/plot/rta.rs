//! GTK/Cairo rendering of RTA band levels as bars. Knows nothing about the
//! engine: it draws the bars it is given.

use super::level_axis::LevelAxis;
use super::text::{Anchor, PX_PER_PT, draw_mark, draw_text};
use super::trace_colour;

/// One band's bar.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RtaBar {
    /// Axis label, empty when this band is not labelled.
    pub(crate) label: String,
    pub(crate) level_db: Option<f64>,
    /// Peak-hold level, drawn as a line above the bar.
    pub(crate) hold_db: Option<f64>,
}

/// Draws `bars` over `axis`'s level range in `unit` across `width` × `height`.
pub(crate) fn draw_rta_plot(
    context: &gtk::cairo::Context,
    width: i32,
    height: i32,
    bars: &[RtaBar],
    axis: &LevelAxis,
    unit: &str,
) {
    let palette = crate::style::current();
    let (width, height) = (f64::from(width), f64::from(height));
    let (left, right, top, bottom) = (48.0, 12.0, 22.0, 40.0);
    let plot_width = (width - left - right).max(1.0);
    let plot_height = (height - top - bottom).max(1.0);
    let (min_db, max_db) = (axis.bottom_db, axis.top_db);
    let y_for = |level_db: f64| {
        top + ((max_db - level_db) / (max_db - min_db)).clamp(0.0, 1.0) * plot_height
    };

    palette.background.source(context);
    context.paint().unwrap_or(());
    context.set_line_width(palette.grid_width * PX_PER_PT);
    // Grid lines on multiples of 10 dB inside the range.
    let mut level = (max_db / 10.0).floor() * 10.0;
    while level >= min_db {
        let y = y_for(level);
        palette.grid.source(context);
        context.move_to(left, y);
        context.line_to(left + plot_width, y);
        let _ = context.stroke();
        palette.axes.source(context);
        draw_text(
            context,
            palette,
            &format!("{level:.0}"),
            left - 5.0,
            y,
            Anchor::RIGHT_MIDDLE,
        );
        level -= 10.0;
    }
    draw_text(context, palette, unit, 3.0, 2.0, Anchor::LEFT_TOP);

    #[allow(clippy::cast_precision_loss)]
    let slot = plot_width / bars.len().max(1) as f64;
    for (index, bar) in bars.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let x = left + slot * index as f64;
        if let Some(level_db) = bar.level_db {
            trace_colour(palette, 0).source(context);
            let y = y_for(level_db);
            context.rectangle(x + slot * 0.12, y, slot * 0.76, top + plot_height - y);
            let _ = context.fill();
        }
        if let Some(hold_db) = bar.hold_db {
            palette.neutral.source(context);
            context.set_line_width(palette.line_width * PX_PER_PT);
            let y = y_for(hold_db);
            context.move_to(x + slot * 0.12, y);
            context.line_to(x + slot * 0.88, y);
            let _ = context.stroke();
        }
        if !bar.label.is_empty() {
            palette.axes.source(context);
            let centre = x + slot / 2.0;
            draw_text(
                context,
                palette,
                &bar.label,
                centre,
                top + plot_height + 4.0,
                Anchor::CENTRE_TOP,
            );
        }
    }
    palette.axes.source(context);
    context.set_line_width(1.0);
    context.move_to(left, top + plot_height);
    context.line_to(left + plot_width, top + plot_height);
    let _ = context.stroke();
    let centre = left + plot_width / 2.0;
    draw_text(
        context,
        palette,
        "f [Hz]",
        centre,
        height - 2.0,
        Anchor::CENTRE_BOTTOM,
    );
    if bars.iter().all(|bar| bar.level_db.is_none()) {
        let text = "Waiting for the analyser";
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

/// Formats a nominal band frequency as an axis label: `31.5`, `1k`, `12.5k`.
pub(crate) fn frequency_label(nominal_hz: f64) -> String {
    if nominal_hz >= 1_000.0 {
        let kilohertz = nominal_hz / 1_000.0;
        if kilohertz.fract() == 0.0 {
            format!("{kilohertz:.0}k")
        } else {
            format!("{kilohertz:.1}k")
        }
    } else if nominal_hz.fract() == 0.0 {
        format!("{nominal_hz:.0}")
    } else {
        format!("{nominal_hz:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::frequency_label;

    #[test]
    fn labels_use_the_nominal_frequencies_compactly() {
        assert_eq!(frequency_label(31.5), "31.5");
        assert_eq!(frequency_label(250.0), "250");
        assert_eq!(frequency_label(1_000.0), "1k");
        assert_eq!(frequency_label(12_500.0), "12.5k");
    }
}

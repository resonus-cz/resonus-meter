//! A plot's level axis: automatic with hysteresis, or set by the user.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};

/// Margin kept between the data and the axis edges in automatic mode.
const MARGIN_DB: f64 = 5.0;
/// The automatic range changes only when data comes this close to an edge…
const EDGE_DB: f64 = 2.0;
/// …or when the range is this much larger than the data needs.
const SLACK_DB: f64 = 30.0;
/// Scroll step and zoom step of the manual range.
const STEP_DB: f64 = 5.0;
const MIN_SPAN_DB: f64 = 20.0;
const MAX_SPAN_DB: f64 = 200.0;

/// The level range shown and how it follows the data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LevelAxis {
    pub(crate) bottom_db: f64,
    pub(crate) top_db: f64,
    /// Whether the range follows the data; scrolling or zooming clears it.
    pub(crate) automatic: bool,
    /// The next [`Self::follow`] fits the range to the data regardless.
    stale: bool,
}

impl LevelAxis {
    pub(crate) const fn new(bottom_db: f64, top_db: f64) -> Self {
        Self {
            bottom_db,
            top_db,
            automatic: true,
            stale: true,
        }
    }

    /// Returns to automatic mode, fitting the range at the next update.
    pub(crate) const fn set_automatic(&mut self) {
        self.automatic = true;
        self.stale = true;
    }

    /// Follows data spanning `low_db..=high_db` in automatic mode. The range
    /// is recomputed only when the data nears an edge or has shrunk well
    /// inside it, so it does not jump with every frame.
    pub(crate) fn follow(&mut self, low_db: f64, high_db: f64) {
        if !self.automatic || !low_db.is_finite() || !high_db.is_finite() {
            return;
        }
        let needed = round_out(low_db, high_db);
        let near_edge = high_db > self.top_db - EDGE_DB || low_db < self.bottom_db + EDGE_DB;
        let slack = (self.top_db - self.bottom_db) - (needed.1 - needed.0) > SLACK_DB;
        if self.stale || near_edge || slack {
            (self.bottom_db, self.top_db) = needed;
            self.stale = false;
        }
    }

    /// Moves the range up (`steps` > 0) or down, in manual mode.
    pub(crate) fn scroll(&mut self, steps: f64) {
        self.automatic = false;
        self.bottom_db += steps * STEP_DB;
        self.top_db += steps * STEP_DB;
    }

    /// Narrows (`steps` > 0) or widens the range around its centre.
    pub(crate) fn zoom(&mut self, steps: f64) {
        self.automatic = false;
        let centre = f64::midpoint(self.bottom_db, self.top_db);
        let span =
            (self.top_db - self.bottom_db - 2.0 * steps * STEP_DB).clamp(MIN_SPAN_DB, MAX_SPAN_DB);
        self.bottom_db = centre - span / 2.0;
        self.top_db = centre + span / 2.0;
    }
}

/// Runs a change on a plot's axis, wherever the plot stores it.
pub(crate) type WithAxis = Rc<dyn Fn(&mut dyn FnMut(&mut LevelAxis))>;

/// Connects scrolling, zooming and the Auto Scale toggle of `plot`.
///
/// The wheel or Up/Down moves the range by 5 dB; Ctrl+wheel or +/− zooms.
/// `with_axis` gives access to the plot's axis wherever it is stored.
pub(crate) fn connect_level_axis_controls(
    plot: &gtk::DrawingArea,
    auto_scale: &gtk::ToggleButton,
    with_axis: WithAxis,
) {
    plot.set_focusable(true);
    plot.update_property(&[gtk::accessible::Property::Description(
        "Scroll or press Up and Down to move the level axis; Ctrl+scroll or plus and minus to zoom.",
    )]);
    let manual = {
        let (plot, auto_scale, with_axis) =
            (plot.clone(), auto_scale.clone(), Rc::clone(&with_axis));
        move |change: &dyn Fn(&mut LevelAxis)| {
            with_axis(&mut |axis| change(axis));
            auto_scale.set_active(false);
            plot.queue_draw();
        }
    };
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    let on_scroll = manual.clone();
    scroll.connect_scroll(move |controller, _, dy| {
        let zoom = controller
            .current_event_state()
            .contains(gdk::ModifierType::CONTROL_MASK);
        // Wheel up shows higher levels or zooms in.
        on_scroll(&|axis| {
            if zoom {
                axis.zoom(-dy);
            } else {
                axis.scroll(-dy);
            }
        });
        glib::Propagation::Stop
    });
    plot.add_controller(scroll);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(move |_, key, _, _| {
        let change: fn(&mut LevelAxis) = match key {
            gdk::Key::Up => |axis| axis.scroll(1.0),
            gdk::Key::Down => |axis| axis.scroll(-1.0),
            gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => |axis| axis.zoom(1.0),
            gdk::Key::minus | gdk::Key::KP_Subtract => |axis| axis.zoom(-1.0),
            _ => return glib::Propagation::Proceed,
        };
        manual(&change);
        glib::Propagation::Stop
    });
    plot.add_controller(keys);
    let plot = plot.clone();
    auto_scale.connect_toggled(move |toggle| {
        if toggle.is_active() {
            with_axis(&mut LevelAxis::set_automatic);
            plot.queue_draw();
        }
    });
}

/// An Auto Scale toggle, on by default.
pub(crate) fn auto_scale_toggle(plot_name: &str) -> gtk::ToggleButton {
    let toggle = gtk::ToggleButton::builder()
        .label("Auto Scale")
        .active(true)
        .tooltip_text("Fit the level axis to the data")
        .build();
    toggle.update_property(&[gtk::accessible::Property::Label(&format!(
        "Automatic level axis of the {plot_name}"
    ))]);
    toggle
}

/// The 10 dB grid range holding `low_db..=high_db` with [`MARGIN_DB`].
fn round_out(low_db: f64, high_db: f64) -> (f64, f64) {
    let top = ((high_db + MARGIN_DB) / 10.0).ceil() * 10.0;
    let bottom = ((low_db - MARGIN_DB) / 10.0).floor() * 10.0;
    (bottom.min(top - MIN_SPAN_DB), top)
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Ranges are exact multiples of 5 dB.
mod tests {
    use super::LevelAxis;

    #[test]
    fn small_changes_keep_the_automatic_range() {
        // 120 dB shown for 70 dB of data: far more than needed.
        let mut axis = LevelAxis::new(0.0, 120.0);
        axis.follow(20.0, 70.0);
        assert_eq!((axis.bottom_db, axis.top_db), (10.0, 80.0));

        // Moving within the range by a few dB changes nothing.
        axis.follow(16.0, 76.0);
        axis.follow(24.0, 66.0);
        assert_eq!((axis.bottom_db, axis.top_db), (10.0, 80.0));
    }

    #[test]
    fn the_range_moves_when_data_nears_an_edge_or_shrinks_far_inside() {
        let mut axis = LevelAxis::new(10.0, 80.0);
        axis.follow(20.0, 79.0);
        assert_eq!(axis.top_db, 90.0);

        axis.follow(40.0, 45.0);
        assert_eq!((axis.bottom_db, axis.top_db), (30.0, 50.0));
    }

    #[test]
    fn scrolling_or_zooming_switches_to_a_manual_range() {
        let mut axis = LevelAxis::new(0.0, 80.0);
        axis.scroll(-2.0);
        assert!(!axis.automatic);
        assert_eq!((axis.bottom_db, axis.top_db), (-10.0, 70.0));

        axis.zoom(2.0);
        assert_eq!((axis.bottom_db, axis.top_db), (0.0, 60.0));
        axis.zoom(100.0);
        assert_eq!(axis.top_db - axis.bottom_db, 20.0);

        axis.follow(-50.0, 120.0);
        assert_eq!(
            (axis.bottom_db, axis.top_db),
            (20.0, 40.0),
            "manual ranges stay put"
        );
    }
}

use crate::marker_editor::present_marker_editor;
use crate::meter::format_elapsed;
use crate::plot::level_axis::{LevelAxis, auto_scale_toggle, connect_level_axis_controls};
use crate::plot::{draw_log_plot, marker_near, visible_level_extent};
use crate::runner::MeasurementRunner;
use adw::prelude::*;
use spl_engine::{LogHistory, MarkerKind, Snapshot};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

const LOG_INTERVAL_VALUES: [f64; 8] = [0.125, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 60.0];
pub(crate) const LOG_INTERVAL_LABELS: [&str; 8] = [
    "0.125 s", "0.25 s", "0.5 s", "1 s", "2 s", "5 s", "10 s", "60 s",
];

pub(crate) struct LogWidgets {
    pub(crate) content: gtk::Box,
    pub(crate) plot: gtk::DrawingArea,
    pub(crate) plot_data: Rc<RefCell<LogPlotData>>,
    pub(crate) status: gtk::Label,
    pub(crate) interval: gtk::DropDown,
    pub(crate) interval_syncing: Rc<Cell<bool>>,
    /// Shown time window; its last entry is the whole session.
    pub(crate) time_window: gtk::DropDown,
}

#[derive(Clone, Copy)]
pub(crate) enum TimeWindow {
    OneMinute,
    TenMinutes,
    OneHour,
    All,
}

impl TimeWindow {
    pub(crate) const fn duration_s(self) -> Option<f64> {
        match self {
            Self::OneMinute => Some(60.0),
            Self::TenMinutes => Some(600.0),
            Self::OneHour => Some(3_600.0),
            Self::All => None,
        }
    }
}

pub(crate) struct PlotRecord {
    pub(crate) t_s: f64,
    /// A pause lies between this record and the previous one, so the trace
    /// must not join them.
    pub(crate) after_pause: bool,
    pub(crate) laf_db: f64,
    pub(crate) lcf_db: f64,
    pub(crate) lzf_db: f64,
}

pub(crate) struct PlotMarker {
    pub(crate) id: u64,
    pub(crate) t_s: f64,
    pub(crate) label: String,
}

pub(crate) struct LogPlotData {
    pub(crate) records: Vec<PlotRecord>,
    pub(crate) markers: Vec<PlotMarker>,
    pub(crate) duration_s: f64,
    pub(crate) window: TimeWindow,
    pub(crate) show_a: bool,
    pub(crate) show_c: bool,
    pub(crate) show_z: bool,
    pub(crate) axis: LevelAxis,
}

impl Default for LogPlotData {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            markers: Vec::new(),
            duration_s: 0.0,
            window: TimeWindow::OneMinute,
            show_a: true,
            show_c: true,
            show_z: true,
            axis: LevelAxis::new(0.0, 100.0),
        }
    }
}

impl LogPlotData {
    pub(crate) fn update(&mut self, history: &LogHistory) {
        let pauses = history
            .markers
            .iter()
            .filter(|marker| marker.kind == MarkerKind::Pause)
            .map(|marker| marker.t_s)
            .collect::<Vec<_>>();
        let mut previous_t_s = f64::NEG_INFINITY;
        self.records = history
            .records
            .iter()
            .map(|record| {
                let after_pause = pauses
                    .iter()
                    .any(|&pause_s| previous_t_s <= pause_s && pause_s < record.t_s);
                previous_t_s = record.t_s;
                PlotRecord {
                    t_s: record.t_s,
                    after_pause,
                    laf_db: record.laf_db,
                    lcf_db: record.lcf_db,
                    lzf_db: record.lzf_db,
                }
            })
            .collect();
        self.markers = history
            .markers
            .iter()
            .map(|marker| PlotMarker {
                id: marker.id,
                t_s: marker.t_s,
                label: marker.label.clone(),
            })
            .collect();
        self.duration_s = history.duration_s;
        if let Some((low, high)) = visible_level_extent(self) {
            self.axis.follow(low, high);
        }
    }
}

pub(crate) fn present_marker_dialog(
    window: &adw::ApplicationWindow,
    runner: &Rc<RefCell<MeasurementRunner>>,
    status: &gtk::Label,
    elapsed_s: f64,
) {
    let dialog = adw::Dialog::builder().title("Add Marker").build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    let entry = gtk::Entry::builder()
        .text("Marker")
        .placeholder_text("Marker name")
        .hexpand(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label("Marker name")]);
    content.append(&entry);
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();
    let cancel = gtk::Button::builder().label("Cancel").build();
    let add = gtk::Button::builder()
        .label("Add Marker")
        .css_classes(["suggested-action"])
        .build();
    actions.append(&cancel);
    actions.append(&add);
    content.append(&actions);
    dialog.set_child(Some(&content));
    let closed_runner = Rc::clone(runner);
    dialog.connect_closed(move |_| {
        closed_runner.borrow_mut().cancel_marker_at(elapsed_s);
    });
    let cancel_dialog = dialog.clone();
    let cancel_runner = Rc::clone(runner);
    cancel.connect_clicked(move |_| {
        cancel_runner.borrow_mut().cancel_marker_at(elapsed_s);
        cancel_dialog.close();
    });
    let marker_runner = Rc::clone(runner);
    let marker_status = status.clone();
    let marker_entry = entry.clone();
    let add_dialog = dialog.clone();
    add.connect_clicked(move |_| {
        let label = marker_entry.text().trim().to_owned();
        let message = if label.is_empty() {
            marker_runner.borrow_mut().cancel_marker_at(elapsed_s);
            "Marker name cannot be empty.".to_owned()
        } else {
            match marker_runner.borrow_mut().add_marker_at(&label, elapsed_s) {
                Ok(()) => format!("Added marker: {label}"),
                Err(error) => format!("Could not add marker: {error}"),
            }
        };
        marker_status.set_label(&message);
        marker_status.update_property(&[gtk::accessible::Property::Description(&message)]);
        add_dialog.close();
    });
    dialog.present(Some(window));
    entry.grab_focus();
    entry.select_region(0, -1);
}

#[allow(clippy::too_many_lines)]
pub(crate) fn log_tab(runner: &Rc<RefCell<MeasurementRunner>>) -> LogWidgets {
    let interval = gtk::DropDown::from_strings(&LOG_INTERVAL_LABELS);
    interval.set_selected(3);
    interval.update_property(&[gtk::accessible::Property::Label("Log interval")]);
    let interval_runner = Rc::clone(runner);
    let interval_syncing = Rc::new(Cell::new(false));
    let interval_syncing_update = Rc::clone(&interval_syncing);
    interval.connect_notify_local(Some("selected"), move |interval, _| {
        if interval_syncing_update.get() {
            return;
        }
        let Some(index) = usize::try_from(interval.selected())
            .ok()
            .filter(|index| *index < LOG_INTERVAL_VALUES.len())
        else {
            return;
        };
        let _ = interval_runner
            .borrow_mut()
            .set_log_interval(LOG_INTERVAL_VALUES[index]);
    });

    let control_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(12)
        .build();
    let interval_label = gtk::Label::builder()
        .label("Log interval")
        .css_classes(["dim-label"])
        .build();
    control_row.append(&interval_label);
    control_row.append(&interval);

    let plot_data = Rc::new(RefCell::new(LogPlotData::default()));
    let plot = gtk::DrawingArea::builder()
        .content_width(400)
        .content_height(220)
        .hexpand(true)
        .vexpand(true)
        .build();
    plot.update_property(&[gtk::accessible::Property::Label(
        "Fast A, C, and Z level history plot",
    )]);
    let draw_data = Rc::clone(&plot_data);
    plot.set_draw_func(move |_, context, width, height| {
        draw_log_plot(context, width, height, &draw_data.borrow());
    });
    // Clicking a marker line edits it; the Markers… button is the keyboard path.
    let click = gtk::GestureClick::new();
    let click_data = Rc::clone(&plot_data);
    let click_runner = Rc::clone(runner);
    click.connect_pressed(move |gesture, _, x, _| {
        let Some(plot) = gesture.widget() else { return };
        let marker = marker_near(&click_data.borrow(), f64::from(plot.width()), x);
        if let (Some(id), Some(root)) = (marker, plot.root()) {
            present_marker_editor(&root, &click_runner, Some(id));
        }
    });
    plot.add_controller(click);
    let edit_markers = gtk::Button::builder().label("Markers…").build();
    edit_markers.set_tooltip_text(Some("Edit marker labels and notes"));
    edit_markers.update_property(&[gtk::accessible::Property::Label("Edit markers")]);
    let edit_runner = Rc::clone(runner);
    edit_markers.connect_clicked(move |button| {
        if let Some(root) = button.root() {
            present_marker_editor(&root, &edit_runner, None);
        }
    });

    let window = gtk::DropDown::from_strings(&["1 min", "10 min", "1 h", "All"]);
    window.update_property(&[gtk::accessible::Property::Label("Shown time window")]);
    let window_data = Rc::clone(&plot_data);
    let window_plot = plot.clone();
    window.connect_notify_local(Some("selected"), move |window, _| {
        let selected = match window.selected() {
            0 => TimeWindow::OneMinute,
            1 => TimeWindow::TenMinutes,
            2 => TimeWindow::OneHour,
            _ => TimeWindow::All,
        };
        window_data.borrow_mut().window = selected;
        window_plot.queue_draw();
    });
    let window_label = gtk::Label::builder()
        .label("Show")
        .css_classes(["dim-label"])
        .build();
    control_row.append(&window_label);
    control_row.append(&window);
    control_row.append(&edit_markers);
    let auto_scale = auto_scale_toggle("Log plot");
    control_row.append(&auto_scale);
    let axis_data = Rc::clone(&plot_data);
    connect_level_axis_controls(
        &plot,
        &auto_scale,
        Rc::new(move |change| change(&mut axis_data.borrow_mut().axis)),
    );

    for (markup, accessible_label, trace) in [
        ("L<sub>AF</sub>", "Show L A Fast trace", Trace::A),
        ("L<sub>CF</sub>", "Show L C Fast trace", Trace::C),
        ("L<sub>ZF</sub>", "Show L Z Fast trace", Trace::Z),
    ] {
        let toggle = gtk::CheckButton::builder().active(true).build();
        let label = gtk::Label::new(None);
        label.set_markup(markup);
        toggle.set_child(Some(&label));
        toggle.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
        let toggle_data = Rc::clone(&plot_data);
        let toggle_plot = plot.clone();
        toggle.connect_toggled(move |toggle| {
            let active = toggle.is_active();
            let mut data = toggle_data.borrow_mut();
            match trace {
                Trace::A => data.show_a = active,
                Trace::C => data.show_c = active,
                Trace::Z => data.show_z = active,
            }
            toggle_plot.queue_draw();
        });
        control_row.append(&toggle);
    }

    let status = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .wrap(true)
        .css_classes(["dim-label"])
        .margin_bottom(12)
        .build();
    status.update_property(&[gtk::accessible::Property::Label("Log status")]);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(24)
        .build();
    content.append(&control_row);
    content.append(&plot);
    content.append(&status);
    LogWidgets {
        content,
        plot,
        plot_data,
        status,
        time_window: window,
        interval,
        interval_syncing,
    }
}

pub(crate) fn sync_interval_selector(
    interval: &gtk::DropDown,
    syncing: &Cell<bool>,
    interval_s: f64,
) {
    let Some(index) = LOG_INTERVAL_VALUES
        .iter()
        .position(|value| (*value - interval_s).abs() < f64::EPSILON)
    else {
        return;
    };
    if let Ok(index) = u32::try_from(index) {
        syncing.set(true);
        interval.set_selected(index);
        syncing.set(false);
    }
}

pub(crate) fn update_log_view(
    runner: &MeasurementRunner,
    snapshot: Snapshot,
    plot_data: &Rc<RefCell<LogPlotData>>,
    plot: &gtk::DrawingArea,
    status: &gtk::Label,
) {
    let window_s = plot_data.borrow().window.duration_s();
    let Some(history) = runner.log_history(window_s) else {
        status.set_label("Logging is unavailable.");
        return;
    };
    plot_data.borrow_mut().update(&history);
    plot.queue_draw();
    let status_text = format!(
        "Live Fast: L<sub>AF</sub> {:.1} dB · L<sub>CF</sub> {:.1} dB · L<sub>ZF</sub> {:.1} dB\nDuration {} · Interval {} s · {} records · {} markers · {}",
        snapshot.a.fast_db,
        snapshot.c.fast_db,
        snapshot.z.fast_db,
        format_elapsed(Duration::from_secs_f64(history.duration_s)),
        history.interval_s,
        history.record_count,
        history.markers.len(),
        recording_state(snapshot),
    );
    status.set_markup(&status_text);
    let accessible_status = format!(
        "Live Fast: L A F {:.1} dB, L C F {:.1} dB, L Z F {:.1} dB. Duration {}. Interval {} seconds. {} records. {} markers. {}.",
        snapshot.a.fast_db,
        snapshot.c.fast_db,
        snapshot.z.fast_db,
        format_elapsed(Duration::from_secs_f64(history.duration_s)),
        history.interval_s,
        history.record_count,
        history.markers.len(),
        recording_state(snapshot),
    );
    status.update_property(&[gtk::accessible::Property::Description(&accessible_status)]);
}

fn recording_state(snapshot: Snapshot) -> &'static str {
    if snapshot.paused {
        "Paused (Leq, peak and maximum are held)"
    } else {
        "Recording"
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Trace {
    A,
    C,
    Z,
}

#[cfg(test)]
mod tests {
    use super::LogPlotData;
    use spl_engine::{DemoRunner, LogHistory};
    use std::time::Duration;

    /// Advances the demo's clock by `ticks` display refreshes of 0.1 s.
    fn advance(demo: &mut DemoRunner, elapsed: &mut Duration, ticks: u32) {
        for _ in 0..ticks {
            *elapsed += Duration::from_millis(100);
            demo.catch_up(*elapsed);
        }
    }

    #[test]
    fn the_first_record_after_a_pause_starts_a_new_trace_segment() {
        let mut demo = DemoRunner::new(48_000.0, None).expect("valid rate");
        let mut elapsed = Duration::ZERO;
        advance(&mut demo, &mut elapsed, 20);
        demo.pause().expect("recording runs");
        advance(&mut demo, &mut elapsed, 20);
        demo.resume().expect("recording is paused");
        advance(&mut demo, &mut elapsed, 20);
        let history: LogHistory = demo.log_history(None).expect("demo session");
        let mut data = LogPlotData::default();

        data.update(&history);

        let breaks = data
            .records
            .iter()
            .map(|record| record.after_pause)
            .collect::<Vec<_>>();
        assert_eq!(breaks, [false, false, true, false]);
    }
}

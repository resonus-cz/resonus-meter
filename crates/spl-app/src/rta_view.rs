//! The RTA tab (specification 5.3): band bars with detector, weighting and
//! peak hold controls.

use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use spl_engine::{BandDetector, Bandwidth, RtaConfig, RtaFrame, RtaSettings, Weighting};

use crate::log_view::LOG_INTERVAL_LABELS;
use crate::plot::level_axis::{LevelAxis, auto_scale_toggle, connect_level_axis_controls};
use crate::plot::rta::{RtaBar, draw_rta_plot, frequency_label};

const DETECTORS: [(BandDetector, &str); 3] = [
    (BandDetector::Fast, "Fast"),
    (BandDetector::Slow, "Slow"),
    (BandDetector::Leq, "Leq"),
];
const WEIGHTINGS: [(Weighting, &str); 3] = [
    (Weighting::A, "A"),
    (Weighting::C, "C"),
    (Weighting::Z, "Z"),
];
/// Level span shown below the highest band in automatic mode.
const AUTO_SPAN_DB: f64 = 70.0;
/// Nominal octave frequencies, the one-third-octave bands that get a label.
const OCTAVE_LABELS_HZ: [f64; 11] = [
    16.0, 31.5, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];

pub(crate) struct RtaView {
    pub(crate) content: gtk::Box,
    pub(crate) plot: gtk::DrawingArea,
    detector: gtk::DropDown,
    weighting: gtk::DropDown,
    peak_hold: gtk::ToggleButton,
    bars: Rc<RefCell<Vec<RtaBar>>>,
    axis: Rc<RefCell<LevelAxis>>,
    /// Peak-hold levels per band, cleared by Reset and by control changes.
    hold: RefCell<Vec<Option<f64>>>,
    unit: Rc<RefCell<&'static str>>,
}

impl RtaView {
    /// Creates the tab; its log interval selector mirrors `log_interval`,
    /// the Log tab's, since band levels are recorded at the same interval.
    pub(crate) fn new(defaults: RtaSettings, log_interval: &gtk::DropDown) -> Rc<Self> {
        let detector = gtk::DropDown::from_strings(&DETECTORS.map(|(_, name)| name));
        detector.update_property(&[gtk::accessible::Property::Label("RTA detector")]);
        let weighting = gtk::DropDown::from_strings(&WEIGHTINGS.map(|(_, name)| name));
        weighting.update_property(&[gtk::accessible::Property::Label("RTA display weighting")]);
        let peak_hold = gtk::ToggleButton::builder().label("Peak Hold").build();
        peak_hold.update_property(&[gtk::accessible::Property::Label("RTA peak hold")]);
        let auto_scale = auto_scale_toggle("RTA");
        let controls = control_row(log_interval, &detector, &weighting, &peak_hold, &auto_scale);

        let bars = Rc::new(RefCell::new(Vec::<RtaBar>::new()));
        let unit = Rc::new(RefCell::new("dB"));
        let plot = gtk::DrawingArea::builder()
            .content_width(400)
            .content_height(220)
            .hexpand(true)
            .vexpand(true)
            .build();
        plot.update_property(&[gtk::accessible::Property::Label("RTA band levels")]);
        let axis = Rc::new(RefCell::new(LevelAxis::new(20.0, 100.0)));
        let draw_bars = Rc::clone(&bars);
        let draw_unit = Rc::clone(&unit);
        let draw_axis = Rc::clone(&axis);
        plot.set_draw_func(move |_, context, width, height| {
            draw_rta_plot(
                context,
                width,
                height,
                &draw_bars.borrow(),
                &draw_axis.borrow(),
                *draw_unit.borrow(),
            );
        });
        let control_axis = Rc::clone(&axis);
        connect_level_axis_controls(
            &plot,
            &auto_scale,
            Rc::new(move |change| change(&mut control_axis.borrow_mut())),
        );
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(12)
            .build();
        content.append(&controls);
        content.append(&plot);
        let view = Rc::new(Self {
            content,
            plot,
            detector,
            weighting,
            peak_hold,
            bars,
            axis,
            hold: RefCell::new(Vec::new()),
            unit,
        });
        view.apply_defaults(defaults);
        for dropdown in [&view.detector, &view.weighting] {
            let weak = Rc::downgrade(&view);
            dropdown.connect_selected_notify(move |_| {
                if let Some(view) = weak.upgrade() {
                    view.clear_hold();
                }
            });
        }
        let weak = Rc::downgrade(&view);
        view.peak_hold.connect_toggled(move |_| {
            if let Some(view) = weak.upgrade() {
                view.clear_hold();
            }
        });
        view
    }

    /// Sets the controls to the defaults from Preferences.
    pub(crate) fn apply_defaults(&self, defaults: RtaSettings) {
        let index = |found: Option<usize>| {
            found
                .and_then(|index| u32::try_from(index).ok())
                .unwrap_or(0)
        };
        self.detector.set_selected(index(
            DETECTORS
                .iter()
                .position(|(detector, _)| *detector == defaults.detector),
        ));
        self.weighting.set_selected(index(
            WEIGHTINGS
                .iter()
                .position(|(weighting, _)| *weighting == defaults.weighting),
        ));
        self.peak_hold.set_active(defaults.peak_hold);
    }

    pub(crate) fn detector(&self) -> BandDetector {
        selected(&self.detector, &DETECTORS)
    }

    pub(crate) fn weighting(&self) -> Weighting {
        selected(&self.weighting, &WEIGHTINGS)
    }

    /// Shows `frame`; `calibrated` picks dB SPL or dBFS for the axis.
    pub(crate) fn show(&self, frame: Option<RtaFrame>, bandwidth: Bandwidth, calibrated: bool) {
        let Some(frame) = frame else { return };
        let levels = frame
            .bands
            .iter()
            .map(|band| band.level_db)
            .collect::<Vec<_>>();
        let mut hold = self.hold.borrow_mut();
        if self.peak_hold.is_active() {
            update_hold(&mut hold, &levels);
        } else {
            hold.clear();
        }
        *self.bars.borrow_mut() = frame
            .bands
            .iter()
            .enumerate()
            .map(|(index, band)| RtaBar {
                label: if bandwidth == Bandwidth::Octave
                    || OCTAVE_LABELS_HZ
                        .iter()
                        .any(|&octave| (octave - band.nominal_hz).abs() < 0.01)
                {
                    frequency_label(band.nominal_hz)
                } else {
                    String::new()
                },
                level_db: band.level_db,
                hold_db: hold.get(index).copied().flatten(),
            })
            .collect();
        let highest = levels
            .iter()
            .chain(hold.iter())
            .flatten()
            .fold(f64::NEG_INFINITY, |highest, &level| highest.max(level));
        self.axis
            .borrow_mut()
            .follow(highest - AUTO_SPAN_DB, highest);
        *self.unit.borrow_mut() = if calibrated { "dB" } else { "dBFS" };
        self.plot.queue_draw();
    }

    pub(crate) fn clear_hold(&self) {
        self.hold.borrow_mut().clear();
    }
}

/// The analyser configuration the RTA preferences describe.
pub(crate) const fn rta_config(settings: RtaSettings) -> RtaConfig {
    RtaConfig {
        bandwidth: settings.bandwidth,
        min_hz: settings.min_hz,
        max_hz: settings.max_hz,
    }
}

/// The tab's controls; the log interval selector follows `log_interval`.
fn control_row(
    log_interval: &gtk::DropDown,
    detector: &gtk::DropDown,
    weighting: &gtk::DropDown,
    peak_hold: &gtk::ToggleButton,
    auto_scale: &gtk::ToggleButton,
) -> gtk::FlowBox {
    let interval = gtk::DropDown::from_strings(&LOG_INTERVAL_LABELS);
    interval.update_property(&[gtk::accessible::Property::Label("Log interval")]);
    interval.set_tooltip_text(Some(
        "Band levels are recorded at the log interval, shared with the Log tab",
    ));
    // The Log tab's selector applies the change; this one follows it.
    log_interval
        .bind_property("selected", &interval, "selected")
        .bidirectional()
        .sync_create()
        .build();
    // Label and control pairs wrap onto a second line on narrow windows.
    let controls = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(12)
        .row_spacing(6)
        .max_children_per_line(5)
        .build();
    for (label, widget) in [
        ("Log interval", interval.upcast_ref::<gtk::Widget>()),
        ("Detector", detector.upcast_ref()),
        ("Weighting", weighting.upcast_ref()),
    ] {
        let pair = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        pair.append(
            &gtk::Label::builder()
                .label(label)
                .css_classes(["dim-label"])
                .build(),
        );
        pair.append(widget);
        controls.append(&pair);
    }
    let toggles = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    toggles.append(peak_hold);
    toggles.append(auto_scale);
    controls.append(&toggles);
    // Only the controls take keyboard focus, not their flow box cells.
    let mut index = 0;
    while let Some(child) = controls.child_at_index(index) {
        child.set_focusable(false);
        index += 1;
    }
    controls
}

fn selected<T: Copy>(dropdown: &gtk::DropDown, choices: &[(T, &str)]) -> T {
    usize::try_from(dropdown.selected())
        .ok()
        .and_then(|index| choices.get(index))
        .unwrap_or(&choices[0])
        .0
}

/// Raises each held level to the new level; a different band count (new
/// bands) starts the hold afresh.
fn update_hold(hold: &mut Vec<Option<f64>>, levels: &[Option<f64>]) {
    if hold.len() != levels.len() {
        hold.clear();
        hold.resize(levels.len(), None);
    }
    for (slot, level) in hold.iter_mut().zip(levels) {
        *slot = match (*slot, *level) {
            (Some(previous), Some(current)) => Some(previous.max(current)),
            (previous, current) => previous.or(current),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::update_hold;

    #[test]
    fn peak_hold_keeps_the_maximum_and_restarts_on_new_bands() {
        let mut hold = Vec::new();
        update_hold(&mut hold, &[Some(50.0), None, Some(40.0)]);
        update_hold(&mut hold, &[Some(45.0), Some(30.0), Some(60.0)]);
        assert_eq!(hold, [Some(50.0), Some(30.0), Some(60.0)]);

        update_hold(&mut hold, &[Some(10.0)]);
        assert_eq!(hold, [Some(10.0)]);
    }
}

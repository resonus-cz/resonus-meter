use adw::prelude::*;
use spl_engine::{Snapshot, WeightingLevels};
use std::{cell::RefCell, rc::Rc, time::Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FrequencyWeighting {
    A,
    C,
    Z,
}

impl FrequencyWeighting {
    pub(crate) fn levels(self, snapshot: Snapshot) -> WeightingLevels {
        match self {
            Self::A => snapshot.a,
            Self::C => snapshot.c,
            Self::Z => snapshot.z,
        }
    }

    pub(crate) fn letter(self) -> char {
        match self {
            Self::A => 'A',
            Self::C => 'C',
            Self::Z => 'Z',
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Quantity {
    Fast,
    Slow,
    Leq,
    Peak,
}

impl Quantity {
    pub(crate) fn level(self, levels: WeightingLevels) -> Option<f64> {
        match self {
            Self::Fast => Some(levels.fast_db),
            Self::Slow => Some(levels.slow_db),
            Self::Leq => levels.leq_db,
            Self::Peak => levels.peak_db,
        }
    }

    pub(crate) fn descriptor_suffix(self) -> &'static str {
        match self {
            Self::Fast => "F",
            Self::Slow => "S",
            Self::Leq => "eq",
            Self::Peak => "peak",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReadingSelection {
    pub(crate) weighting: FrequencyWeighting,
    pub(crate) quantity: Quantity,
}

pub(crate) struct MeterWidgets {
    pub(crate) content: gtk::Box,
    pub(crate) reading: gtk::Label,
    pub(crate) descriptor: gtk::Label,
    pub(crate) unit: gtk::Label,
    pub(crate) elapsed: gtk::Label,
    pub(crate) uncalibrated: gtk::Box,
    pub(crate) overload: gtk::Box,
    pub(crate) fast_max: gtk::Label,
}

impl ReadingSelection {
    pub(crate) const DEFAULT: Self = Self {
        weighting: FrequencyWeighting::A,
        quantity: Quantity::Fast,
    };

    pub(crate) fn descriptor(self) -> String {
        format!(
            "L{}{}",
            self.weighting.letter(),
            self.quantity.descriptor_suffix()
        )
    }

    pub(crate) fn descriptor_markup(self) -> String {
        format!(
            "L<sub>{}{}</sub>",
            self.weighting.letter(),
            self.quantity.descriptor_suffix()
        )
    }

    pub(crate) fn level(self, snapshot: Snapshot) -> Option<f64> {
        self.quantity.level(self.weighting.levels(snapshot))
    }

    pub(crate) fn fast_max(self, snapshot: Snapshot) -> f64 {
        self.weighting.levels(snapshot).fast_max_db
    }
}

pub(crate) fn meter_surface() -> MeterWidgets {
    let reading = gtk::Label::builder()
        .label("—")
        .halign(gtk::Align::Center)
        .css_classes(["meter-reading"])
        .build();
    reading.update_property(&[gtk::accessible::Property::Label("Sound level")]);
    let descriptor = gtk::Label::builder()
        .label("L")
        .halign(gtk::Align::Center)
        .css_classes(["title-2"])
        .build();
    let unit = gtk::Label::builder()
        .label("dB(FS)")
        .halign(gtk::Align::Center)
        .css_classes(["title-3"])
        .build();
    let reading_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Baseline)
        .build();
    reading_row.append(&reading);
    reading_row.append(&unit);

    let elapsed = gtk::Label::builder()
        .label("00:00:00")
        .css_classes(["dim-label"])
        .build();
    elapsed.update_property(&[gtk::accessible::Property::Label("Elapsed time since reset")]);
    let uncalibrated = indicator("dialog-warning-symbolic", "Uncalibrated");
    let overload = indicator("dialog-warning-symbolic", "Overload");
    overload.set_visible(false);
    let secondary = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::Center)
        .build();
    secondary.append(&elapsed);
    secondary.append(&uncalibrated);
    secondary.append(&overload);

    let fast_max = gtk::Label::builder()
        .label("L")
        .halign(gtk::Align::Center)
        .css_classes(["dim-label"])
        .build();
    fast_max.update_property(&[gtk::accessible::Property::Label(
        "Maximum Fast level since reset",
    )]);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(24)
        .margin_end(24)
        .build();
    content.append(&reading_row);
    content.append(&descriptor);
    content.append(&secondary);
    content.append(&fast_max);
    MeterWidgets {
        content,
        reading,
        descriptor,
        unit,
        elapsed,
        uncalibrated,
        overload,
        fast_max,
    }
}

/// Returns the ribbon's Reading group: weighting (row 0) and quantity (row 1) toggles
/// with their labels (ADR 0017).
pub(crate) fn selection_controls(selection: &Rc<RefCell<ReadingSelection>>) -> gtk::Grid {
    let weighting_selection = Rc::clone(selection);
    let weighting_controls = linked_controls(
        &[
            ("A", "A weighting", FrequencyWeighting::A),
            ("C", "C weighting", FrequencyWeighting::C),
            ("Z", "Z weighting", FrequencyWeighting::Z),
        ],
        "Frequency weighting",
        move |weighting| weighting_selection.borrow_mut().weighting = weighting,
    );
    let quantity_selection = Rc::clone(selection);
    let quantity_controls = linked_controls(
        &[
            ("L<sub>F</sub>", "Fast level", Quantity::Fast),
            ("L<sub>S</sub>", "Slow level", Quantity::Slow),
            ("L<sub>eq</sub>", "Equivalent level", Quantity::Leq),
            ("L<sub>peak</sub>", "Sample peak level", Quantity::Peak),
        ],
        "Level quantity",
        move |quantity| quantity_selection.borrow_mut().quantity = quantity,
    );
    let controls = crate::layout::ribbon_grid();
    let rows = [
        ("Weighting", &weighting_controls),
        ("Quantity", &quantity_controls),
    ];
    for (row, (title, toggles)) in (0..).zip(rows) {
        let label = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        toggles.set_halign(gtk::Align::Start);
        toggles.set_valign(gtk::Align::Center);
        controls.attach(&label, 0, row, 1, 1);
        controls.attach(toggles, 1, row, 1, 1);
    }
    controls
}

pub(crate) fn install_meter_styles() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        ".meter-reading {
            font-size: 72pt;
            font-variant-numeric: tabular-nums;
        }",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn indicator(icon_name: &str, text: &str) -> gtk::Box {
    let indicator = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .build();
    indicator.update_property(&[gtk::accessible::Property::Label(text)]);
    indicator.append(&gtk::Image::from_icon_name(icon_name));
    indicator.append(&gtk::Label::new(Some(text)));
    indicator
}

fn linked_controls<T: Copy + 'static>(
    choices: &[(&str, &str, T)],
    accessible_name: &str,
    on_selected: impl Fn(T) + 'static,
) -> gtk::Box {
    let controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["linked"])
        .build();
    controls.update_property(&[gtk::accessible::Property::Label(accessible_name)]);
    let mut group: Option<gtk::ToggleButton> = None;
    let on_selected = Rc::new(on_selected);
    for (markup, accessible_label, value) in choices {
        let mut builder = gtk::ToggleButton::builder();
        if let Some(first) = &group {
            builder = builder.group(first);
        } else {
            builder = builder.active(true);
        }
        let button = builder.build();
        let label = gtk::Label::new(None);
        label.set_markup(markup);
        button.set_child(Some(&label));
        button.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
        let on_selected = Rc::clone(&on_selected);
        let value = *value;
        button.connect_toggled(move |button| {
            if button.is_active() {
                on_selected(value);
            }
        });
        controls.append(&button);
        group.get_or_insert(button);
    }
    controls
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_display(
    snapshot: Snapshot,
    selection: ReadingSelection,
    elapsed_time: Duration,
    reading: &gtk::Label,
    descriptor: &gtk::Label,
    unit: &gtk::Label,
    elapsed: &gtk::Label,
    uncalibrated: &gtk::Box,
    overload: &gtk::Box,
    fast_max: &gtk::Label,
) {
    let display_unit = if snapshot.calibrated { "dB" } else { "dB(FS)" };
    let level = format_level(selection.level(snapshot));
    reading.set_label(&level);
    reading.update_property(&[gtk::accessible::Property::ValueText(&level)]);
    descriptor.set_markup(&selection.descriptor_markup());
    descriptor.update_property(&[gtk::accessible::Property::Label(&selection.descriptor())]);
    unit.set_label(display_unit);
    elapsed.set_label(&format_elapsed(elapsed_time));
    uncalibrated.set_visible(!snapshot.calibrated);
    overload.set_visible(snapshot.clipped);
    fast_max.set_markup(&format!(
        "L<sub>{}F,max</sub> {} {display_unit}",
        selection.weighting.letter(),
        format_level(Some(selection.fast_max(snapshot)))
    ));
}

pub(crate) fn format_level(level: Option<f64>) -> String {
    level
        .filter(|value| value.is_finite())
        .map_or_else(|| "—".to_owned(), |value| format!("{value:.1}"))
}

pub(crate) fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::{FrequencyWeighting, Quantity, ReadingSelection, format_elapsed, format_level};
    use spl_engine::{Snapshot, WeightingLevels};
    use std::time::Duration;

    const LEVELS_A: WeightingLevels = WeightingLevels {
        fast_db: 10.0,
        slow_db: 11.0,
        leq_db: Some(12.0),
        peak_db: Some(13.0),
        fast_max_db: 14.0,
    };
    const LEVELS_C: WeightingLevels = WeightingLevels {
        fast_db: 20.0,
        slow_db: 21.0,
        leq_db: Some(22.0),
        peak_db: Some(23.0),
        fast_max_db: 24.0,
    };
    const LEVELS_Z: WeightingLevels = WeightingLevels {
        fast_db: 30.0,
        slow_db: 31.0,
        leq_db: Some(32.0),
        peak_db: Some(33.0),
        fast_max_db: 34.0,
    };
    const SNAPSHOT: Snapshot = Snapshot {
        a: LEVELS_A,
        c: LEVELS_C,
        z: LEVELS_Z,
        clipped: false,
        calibrated: false,
        sample_count: 0,
        paused: false,
    };

    #[test]
    fn selection_uses_the_requested_weighting_and_quantity() {
        let selection = ReadingSelection {
            weighting: FrequencyWeighting::C,
            quantity: Quantity::Peak,
        };

        assert_eq!(selection.descriptor(), "LCpeak");
        assert_eq!(selection.level(SNAPSHOT), Some(23.0));
        assert!((selection.fast_max(SNAPSHOT) - 24.0).abs() < f64::EPSILON);
    }

    #[test]
    fn display_formatting_handles_pending_and_elapsed_values() {
        assert_eq!(format_level(None), "—");
        assert_eq!(format_level(Some(f64::NEG_INFINITY)), "—");
        assert_eq!(format_level(Some(42.25)), "42.2");
        assert_eq!(format_elapsed(Duration::from_secs(3_661)), "01:01:01");
    }

    #[test]
    fn descriptor_uses_standard_quantity_suffixes() {
        for (quantity, suffix) in [
            (Quantity::Fast, "F"),
            (Quantity::Slow, "S"),
            (Quantity::Leq, "eq"),
            (Quantity::Peak, "peak"),
        ] {
            let selection = ReadingSelection {
                weighting: FrequencyWeighting::Z,
                quantity,
            };
            assert_eq!(selection.descriptor(), format!("LZ{suffix}"));
        }
    }
}

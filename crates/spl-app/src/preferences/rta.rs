use super::{PreferencesContext, string_list};
use adw::prelude::*;
use spl_engine::{BandDetector, Bandwidth, RtaSettings, Weighting, rta_bands};

use crate::plot::rta::frequency_label;

const BANDWIDTHS: [(Bandwidth, &str); 2] = [
    (Bandwidth::ThirdOctave, "One-third octave"),
    (Bandwidth::Octave, "Octave"),
];
const DETECTORS: [(BandDetector, &str); 3] = [
    (BandDetector::Fast, "Fast"),
    (BandDetector::Slow, "Slow"),
    (BandDetector::Leq, "Leq"),
];
const WEIGHTINGS: [(Weighting, &str); 3] = [
    (Weighting::A, "A"),
    (Weighting::C, "C"),
    (Weighting::Z, "Z (flat)"),
];

/// RTA preferences (specification 5.4).
pub(crate) fn rta_page(context: &PreferencesContext) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .name("rta")
        .title("RTA")
        .icon_name("view-continuous-symbolic")
        .build();
    let current = context.settings.borrow().rta();
    // Range choices are the one-third-octave nominal frequencies.
    let frequencies = rta_bands(Bandwidth::ThirdOctave, 20.0, 20_000.0)
        .into_iter()
        .map(|band| band.nominal_hz)
        .collect::<Vec<_>>();
    let labels = frequencies
        .iter()
        .map(|&frequency| format!("{} Hz", frequency_label(frequency)))
        .collect::<Vec<_>>();
    let position = |frequency: f64| {
        frequencies
            .iter()
            .position(|&candidate| (candidate - frequency).abs() < 0.01)
            .and_then(|index| u32::try_from(index).ok())
            .unwrap_or(0)
    };

    let BandRows {
        group: bands,
        bandwidth,
        from,
        to,
        record,
    } = band_rows(current, &labels, position);
    let display = adw::PreferencesGroup::builder()
        .title("RTA tab defaults")
        .build();
    let detector = choice_row("Detector", &DETECTORS, current.detector);
    let weighting = choice_row("Display weighting", &WEIGHTINGS, current.weighting);
    weighting.set_subtitle("Applied per band at its mid-band frequency");
    let peak_hold = adw::SwitchRow::builder()
        .title("Peak hold")
        .active(current.peak_hold)
        .build();
    display.add(&detector);
    display.add(&weighting);
    display.add(&peak_hold);
    let status = adw::ActionRow::builder().title("Saved to settings").build();
    display.add(&status);
    page.add(&bands);
    page.add(&display);

    let apply = {
        let context = context.clone();
        let (bandwidth, from, to) = (bandwidth.clone(), from.clone(), to.clone());
        let (detector, weighting, peak_hold, record) = (
            detector.clone(),
            weighting.clone(),
            peak_hold.clone(),
            record.clone(),
        );
        let status = status.clone();
        let frequencies = frequencies.clone();
        move || {
            let frequency = |row: &adw::ComboRow| {
                usize::try_from(row.selected())
                    .ok()
                    .and_then(|index| frequencies.get(index).copied())
            };
            let rta = RtaSettings {
                bandwidth: chosen(&bandwidth, &BANDWIDTHS),
                min_hz: frequency(&from).unwrap_or(20.0),
                max_hz: frequency(&to).unwrap_or(20_000.0),
                detector: chosen(&detector, &DETECTORS),
                weighting: chosen(&weighting, &WEIGHTINGS),
                peak_hold: peak_hold.is_active(),
                record: record.is_active(),
            };
            let result = context.settings.borrow_mut().set_rta(rta);
            status.set_title(match &result {
                Ok(()) if rta.min_hz >= rta.max_hz => {
                    "Lowest band must be below the highest; using 20 Hz – 20 kHz"
                }
                Ok(()) => "Saved to settings",
                Err(_) => "Applies until the app closes; not saved",
            });
            (context.on_rta_changed)(context.settings.borrow().rta());
        }
    };
    for row in [&bandwidth, &from, &to, &detector, &weighting] {
        let apply = apply.clone();
        row.connect_selected_notify(move |_| apply());
    }
    for row in [&peak_hold, &record] {
        let apply = apply.clone();
        row.connect_active_notify(move |_| apply());
    }
    page
}

/// The Bands group and its rows.
struct BandRows {
    group: adw::PreferencesGroup,
    bandwidth: adw::ComboRow,
    from: adw::ComboRow,
    to: adw::ComboRow,
    record: adw::SwitchRow,
}

fn band_rows(current: RtaSettings, labels: &[String], position: impl Fn(f64) -> u32) -> BandRows {
    let group = adw::PreferencesGroup::builder()
        .title("Bands")
        .description("Bands above the input's Nyquist frequency are not shown.")
        .build();
    let bandwidth = choice_row("Bandwidth", &BANDWIDTHS, current.bandwidth);
    let from = adw::ComboRow::builder()
        .title("Lowest band")
        .model(&string_list(labels))
        .selected(position(current.min_hz))
        .build();
    let to = adw::ComboRow::builder()
        .title("Highest band")
        .model(&string_list(labels))
        .selected(position(current.max_hz))
        .build();
    let record = adw::SwitchRow::builder()
        .title("Record band levels")
        .subtitle(
            "Z-weighted Leq per band for each log interval and segment, saved with the \
             measurement; applies from the next Reset",
        )
        .active(current.record)
        .build();
    group.add(&bandwidth);
    group.add(&from);
    group.add(&to);
    group.add(&record);
    BandRows {
        group,
        bandwidth,
        from,
        to,
        record,
    }
}

fn choice_row<T: Copy + PartialEq>(
    title: &str,
    choices: &[(T, &str)],
    current: T,
) -> adw::ComboRow {
    let names = choices
        .iter()
        .map(|(_, name)| (*name).to_owned())
        .collect::<Vec<_>>();
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&string_list(&names))
        .selected(
            choices
                .iter()
                .position(|(choice, _)| *choice == current)
                .and_then(|index| u32::try_from(index).ok())
                .unwrap_or(0),
        )
        .build();
    row.update_property(&[gtk::accessible::Property::Label(title)]);
    row
}

fn chosen<T: Copy>(row: &adw::ComboRow, choices: &[(T, &str)]) -> T {
    usize::try_from(row.selected())
        .ok()
        .and_then(|index| choices.get(index))
        .unwrap_or(&choices[0])
        .0
}

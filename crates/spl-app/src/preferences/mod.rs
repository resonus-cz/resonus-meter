mod calibration;
mod device;
mod general;
mod rta;

use crate::runner::MeasurementRunner;
use adw::prelude::*;
use calibration::calibration_page;
use device::sound_device_page;
use general::general_page;
use rta::rta_page;
use spl_engine::{CalibrationStore, Settings};
use std::{cell::RefCell, path::Path, rc::Rc, time::Instant};

/// Application state the preference pages read and change.
#[derive(Clone)]
pub(crate) struct PreferencesContext {
    pub(crate) runner: Rc<RefCell<MeasurementRunner>>,
    pub(crate) store: Rc<RefCell<CalibrationStore>>,
    /// Why the calibration history is kept in memory only, if it is.
    pub(crate) store_warning: Option<Rc<str>>,
    pub(crate) settings: Rc<RefCell<Settings>>,
    /// Where session journals are written.
    pub(crate) sessions_directory: Rc<Path>,
    /// Applies changed RTA preferences to the RTA tab.
    pub(crate) on_rta_changed: Rc<dyn Fn(spl_engine::RtaSettings)>,
    pub(crate) title: adw::WindowTitle,
    pub(crate) input_status: gtk::Label,
    pub(crate) reset_time: Rc<RefCell<Instant>>,
}

/// Shows the preferences, at the page named `page` if given ("sound-device", "calibration").
pub(crate) fn present_preferences(
    window: &adw::ApplicationWindow,
    context: &PreferencesContext,
    page: Option<&str>,
) {
    let dialog = adw::PreferencesDialog::builder()
        .title("Preferences")
        .content_width(620)
        .content_height(560)
        .build();
    let (calibration, refresh_calibration) = calibration_page(context);
    dialog.add(&general_page(context));
    dialog.add(&sound_device_page(context, &refresh_calibration));
    dialog.add(&calibration);
    dialog.add(&rta_page(context));
    if let Some(page) = page {
        dialog.set_visible_page_name(page);
    }
    dialog.present(Some(window));
}

pub(crate) fn read_only_row(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .build();
    row.update_property(&[gtk::accessible::Property::Label(title)]);
    row
}

pub(crate) fn string_list(items: &[String]) -> gtk::StringList {
    let items = items.iter().map(String::as_str).collect::<Vec<_>>();
    gtk::StringList::new(&items)
}

pub(crate) fn update_full_scale_equivalence(row: &adw::ActionRow, runner: &MeasurementRunner) {
    let subtitle = runner.full_scale_sine_spl_db().map_or_else(
        || "No calibration offset is active".to_owned(),
        |level_db| format!("A 0 dBFS-peak sine (−3.01 dBFS RMS) reads {level_db:.2} dB SPL"),
    );
    row.set_subtitle(&subtitle);
    row.update_property(&[gtk::accessible::Property::Description(&subtitle)]);
}

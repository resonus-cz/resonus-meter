use adw::prelude::*;
use gtk::glib;
use spl_engine::{CalibrationKind, CalibrationRecord};
use std::cell::RefCell;

/// Saved calibrations of the current input, newest first (specification 4.9, step 6).
pub(super) struct HistoryWidgets {
    pub(super) group: adw::PreferencesGroup,
    storage: adw::ActionRow,
    rows: RefCell<Vec<adw::ActionRow>>,
}

impl HistoryWidgets {
    pub(super) fn new() -> Self {
        let group = adw::PreferencesGroup::builder()
            .title("History")
            .description(
                "Every saved calibration of this input. The newest record is active; restoring \
                 an older one saves it again as the newest.",
            )
            .build();
        let storage = adw::ActionRow::builder().title("Saved to").build();
        storage.update_property(&[gtk::accessible::Property::Label("Calibration history file")]);
        group.add(&storage);
        Self {
            group,
            storage,
            rows: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn set_storage_status(&self, status: &str) {
        self.storage.set_subtitle(status);
        self.storage
            .update_property(&[gtk::accessible::Property::Description(status)]);
    }

    /// Replaces the listed records; `on_restore` receives the chosen record.
    pub(super) fn show(
        &self,
        records: &[&CalibrationRecord],
        on_restore: &(impl Fn(CalibrationRecord) + Clone + 'static),
    ) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
        let mut rows = self.rows.borrow_mut();
        if records.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No saved calibration")
                .subtitle("Readings of this input are uncalibrated (dBFS).")
                .build();
            self.group.add(&row);
            rows.push(row);
        }
        for (index, record) in records.iter().enumerate() {
            let title = record_title(record);
            let row = adw::ActionRow::builder()
                .title(&title)
                .subtitle(record_subtitle(record))
                .build();
            if index == 0 {
                // Text, not colour, marks the active record.
                row.add_suffix(&gtk::Label::new(Some("Active")));
            } else {
                let restore = gtk::Button::builder()
                    .label("Restore")
                    .valign(gtk::Align::Center)
                    .build();
                restore.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Restore calibration {title}"
                ))]);
                let record = (*record).clone();
                let on_restore = on_restore.clone();
                restore.connect_clicked(move |_| on_restore(record.clone()));
                row.add_suffix(&restore);
            }
            self.group.add(&row);
            rows.push(row);
        }
    }
}

fn record_title(record: &CalibrationRecord) -> String {
    match (
        record.kind,
        record.calibrator_level_db,
        record.tone_frequency_hz,
    ) {
        (CalibrationKind::Calibrator, Some(level_db), Some(frequency_hz)) => format!(
            "C = {:+.2} dB · calibrator {level_db:.1} dB at {frequency_hz:.0} Hz",
            record.offset_db
        ),
        (CalibrationKind::Calibrator, ..) => {
            format!("C = {:+.2} dB · calibrator", record.offset_db)
        }
        (CalibrationKind::Manual, ..) => format!("C = {:+.2} dB · manual", record.offset_db),
    }
}

fn record_subtitle(record: &CalibrationRecord) -> String {
    let saved = local_time(record.saved_utc);
    let mut subtitle = if record.saved_utc == record.calibrated_utc {
        saved
    } else {
        format!(
            "{saved} (restored, measured {})",
            local_time(record.calibrated_utc)
        )
    };
    if !record.note.is_empty() {
        subtitle.push_str(" · ");
        subtitle.push_str(&record.note);
    }
    subtitle
}

/// Formats a UTC instant in the user's locale and time zone.
fn local_time(utc: time::OffsetDateTime) -> String {
    glib::DateTime::from_unix_utc(utc.unix_timestamp())
        .and_then(|date| date.to_local())
        .and_then(|date| date.format("%x %X"))
        .map_or_else(|_| format!("{utc} UTC"), String::from)
}

#[cfg(test)]
mod tests {
    use super::record_title;
    use spl_engine::{CalibrationProposal, CalibrationRecord, InputProfile};
    use time::OffsetDateTime;

    #[test]
    fn titles_state_the_offset_and_its_origin() {
        let input = InputProfile {
            device_id: "mic".to_owned(),
            device_name: "Mic".to_owned(),
            channel_index: 0,
            sample_rate_hz: 48_000,
        };
        let proposal = CalibrationProposal {
            calibrator_level_db: 94.0,
            measured_level_dbfs: -23.0,
            frequency_hz: 1_000.2,
            offset_db: 117.0,
            stability_db: 0.01,
            tone_to_background_db: 45.0,
            peak_dbfs: -20.0,
        };
        let now = OffsetDateTime::UNIX_EPOCH;

        assert_eq!(
            record_title(&CalibrationRecord::from_proposal(
                &input, &proposal, "", now
            )),
            "C = +117.00 dB · calibrator 94.0 dB at 1000 Hz"
        );
        assert_eq!(
            record_title(&CalibrationRecord::manual(&input, -3.5, now)),
            "C = -3.50 dB · manual"
        );
    }
}

use super::super::read_only_row;
use adw::prelude::*;
use spl_engine::CalibrationCaptureState;

/// Calibrator capture controls (specification 4.9, steps 1–4).
pub(super) struct CaptureWidgets {
    pub(super) group: adw::PreferencesGroup,
    pub(super) level: adw::SpinRow,
    pub(super) frequency: adw::SpinRow,
    pub(super) note: adw::EntryRow,
    pub(super) button: gtk::Button,
    pub(super) save: gtk::Button,
    progress: gtk::ProgressBar,
    result: adw::ActionRow,
}

impl CaptureWidgets {
    pub(super) fn new() -> Self {
        let group = adw::PreferencesGroup::builder()
            .title("Calibrator capture")
            .description(
                "Capture eight seconds of raw audio. The tone is measured unweighted, so the \
                 offset holds for A, C and Z weighting. Input gain is controlled by \
                 PipeWire/GNOME; changing the system, driver or interface gain invalidates the \
                 calibration. The proposal is not applied until you save it.",
            )
            .build();
        let level = adw::SpinRow::with_range(0.0, 200.0, 0.1);
        level.set_title("Calibrator level");
        level.set_subtitle("dB SPL");
        level.set_value(94.0);
        level.update_property(&[gtk::accessible::Property::Label(
            "Calibrator reference level in decibels SPL",
        )]);
        group.add(&level);
        let frequency = adw::SpinRow::with_range(1.0, 20_000.0, 1.0);
        frequency.set_title("Calibrator frequency");
        frequency.set_subtitle("Hz");
        frequency.set_value(1_000.0);
        frequency.update_property(&[gtk::accessible::Property::Label(
            "Calibrator reference frequency in hertz",
        )]);
        group.add(&frequency);

        let button = gtk::Button::builder()
            .label("Capture")
            .valign(gtk::Align::Center)
            .build();
        button.update_property(&[gtk::accessible::Property::Label(
            "Capture eight-second calibration tone",
        )]);
        let capture_row = adw::ActionRow::builder()
            .title("Capture calibration tone")
            .subtitle("Records raw, unweighted samples")
            .build();
        capture_row.add_suffix(&button);
        group.add(&capture_row);

        let progress = gtk::ProgressBar::builder()
            .show_text(true)
            .valign(gtk::Align::Center)
            .build();
        progress.set_text(Some("Ready to capture"));
        progress.update_property(&[gtk::accessible::Property::Label(
            "Calibration capture progress",
        )]);
        let progress_row = adw::ActionRow::builder().title("Capture progress").build();
        progress_row.add_suffix(&progress);
        group.add(&progress_row);

        let note = adw::EntryRow::builder().title("Note (optional)").build();
        note.update_property(&[gtk::accessible::Property::Label(
            "Note saved with the calibration record",
        )]);
        group.add(&note);

        let result = read_only_row(
            "Capture result",
            "Start the calibrator, then press Capture.",
        );
        let save = gtk::Button::builder()
            .label("Save")
            .sensitive(false)
            .valign(gtk::Align::Center)
            .css_classes(["suggested-action"])
            .build();
        save.update_property(&[gtk::accessible::Property::Label(
            "Save approved calibration proposal",
        )]);
        result.add_suffix(&save);
        group.add(&result);

        Self {
            group,
            level,
            frequency,
            note,
            button,
            save,
            progress,
            result,
        }
    }

    /// Shows that a capture has started.
    pub(super) fn show_started(&self) {
        self.progress.set_fraction(0.0);
        self.progress.set_text(Some("Capturing 0%"));
        self.show_message("Listening for the calibrator tone…");
        self.save.set_sensitive(false);
    }

    pub(super) fn show_message(&self, message: &str) {
        self.result.set_subtitle(message);
        self.result
            .update_property(&[gtk::accessible::Property::Description(message)]);
    }

    /// Mirrors the engine's capture progress or finished result.
    pub(super) fn show_state(&self, state: CalibrationCaptureState) {
        match state {
            CalibrationCaptureState::Idle => {}
            CalibrationCaptureState::Capturing {
                captured_samples,
                target_samples,
                peak_dbfs,
            } => {
                #[allow(clippy::cast_precision_loss)]
                let fraction = captured_samples as f64 / target_samples as f64;
                self.progress.set_fraction(fraction);
                self.progress
                    .set_text(Some(&format!("Capturing {:.0}%", fraction * 100.0)));
                self.result.set_subtitle(&format!(
                    "Listening for the calibrator tone… Peak {peak_dbfs:.1} dBFS; {:.1} dB headroom.",
                    (-peak_dbfs).max(0.0),
                ));
                self.save.set_sensitive(false);
                self.button.set_sensitive(false);
            }
            CalibrationCaptureState::Complete(Ok(proposal)) => {
                self.progress.set_fraction(1.0);
                self.progress.set_text(Some("Capture complete"));
                self.show_message(&format!(
                    "{:.2} Hz · tone {:.2} dBFS · peak {:.2} dBFS · {:.2} dB headroom · C = {:+.2} dB · stability {:.3} dB · tone/background {:.1} dB",
                    proposal.frequency_hz,
                    proposal.measured_level_dbfs,
                    proposal.peak_dbfs,
                    (-proposal.peak_dbfs).max(0.0),
                    proposal.offset_db,
                    proposal.stability_db,
                    proposal.tone_to_background_db,
                ));
                self.save.set_sensitive(true);
                self.button.set_sensitive(true);
            }
            CalibrationCaptureState::Complete(Err(error)) => {
                self.progress.set_fraction(0.0);
                self.progress.set_text(Some("Capture failed"));
                self.show_message(&format!("Capture rejected: {error}"));
                self.save.set_sensitive(false);
                self.button.set_sensitive(true);
            }
        }
    }
}

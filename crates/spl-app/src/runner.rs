use std::{cell::RefCell, path::Path};

use spl_engine::{
    BandDetector, Calibration, CalibrationCaptureState, CalibrationProposal, CalibrationRecord,
    CalibrationStore, DEMO_DEVICE_ID, DemoRunner, InputProfile, LogHistory, RtaConfig, RtaFrame,
    SaveMetadata, SaveRange, SegmentAggregate, Snapshot, Weighting,
};
#[cfg(feature = "cpal-backend")]
use {
    spl_audio::{DeviceInfo, StreamConfig},
    spl_engine::{CaptureRunner, RecentInput},
};

const DEMO_SAMPLE_RATE_HZ: f64 = 48_000.0;

pub(crate) enum MeasurementRunner {
    Demo(Box<DemoRunner>),
    #[cfg(feature = "cpal-backend")]
    Live {
        runner: Box<CaptureRunner>,
        device_id: String,
        device_name: String,
        channel_index: u16,
        sample_rate_hz: u32,
    },
}

impl MeasurementRunner {
    /// Creates the demo runner with the demo profile's stored calibration.
    pub(crate) fn demo(store: &CalibrationStore) -> Self {
        let calibration = store
            .active(&demo_profile())
            .map(CalibrationRecord::calibration);
        Self::Demo(Box::new(
            DemoRunner::new(DEMO_SAMPLE_RATE_HZ, calibration)
                .expect("the built-in demo sample rate is supported"),
        ))
    }

    /// Returns the calibration profile of the active input.
    pub(crate) fn input_profile(&self) -> InputProfile {
        match self {
            Self::Demo(_) => demo_profile(),
            #[cfg(feature = "cpal-backend")]
            Self::Live {
                device_id,
                device_name,
                channel_index,
                sample_rate_hz,
                ..
            } => InputProfile {
                device_id: device_id.clone(),
                device_name: device_name.clone(),
                channel_index: *channel_index,
                sample_rate_hz: *sample_rate_hz,
            },
        }
    }

    /// Returns the active input as an entry of the recently used inputs.
    #[cfg(feature = "cpal-backend")]
    pub(crate) fn recent_input(&self) -> RecentInput {
        let profile = self.input_profile();
        RecentInput {
            device_id: profile.device_id,
            channel_index: profile.channel_index,
            sample_rate_hz: profile.sample_rate_hz,
        }
    }

    pub(crate) fn tick(&mut self) -> Snapshot {
        match self {
            Self::Demo(runner) => runner.tick(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.tick(),
        }
    }

    pub(crate) fn reset(&mut self) {
        match self {
            Self::Demo(runner) => runner.reset(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.reset(),
        }
    }

    pub(crate) fn set_calibration(&mut self, calibration: Option<Calibration>) {
        match self {
            Self::Demo(runner) => runner.set_calibration(calibration),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.set_calibration(calibration),
        }
    }

    pub(crate) fn set_manual_calibration(&mut self, offset_db: f64) {
        match self {
            Self::Demo(runner) => runner.set_manual_calibration(offset_db),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.set_manual_calibration(offset_db),
        }
    }

    /// Short calibration state for the ribbon, e.g. "Calibrator 1000 Hz\n94.0 dB".
    pub(crate) fn calibration_status(&self) -> String {
        let calibration = match self {
            Self::Demo(runner) => runner.calibration(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.calibration(),
        };
        calibration_status(calibration)
    }

    pub(crate) fn calibration_offset_db(&self) -> Option<f64> {
        match self {
            Self::Demo(runner) => runner.calibration_offset_db(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.calibration_offset_db(),
        }
    }

    pub(crate) fn start_calibration_capture(
        &mut self,
        calibrator_level_db: f64,
        target_frequency_hz: f64,
    ) -> Result<(), String> {
        match self {
            Self::Demo(runner) => {
                runner.start_calibration_capture(calibrator_level_db, target_frequency_hz)
            }
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => {
                runner.start_calibration_capture(calibrator_level_db, target_frequency_hz)
            }
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn calibration_capture_state(&self) -> CalibrationCaptureState {
        match self {
            Self::Demo(runner) => runner.calibration_capture_state(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.calibration_capture_state(),
        }
    }

    pub(crate) fn save_calibration_proposal(&mut self) -> Result<CalibrationProposal, String> {
        match self {
            Self::Demo(runner) => runner.save_calibration_proposal(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.save_calibration_proposal(),
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn full_scale_sine_spl_db(&self) -> Option<f64> {
        match self {
            Self::Demo(runner) => runner.full_scale_sine_spl_db(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.full_scale_sine_spl_db(),
        }
    }

    pub(crate) fn subtitle(&self) -> String {
        match self {
            Self::Demo(_) => "Demo source".to_owned(),
            #[cfg(feature = "cpal-backend")]
            Self::Live {
                device_name,
                channel_index,
                sample_rate_hz,
                ..
            } => format!(
                "{device_name} · channel {} · {sample_rate_hz} Hz",
                channel_index + 1
            ),
        }
    }

    pub(crate) fn input_status(&self) -> Option<String> {
        match self {
            Self::Demo(_) => None,
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => {
                let dropped = runner.dropped_samples();
                let errors = runner.stream_error_count();
                match (dropped, errors) {
                    (0, 0) => None,
                    (dropped, 0) => {
                        Some(format!("Live input is dropping samples ({dropped} total)."))
                    }
                    (0, errors) => Some(format!("Live input reported {errors} stream error(s).")),
                    (dropped, errors) => Some(format!(
                        "Live input is dropping samples ({dropped}) and reported {errors} stream error(s)."
                    )),
                }
            }
        }
    }

    #[cfg(feature = "cpal-backend")]
    pub(crate) fn live_configuration(&self) -> Option<(String, u16, u32)> {
        match self {
            Self::Demo(_) => None,
            Self::Live {
                device_id,
                channel_index,
                sample_rate_hz,
                ..
            } => Some((device_id.clone(), *channel_index, *sample_rate_hz)),
        }
    }

    pub(crate) fn configured_rate_label(&self) -> String {
        match self {
            Self::Demo(_) => "48,000 Hz (demo)".to_owned(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { sample_rate_hz, .. } => format!("{sample_rate_hz} Hz"),
        }
    }

    pub(crate) fn save_measurement_with_metadata(
        &self,
        path: &std::path::Path,
        metadata: SaveMetadata,
    ) -> Result<(), String> {
        match self {
            Self::Demo(runner) => runner
                .save_measurement_with_metadata(path, metadata)
                .map_err(|error| error.to_string()),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner
                .save_measurement_with_metadata(path, metadata)
                .map_err(|error| error.to_string()),
        }
    }

    pub(crate) fn start_journal(&mut self, sessions_directory: &Path) -> Result<(), String> {
        match self {
            Self::Demo(runner) => runner.start_journal(sessions_directory),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.start_journal(sessions_directory),
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn flush_journal(&mut self) -> Result<(), String> {
        match self {
            Self::Demo(runner) => runner.flush_journal(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.flush_journal(),
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn discard_journal(&mut self) -> Result<(), String> {
        match self {
            Self::Demo(runner) => runner.discard_journal(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.discard_journal(),
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn recover_session(&mut self, directory: &Path) -> Result<(), String> {
        match self {
            Self::Demo(runner) => runner.recover_session(directory),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.recover_session(directory),
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn has_unsaved_changes(&self) -> bool {
        match self {
            Self::Demo(runner) => runner.has_unsaved_changes(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.has_unsaved_changes(),
        }
    }

    /// Pauses a running recording or resumes a paused one.
    pub(crate) fn toggle_pause(&mut self) -> Result<(), String> {
        match self {
            Self::Demo(runner) => {
                if runner.is_paused() {
                    runner.resume()
                } else {
                    runner.pause()
                }
            }
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => {
                if runner.is_paused() {
                    runner.resume()
                } else {
                    runner.pause()
                }
            }
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn range_summary(&self, range: SaveRange) -> Result<SegmentAggregate, String> {
        match self {
            Self::Demo(runner) => runner.range_summary(range),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.range_summary(range),
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn new_session(&mut self) -> Result<(), String> {
        match self {
            Self::Demo(runner) => runner.new_session(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.new_session(),
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn edit_marker(&mut self, id: u64, label: &str, note: &str) -> Result<(), String> {
        match self {
            Self::Demo(runner) => runner.edit_marker(id, label, note),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.edit_marker(id, label, note),
        }
        .map_err(|error| error.to_string())
    }

    /// Chooses the bands the session records; see `Engine::set_band_log`.
    pub(crate) fn set_band_log(&mut self, bands: Option<RtaConfig>) {
        match self {
            Self::Demo(runner) => runner.set_band_log(bands),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.set_band_log(bands),
        }
    }

    pub(crate) fn set_rta(&mut self, config: Option<RtaConfig>) {
        match self {
            Self::Demo(runner) => runner.set_rta(config),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.set_rta(config),
        }
    }

    pub(crate) fn rta_frame(
        &self,
        detector: BandDetector,
        weighting: Weighting,
    ) -> Option<RtaFrame> {
        match self {
            Self::Demo(runner) => runner.rta_frame(detector, weighting),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.rta_frame(detector, weighting),
        }
    }

    pub(crate) fn reserve_marker_timestamp(&mut self) -> Result<f64, String> {
        match self {
            Self::Demo(runner) => runner.reserve_marker_timestamp(),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.reserve_marker_timestamp(),
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn add_marker_at(&mut self, label: &str, elapsed_s: f64) -> Result<(), String> {
        match self {
            Self::Demo(runner) => runner.add_marker_at(label, elapsed_s),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.add_marker_at(label, elapsed_s),
        }
        .map_err(|error| error.to_string())
    }

    pub(crate) fn cancel_marker_at(&mut self, elapsed_s: f64) {
        let result = match self {
            Self::Demo(runner) => runner.cancel_marker_at(elapsed_s),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.cancel_marker_at(elapsed_s),
        };
        let _ = result;
    }

    /// Returns the log history for the last `window_s` seconds, or the whole
    /// session with `None`.
    pub(crate) fn log_history(&self, window_s: Option<f64>) -> Option<LogHistory> {
        match self {
            Self::Demo(runner) => runner.log_history(window_s),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.log_history(window_s),
        }
    }

    pub(crate) fn set_log_interval(&mut self, interval_s: f64) -> Result<(), String> {
        match self {
            Self::Demo(runner) => runner.set_log_interval(interval_s),
            #[cfg(feature = "cpal-backend")]
            Self::Live { runner, .. } => runner.set_log_interval(interval_s),
        }
        .map_err(|error| error.to_string())
    }
}

/// Makes `next` the active runner and deletes the replaced recording's
/// journal, which is discarded with it. `next` must already journal.
pub(crate) fn replace_runner(
    active: &RefCell<MeasurementRunner>,
    next: MeasurementRunner,
) -> Result<(), String> {
    let mut replaced = active.replace(next);
    replaced.discard_journal()
}

fn demo_profile() -> InputProfile {
    InputProfile {
        device_id: DEMO_DEVICE_ID.to_owned(),
        device_name: "Demo sine source (1 kHz)".to_owned(),
        channel_index: 0,
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        sample_rate_hz: DEMO_SAMPLE_RATE_HZ as u32,
    }
}

/// Opens a live input with its profile's stored calibration.
#[cfg(feature = "cpal-backend")]
pub(crate) fn open_capture(
    device: &DeviceInfo,
    channel_index: u16,
    sample_rate_hz: u32,
    store: &CalibrationStore,
) -> Result<MeasurementRunner, String> {
    let profile = InputProfile {
        device_id: device.id.clone(),
        device_name: device.display_name.clone(),
        channel_index,
        sample_rate_hz,
    };
    let calibration = store.active(&profile).map(CalibrationRecord::calibration);
    let config = StreamConfig {
        device_id: device.id.clone(),
        channel_index,
        sample_rate_hz,
        buffer_frames: 0,
    };
    CaptureRunner::open(config, calibration)
        .map(|runner| MeasurementRunner::Live {
            runner: Box::new(runner),
            device_id: device.id.clone(),
            device_name: device.display_name.clone(),
            channel_index,
            sample_rate_hz,
        })
        .map_err(|error| format!("Could not open {}: {error}", device.display_name))
}

#[cfg(test)]
mod tests {
    use super::MeasurementRunner;
    use spl_engine::{CalibrationRecord, CalibrationStore};

    #[test]
    // The pattern cannot fail in demo-only builds, which have no live variant.
    #[allow(irrefutable_let_patterns)]
    fn demo_runner_produces_audio_as_time_passes() {
        let MeasurementRunner::Demo(mut demo) =
            MeasurementRunner::demo(&CalibrationStore::unsaved())
        else {
            panic!("the demo constructor returns the demo variant");
        };

        let snapshot = demo.catch_up(std::time::Duration::from_millis(125));

        assert_eq!(snapshot.sample_count, 6_000);
    }

    #[test]
    fn demo_runner_applies_the_demo_profiles_stored_calibration() {
        let profile = MeasurementRunner::demo(&CalibrationStore::unsaved()).input_profile();
        let mut store = CalibrationStore::unsaved();
        store
            .append(CalibrationRecord::manual(
                &profile,
                110.0,
                time::OffsetDateTime::UNIX_EPOCH,
            ))
            .expect("in-memory append");

        let runner = MeasurementRunner::demo(&store);

        assert_eq!(runner.calibration_offset_db(), Some(110.0));
    }
}

/// Describes a calibration in a few words, the dB value on its own line (ribbon note,
/// user 2026-11-06): `Uncalibrated`, `Manual offset\n117.0 dB`,
/// `Calibrator 1000 Hz\n94.0 dB` (calibrator frequency and level) or
/// `Calibrator offset\n117.0 dB`.
pub(crate) fn calibration_status(calibration: Option<&spl_engine::Calibration>) -> String {
    use spl_engine::CalibrationType;
    match calibration {
        Some(c) if c.calibration_type == CalibrationType::Calibrator => {
            match (c.calibrator_level_db, c.calibrator_freq_hz) {
                (Some(level), Some(frequency)) => {
                    format!("Calibrator {frequency:.0} Hz\n{level:.1} dB")
                }
                _ => format!("Calibrator offset\n{:.1} dB", c.offset_db),
            }
        }
        Some(c) if c.calibration_type == CalibrationType::Manual => {
            format!("Manual offset\n{:.1} dB", c.offset_db)
        }
        _ => "Uncalibrated".to_owned(),
    }
}

#[cfg(test)]
mod status_tests {
    use super::calibration_status;
    use spl_engine::{Calibration, CalibrationType};

    #[test]
    fn describes_each_kind_of_calibration() {
        assert_eq!(calibration_status(None), "Uncalibrated");
        assert_eq!(
            calibration_status(Some(&Calibration::uncalibrated())),
            "Uncalibrated"
        );
        let calibrator = Calibration {
            offset_db: 117.01,
            calibration_type: CalibrationType::Calibrator,
            calibrator_level_db: Some(94.0),
            calibrator_freq_hz: Some(1000.0),
            calibrated_utc: None,
        };
        assert_eq!(
            calibration_status(Some(&calibrator)),
            "Calibrator 1000 Hz\n94.0 dB"
        );
        let manual = Calibration {
            calibration_type: CalibrationType::Manual,
            calibrator_level_db: None,
            calibrator_freq_hz: None,
            ..calibrator
        };
        assert_eq!(calibration_status(Some(&manual)), "Manual offset\n117.0 dB");
    }
}

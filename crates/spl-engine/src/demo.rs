use crate::{
    BandDetector, CalibrationCaptureState, Engine, EngineError, JournalError, LogHistory,
    RtaConfig, RtaFrame, SOFTWARE_NAME, SaveMetadata, SaveRange, SegmentAggregate, SessionConfig,
    Snapshot, Weighting,
};
use spl_audio::SineSource;
use spl_dsp::calibration::{CalibrationError, CalibrationProposal};
use spl_session::Calibration;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// Device id recorded for demo sessions; also the demo's calibration profile.
pub const DEMO_DEVICE_ID: &str = "demo.sine-1khz";

/// Most audio one tick generates, so a stalled UI catches up gradually.
const MAX_CATCH_UP: Duration = Duration::from_secs(1);
const BLOCK_SAMPLES: usize = 4_800;

/// Deterministic engine input for demos, UI development, and integration tests.
#[derive(Debug)]
pub struct DemoRunner {
    source: SineSource,
    engine: Engine,
    block: Box<[f32]>,
    sample_rate_hz: f64,
    started: Instant,
    /// Samples generated since `started`; the sample clock is wall-clock time.
    generated_samples: u64,
}

impl DemoRunner {
    /// Creates a 1 kHz demo source at -20 dBFS RMS.
    ///
    /// # Errors
    ///
    /// Returns an error if `sample_rate_hz` is unsupported.
    pub fn new(sample_rate_hz: f64, calibration: Option<Calibration>) -> Result<Self, EngineError> {
        let amplitude = 10.0_f32.powf(-20.0 / 20.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let device_sample_rate_hz = sample_rate_hz as u32;
        let session_calibration = calibration
            .clone()
            .unwrap_or_else(Calibration::uncalibrated);
        let mut engine = Engine::new(sample_rate_hz, calibration)?;
        engine.start_session(
            SessionConfig {
                name: "Demo measurement".to_owned(),
                software: format!("{SOFTWARE_NAME} {}", env!("CARGO_PKG_VERSION")),
                device: spl_session::Device {
                    id: DEMO_DEVICE_ID.to_owned(),
                    name: "Demo sine source (1 kHz)".to_owned(),
                    backend: "demo".to_owned(),
                    channel: 1,
                    sample_rate_hz: device_sample_rate_hz,
                },
                calibration: session_calibration,
                log_interval_s: 1.0,
                band_log: None,
                author: None,
                tags: vec!["demo".to_owned()],
                description: "Deterministic 1 kHz demo sine measurement.".to_owned(),
            },
            OffsetDateTime::now_utc(),
        )?;
        Ok(Self {
            source: SineSource::new(1_000.0, sample_rate_hz, amplitude),
            engine,
            block: vec![0.0; BLOCK_SAMPLES].into_boxed_slice(),
            sample_rate_hz,
            started: Instant::now(),
            generated_samples: 0,
        })
    }

    /// Generates the audio due since the runner was created and returns the
    /// snapshot, so session time follows the wall clock like a real input.
    pub fn tick(&mut self) -> Snapshot {
        self.catch_up(self.started.elapsed())
    }

    /// Generates the samples due `elapsed` after the runner was created, at
    /// most [`MAX_CATCH_UP`] worth per call, and returns the snapshot.
    ///
    /// [`Self::tick`] passes the wall-clock time; tests pass chosen times.
    pub fn catch_up(&mut self, elapsed: Duration) -> Snapshot {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let due_samples = (elapsed.as_secs_f64() * self.sample_rate_hz).floor() as u64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let limit = (MAX_CATCH_UP.as_secs_f64() * self.sample_rate_hz) as u64;
        let mut pending = due_samples
            .saturating_sub(self.generated_samples)
            .min(limit);
        while pending > 0 {
            let count = usize::try_from(pending)
                .unwrap_or(BLOCK_SAMPLES)
                .min(BLOCK_SAMPLES);
            let block = &mut self.block[..count];
            self.source.fill(block);
            self.engine.process_block(block);
            self.generated_samples += count as u64;
            pending -= count as u64;
        }
        self.engine.snapshot()
    }

    /// Clears level accumulators while preserving the deterministic input source.
    pub fn reset(&mut self) {
        self.engine.reset();
    }

    /// Replaces the active calibration without resetting the recording.
    pub fn set_calibration(&mut self, calibration: Option<Calibration>) {
        self.engine.set_calibration(calibration);
    }

    /// Applies a manually entered offset without resetting the recording.
    pub fn set_manual_calibration(&mut self, offset_db: f64) {
        self.engine.set_manual_calibration(offset_db);
    }

    /// Returns the active calibration, if one has been applied.
    #[must_use]
    pub fn calibration(&self) -> Option<&Calibration> {
        self.engine.calibration()
    }

    /// Returns the active demo calibration offset, if one has been applied.
    #[must_use]
    pub fn calibration_offset_db(&self) -> Option<f64> {
        self.engine.calibration_offset_db()
    }

    /// Starts an eight-second raw calibration capture.
    ///
    /// # Errors
    ///
    /// Returns an error when the target frequency is invalid for the stream.
    pub fn start_calibration_capture(
        &mut self,
        calibrator_level_db: f64,
        target_frequency_hz: f64,
    ) -> Result<(), CalibrationError> {
        self.engine
            .start_calibration_capture(calibrator_level_db, target_frequency_hz)
    }

    /// Returns automatic calibration capture progress or result.
    #[must_use]
    pub fn calibration_capture_state(&self) -> CalibrationCaptureState {
        self.engine.calibration_capture_state()
    }

    /// Commits the completed automatic proposal to this runner's active session.
    ///
    /// # Errors
    ///
    /// Returns an error when no successful proposal is awaiting approval.
    pub fn save_calibration_proposal(&mut self) -> Result<CalibrationProposal, CalibrationError> {
        self.engine.save_calibration_proposal()
    }

    /// Returns the calibrated SPL reading for a full-scale sine wave.
    #[must_use]
    pub fn full_scale_sine_spl_db(&self) -> Option<f64> {
        self.engine.full_scale_sine_spl_db()
    }

    /// Adds a user marker to the active deterministic demo session.
    ///
    /// # Errors
    ///
    /// Returns an error when the demo session is unavailable.
    pub fn add_marker(&mut self, label: impl Into<String>) -> Result<(), EngineError> {
        self.engine.add_marker(label)
    }

    /// Starts journaling the session; see [`Engine::start_journal`].
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be written.
    pub fn start_journal(
        &mut self,
        sessions_directory: &std::path::Path,
    ) -> Result<(), JournalError> {
        self.engine.start_journal(sessions_directory)
    }

    /// Writes the journal; see [`Engine::flush_journal`].
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be written.
    pub fn flush_journal(&mut self) -> Result<(), JournalError> {
        self.engine.flush_journal()
    }

    /// Deletes the journal; see [`Engine::discard_journal`].
    ///
    /// # Errors
    ///
    /// Returns an error when the journal folder cannot be removed.
    pub fn discard_journal(&mut self) -> Result<(), JournalError> {
        self.engine.discard_journal()
    }

    /// Continues a journaled session, paused; see [`Engine::recover_session`].
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be recovered.
    pub fn recover_session(&mut self, directory: &std::path::Path) -> Result<(), JournalError> {
        self.engine
            .recover_session(directory, OffsetDateTime::now_utc())
    }

    /// Returns whether anything was recorded or marked since the last save.
    #[must_use]
    pub fn has_unsaved_changes(&self) -> bool {
        self.engine.has_unsaved_changes()
    }

    /// Returns the combined values a save of `range` would contain now.
    ///
    /// # Errors
    ///
    /// Returns an error when the markers do not delimit a range.
    pub fn range_summary(&self, range: SaveRange) -> Result<SegmentAggregate, EngineError> {
        self.engine.range_summary(range)
    }

    /// Starts a fresh session; see [`Engine::new_session`].
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active.
    pub fn new_session(&mut self) -> Result<(), EngineError> {
        self.engine.new_session(OffsetDateTime::now_utc())
    }

    /// Changes a marker's label and note; see [`Engine::edit_marker`].
    ///
    /// # Errors
    ///
    /// Returns an error when the session has no such marker.
    pub fn edit_marker(&mut self, id: u64, label: &str, note: &str) -> Result<(), EngineError> {
        self.engine.edit_marker(id, label, note)
    }

    /// Turns the analyser on or off; see [`Engine::set_rta`].
    pub fn set_rta(&mut self, config: Option<RtaConfig>) {
        self.engine.set_rta(config);
    }

    /// Returns the analyser's band levels; see [`Engine::rta_frame`].
    #[must_use]
    pub fn rta_frame(&self, detector: BandDetector, weighting: Weighting) -> Option<RtaFrame> {
        self.engine.rta_frame(detector, weighting)
    }

    /// Pauses the recording; see [`Engine::pause`].
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active or it is already paused.
    pub fn pause(&mut self) -> Result<(), EngineError> {
        self.engine.pause()
    }

    /// Resumes a paused recording; see [`Engine::resume`].
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active or it is not paused.
    pub fn resume(&mut self) -> Result<(), EngineError> {
        self.engine.resume()
    }

    /// Returns whether the recording is paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.engine.is_paused()
    }

    /// Reserves the current session time for a marker whose label is entered later.
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active or another marker label is pending.
    pub fn reserve_marker_timestamp(&mut self) -> Result<f64, EngineError> {
        self.engine.reserve_marker_timestamp()
    }

    /// Adds a marker at a timestamp previously reserved by this runner.
    ///
    /// # Errors
    ///
    /// Returns an error when the timestamp was not reserved by the active session.
    pub fn add_marker_at(
        &mut self,
        label: impl Into<String>,
        elapsed_s: f64,
    ) -> Result<(), EngineError> {
        self.engine.add_marker_at(label, elapsed_s)
    }

    /// Cancels a marker timestamp reservation without changing the recording.
    ///
    /// # Errors
    ///
    /// Returns an error when the timestamp was not reserved by the active session.
    pub fn cancel_marker_at(&mut self, elapsed_s: f64) -> Result<(), EngineError> {
        self.engine.cancel_marker_at(elapsed_s)
    }

    /// Returns the active demo session's log history.
    #[must_use]
    pub fn log_history(&self, window_s: Option<f64>) -> Option<LogHistory> {
        self.engine.log_history(window_s)
    }

    /// Chooses the recorded bands; see [`Engine::set_band_log`].
    pub fn set_band_log(&mut self, bands: Option<RtaConfig>) {
        self.engine.set_band_log(bands);
    }

    /// Changes the active demo session's logging interval.
    ///
    /// # Errors
    ///
    /// Returns an error when the interval is invalid or the session is unavailable.
    pub fn set_log_interval(&mut self, interval_s: f64) -> Result<(), EngineError> {
        self.engine.set_log_interval(interval_s)
    }

    /// Writes the current deterministic demo session to a portable bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination cannot be written.
    pub fn save_measurement(&self, path: &std::path::Path) -> Result<(), EngineError> {
        self.engine.save_measurement(path)
    }

    /// Writes the current demo session with metadata chosen for this export.
    ///
    /// # Errors
    ///
    /// Returns an error when the active session cannot be saved.
    pub fn save_measurement_with_metadata(
        &self,
        path: &std::path::Path,
        metadata: SaveMetadata,
    ) -> Result<(), EngineError> {
        self.engine.save_measurement_with_metadata(path, metadata)
    }
}

use crate::{
    BandDetector, CalibrationCaptureState, Engine, EngineError, JournalError, LogHistory,
    RtaConfig, RtaFrame, SOFTWARE_NAME, SaveMetadata, SaveRange, SegmentAggregate, SessionConfig,
    Snapshot, Weighting,
};
use spl_audio::{AudioSource, CpalSource, SampleReceiver, StreamConfig, sample_queue};
use spl_dsp::calibration::{CalibrationError, CalibrationProposal};
use spl_session::Calibration;
use time::OffsetDateTime;

/// A running cpal capture stream connected to the measurement engine.
pub struct CaptureRunner {
    source: CpalSource,
    receiver: SampleReceiver,
    engine: Engine,
    scratch: Box<[f32]>,
}

impl CaptureRunner {
    /// Opens the requested input and constructs a measurement engine at its rate.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected device or stream configuration cannot
    /// be opened, or the configured rate is unsupported by the DSP layer.
    pub fn open(
        config: StreamConfig,
        calibration: Option<Calibration>,
    ) -> Result<Self, CaptureError> {
        let session_calibration = calibration
            .clone()
            .unwrap_or_else(Calibration::uncalibrated);
        let mut engine = Engine::new(f64::from(config.sample_rate_hz), calibration)?;
        engine.start_session(
            SessionConfig {
                band_log: None,
                name: "Live measurement".to_owned(),
                software: format!("{SOFTWARE_NAME} {}", env!("CARGO_PKG_VERSION")),
                device: spl_session::Device {
                    id: config.device_id.clone(),
                    name: "Live input".to_owned(),
                    backend: "cpal".to_owned(),
                    channel: u32::from(config.channel_index) + 1,
                    sample_rate_hz: config.sample_rate_hz,
                },
                calibration: session_calibration,
                log_interval_s: 1.0,
                author: None,
                tags: Vec::new(),
                description: String::new(),
            },
            OffsetDateTime::now_utc(),
        )?;
        let (sink, receiver) = sample_queue(192_000);
        let mut source = CpalSource::new();
        source.open(config, sink).map_err(CaptureError::Audio)?;
        Ok(Self {
            source,
            receiver,
            engine,
            scratch: vec![0.0; 4_800].into_boxed_slice(),
        })
    }

    /// Drains all currently queued samples and returns the latest snapshot.
    pub fn tick(&mut self) -> Snapshot {
        while self
            .engine
            .process_available(&mut self.receiver, &mut self.scratch)
            == self.scratch.len()
        {}
        self.engine.snapshot()
    }

    /// Returns callback samples dropped because the queue was full.
    #[must_use]
    pub fn dropped_samples(&self) -> u64 {
        self.receiver.dropped_samples()
    }

    /// Returns asynchronous cpal stream errors observed since opening.
    #[must_use]
    pub fn stream_error_count(&self) -> u64 {
        self.source.stream_error_count()
    }

    /// Stops the capture stream.
    pub fn close(&mut self) {
        self.source.close();
    }

    /// Clears level accumulators while leaving the capture stream running.
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

    /// Returns the active input calibration offset.
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

    /// Adds a user marker to the active measurement session.
    ///
    /// # Errors
    ///
    /// Returns an error when the session is unavailable.
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

    /// Returns the active session's log history.
    #[must_use]
    pub fn log_history(&self, window_s: Option<f64>) -> Option<LogHistory> {
        self.engine.log_history(window_s)
    }

    /// Chooses the recorded bands; see [`Engine::set_band_log`].
    pub fn set_band_log(&mut self, bands: Option<RtaConfig>) {
        self.engine.set_band_log(bands);
    }

    /// Changes the active session's logging interval.
    ///
    /// # Errors
    ///
    /// Returns an error when the interval is invalid or the session is unavailable.
    pub fn set_log_interval(&mut self, interval_s: f64) -> Result<(), EngineError> {
        self.engine.set_log_interval(interval_s)
    }

    /// Writes the current capture session to a portable bundle.
    ///
    /// # Errors
    ///
    /// Returns an error when the active session cannot be saved.
    pub fn save_measurement(&self, path: &std::path::Path) -> Result<(), EngineError> {
        self.engine.save_measurement(path)
    }

    /// Writes the current capture session with metadata chosen for this export.
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

/// Failure opening a capture runner.
#[derive(Debug)]
pub enum CaptureError {
    /// The cpal source could not open the configured input.
    Audio(spl_audio::AudioError),
    /// The requested rate cannot be processed by the DSP engine.
    Engine(EngineError),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Audio(error) => error.fmt(formatter),
            Self::Engine(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CaptureError {}

impl From<EngineError> for CaptureError {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}

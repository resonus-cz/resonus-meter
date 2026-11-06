mod capture;
mod journal;
mod rta;

use crate::recorder::{self, SessionConfig};
use crate::{
    EngineError, LogHistory, SaveMetadata, SaveRange, SegmentAggregate, Snapshot, WeightingLevels,
};
use capture::CalibrationCapture;
use spl_audio::SampleReceiver;
use spl_dsp::{
    detector::{LevelAccumulator, TimeDetector, TimeWeighting},
    units::{CLIP_LEVEL, calibrated_level_db},
    weighting::{UnsupportedSampleRate, Weighting, WeightingFilter},
};
use spl_session::{Calibration, CalibrationType, MarkerKind};
use time::OffsetDateTime;

/// Single-threaded measurement pipeline for a negotiated mono stream.
#[derive(Debug)]
pub struct Engine {
    sample_rate_hz: f64,
    calibration: Option<Calibration>,
    calibration_capture: Option<CalibrationCapture>,
    a: Chain,
    c: Chain,
    z: Chain,
    clipped: bool,
    sample_count: u64,
    recorder: Option<recorder::SessionRecorder>,
    journal: Option<spl_session::JournalWriter>,
    /// Where the session journals, kept so Reset can start a fresh one.
    sessions_directory: Option<std::path::PathBuf>,
    /// A journal failure outside [`Self::flush_journal`], reported by its next call.
    journal_error: Option<crate::JournalError>,
    rta: Option<rta::Rta>,
}

impl Engine {
    /// Creates an engine for the actual stream sample rate.
    ///
    /// # Errors
    ///
    /// Returns an error when the stream rate is unsupported by the DSP layer.
    pub fn new(sample_rate_hz: f64, calibration: Option<Calibration>) -> Result<Self, EngineError> {
        Ok(Self {
            sample_rate_hz,
            calibration,
            calibration_capture: None,
            a: Chain::new(Weighting::A, sample_rate_hz)?,
            c: Chain::new(Weighting::C, sample_rate_hz)?,
            z: Chain::new(Weighting::Z, sample_rate_hz)?,
            clipped: false,
            sample_count: 0,
            recorder: None,
            journal: None,
            sessions_directory: None,
            journal_error: None,
            rta: None,
        })
    }

    /// Processes a block delivered by the engine thread.
    ///
    /// Callback delivery remains lock-free; automatic calibration copies raw
    /// samples here into storage preallocated when the user starts capture.
    pub fn process_block(&mut self, samples: &[f32]) {
        for &sample in samples {
            let clipped = sample.abs() >= CLIP_LEVEL;
            self.clipped |= clipped;
            let sample = f64::from(sample);
            let measuring = !self.is_paused();
            let a_sample = self.a.process(sample, measuring);
            let c_sample = self.c.process(sample, measuring);
            let z_sample = self.z.process(sample, measuring);
            self.sample_count += 1;
            if let Some(rta) = &mut self.rta {
                rta.bank.process(sample, measuring);
            }
            if let Some(recorder) = &mut self.recorder {
                recorder.process_sample(
                    sample,
                    a_sample,
                    c_sample,
                    z_sample,
                    clipped,
                    [self.a.fast_dbfs(), self.c.fast_dbfs(), self.z.fast_dbfs()],
                    active_offset_db(self.calibration.as_ref()),
                );
            }
        }
        self.capture_raw_samples(samples);
    }

    /// Drains queued input into `scratch` and processes it without allocating.
    ///
    /// Returns the number of samples processed.
    pub fn process_available(
        &mut self,
        receiver: &mut SampleReceiver,
        scratch: &mut [f32],
    ) -> usize {
        let count = receiver.pop_into(scratch);
        self.process_block(&scratch[..count]);
        count
    }

    /// Clears display accumulators and overload state without stopping input.
    pub fn reset(&mut self) {
        if let Some(recorder) = &mut self.recorder {
            recorder.reset();
        }
        self.a.reset();
        self.c.reset();
        self.z.reset();
        self.clipped = false;
        self.sample_count = 0;
        self.reset_rta();
        self.restart_journal();
    }

    fn reset_rta(&mut self) {
        if let Some(rta) = &mut self.rta {
            rta.bank.reset_leq();
        }
    }

    /// Discards the active session and starts a fresh one at `started_utc`
    /// with the same input, calibration and log interval; it opens with a
    /// `start` marker and journals into a new folder.
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active.
    pub fn new_session(&mut self, started_utc: OffsetDateTime) -> Result<(), EngineError> {
        let recorder = self.recorder.as_ref().ok_or(EngineError::NoActiveSession)?;
        let config = recorder.config().clone();
        self.recorder = Some(recorder::SessionRecorder::new(
            config,
            self.sample_rate_hz,
            started_utc,
        ));
        self.a.reset();
        self.c.reset();
        self.z.reset();
        self.clipped = false;
        self.sample_count = 0;
        self.reset_rta();
        self.restart_journal();
        Ok(())
    }

    /// Chooses the bands whose `LZeq` the session records per log interval and
    /// segment, or none (ADR 0015). The choice applies from the next Reset or
    /// New Session, or at once while nothing has been recorded yet.
    pub fn set_band_log(&mut self, bands: Option<crate::RtaConfig>) {
        if let Some(recorder) = &mut self.recorder {
            recorder.set_band_log(bands);
        }
    }

    /// Replaces the active device-profile calibration without resetting.
    pub fn set_calibration(&mut self, calibration: Option<Calibration>) {
        self.calibration = calibration;
        let calibration = self.calibration_for_session();
        if let Some(recorder) = &mut self.recorder {
            recorder.set_calibration(calibration);
        }
    }

    /// Applies a manually entered offset, recorded as a `manual` calibration.
    pub fn set_manual_calibration(&mut self, offset_db: f64) {
        self.set_calibration(Some(Calibration {
            offset_db,
            calibration_type: CalibrationType::Manual,
            calibrator_level_db: None,
            calibrator_freq_hz: None,
            calibrated_utc: Some(OffsetDateTime::now_utc()),
        }));
    }

    /// Returns the active calibration, if one has been supplied.
    #[must_use]
    pub fn calibration(&self) -> Option<&Calibration> {
        self.calibration.as_ref()
    }

    /// Returns the active calibration offset, if one has been supplied.
    #[must_use]
    pub fn calibration_offset_db(&self) -> Option<f64> {
        self.calibration
            .as_ref()
            .map(|calibration| calibration.offset_db)
    }

    /// Returns a copy of the latest display state.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let offset_db = active_offset_db(self.calibration.as_ref());
        Snapshot {
            a: self.a.levels(offset_db),
            c: self.c.levels(offset_db),
            z: self.z.levels(offset_db),
            clipped: self.clipped,
            calibrated: self.calibration.is_some(),
            sample_count: self.sample_count,
            paused: self.is_paused(),
        }
    }

    /// Pauses the recording with a `pause` marker.
    ///
    /// The input keeps running: Fast and Slow stay live, while Leq, peak, Fast
    /// maximum and the log stop until [`Self::resume`]. Session time keeps
    /// counting, so later timestamps stay correct.
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active or it is already paused.
    pub fn pause(&mut self) -> Result<(), EngineError> {
        let recorder = self.recorder.as_mut().ok_or(EngineError::NoActiveSession)?;
        if recorder.is_paused() {
            return Err(EngineError::AlreadyPaused);
        }
        recorder.pause(active_offset_db(self.calibration.as_ref()));
        self.journal_marker_change();
        Ok(())
    }

    /// Resumes a paused recording with a `resume` marker.
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active or it is not paused.
    pub fn resume(&mut self) -> Result<(), EngineError> {
        let recorder = self.recorder.as_mut().ok_or(EngineError::NoActiveSession)?;
        if !recorder.is_paused() {
            return Err(EngineError::NotPaused);
        }
        recorder.resume(active_offset_db(self.calibration.as_ref()));
        self.journal_marker_change();
        Ok(())
    }

    /// Returns whether the active recording is paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.recorder
            .as_ref()
            .is_some_and(recorder::SessionRecorder::is_paused)
    }

    /// Starts a UI-independent session recorder for subsequent processed samples.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested log interval is not finite and positive.
    pub fn start_session(
        &mut self,
        config: SessionConfig,
        started_utc: OffsetDateTime,
    ) -> Result<(), EngineError> {
        if !config.log_interval_s.is_finite() || config.log_interval_s <= 0.0 {
            return Err(EngineError::InvalidLogInterval);
        }
        self.recorder = Some(recorder::SessionRecorder::new(
            config,
            self.sample_rate_hz,
            started_utc,
        ));
        Ok(())
    }

    /// Closes the active segment with a user marker and starts the next one.
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active.
    pub fn add_marker(&mut self, label: impl Into<String>) -> Result<(), EngineError> {
        let recorder = self.recorder.as_mut().ok_or(EngineError::NoActiveSession)?;
        recorder.marker(
            MarkerKind::User,
            label,
            active_offset_db(self.calibration.as_ref()),
        );
        self.journal_marker_change();
        Ok(())
    }

    /// Reserves the current session time for a marker whose label is entered later.
    ///
    /// The reservation snapshots the recorder's segment state so audio processed
    /// while the label dialog is open belongs to the following segment.
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active or a marker is already pending.
    pub fn reserve_marker_timestamp(&mut self) -> Result<f64, EngineError> {
        let recorder = self.recorder.as_mut().ok_or(EngineError::NoActiveSession)?;
        recorder
            .reserve_marker_timestamp()
            .ok_or(EngineError::MarkerTimestampAlreadyReserved)
    }

    /// Closes the active segment at a previously reserved session timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error when no active session reserved `elapsed_s`.
    pub fn add_marker_at(
        &mut self,
        label: impl Into<String>,
        elapsed_s: f64,
    ) -> Result<(), EngineError> {
        let recorder = self.recorder.as_mut().ok_or(EngineError::NoActiveSession)?;
        recorder
            .marker_at(
                label,
                elapsed_s,
                active_offset_db(self.calibration.as_ref()),
            )
            .map_err(|()| EngineError::InvalidMarkerTimestamp)?;
        self.journal_marker_change();
        Ok(())
    }

    /// Changes the label and note of marker `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active or it has no such marker.
    pub fn edit_marker(&mut self, id: u64, label: &str, note: &str) -> Result<(), EngineError> {
        let recorder = self.recorder.as_mut().ok_or(EngineError::NoActiveSession)?;
        if !recorder.edit_marker(id, label, note) {
            return Err(EngineError::UnknownMarker(id));
        }
        self.journal_marker_change();
        Ok(())
    }

    /// Cancels a marker timestamp reservation without changing the recording.
    ///
    /// # Errors
    ///
    /// Returns an error when no active session reserved `elapsed_s`.
    pub fn cancel_marker_at(&mut self, elapsed_s: f64) -> Result<(), EngineError> {
        let recorder = self.recorder.as_mut().ok_or(EngineError::NoActiveSession)?;
        recorder
            .cancel_marker_at(elapsed_s)
            .map_err(|()| EngineError::InvalidMarkerTimestamp)
    }

    /// Returns the active session's Fast-level history for a view of the last
    /// `window_s` seconds, or of the whole session with `None`.
    ///
    /// Memory holds the last hour at full resolution; a longer view gets an
    /// overview of at most a few thousand points, each the highest levels of
    /// the rows it combines (ADR 0014).
    #[must_use]
    pub fn log_history(&self, window_s: Option<f64>) -> Option<LogHistory> {
        self.recorder.as_ref().map(|recorder| {
            let (records, record_count) = recorder.plot_rows(window_s);
            LogHistory {
                records,
                record_count,
                markers: recorder.markers().to_vec(),
                duration_s: recorder.current_elapsed_s(),
                interval_s: recorder.log_interval_s(),
            }
        })
    }

    /// Changes the active session's log interval and creates an interval marker.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid interval or when no session is active.
    pub fn set_log_interval(&mut self, interval_s: f64) -> Result<(), EngineError> {
        if !interval_s.is_finite() || interval_s <= 0.0 {
            return Err(EngineError::InvalidLogInterval);
        }
        let recorder = self.recorder.as_mut().ok_or(EngineError::NoActiveSession)?;
        recorder.set_log_interval(interval_s, active_offset_db(self.calibration.as_ref()));
        self.journal_marker_change();
        Ok(())
    }

    /// Writes the current session to a portable measurement bundle.
    ///
    /// Saving takes a completed snapshot of the active segment and leaves
    /// measurement processing and recording running.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no session or the bundle cannot be written.
    pub fn save_measurement(&self, path: &std::path::Path) -> Result<(), EngineError> {
        let recorder = self.recorder.as_ref().ok_or(EngineError::NoActiveSession)?;
        recorder.write(
            path,
            active_offset_db(self.calibration.as_ref()),
            None,
            self.journal.as_ref(),
        )
    }

    /// Writes the current session with metadata chosen for this export.
    ///
    /// Saving takes a completed snapshot of the active segment and leaves
    /// measurement processing and recording running.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no session or the bundle cannot be written.
    pub fn save_measurement_with_metadata(
        &self,
        path: &std::path::Path,
        metadata: SaveMetadata,
    ) -> Result<(), EngineError> {
        let recorder = self.recorder.as_ref().ok_or(EngineError::NoActiveSession)?;
        recorder.write(
            path,
            active_offset_db(self.calibration.as_ref()),
            Some(metadata),
            self.journal.as_ref(),
        )
    }

    /// Returns the combined values a save of `range` would contain now.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no session or the markers do not
    /// delimit a range.
    pub fn range_summary(&self, range: SaveRange) -> Result<SegmentAggregate, EngineError> {
        let recorder = self.recorder.as_ref().ok_or(EngineError::NoActiveSession)?;
        recorder
            .range_summary(range, active_offset_db(self.calibration.as_ref()))
            .map_err(EngineError::InvalidRange)
    }

    fn calibration_for_session(&self) -> Calibration {
        self.calibration
            .clone()
            .unwrap_or_else(Calibration::uncalibrated)
    }
}

/// Offset applied to dBFS levels; uncalibrated input reads in dBFS (C = 0).
fn active_offset_db(calibration: Option<&Calibration>) -> f64 {
    calibration.map_or(0.0, |calibration| calibration.offset_db)
}

#[derive(Debug)]
struct Chain {
    filter: WeightingFilter,
    fast: TimeDetector,
    slow: TimeDetector,
    accumulator: LevelAccumulator,
    /// Largest Fast level while measuring; the detector's own maximum would
    /// include paused input.
    fast_max_dbfs: f64,
}

impl Chain {
    fn new(weighting: Weighting, sample_rate_hz: f64) -> Result<Self, UnsupportedSampleRate> {
        Ok(Self {
            filter: WeightingFilter::new(weighting, sample_rate_hz)?,
            fast: TimeDetector::new(sample_rate_hz, TimeWeighting::Fast),
            slow: TimeDetector::new(sample_rate_hz, TimeWeighting::Slow),
            accumulator: LevelAccumulator::default(),
            fast_max_dbfs: f64::NEG_INFINITY,
        })
    }

    /// Filters `sample`; `measuring` adds it to Leq, peak and Fast maximum.
    fn process(&mut self, sample: f64, measuring: bool) -> f64 {
        let weighted = self.filter.process(sample);
        let fast_dbfs = self.fast.process(weighted);
        self.slow.process(weighted);
        if measuring {
            self.accumulator.process(weighted);
            self.fast_max_dbfs = self.fast_max_dbfs.max(fast_dbfs);
        }
        weighted
    }

    fn fast_dbfs(&self) -> f64 {
        self.fast.level_dbfs().unwrap_or(f64::NEG_INFINITY)
    }

    fn reset(&mut self) {
        self.filter.reset();
        self.fast.reset();
        self.slow.reset();
        self.accumulator.reset();
        self.fast_max_dbfs = f64::NEG_INFINITY;
    }

    fn levels(&self, offset_db: f64) -> WeightingLevels {
        WeightingLevels {
            fast_db: self.fast.level_dbfs().map_or(f64::NEG_INFINITY, |level| {
                calibrated_level_db(level, offset_db)
            }),
            slow_db: self.slow.level_dbfs().map_or(f64::NEG_INFINITY, |level| {
                calibrated_level_db(level, offset_db)
            }),
            leq_db: self
                .accumulator
                .leq_dbfs()
                .map(|level| calibrated_level_db(level, offset_db)),
            peak_db: self
                .accumulator
                .peak_dbfs()
                .map(|level| calibrated_level_db(level, offset_db)),
            fast_max_db: calibrated_level_db(self.fast_max_dbfs, offset_db),
        }
    }
}

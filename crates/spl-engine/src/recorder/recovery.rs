//! Persisting a running session so it can continue after a crash (ADR 0010).

use serde::{Deserialize, Serialize};
use spl_session::{LogRecord, Marker, MarkerKind, Segment};
use time::OffsetDateTime;

use super::bands::{BandRecorder, BandRecorderState};
use super::levels::{LevelsState, SegmentLevels};
use super::{SessionConfig, SessionRecorder};

/// Label of the pause marker placed where a recovered recording stopped.
const INTERRUPTION_LABEL: &str = "Pause (recording interrupted)";

/// Exact recorder state at a journal flush; the log rows live in `log.csv`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RecorderState {
    config: SessionConfig,
    #[serde(with = "time::serde::rfc3339")]
    started_utc: OffsetDateTime,
    sample_rate_hz: f64,
    session_sample_count: u64,
    recorded_samples: u64,
    next_marker_id: u64,
    start_marker_id: u64,
    start_s: f64,
    segment_clipped: bool,
    paused: bool,
    pending_marker_id: Option<u64>,
    /// Number of `log.csv` rows that belong to this state.
    pub(crate) log_rows: usize,
    open_segment: [LevelsState; 3],
    markers: Vec<Marker>,
    segments: Vec<Segment>,
    /// The band log, when the session records one.
    #[serde(default)]
    pub(crate) bands: Option<BandRecorderState>,
}

impl RecorderState {
    pub(crate) const fn sample_rate_hz(&self) -> f64 {
        self.sample_rate_hz
    }

    pub(crate) const fn config(&self) -> &SessionConfig {
        &self.config
    }

    pub(crate) const fn started_utc(&self) -> OffsetDateTime {
        self.started_utc
    }

    /// Whether samples were recorded or markers added beyond the first.
    pub(crate) fn holds_data(&self) -> bool {
        self.recorded_samples > 0 || self.markers.len() > 1
    }

    /// Elapsed session time when the state was taken.
    #[allow(clippy::cast_precision_loss)]
    pub(crate) fn elapsed_s(&self) -> f64 {
        self.session_sample_count as f64 / self.sample_rate_hz
    }
}

impl SessionRecorder {
    /// Returns the state a journal flush persists, whose `log.csv` will hold
    /// `log_rows` rows and `rta_log.csv` `band_rows`. A marker whose label is
    /// still being entered is not part of it.
    pub(crate) fn recovery_state(&self, log_rows: usize, band_rows: usize) -> RecorderState {
        RecorderState {
            config: self.config.clone(),
            started_utc: self.started_utc,
            sample_rate_hz: self.sample_rate_hz,
            session_sample_count: self.session_sample_count,
            recorded_samples: self.recorded_samples,
            next_marker_id: self.next_marker_id,
            start_marker_id: self.start_marker_id,
            start_s: self.start_s,
            segment_clipped: self.segment_clipped,
            paused: self.paused,
            pending_marker_id: self.pending_marker_id,
            log_rows,
            open_segment: [self.a.state(), self.c.state(), self.z.state()],
            markers: self.markers.clone(),
            segments: self.segments.clone(),
            bands: self.bands.as_ref().map(|bands| bands.state(band_rows)),
        }
    }

    /// Rebuilds a recorder from `state`, paused. Its log rows stay in the
    /// journal; [`Self::restore_row`] hands them to the Log view.
    ///
    /// A recording that was running gets a `pause` marker where the state was
    /// taken. The session clock then moves on to `now_utc`, so later markers
    /// and rows keep true wall-clock timestamps.
    pub(crate) fn restore(state: RecorderState, now_utc: OffsetDateTime) -> Self {
        let [a, c, z] = state.open_segment;
        let calibration_offset_db = state.config.calibration.offset_db;
        let marker_revision = state.markers.len() as u64;
        let mut recorder = Self {
            next_log_sample: state.session_sample_count,
            session_sample_count: state.session_sample_count,
            recorded_samples: state.recorded_samples,
            next_marker_id: state.next_marker_id,
            start_marker_id: state.start_marker_id,
            start_s: state.start_s,
            segments: state.segments,
            markers: state.markers,
            a: SegmentLevels::from_state(a),
            c: SegmentLevels::from_state(c),
            z: SegmentLevels::from_state(z),
            bands: state
                .bands
                .map(|bands| BandRecorder::restore(bands, state.sample_rate_hz)),
            segment_clipped: state.segment_clipped,
            interval_clipped: false,
            pending_marker_id: state.pending_marker_id,
            reserved_marker: None,
            paused: state.paused,
            // Recovered markers are unsaved changes.
            marker_revision,
            saved: std::cell::Cell::new(None),
            ..Self::new(state.config, state.sample_rate_hz, state.started_utc)
        };
        if !recorder.paused {
            recorder.marker(MarkerKind::Pause, INTERRUPTION_LABEL, calibration_offset_db);
            recorder.paused = true;
        }
        let gap_s =
            (now_utc - recorder.started_utc).as_seconds_f64() - recorder.current_elapsed_s();
        if gap_s > 0.0 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let gap_samples = (gap_s * recorder.sample_rate_hz).round() as u64;
            recorder.session_sample_count += gap_samples;
        }
        recorder
    }

    /// Adds a journaled row of the recovered session to the Log view.
    pub(crate) fn restore_row(&mut self, row: LogRecord) {
        self.history.push(row);
    }
}

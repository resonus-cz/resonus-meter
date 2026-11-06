//! UI-independent recording of engine samples into a portable session.

mod bands;
mod history;
mod levels;
mod recovery;
mod reserved_marker;
mod saving;

pub(crate) use levels::LevelsState;
pub(crate) use recovery::RecorderState;

use bands::BandRecorder;
use history::PlotHistory;
use levels::SegmentLevels;
use reserved_marker::ReservedMarker;
use time::{Duration, OffsetDateTime};

use spl_dsp::units::calibrated_level_db;
use spl_session::{Calibration, Device, FinalValues, LogRecord, Marker, MarkerKind, Segment};

use serde::{Deserialize, Serialize};
use std::cell::Cell;

/// Configuration for a measurement session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    /// User-visible measurement name.
    pub name: String,
    /// Application name and version saved in the bundle.
    pub software: String,
    /// Input device that provides the samples.
    pub device: Device,
    /// Initial calibration for the active input profile.
    pub calibration: Calibration,
    /// Fast-level log interval in seconds.
    pub log_interval_s: f64,
    /// Optional measurement author.
    pub author: Option<String>,
    /// Searchable measurement tags.
    pub tags: Vec<String>,
    /// User-provided Markdown description.
    pub description: String,
    /// Bands whose `LZeq` is recorded per log interval and segment, if any.
    /// A change applies from the next Reset.
    #[serde(default)]
    pub band_log: Option<crate::RtaConfig>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionRecorder {
    config: SessionConfig,
    started_utc: OffsetDateTime,
    sample_rate_hz: f64,
    next_log_sample: u64,
    session_sample_count: u64,
    /// Samples that entered the measurement, excluding paused ones.
    recorded_samples: u64,
    next_marker_id: u64,
    start_marker_id: u64,
    start_s: f64,
    segments: Vec<Segment>,
    markers: Vec<Marker>,
    /// Log rows not yet in the journal; without a journal, all of them.
    unjournaled: Vec<LogRecord>,
    /// The rows the Log view needs, bounded for long sessions.
    history: PlotHistory,
    a: SegmentLevels,
    c: SegmentLevels,
    z: SegmentLevels,
    bands: Option<BandRecorder>,
    segment_clipped: bool,
    interval_clipped: bool,
    pending_marker_id: Option<u64>,
    reserved_marker: Option<ReservedMarker>,
    /// While paused, samples advance the session clock only.
    paused: bool,
    /// Counts marker additions and edits, to notice changes since a save.
    marker_revision: u64,
    /// Recorded samples and marker revision at the last save, if any.
    saved: Cell<Option<(u64, u64)>>,
}

#[derive(Debug, Clone, Copy)]
struct ClosedSegment {
    end_s: f64,
    sample_count: u64,
    clipped: bool,
    values: FinalValues,
    retroactive: bool,
    calibration_offset_db: f64,
}

impl SessionRecorder {
    pub(crate) fn new(
        config: SessionConfig,
        sample_rate_hz: f64,
        started_utc: OffsetDateTime,
    ) -> Self {
        let start = Marker {
            id: 1,
            t_s: 0.0,
            utc: started_utc,
            kind: MarkerKind::Start,
            label: "Start".to_owned(),
            note: String::new(),
        };
        let interval_samples = interval_samples(config.log_interval_s, sample_rate_hz);
        let bands = config
            .band_log
            .map(|bands| BandRecorder::new(bands, sample_rate_hz));
        Self {
            config,
            started_utc,
            sample_rate_hz,
            next_log_sample: interval_samples,
            session_sample_count: 0,
            recorded_samples: 0,
            next_marker_id: 2,
            start_marker_id: start.id,
            start_s: 0.0,
            segments: Vec::new(),
            markers: vec![start],
            unjournaled: Vec::new(),
            history: PlotHistory::default(),
            a: SegmentLevels::default(),
            c: SegmentLevels::default(),
            z: SegmentLevels::default(),
            bands,
            segment_clipped: false,
            interval_clipped: false,
            pending_marker_id: Some(1),
            reserved_marker: None,
            paused: false,
            marker_revision: 0,
            saved: Cell::new(None),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn process_sample(
        &mut self,
        raw_sample: f64,
        a_sample: f64,
        c_sample: f64,
        z_sample: f64,
        clipped: bool,
        fast_dbfs: [f64; 3],
        calibration_offset_db: f64,
    ) {
        // `t_s` derives from the stream's sample count, so paused samples
        // still count: timestamps after a pause stay true to wall-clock time.
        self.session_sample_count += 1;
        if let Some(bands) = &mut self.bands {
            bands.process(raw_sample, !self.paused);
        }
        if self.paused {
            return;
        }
        self.recorded_samples += 1;
        self.a.process(a_sample, fast_dbfs[0]);
        self.c.process(c_sample, fast_dbfs[1]);
        self.z.process(z_sample, fast_dbfs[2]);
        self.segment_clipped |= clipped;
        self.interval_clipped |= clipped;
        if let Some(marker) = &mut self.reserved_marker {
            marker.process_following([a_sample, c_sample, z_sample], fast_dbfs, clipped);
        }

        if self.session_sample_count < self.next_log_sample {
            return;
        }

        let t_s = self.elapsed_s(self.session_sample_count);
        let row = LogRecord {
            t_s,
            utc: self.utc_at(t_s),
            marker_id: self.pending_marker_id.take(),
            clipped: self.interval_clipped,
            laf_db: calibrated_level_db(fast_dbfs[0], calibration_offset_db),
            lcf_db: calibrated_level_db(fast_dbfs[1], calibration_offset_db),
            lzf_db: calibrated_level_db(fast_dbfs[2], calibration_offset_db),
        };
        self.push_row(row);
        if let Some(bands) = &mut self.bands {
            bands.log_row(row.t_s, row.utc, calibration_offset_db);
        }
        self.interval_clipped = false;
        self.next_log_sample = self.next_log_sample.saturating_add(interval_samples(
            self.config.log_interval_s,
            self.sample_rate_hz,
        ));
    }

    pub(crate) fn marker(
        &mut self,
        kind: MarkerKind,
        label: impl Into<String>,
        calibration_offset_db: f64,
    ) {
        self.reserved_marker = None;
        if let Some(bands) = &mut self.bands {
            bands.cancel_reserved();
        }
        let segment_sample_count = self.a.sample_count();
        let end_s = self.elapsed_s(self.session_sample_count);
        let values = Self::values_from(&self.a, &self.c, &self.z, calibration_offset_db);
        self.append_marker(
            kind,
            label,
            ClosedSegment {
                end_s,
                sample_count: segment_sample_count,
                clipped: self.segment_clipped,
                values,
                retroactive: false,
                calibration_offset_db,
            },
        );
        self.a = SegmentLevels::default();
        self.c = SegmentLevels::default();
        self.z = SegmentLevels::default();
        self.segment_clipped = false;
    }

    /// Closes the current segment with a `pause` marker; until [`Self::resume`],
    /// samples are neither accumulated nor logged.
    pub(crate) fn pause(&mut self, calibration_offset_db: f64) {
        self.marker(MarkerKind::Pause, "Pause", calibration_offset_db);
        self.paused = true;
    }

    /// Closes the empty paused segment with a `resume` marker and restarts the
    /// log interval grid at the resume time.
    pub(crate) fn resume(&mut self, calibration_offset_db: f64) {
        self.marker(MarkerKind::Resume, "Resume", calibration_offset_db);
        self.paused = false;
        self.next_log_sample = self.session_sample_count.saturating_add(interval_samples(
            self.config.log_interval_s,
            self.sample_rate_hz,
        ));
        self.interval_clipped = false;
    }

    pub(crate) const fn is_paused(&self) -> bool {
        self.paused
    }

    pub(crate) fn reset(&mut self) {
        self.started_utc = self.utc_at(self.elapsed_s(self.session_sample_count));
        self.next_log_sample = interval_samples(self.config.log_interval_s, self.sample_rate_hz);
        self.session_sample_count = 0;
        self.recorded_samples = 0;
        self.marker_revision = 0;
        self.saved.set(None);
        self.next_marker_id = 2;
        self.start_marker_id = 1;
        self.start_s = 0.0;
        self.segments.clear();
        self.markers = vec![Marker {
            id: 1,
            t_s: 0.0,
            utc: self.started_utc,
            kind: MarkerKind::Reset,
            label: "Reset".to_owned(),
            note: String::new(),
        }];
        self.unjournaled.clear();
        self.history = PlotHistory::default();
        self.a = SegmentLevels::default();
        self.c = SegmentLevels::default();
        self.z = SegmentLevels::default();
        self.bands = self
            .config
            .band_log
            .map(|bands| BandRecorder::new(bands, self.sample_rate_hz));
        self.segment_clipped = false;
        self.interval_clipped = false;
        self.pending_marker_id = Some(1);
        self.reserved_marker = None;
        self.paused = false;
    }

    fn append_marker(
        &mut self,
        kind: MarkerKind,
        label: impl Into<String>,
        segment: ClosedSegment,
    ) {
        let marker = Marker {
            id: self.next_marker_id,
            t_s: segment.end_s,
            utc: self.utc_at(segment.end_s),
            kind,
            label: label.into(),
            note: String::new(),
        };
        #[allow(clippy::cast_possible_truncation)]
        let number = self.segments.len() as u64 + 1;
        if let Some(bands) = &mut self.bands {
            if segment.retroactive {
                bands.place_reserved(number, segment.calibration_offset_db);
            } else {
                bands.close_segment(number, segment.calibration_offset_db);
            }
        }
        self.next_marker_id += 1;
        self.marker_revision += 1;
        self.segments.push(Segment {
            #[allow(clippy::cast_possible_truncation)]
            number: self.segments.len() as u64 + 1,
            start_marker_id: self.start_marker_id,
            end_marker_id: marker.id,
            start_s: self.start_s,
            end_s: segment.end_s,
            sample_count: segment.sample_count,
            clipped: segment.clipped,
            values: segment.values,
        });
        self.start_marker_id = marker.id;
        self.start_s = segment.end_s;
        let marker_id = marker.id;
        self.markers.push(marker);
        // Like any marker, it is noted in the first row after it. Rows after
        // a reserved marker stay out of the journal until it is placed (see
        // `journal_rows`), so that row is still here.
        if segment.retroactive {
            self.history.note_marker(segment.end_s, marker_id);
            if let Some(record) = self
                .unjournaled
                .iter_mut()
                .find(|record| record.t_s > segment.end_s && record.marker_id.is_none())
            {
                record.marker_id = Some(marker_id);
                return;
            }
        }
        self.pending_marker_id = Some(marker_id);
    }

    /// Changes a marker's label and note; returns `false` for an unknown id.
    pub(crate) fn edit_marker(&mut self, id: u64, label: &str, note: &str) -> bool {
        let Some(marker) = self.markers.iter_mut().find(|marker| marker.id == id) else {
            return false;
        };
        label.clone_into(&mut marker.label);
        note.clone_into(&mut marker.note);
        self.marker_revision += 1;
        true
    }

    /// Chooses the bands recorded from the next Reset, or at once while
    /// nothing has been recorded or marked yet.
    pub(crate) fn set_band_log(&mut self, bands: Option<crate::RtaConfig>) {
        if self.config.band_log == bands {
            return;
        }
        self.config.band_log = bands;
        if self.recorded_samples == 0 && self.markers.len() == 1 {
            self.bands = bands.map(|bands| BandRecorder::new(bands, self.sample_rate_hz));
        }
    }

    pub(crate) fn set_calibration(&mut self, calibration: Calibration) {
        self.config.calibration = calibration;
    }

    pub(crate) fn set_log_interval(&mut self, interval_s: f64, calibration_offset_db: f64) {
        self.marker(
            MarkerKind::IntervalChange,
            format!("Log interval changed to {interval_s} s"),
            calibration_offset_db,
        );
        self.config.log_interval_s = interval_s;
        self.next_log_sample = self
            .session_sample_count
            .saturating_add(interval_samples(interval_s, self.sample_rate_hz));
        self.interval_clipped = false;
    }

    fn push_row(&mut self, row: LogRecord) {
        self.history.push(row);
        self.unjournaled.push(row);
    }

    /// Returns the Log view rows for the last `window_s` (all with `None`)
    /// and the number of rows logged.
    pub(crate) fn plot_rows(&self, window_s: Option<f64>) -> (Vec<LogRecord>, usize) {
        (self.history.window(window_s), self.history.rows())
    }

    pub(crate) fn markers(&self) -> &[Marker] {
        &self.markers
    }

    pub(crate) fn current_elapsed_s(&self) -> f64 {
        self.elapsed_s(self.session_sample_count)
    }

    /// Returns the session configuration, with the current calibration and
    /// log interval.
    pub(crate) const fn config(&self) -> &SessionConfig {
        &self.config
    }

    pub(crate) const fn started_utc(&self) -> OffsetDateTime {
        self.started_utc
    }

    pub(crate) fn log_interval_s(&self) -> f64 {
        self.config.log_interval_s
    }

    fn values_from(
        a: &SegmentLevels,
        c: &SegmentLevels,
        z: &SegmentLevels,
        calibration_offset_db: f64,
    ) -> FinalValues {
        FinalValues {
            a: a.final_values(calibration_offset_db),
            c: c.final_values(calibration_offset_db),
            z: z.final_values(calibration_offset_db),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn elapsed_s(&self, sample_count: u64) -> f64 {
        sample_count as f64 / self.sample_rate_hz
    }

    fn utc_at(&self, elapsed_s: f64) -> OffsetDateTime {
        self.started_utc + Duration::seconds_f64(elapsed_s)
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn interval_samples(interval_s: f64, sample_rate_hz: f64) -> u64 {
    // The session API requires a finite positive interval and the engine uses
    // a finite positive stream rate; one sample is the shortest practical interval.
    ((interval_s * sample_rate_hz).round() as u64).max(1)
}

#[cfg(test)]
mod tests;

//! Saving the recorded session, and what the journal writes.

use spl_session::{
    BundleWriter, JournalWriter, LogRecord, MarkerKind, MeasurementBundle, MeasurementMetadata,
    RangeError, Segment, SegmentAggregate, aggregate_segments, segments_in_range,
};

use super::{SessionRecorder, bands::BandRecorder};
use crate::{EngineError, SaveMetadata, SaveRange};

impl SessionRecorder {
    /// Returns the rows the next journal flush may append: all rows not yet
    /// journaled, except those after a marker whose label is still being
    /// entered, since that marker may still be noted in one of them.
    pub(crate) fn journal_rows(&self) -> &[LogRecord] {
        let count = self
            .reserved_marker
            .as_ref()
            .map_or(self.unjournaled.len(), |marker| {
                self.unjournaled
                    .partition_point(|row| row.t_s <= marker.elapsed_s())
            });
        &self.unjournaled[..count]
    }

    /// Forgets the first `count` rows of [`Self::journal_rows`] and
    /// `band_count` of [`Self::journal_band_rows`] once the journal holds them.
    pub(crate) fn journaled(&mut self, count: usize, band_count: usize) {
        self.unjournaled.drain(..count.min(self.unjournaled.len()));
        if let Some(bands) = &mut self.bands {
            bands.journaled(band_count);
        }
    }

    /// Returns the band rows the next journal flush appends.
    pub(crate) fn journal_band_rows(&self) -> &[spl_session::BandRow] {
        self.bands
            .as_ref()
            .map_or(&[], |bands| bands.journal_rows())
    }

    /// Nominal frequencies of the recorded bands, if any.
    pub(crate) fn band_nominal_hz(&self) -> Option<Vec<f64>> {
        self.bands.as_ref().map(BandRecorder::nominal_hz)
    }

    /// Writes the session; the rows already in `journal` are read from it.
    pub(crate) fn write(
        &self,
        path: &std::path::Path,
        calibration_offset_db: f64,
        metadata: Option<SaveMetadata>,
        journal: Option<&JournalWriter>,
    ) -> Result<(), EngineError> {
        let range = metadata
            .as_ref()
            .map(|metadata| metadata.range)
            .unwrap_or_default();
        let bundle = self.bundle(calibration_offset_db, metadata);
        let bundle = match resolve_range(&bundle.segments, range) {
            Some((start, end)) => bundle
                .select_range(start, end)
                .map_err(EngineError::InvalidRange)?,
            None => bundle,
        };
        match journal {
            Some(journal) => BundleWriter.write_with_journal(path, &bundle, journal),
            None => BundleWriter.write(path, &bundle),
        }
        .map_err(EngineError::Session)?;
        self.saved
            .set(Some((self.recorded_samples, self.marker_revision)));
        Ok(())
    }

    /// Combines the segments a save of `range` would contain, as if saved now.
    pub(crate) fn range_summary(
        &self,
        range: SaveRange,
        calibration_offset_db: f64,
    ) -> Result<SegmentAggregate, RangeError> {
        let mut segments = self.segments.clone();
        // The open segment, closed by the stop marker a save would add.
        segments.push(Segment {
            #[allow(clippy::cast_possible_truncation)]
            number: segments.len() as u64 + 1,
            start_marker_id: self.start_marker_id,
            end_marker_id: self.next_marker_id,
            start_s: self.start_s,
            end_s: self.current_elapsed_s(),
            sample_count: self.a.sample_count(),
            clipped: self.segment_clipped,
            values: Self::values_from(&self.a, &self.c, &self.z, calibration_offset_db),
        });
        let (start, end) = resolve_range(&segments, range)
            .unwrap_or((segments[0].start_marker_id, self.next_marker_id));
        Ok(aggregate_segments(segments_in_range(
            &segments, start, end,
        )?)?)
    }

    /// Returns whether anything was recorded or marked since the last save.
    pub(crate) fn has_unsaved_changes(&self) -> bool {
        match self.saved.get() {
            Some(saved) => saved != (self.recorded_samples, self.marker_revision),
            None => self.recorded_samples > 0 || self.marker_revision > 0,
        }
    }

    /// Returns the bundle a save would write now, without its log rows, which
    /// the journal appends separately.
    pub(crate) fn journal_snapshot(&mut self, calibration_offset_db: f64) -> MeasurementBundle {
        let log = std::mem::take(&mut self.unjournaled);
        let history = std::mem::take(&mut self.history);
        let snapshot = self.bundle(calibration_offset_db, None);
        self.unjournaled = log;
        self.history = history;
        snapshot
    }

    pub(super) fn bundle(
        &self,
        calibration_offset_db: f64,
        metadata: Option<SaveMetadata>,
    ) -> MeasurementBundle {
        let mut complete = self.clone();
        complete.marker(MarkerKind::Stop, "Stop", calibration_offset_db);
        let summary = aggregate_segments(&complete.segments)
            .expect("session recorder always supplies complete final values");
        let end_s = complete
            .segments
            .last()
            .map_or(0.0, |segment| segment.end_s);
        let end_utc = complete.utc_at(end_s);
        let metadata = metadata.unwrap_or_else(|| SaveMetadata {
            name: complete.config.name.clone(),
            description: complete.config.description.clone(),
            ..SaveMetadata::default()
        });
        let bands = complete
            .bands
            .as_ref()
            .map(|bands| bands.band_log(&complete.segments));
        MeasurementBundle {
            metadata: MeasurementMetadata {
                name: metadata.name,
                start_utc: complete.started_utc,
                end_utc,
                software: complete.config.software,
                log_interval_s: complete.config.log_interval_s,
                reference_pressure_upa: 20,
                device: complete.config.device,
                calibration: complete.config.calibration,
                author: complete.config.author,
                tags: complete.config.tags,
                description: metadata.description,
            },
            log: complete.unjournaled,
            markers: complete.markers,
            segments: complete.segments,
            summary,
            attachments: metadata.attachments,
            bands,
        }
    }
}

/// Returns the marker ids `range` stands for in `segments`, or `None` for
/// the whole session.
fn resolve_range(segments: &[Segment], range: SaveRange) -> Option<(u64, u64)> {
    if range == SaveRange::default() {
        return None;
    }
    let start = range
        .start_marker_id
        .or_else(|| segments.first().map(|segment| segment.start_marker_id))?;
    let end = range
        .end_marker_id
        .or_else(|| segments.last().map(|segment| segment.end_marker_id))?;
    Some((start, end))
}

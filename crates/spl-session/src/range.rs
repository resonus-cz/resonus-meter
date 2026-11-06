//! Saving the part of a session between two markers (specification 6.2).

use thiserror::Error;

use crate::{
    AggregateError, LogRecord, MeasurementBundle, Segment, aggregate_band_segments,
    aggregate_segments,
};

/// Failure selecting a marker range.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RangeError {
    /// No segment starts at the requested start marker.
    #[error("marker {0} does not start a segment")]
    UnknownStart(u64),
    /// No segment ends at the requested end marker.
    #[error("marker {0} does not end a segment")]
    UnknownEnd(u64),
    /// The end marker does not follow the start marker.
    #[error("the range must end after it starts")]
    EndBeforeStart,
    /// The selected segments could not be combined.
    #[error(transparent)]
    Aggregate(#[from] AggregateError),
}

/// Returns the segments from the one opened by `start_marker_id` to the one
/// closed by `end_marker_id`. Both the save dialog preview and the writer
/// use this selection.
///
/// # Errors
///
/// Returns an error when either marker does not delimit a segment or the
/// end precedes the start.
pub fn segments_in_range(
    segments: &[Segment],
    start_marker_id: u64,
    end_marker_id: u64,
) -> Result<&[Segment], RangeError> {
    let first = segments
        .iter()
        .position(|segment| segment.start_marker_id == start_marker_id)
        .ok_or(RangeError::UnknownStart(start_marker_id))?;
    let last = segments
        .iter()
        .position(|segment| segment.end_marker_id == end_marker_id)
        .ok_or(RangeError::UnknownEnd(end_marker_id))?;
    if last < first {
        return Err(RangeError::EndBeforeStart);
    }
    Ok(&segments[first..=last])
}

impl MeasurementBundle {
    /// Keeps only the segments between two markers, their markers and the
    /// log rows inside them, and summarises those segments.
    ///
    /// Times keep their session values (`t_s` still counts from the session
    /// start); the end time becomes the end marker's.
    ///
    /// # Errors
    ///
    /// Returns an error when the markers do not delimit a range.
    pub fn select_range(
        mut self,
        start_marker_id: u64,
        end_marker_id: u64,
    ) -> Result<Self, RangeError> {
        let selected = segments_in_range(&self.segments, start_marker_id, end_marker_id)?.to_vec();
        let summary = aggregate_segments(&selected)?;
        if selected.is_empty() {
            return Err(RangeError::EndBeforeStart);
        }
        let kept_ids = selected
            .iter()
            .map(|segment| segment.start_marker_id)
            .chain([end_marker_id])
            .collect::<Vec<_>>();
        self.markers.retain(|marker| kept_ids.contains(&marker.id));
        if let Some(end) = self
            .markers
            .iter()
            .find(|marker| marker.id == end_marker_id)
        {
            self.metadata.end_utc = end.utc;
        }
        if let Some(bands) = &mut self.bands {
            bands
                .segments
                .retain(|band| selected.iter().any(|segment| segment.number == band.number));
            bands.summary_db =
                aggregate_band_segments(&selected, &bands.segments, bands.bands_hz.len());
        }
        self.segments = selected;
        self.summary = summary;
        self.log = std::mem::take(&mut self.log)
            .into_iter()
            .filter_map(|row| self.kept_row(row))
            .collect();
        if let Some(mut bands) = self.bands.take() {
            bands.rows.retain(|row| self.keeps_time(row.t_s));
            self.bands = Some(bands);
        }
        Ok(self)
    }

    /// Whether a log row ending at `t_s` lies inside the bundle's segments.
    pub(crate) fn keeps_time(&self, t_s: f64) -> bool {
        let start_s = self.segments.first().map_or(0.0, |segment| segment.start_s);
        let end_s = self
            .segments
            .last()
            .map_or(f64::INFINITY, |segment| segment.end_s);
        // A row covers the interval ending at its `t_s`.
        t_s > start_s && t_s <= end_s
    }

    /// Returns `row` as the bundle keeps it: `None` outside its segments, and
    /// without a reference to a marker it does not hold.
    pub(crate) fn kept_row(&self, mut row: LogRecord) -> Option<LogRecord> {
        if !self.keeps_time(row.t_s) {
            return None;
        }
        row.marker_id = row
            .marker_id
            .filter(|id| self.markers.iter().any(|marker| marker.id == *id));
        Some(row)
    }
}

#[cfg(test)]
mod tests {
    use super::{RangeError, segments_in_range};
    use crate::test_support::{bundle, timestamp};
    use crate::{FinalValues, LevelFinal, LogRecord, Marker, MarkerKind, Segment};

    fn segment(number: u64, start_s: f64, end_s: f64, leq_db: f64) -> Segment {
        Segment {
            number,
            start_marker_id: number,
            end_marker_id: number + 1,
            start_s,
            end_s,
            sample_count: 1_000,
            clipped: number == 3,
            values: FinalValues {
                a: LevelFinal::new(leq_db, leq_db + 10.0, leq_db + 3.0),
                c: LevelFinal::new(leq_db, leq_db + 10.0, leq_db + 3.0),
                z: LevelFinal::new(leq_db, leq_db + 10.0, leq_db + 3.0),
            },
        }
    }

    fn marker(id: u64, t_s: f64) -> Marker {
        Marker {
            id,
            t_s,
            utc: timestamp(u8::try_from(id).expect("small id")),
            kind: if id == 1 {
                MarkerKind::Start
            } else {
                MarkerKind::User
            },
            label: format!("M{id}"),
            note: String::new(),
        }
    }

    fn row(t_s: f64, marker_id: Option<u64>) -> LogRecord {
        LogRecord {
            t_s,
            utc: timestamp(0),
            marker_id,
            clipped: false,
            laf_db: 40.0,
            lcf_db: 41.0,
            lzf_db: 42.0,
        }
    }

    /// Four 10 s segments delimited by markers 1–5.
    fn four_segments() -> crate::MeasurementBundle {
        let mut bundle = bundle();
        bundle.segments = (1..=4)
            .map(|number| {
                #[allow(clippy::cast_precision_loss)]
                let start_s = (number - 1) as f64 * 10.0;
                #[allow(clippy::cast_precision_loss)]
                let leq_db = 40.0 + number as f64;
                segment(number, start_s, start_s + 10.0, leq_db)
            })
            .collect();
        #[allow(clippy::cast_precision_loss)]
        let markers = (1..=5)
            .map(|id| marker(id, (id - 1) as f64 * 10.0))
            .collect();
        bundle.markers = markers;
        bundle.log = vec![
            row(10.0, Some(1)),
            row(20.0, Some(2)),
            row(30.0, Some(3)),
            row(40.0, Some(4)),
        ];
        bundle
    }

    #[test]
    fn the_selection_runs_from_the_start_to_the_end_marker() {
        let bundle = four_segments();

        let selected = segments_in_range(&bundle.segments, 2, 4).expect("valid range");

        let numbers = selected
            .iter()
            .map(|segment| segment.number)
            .collect::<Vec<_>>();
        assert_eq!(numbers, [2, 3]);
        assert_eq!(
            segments_in_range(&bundle.segments, 3, 3),
            Err(RangeError::EndBeforeStart)
        );
        assert_eq!(
            segments_in_range(&bundle.segments, 9, 4),
            Err(RangeError::UnknownStart(9))
        );
    }

    #[test]
    fn a_selected_bundle_keeps_only_the_range() {
        let selected = four_segments().select_range(2, 4).expect("valid range");

        let marker_ids = selected
            .markers
            .iter()
            .map(|marker| marker.id)
            .collect::<Vec<_>>();
        assert_eq!(marker_ids, [2, 3, 4]);
        let rows = selected
            .log
            .iter()
            .map(|row| (row.t_s, row.marker_id))
            .collect::<Vec<_>>();
        // The row at 20 s still references marker 2; the one at 30 s keeps 3.
        assert_eq!(rows, [(20.0, Some(2)), (30.0, Some(3))]);
        assert_eq!(selected.metadata.end_utc, timestamp(4));
        assert!(selected.summary.clipped);
        assert_eq!(selected.summary.sample_count, 2_000);
        let leq = selected.summary.values.a.leq_db.expect("recorded");
        // Energy mean of 42 and 43 dB.
        let expected = 10.0 * f64::midpoint(10_f64.powf(4.2), 10_f64.powf(4.3)).log10();
        assert!((leq - expected).abs() < 1e-9);
    }

    #[test]
    fn a_selected_bundle_keeps_the_band_levels_of_the_range() {
        let mut bundle = four_segments();
        bundle.bands = Some(crate::BandLog {
            bands_per_octave: 1,
            bands_hz: vec![1_000.0],
            rows: [10.0, 20.0, 30.0, 40.0]
                .map(|t_s| crate::BandRow {
                    t_s,
                    utc: timestamp(0),
                    levels_db: vec![Some(t_s)],
                })
                .to_vec(),
            segments: (1..=4)
                .map(|number| crate::BandSegment {
                    number,
                    levels_db: vec![Some(if number == 2 { 50.0 } else { 60.0 })],
                })
                .collect(),
            summary_db: vec![None],
        });

        let bands = bundle
            .select_range(2, 4)
            .expect("valid range")
            .bands
            .expect("bands");

        let times = bands.rows.iter().map(|row| row.t_s).collect::<Vec<_>>();
        assert_eq!(times, [20.0, 30.0]);
        let numbers = bands
            .segments
            .iter()
            .map(|segment| segment.number)
            .collect::<Vec<_>>();
        assert_eq!(numbers, [2, 3]);
        // Equal lengths: the energy mean of 50 and 60 dB.
        let expected_db = 10.0 * f64::midpoint(1e5, 1e6).log10();
        assert!((bands.summary_db[0].expect("measured") - expected_db).abs() < 1e-9);
    }
}

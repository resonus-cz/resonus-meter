//! Typed records used by a measurement session.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

/// Final Leq, sample-peak and Fast-maximum values for one frequency weighting.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LevelFinal {
    /// Equivalent continuous sound level in dB.
    pub leq_db: Option<f64>,
    /// Maximum absolute weighted sample level in dB.
    pub peak_db: Option<f64>,
    /// Maximum Fast time-weighted level in dB (`LXFmax`).
    pub fast_max_db: Option<f64>,
}

impl LevelFinal {
    /// The values of a segment without samples.
    pub const EMPTY: Self = Self {
        leq_db: None,
        peak_db: None,
        fast_max_db: None,
    };

    /// Creates a final level with all values present.
    #[must_use]
    pub const fn new(leq_db: f64, peak_db: f64, fast_max_db: f64) -> Self {
        Self {
            leq_db: Some(leq_db),
            peak_db: Some(peak_db),
            fast_max_db: Some(fast_max_db),
        }
    }
}

/// Final levels for the A, C, and Z frequency weightings.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FinalValues {
    /// A-weighted final values.
    pub a: LevelFinal,
    /// C-weighted final values.
    pub c: LevelFinal,
    /// Z-weighted final values.
    pub z: LevelFinal,
}

/// A Fast-weighted log row written at the end of a log interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogRecord {
    /// Elapsed time derived from the processed sample count.
    pub t_s: f64,
    /// Session-start UTC plus [`Self::t_s`].
    pub utc: OffsetDateTime,
    /// Marker occurring within this interval, if any.
    pub marker_id: Option<u64>,
    /// Whether an overload occurred within this interval.
    pub clipped: bool,
    /// A-weighted Fast level in dB.
    pub laf_db: f64,
    /// C-weighted Fast level in dB.
    pub lcf_db: f64,
    /// Z-weighted Fast level in dB.
    pub lzf_db: f64,
}

/// The reason a marker was inserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    /// Session start.
    Start,
    /// Session stop.
    Stop,
    /// User-created marker.
    User,
    /// Display reset.
    Reset,
    /// Log interval changed.
    IntervalChange,
    /// Recording paused; the segment it opens holds no samples.
    Pause,
    /// Recording resumed after a pause.
    Resume,
}

impl MarkerKind {
    pub(crate) const fn as_csv(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::User => "user",
            Self::Reset => "reset",
            Self::IntervalChange => "interval_change",
            Self::Pause => "pause",
            Self::Resume => "resume",
        }
    }
}

/// A marker delimiting adjacent measurement segments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// Unique marker identifier within the session.
    pub id: u64,
    /// Elapsed time derived from the processed sample count.
    pub t_s: f64,
    /// Session-start UTC plus [`Self::t_s`].
    #[serde(with = "time::serde::rfc3339")]
    pub utc: OffsetDateTime,
    /// Reason the marker was added.
    pub kind: MarkerKind,
    /// Short user-visible label.
    pub label: String,
    /// Optional user note.
    pub note: String,
}

/// Final values between two consecutive markers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// One-based segment sequence number.
    pub number: u64,
    /// Marker that opened this segment.
    pub start_marker_id: u64,
    /// Marker that closed this segment.
    pub end_marker_id: u64,
    /// Elapsed time at the start marker.
    pub start_s: f64,
    /// Elapsed time at the end marker.
    pub end_s: f64,
    /// Count of weighted samples represented by the final values.
    pub sample_count: u64,
    /// Whether an overload occurred in this segment.
    pub clipped: bool,
    /// A/C/Z final values.
    pub values: FinalValues,
}

impl Segment {
    /// Returns the duration delimited by the two markers.
    #[must_use]
    pub fn duration_s(&self) -> f64 {
        self.end_s - self.start_s
    }
}

/// Result of combining a contiguous set of segments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentAggregate {
    /// Number of samples contributing to the combined Leq.
    pub sample_count: u64,
    /// Whether any included segment clipped.
    pub clipped: bool,
    /// Combined A/C/Z Leq energy means and sample peaks.
    pub values: FinalValues,
}

/// A device used to capture the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Stable backend device identifier.
    pub id: String,
    /// Human-readable device name.
    pub name: String,
    /// Audio backend name.
    pub backend: String,
    /// One-based input channel number.
    pub channel: u32,
    /// Actual negotiated sample rate.
    pub sample_rate_hz: u32,
}

/// How the session's calibration offset was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationType {
    /// Captured from an acoustic calibrator.
    Calibrator,
    /// Entered by the user.
    Manual,
    /// No calibration was applied.
    None,
}

impl CalibrationType {
    pub(crate) const fn as_yaml(self) -> &'static str {
        match self {
            Self::Calibrator => "calibrator",
            Self::Manual => "manual",
            Self::None => "none",
        }
    }
}

/// Calibration metadata saved with a measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// Calibration offset in dB.
    pub offset_db: f64,
    /// Origin of the calibration.
    pub calibration_type: CalibrationType,
    /// Calibrator sound pressure level, when applicable.
    pub calibrator_level_db: Option<f64>,
    /// Calibrator frequency, when applicable.
    pub calibrator_freq_hz: Option<f64>,
    /// When the calibration was created, when applicable.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub calibrated_utc: Option<OffsetDateTime>,
}

impl Calibration {
    /// Metadata for a measurement taken without calibration (offset 0 dB).
    #[must_use]
    pub const fn uncalibrated() -> Self {
        Self {
            offset_db: 0.0,
            calibration_type: CalibrationType::None,
            calibrator_level_db: None,
            calibrator_freq_hz: None,
            calibrated_utc: None,
        }
    }
}

/// Front-matter and prose metadata for a portable measurement.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementMetadata {
    /// Display name of the measurement.
    pub name: String,
    /// Session start in UTC.
    pub start_utc: OffsetDateTime,
    /// Session end in UTC.
    pub end_utc: OffsetDateTime,
    /// Application name and version.
    pub software: String,
    /// Live log interval in seconds.
    pub log_interval_s: f64,
    /// Reference pressure in micropascals.
    pub reference_pressure_upa: u32,
    /// Capture device details.
    pub device: Device,
    /// Calibration used during the session.
    pub calibration: Calibration,
    /// Optional measurement author.
    pub author: Option<String>,
    /// Searchable measurement tags.
    pub tags: Vec<String>,
    /// User-supplied Markdown description.
    pub description: String,
}

/// All data needed to write one portable measurement folder.
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementBundle {
    /// Measurement metadata and user prose.
    pub metadata: MeasurementMetadata,
    /// Fast-level rows.
    pub log: Vec<LogRecord>,
    /// Segment delimiters.
    pub markers: Vec<Marker>,
    /// Closed segment final values.
    pub segments: Vec<Segment>,
    /// Overall or selected-range final values.
    pub summary: SegmentAggregate,
    /// Files copied into the bundle's `attachments/` folder.
    pub attachments: Vec<Attachment>,
    /// Band levels, when the session recorded them.
    pub bands: Option<crate::BandLog>,
}

/// A user file saved with a measurement, such as a photo or a datasheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// File to copy into the bundle.
    pub source: std::path::PathBuf,
    /// Title shown for the file.
    pub title: String,
}

/// An invalid segment prevents final-value aggregation.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AggregateError {
    /// A segment with samples omitted one of its required final values.
    #[error("segment {segment_number} has samples but is missing a final value")]
    MissingFinalValue {
        /// Sequence number of the invalid segment.
        segment_number: u64,
    },
    /// Segment sample counts overflowed while being combined.
    #[error("combined segment sample count overflowed")]
    SampleCountOverflow,
}

/// Combines segment Leq values by energy mean and sample peaks by maximum.
///
/// Empty segments are ignored. Each non-empty segment must contain all three
/// Leq and peak values.
///
/// # Errors
///
/// Returns an error when a non-empty segment omits a final value or the total
/// sample count overflows.
pub fn aggregate_segments<'a>(
    segments: impl IntoIterator<Item = &'a Segment>,
) -> Result<SegmentAggregate, AggregateError> {
    let mut total_samples = 0_u64;
    let mut clipped = false;
    let mut a = AggregateLevel::default();
    let mut c = AggregateLevel::default();
    let mut z = AggregateLevel::default();

    for segment in segments {
        clipped |= segment.clipped;
        if segment.sample_count == 0 {
            continue;
        }
        total_samples = total_samples
            .checked_add(segment.sample_count)
            .ok_or(AggregateError::SampleCountOverflow)?;
        a.add(segment.values.a, segment.sample_count, segment.number)?;
        c.add(segment.values.c, segment.sample_count, segment.number)?;
        z.add(segment.values.z, segment.sample_count, segment.number)?;
    }

    Ok(SegmentAggregate {
        sample_count: total_samples,
        clipped,
        values: FinalValues {
            a: a.finish(total_samples),
            c: c.finish(total_samples),
            z: z.finish(total_samples),
        },
    })
}

#[derive(Debug, Default)]
struct AggregateLevel {
    energy_sum: f64,
    energy_compensation: f64,
    peak_db: Option<f64>,
    fast_max_db: Option<f64>,
}

impl AggregateLevel {
    fn add(
        &mut self,
        values: LevelFinal,
        sample_count: u64,
        segment_number: u64,
    ) -> Result<(), AggregateError> {
        let Some(leq_db) = values.leq_db else {
            return Err(AggregateError::MissingFinalValue { segment_number });
        };
        let Some(peak_db) = values.peak_db else {
            return Err(AggregateError::MissingFinalValue { segment_number });
        };
        let Some(fast_max_db) = values.fast_max_db else {
            return Err(AggregateError::MissingFinalValue { segment_number });
        };
        let contribution = sample_count_as_f64(sample_count) * 10_f64.powf(leq_db / 10.0);
        let adjusted = contribution - self.energy_compensation;
        let sum = self.energy_sum + adjusted;
        self.energy_compensation = (sum - self.energy_sum) - adjusted;
        self.energy_sum = sum;
        if self
            .peak_db
            .is_none_or(|current| peak_db.total_cmp(&current) == Ordering::Greater)
        {
            self.peak_db = Some(peak_db);
        }
        if self
            .fast_max_db
            .is_none_or(|current| fast_max_db.total_cmp(&current) == Ordering::Greater)
        {
            self.fast_max_db = Some(fast_max_db);
        }
        Ok(())
    }

    fn finish(self, total_samples: u64) -> LevelFinal {
        if total_samples == 0 {
            return LevelFinal::EMPTY;
        }
        LevelFinal {
            leq_db: Some(10.0 * (self.energy_sum / sample_count_as_f64(total_samples)).log10()),
            peak_db: self.peak_db,
            fast_max_db: self.fast_max_db,
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn sample_count_as_f64(sample_count: u64) -> f64 {
    // Sample counts are combined with f64 DSP energy accumulators.
    sample_count as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(leq_db: f64, peak_db: f64, fast_max_db: f64) -> FinalValues {
        FinalValues {
            a: LevelFinal::new(leq_db, peak_db, fast_max_db),
            c: LevelFinal::new(leq_db + 1.0, peak_db + 1.0, fast_max_db + 1.0),
            z: LevelFinal::new(leq_db + 2.0, peak_db + 2.0, fast_max_db + 2.0),
        }
    }

    #[test]
    fn aggregation_uses_energy_mean_sample_peak_and_fast_maximum() {
        let segments = [
            Segment {
                number: 1,
                start_marker_id: 1,
                end_marker_id: 2,
                start_s: 0.0,
                end_s: 1.0,
                sample_count: 100,
                clipped: false,
                values: values(60.0, 70.0, 63.0),
            },
            Segment {
                number: 2,
                start_marker_id: 2,
                end_marker_id: 3,
                start_s: 1.0,
                end_s: 3.0,
                sample_count: 100,
                clipped: true,
                values: values(70.0, 65.0, 72.5),
            },
        ];

        let combined = aggregate_segments(&segments).expect("valid segments aggregate");

        assert_eq!(combined.sample_count, 200);
        assert!(combined.clipped);
        assert!((combined.values.a.leq_db.expect("Leq exists") - 67.403_626).abs() < 0.000_001);
        assert_eq!(combined.values.a.peak_db, Some(70.0));
        assert_eq!(combined.values.a.fast_max_db, Some(72.5));
    }

    #[test]
    fn non_empty_segment_requires_final_values() {
        let segment = Segment {
            number: 4,
            start_marker_id: 4,
            end_marker_id: 5,
            start_s: 0.0,
            end_s: 1.0,
            sample_count: 1,
            clipped: false,
            values: FinalValues {
                a: LevelFinal::EMPTY,
                c: LevelFinal::new(1.0, 2.0, 1.5),
                z: LevelFinal::new(1.0, 2.0, 1.5),
            },
        };

        assert_eq!(
            aggregate_segments([&segment]),
            Err(AggregateError::MissingFinalValue { segment_number: 4 })
        );
    }
}

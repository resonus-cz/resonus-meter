//! Shared test fixtures.

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use crate::{
    Calibration, CalibrationType, Device, FinalValues, LevelFinal, LogRecord, Marker, MarkerKind,
    MeasurementBundle, MeasurementMetadata, Segment, SegmentAggregate,
};

pub(crate) fn timestamp(second: u8) -> OffsetDateTime {
    PrimitiveDateTime::new(
        Date::from_calendar_date(2026, Month::October, 16).expect("valid date"),
        Time::from_hms_milli(9, 12, second, 250).expect("valid time"),
    )
    .assume_offset(UtcOffset::UTC)
}

fn values() -> FinalValues {
    FinalValues {
        a: LevelFinal::new(41.05, 66.10, 44.20),
        c: LevelFinal::new(49.00, 70.40, 52.00),
        z: LevelFinal::new(54.90, 72.30, 57.50),
    }
}

pub(crate) fn bundle() -> MeasurementBundle {
    let values = values();
    MeasurementBundle {
        metadata: MeasurementMetadata {
            name: "Office HVAC noise".to_owned(),
            start_utc: timestamp(3),
            end_utc: timestamp(4),
            software: "SPL Meter 0.1.0".to_owned(),
            log_interval_s: 1.0,
            reference_pressure_upa: 20,
            device: Device {
                id: "pipewire.mic".to_owned(),
                name: "Measurement Mic".to_owned(),
                backend: "pipewire".to_owned(),
                channel: 1,
                sample_rate_hz: 48_000,
            },
            calibration: Calibration {
                offset_db: 120.37,
                calibration_type: CalibrationType::Calibrator,
                calibrator_level_db: Some(94.0),
                calibrator_freq_hz: Some(1_000.0),
                calibrated_utc: Some(timestamp(1)),
            },
            author: Some("J. Novak".to_owned()),
            tags: vec!["hvac".to_owned()],
            description: "Steady fan noise.".to_owned(),
        },
        log: vec![LogRecord {
            t_s: 1.0,
            utc: timestamp(4),
            marker_id: None,
            clipped: false,
            laf_db: 41.05,
            lcf_db: 49.0,
            lzf_db: 54.9,
        }],
        markers: vec![
            Marker {
                id: 1,
                t_s: 0.0,
                utc: timestamp(3),
                kind: MarkerKind::Start,
                label: "Start".to_owned(),
                note: String::new(),
            },
            Marker {
                id: 2,
                t_s: 1.0,
                utc: timestamp(4),
                kind: MarkerKind::Stop,
                label: "Stop".to_owned(),
                note: String::new(),
            },
        ],
        segments: vec![Segment {
            number: 1,
            start_marker_id: 1,
            end_marker_id: 2,
            start_s: 0.0,
            end_s: 1.0,
            sample_count: 48_000,
            clipped: false,
            values,
        }],
        summary: SegmentAggregate {
            sample_count: 48_000,
            clipped: false,
            values,
        },
        attachments: Vec::new(),
        bands: None,
    }
}

/// Bands at 500 Hz and 1 kHz with one row and the values of segment 1.
pub(crate) fn band_log() -> crate::BandLog {
    crate::BandLog {
        bands_per_octave: 1,
        bands_hz: vec![500.0, 1_000.0],
        rows: vec![crate::BandRow {
            t_s: 1.0,
            utc: timestamp(4),
            levels_db: vec![Some(38.5), None],
        }],
        segments: vec![crate::BandSegment {
            number: 1,
            levels_db: vec![Some(38.5), Some(35.25)],
        }],
        summary_db: vec![Some(38.5), Some(35.25)],
    }
}

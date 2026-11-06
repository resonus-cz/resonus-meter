//! The CSV files of a bundle.

use std::{fs::File, path::Path};

use csv::{Terminator, WriterBuilder};

use super::{SessionError, clipped, fixed, optional_fixed, utc_millis};
use crate::{LogRecord, Marker, Segment};

pub(crate) const LOG_HEADERS: [&str; 7] = ["t_s", "utc", "marker_id", "clip", "LAF", "LCF", "LZF"];
const MARKER_HEADERS: [&str; 6] = ["id", "t_s", "utc", "kind", "label", "note"];
// Fast maxima follow the 1.0 columns, so positional readers keep working.
const SEGMENT_HEADERS: [&str; 16] = [
    "segment",
    "start_marker",
    "end_marker",
    "start_s",
    "end_s",
    "duration_s",
    "clip",
    "LAeq",
    "LApeak",
    "LCeq",
    "LCpeak",
    "LZeq",
    "LZpeak",
    "LAFmax",
    "LCFmax",
    "LZFmax",
];

pub(super) fn write_log(
    path: &Path,
    records: impl Iterator<Item = Result<LogRecord, SessionError>>,
) -> Result<(), SessionError> {
    let mut writer = csv_writer(path)?;
    writer.write_record(LOG_HEADERS)?;
    for record in records {
        writer.write_record(log_row(&record?))?;
    }
    finish_csv(writer, path)
}

/// Formats one `log.csv` row.
pub(crate) fn log_row(record: &LogRecord) -> [String; 7] {
    [
        fixed(record.t_s, 3),
        utc_millis(record.utc),
        record
            .marker_id
            .map_or_else(String::new, |id| id.to_string()),
        clipped(record.clipped),
        fixed(record.laf_db, 2),
        fixed(record.lcf_db, 2),
        fixed(record.lzf_db, 2),
    ]
}

pub(crate) fn write_markers(path: &Path, markers: &[Marker]) -> Result<(), SessionError> {
    let mut writer = csv_writer(path)?;
    writer.write_record(MARKER_HEADERS)?;
    for marker in markers {
        writer.write_record([
            marker.id.to_string(),
            fixed(marker.t_s, 3),
            utc_millis(marker.utc),
            marker.kind.as_csv().to_owned(),
            marker.label.clone(),
            marker.note.clone(),
        ])?;
    }
    finish_csv(writer, path)
}

pub(crate) fn write_segments(path: &Path, segments: &[Segment]) -> Result<(), SessionError> {
    let mut writer = csv_writer(path)?;
    writer.write_record(SEGMENT_HEADERS)?;
    for segment in segments {
        writer.write_record([
            segment.number.to_string(),
            segment.start_marker_id.to_string(),
            segment.end_marker_id.to_string(),
            fixed(segment.start_s, 3),
            fixed(segment.end_s, 3),
            fixed(segment.duration_s(), 3),
            clipped(segment.clipped),
            optional_fixed(segment.values.a.leq_db, 2),
            optional_fixed(segment.values.a.peak_db, 2),
            optional_fixed(segment.values.c.leq_db, 2),
            optional_fixed(segment.values.c.peak_db, 2),
            optional_fixed(segment.values.z.leq_db, 2),
            optional_fixed(segment.values.z.peak_db, 2),
            optional_fixed(segment.values.a.fast_max_db, 2),
            optional_fixed(segment.values.c.fast_max_db, 2),
            optional_fixed(segment.values.z.fast_max_db, 2),
        ])?;
    }
    finish_csv(writer, path)
}

pub(crate) fn csv_writer(path: &Path) -> Result<csv::Writer<File>, SessionError> {
    let file = File::create(path).map_err(|source| SessionError::Io {
        path: path.to_owned(),
        source,
    })?;
    Ok(WriterBuilder::new()
        .has_headers(false)
        .terminator(Terminator::Any(b'\n'))
        .from_writer(file))
}

pub(crate) fn finish_csv(mut writer: csv::Writer<File>, path: &Path) -> Result<(), SessionError> {
    writer.flush().map_err(|source| SessionError::Io {
        path: path.to_owned(),
        source,
    })?;
    let file = writer.into_inner().map_err(|error| SessionError::Io {
        path: path.to_owned(),
        source: error.into_error(),
    })?;
    file.sync_all().map_err(|source| SessionError::Io {
        path: path.to_owned(),
        source,
    })
}

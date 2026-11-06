//! `rta_log.csv` and `rta_segments.csv`: the band-level files of a bundle.

use std::path::Path;

use super::{SessionError, csv_writer, finish_csv, fixed, optional_fixed, utc_millis};
use crate::{BandLog, BandRow, Segment, band_column};

pub(crate) const BAND_LOG_FILE: &str = "rta_log.csv";
pub(crate) const BAND_SEGMENTS_FILE: &str = "rta_segments.csv";

/// The header of `rta_log.csv` for `bands_hz`.
pub(crate) fn band_log_headers(bands_hz: &[f64]) -> Vec<String> {
    ["t_s", "utc"]
        .into_iter()
        .map(str::to_owned)
        .chain(bands_hz.iter().map(|&nominal_hz| band_column(nominal_hz)))
        .collect()
}

/// Formats one `rta_log.csv` row.
pub(crate) fn band_row(row: &BandRow) -> Vec<String> {
    [fixed(row.t_s, 3), utc_millis(row.utc)]
        .into_iter()
        .chain(row.levels_db.iter().map(|&level| optional_fixed(level, 2)))
        .collect()
}

pub(super) fn write_band_log(
    path: &Path,
    bands_hz: &[f64],
    rows: impl Iterator<Item = Result<BandRow, SessionError>>,
) -> Result<(), SessionError> {
    let mut writer = csv_writer(path)?;
    writer.write_record(band_log_headers(bands_hz))?;
    for row in rows {
        writer.write_record(band_row(&row?))?;
    }
    finish_csv(writer, path)
}

pub(crate) fn write_band_segments(
    path: &Path,
    bands: &BandLog,
    segments: &[Segment],
) -> Result<(), SessionError> {
    let mut writer = csv_writer(path)?;
    let headers = [
        "segment",
        "start_marker",
        "end_marker",
        "start_s",
        "end_s",
        "duration_s",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(
        bands
            .bands_hz
            .iter()
            .map(|&nominal_hz| band_column(nominal_hz)),
    );
    writer.write_record(headers)?;
    for segment in segments {
        let levels = bands
            .segments
            .iter()
            .find(|bands| bands.number == segment.number)
            .map_or(&[][..], |bands| &bands.levels_db);
        let cells = [
            segment.number.to_string(),
            segment.start_marker_id.to_string(),
            segment.end_marker_id.to_string(),
            fixed(segment.start_s, 3),
            fixed(segment.end_s, 3),
            fixed(segment.duration_s(), 3),
        ]
        .into_iter()
        .chain(
            (0..bands.bands_hz.len())
                .map(|index| optional_fixed(levels.get(index).copied().flatten(), 2)),
        );
        writer.write_record(cells.collect::<Vec<_>>())?;
    }
    finish_csv(writer, path)
}

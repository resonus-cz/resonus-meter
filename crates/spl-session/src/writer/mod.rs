//! Portable measurement bundle writer.

mod attachments;
mod band_files;
mod csv_files;
mod measurement;

pub(crate) use band_files::{
    BAND_LOG_FILE, BAND_SEGMENTS_FILE, band_log_headers, band_row, write_band_segments,
};
pub(crate) use csv_files::{
    LOG_HEADERS, csv_writer, finish_csv, log_row, write_markers, write_segments,
};
pub(crate) use measurement::write_measurement_atomically;

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;
use time::OffsetDateTime;

use crate::{BandRow, FinalValues, JournalWriter, MeasurementBundle};
use csv_files::write_log;

/// Error writing a portable measurement bundle.
#[derive(Debug, Error)]
pub enum SessionError {
    /// A filesystem operation failed.
    #[error("could not access {path}: {source}")]
    Io {
        /// Path involved in the failed operation.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: io::Error,
    },
    /// CSV serialization failed.
    #[error("could not write CSV: {0}")]
    Csv(#[from] csv::Error),
    /// A model value cannot be represented in the portable format.
    #[error("{field} must be finite")]
    NonFinite {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// The requested bundle path already contains a measurement.
    #[error("bundle directory {0} already exists and is not empty")]
    NonEmptyBundle(PathBuf),
    /// A journal file has unexpected content.
    #[error("journal file {path} is invalid: {reason}")]
    InvalidJournal {
        /// File that failed.
        path: PathBuf,
        /// What was wrong.
        reason: String,
    },
}

/// Writes the canonical folder representation of a measurement.
#[derive(Debug, Default, Clone, Copy)]
pub struct BundleWriter;

impl BundleWriter {
    /// Writes a measurement folder at `bundle_path`.
    ///
    /// The caller supplies the complete output directory, rather than relying
    /// on an application-specific save location. CSV files use the canonical
    /// names from the `files` front-matter block. `measurement.md` is written
    /// last using a temporary sibling file, `fsync`, and atomic rename.
    ///
    /// # Errors
    ///
    /// Returns an error when the data contains a non-finite numeric value, the
    /// destination is a non-empty directory, or filesystem/CSV writing fails.
    pub fn write(self, bundle_path: &Path, bundle: &MeasurementBundle) -> Result<(), SessionError> {
        write_rows(bundle_path, bundle, None)
    }

    /// Writes a measurement folder like [`Self::write`] for a session whose
    /// first log and band rows are in `journal`; the bundle holds the rows
    /// that follow.
    /// The rows are copied one at a time, so a long log never has to be in
    /// memory at once.
    ///
    /// # Errors
    ///
    /// As [`Self::write`], and when the journal's `log.csv` cannot be read.
    pub fn write_with_journal(
        self,
        bundle_path: &Path,
        bundle: &MeasurementBundle,
        journal: &JournalWriter,
    ) -> Result<(), SessionError> {
        write_rows(bundle_path, bundle, Some(journal))
    }
}

fn write_rows(
    bundle_path: &Path,
    bundle: &MeasurementBundle,
    journal: Option<&JournalWriter>,
) -> Result<(), SessionError> {
    validate_bundle(bundle)?;
    create_bundle_directory(bundle_path)?;

    let journal_rows = journal.map(JournalWriter::rows).transpose()?;
    let rows = journal_rows
        .into_iter()
        .flatten()
        .chain(bundle.log.iter().copied().map(Ok))
        .filter_map(|row| match row {
            Ok(row) => bundle.kept_row(row).map(Ok),
            Err(error) => Some(Err(error)),
        });
    write_log(&bundle_path.join("log.csv"), rows)?;
    write_markers(&bundle_path.join("markers.csv"), &bundle.markers)?;
    write_segments(&bundle_path.join("segments.csv"), &bundle.segments)?;
    if let Some(bands) = &bundle.bands {
        // A journal that has not written band rows yet may have no band log.
        let journal_rows = journal
            .filter(|journal| journal.band_rows() > 0)
            .map(|journal| journal.band_log_rows(&bands.bands_hz))
            .transpose()?;
        let rows = journal_rows
            .into_iter()
            .flatten()
            .chain(bands.rows.iter().cloned().map(Ok))
            .filter(|row| {
                row.as_ref()
                    .map_or(true, |row: &BandRow| bundle.keeps_time(row.t_s))
            });
        band_files::write_band_log(&bundle_path.join(BAND_LOG_FILE), &bands.bands_hz, rows)?;
        write_band_segments(
            &bundle_path.join(BAND_SEGMENTS_FILE),
            bands,
            &bundle.segments,
        )?;
    }
    let attachments = attachments::copy_attachments(bundle_path, &bundle.attachments)?;
    write_measurement_atomically(&bundle_path.join("measurement.md"), bundle, &attachments)
}

fn create_bundle_directory(path: &Path) -> Result<(), SessionError> {
    if path.exists() {
        let mut entries = fs::read_dir(path).map_err(|source| SessionError::Io {
            path: path.to_owned(),
            source,
        })?;
        if entries.next().is_some() {
            return Err(SessionError::NonEmptyBundle(path.to_owned()));
        }
    } else {
        fs::create_dir_all(path).map_err(|source| SessionError::Io {
            path: path.to_owned(),
            source,
        })?;
    }
    Ok(())
}

pub(crate) fn validate_bundle(bundle: &MeasurementBundle) -> Result<(), SessionError> {
    finite(bundle.metadata.log_interval_s, "log_interval_s")?;
    finite(
        bundle.metadata.calibration.offset_db,
        "calibration.offset_db",
    )?;
    for record in &bundle.log {
        finite(record.t_s, "log.t_s")?;
        finite(record.laf_db, "log.LAF")?;
        finite(record.lcf_db, "log.LCF")?;
        finite(record.lzf_db, "log.LZF")?;
    }
    for marker in &bundle.markers {
        finite(marker.t_s, "marker.t_s")?;
    }
    for segment in &bundle.segments {
        finite(segment.start_s, "segment.start_s")?;
        finite(segment.end_s, "segment.end_s")?;
        validate_values(segment.values)?;
    }
    if let Some(bands) = &bundle.bands {
        let levels = bands
            .rows
            .iter()
            .flat_map(|row| &row.levels_db)
            .chain(bands.segments.iter().flat_map(|segment| &segment.levels_db))
            .chain(&bands.summary_db);
        for level in levels.flatten() {
            finite(*level, "band level")?;
        }
    }
    validate_values(bundle.summary.values)
}

fn validate_values(values: FinalValues) -> Result<(), SessionError> {
    for (name, value) in [
        ("LAeq", values.a.leq_db),
        ("LApeak", values.a.peak_db),
        ("LCeq", values.c.leq_db),
        ("LCpeak", values.c.peak_db),
        ("LZeq", values.z.leq_db),
        ("LZpeak", values.z.peak_db),
        ("LAFmax", values.a.fast_max_db),
        ("LCFmax", values.c.fast_max_db),
        ("LZFmax", values.z.fast_max_db),
    ] {
        if let Some(value) = value {
            finite(value, name)?;
        }
    }
    Ok(())
}

fn finite(value: f64, field: &'static str) -> Result<(), SessionError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(SessionError::NonFinite { field })
    }
}

fn fixed(value: f64, precision: usize) -> String {
    format!("{value:.precision$}")
}

fn optional_fixed(value: Option<f64>, precision: usize) -> String {
    value.map_or_else(String::new, |value| fixed(value, precision))
}

fn clipped(value: bool) -> String {
    u8::from(value).to_string()
}

fn utc_millis(value: OffsetDateTime) -> String {
    let value = value.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
        value.millisecond()
    )
}

#[cfg(test)]
mod tests;

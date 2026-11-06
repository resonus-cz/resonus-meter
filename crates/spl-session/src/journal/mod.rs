//! The live journal of an unsaved session (specification 6.6, ADR 0010).
//!
//! The journal folder has the layout of a saved bundle. `log.csv` only grows;
//! `markers.csv`, `segments.csv` and `measurement.md` are replaced atomically
//! at every flush and show the session as if it were saved then. The opaque
//! `recovery.toml`, written last, lets the engine continue the session after
//! a crash; it names how many `log.csv` rows belong to it. Saving a session
//! reads its log rows back from here, so they need not stay in memory.

use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use csv::{ReaderBuilder, Terminator, WriterBuilder};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::writer::{
    BAND_LOG_FILE, BAND_SEGMENTS_FILE, LOG_HEADERS, band_log_headers, band_row, csv_writer,
    finish_csv, log_row, validate_bundle, write_band_segments, write_markers,
    write_measurement_atomically, write_segments,
};
use crate::{BandRow, LogRecord, MeasurementBundle, SessionError};

/// File holding the engine's recovery state.
pub const RECOVERY_FILE: &str = "recovery.toml";
const LOG_FILE: &str = "log.csv";

/// Writes the journal folder of one running session.
#[derive(Debug)]
pub struct JournalWriter {
    directory: PathBuf,
    log_rows: usize,
    band_rows: usize,
}

impl JournalWriter {
    /// Starts an empty journal in `directory`, replacing any previous one.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder or `log.csv` cannot be created.
    pub fn create(directory: &Path) -> Result<Self, SessionError> {
        fs::create_dir_all(directory).map_err(|source| io_error(directory, source))?;
        let path = directory.join(LOG_FILE);
        let mut writer = csv_writer(&path)?;
        writer.write_record(LOG_HEADERS)?;
        finish_csv(writer, &path)?;
        Ok(Self {
            directory: directory.to_owned(),
            log_rows: 0,
            band_rows: 0,
        })
    }

    /// Continues the journal in `directory` after recovery. `log.csv` keeps
    /// its first `rows` rows, and `rta_log.csv` the first rows of `bands`
    /// (its nominal frequencies and row count); rows of a flush that did not
    /// complete are cut off.
    ///
    /// # Errors
    ///
    /// Returns an error when a log has fewer valid rows or cannot be written.
    pub fn resume(
        directory: &Path,
        rows: usize,
        bands: Option<(&[f64], usize)>,
    ) -> Result<Self, SessionError> {
        truncate_after(JournalRows::open(directory, rows)?)?;
        let band_rows = match bands {
            Some((bands_hz, band_rows)) => {
                truncate_after(JournalRows::open_bands(directory, bands_hz, band_rows)?)?;
                band_rows
            }
            None => 0,
        };
        Ok(Self {
            directory: directory.to_owned(),
            log_rows: rows,
            band_rows,
        })
    }

    /// Returns the journal folder.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Returns the number of rows in `log.csv`.
    #[must_use]
    pub const fn log_rows(&self) -> usize {
        self.log_rows
    }

    /// Returns the number of rows in `rta_log.csv`.
    #[must_use]
    pub const fn band_rows(&self) -> usize {
        self.band_rows
    }

    /// Reads back the rows of `log.csv`, oldest first.
    ///
    /// # Errors
    ///
    /// Returns an error when `log.csv` cannot be opened.
    pub fn rows(&self) -> Result<JournalRows<LogRecord>, SessionError> {
        JournalRows::open(&self.directory, self.log_rows)
    }

    /// Reads back the rows of `rta_log.csv` for bands `bands_hz`.
    ///
    /// # Errors
    ///
    /// Returns an error when `rta_log.csv` cannot be opened or has other bands.
    pub fn band_log_rows(&self, bands_hz: &[f64]) -> Result<JournalRows<BandRow>, SessionError> {
        JournalRows::open_bands(&self.directory, bands_hz, self.band_rows)
    }

    /// Appends `new_rows` to `log.csv` and `new_band_rows` to `rta_log.csv`,
    /// replaces the snapshot files with `snapshot`, and finally writes
    /// `recovery_state`. The log rows of `snapshot` itself are ignored.
    ///
    /// # Errors
    ///
    /// Returns an error when a value is not finite or a file cannot be written.
    pub fn flush(
        &mut self,
        new_rows: &[LogRecord],
        new_band_rows: &[BandRow],
        snapshot: &MeasurementBundle,
        recovery_state: &str,
    ) -> Result<(), SessionError> {
        validate_bundle(snapshot)?;
        append_rows(&self.directory.join(LOG_FILE), new_rows.iter().map(log_row))?;
        self.log_rows += new_rows.len();
        if let Some(bands) = &snapshot.bands {
            let path = self.directory.join(BAND_LOG_FILE);
            // Bands chosen after the journal started, or changed before
            // anything was recorded, start the file afresh.
            if self.band_rows == 0 {
                let mut writer = csv_writer(&path)?;
                writer.write_record(band_log_headers(&bands.bands_hz))?;
                finish_csv(writer, &path)?;
            }
            append_rows(&path, new_band_rows.iter().map(band_row))?;
            self.band_rows += new_band_rows.len();
            replace(&self.directory.join(BAND_SEGMENTS_FILE), |path| {
                write_band_segments(path, bands, &snapshot.segments)
            })?;
        }
        replace(&self.directory.join("markers.csv"), |path| {
            write_markers(path, &snapshot.markers)
        })?;
        replace(&self.directory.join("segments.csv"), |path| {
            write_segments(path, &snapshot.segments)
        })?;
        write_measurement_atomically(&self.directory.join("measurement.md"), snapshot, &[])?;
        replace(&self.directory.join(RECOVERY_FILE), |path| {
            write_synced(path, recovery_state)
        })
    }

    /// Deletes the journal folder.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder cannot be removed.
    pub fn remove(self) -> Result<(), SessionError> {
        fs::remove_dir_all(&self.directory).map_err(|source| io_error(&self.directory, source))
    }
}

/// The first rows of a journal's `log.csv` or `rta_log.csv`, read one at a
/// time so a long session never has to fit in memory.
///
/// Rows beyond the requested count were appended by a flush that did not
/// complete and are not read, including a partly written last line. Fewer
/// valid rows than requested end the iteration with an error.
#[derive(Debug)]
pub struct JournalRows<T> {
    reader: csv::Reader<File>,
    path: PathBuf,
    remaining: usize,
    read: usize,
    parse: fn(&csv::StringRecord) -> Option<T>,
}

impl JournalRows<LogRecord> {
    /// Opens the first `rows` rows of the `log.csv` in `directory`.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be opened or has another header.
    pub fn open(directory: &Path, rows: usize) -> Result<Self, SessionError> {
        Self::open_file(&directory.join(LOG_FILE), &LOG_HEADERS, rows, parse_log_row)
    }
}

impl JournalRows<BandRow> {
    /// Opens the first `rows` rows of the `rta_log.csv` in `directory`,
    /// which must hold the bands `bands_hz`.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be opened or has other bands.
    pub fn open_bands(
        directory: &Path,
        bands_hz: &[f64],
        rows: usize,
    ) -> Result<Self, SessionError> {
        let headers = band_log_headers(bands_hz);
        Self::open_file(
            &directory.join(BAND_LOG_FILE),
            &headers,
            rows,
            parse_band_row,
        )
    }
}

impl<T> JournalRows<T> {
    fn open_file(
        path: &Path,
        headers: &[impl AsRef<str>],
        rows: usize,
        parse: fn(&csv::StringRecord) -> Option<T>,
    ) -> Result<Self, SessionError> {
        let file = File::open(path).map_err(|source| io_error(path, source))?;
        let mut reader = ReaderBuilder::new().from_reader(file);
        let read_headers = reader.headers()?;
        if !read_headers.iter().eq(headers.iter().map(AsRef::as_ref)) {
            return Err(invalid(path, "unexpected header".to_owned()));
        }
        Ok(Self {
            reader,
            path: path.to_owned(),
            remaining: rows,
            read: 0,
            parse,
        })
    }

    fn next_row(&mut self) -> Result<T, SessionError> {
        let mut row = csv::StringRecord::new();
        if !self.reader.read_record(&mut row)? {
            return Err(invalid(
                &self.path,
                format!(
                    "{} rows expected, {} found",
                    self.read + self.remaining,
                    self.read
                ),
            ));
        }
        (self.parse)(&row).ok_or_else(|| {
            invalid(
                &self.path,
                format!("row {} cannot be parsed", self.read + 1),
            )
        })
    }
}

impl<T> Iterator for JournalRows<T> {
    type Item = Result<T, SessionError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let row = self.next_row();
        // An error ends the iteration.
        self.remaining = if row.is_ok() { self.remaining - 1 } else { 0 };
        self.read += 1;
        Some(row)
    }
}

/// Reads all of `rows` and cuts its file off after them.
fn truncate_after<T>(mut rows: JournalRows<T>) -> Result<(), SessionError> {
    for row in rows.by_ref() {
        row?;
    }
    let length = rows.reader.position().byte();
    OpenOptions::new()
        .write(true)
        .open(&rows.path)
        .and_then(|file| file.set_len(length).and_then(|()| file.sync_all()))
        .map_err(|source| io_error(&rows.path, source))
}

/// Appends CSV rows to the existing file at `path`.
fn append_rows<R: IntoIterator<Item = impl AsRef<[u8]>>>(
    path: &Path,
    rows: impl Iterator<Item = R>,
) -> Result<(), SessionError> {
    let mut rows = rows.peekable();
    if rows.peek().is_none() {
        return Ok(());
    }
    let file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|source| io_error(path, source))?;
    let mut writer = WriterBuilder::new()
        .has_headers(false)
        .terminator(Terminator::Any(b'\n'))
        .from_writer(file);
    for row in rows {
        writer.write_record(row)?;
    }
    finish_csv(writer, path)
}

fn invalid(path: &Path, reason: String) -> SessionError {
    SessionError::InvalidJournal {
        path: path.to_owned(),
        reason,
    }
}

fn parse_band_row(row: &csv::StringRecord) -> Option<BandRow> {
    let levels_db = row
        .iter()
        .skip(2)
        .map(|cell| match cell {
            "" => Some(None),
            cell => cell.parse().ok().map(Some),
        })
        .collect::<Option<_>>()?;
    Some(BandRow {
        t_s: row.get(0)?.parse().ok()?,
        utc: OffsetDateTime::parse(row.get(1)?, &Rfc3339).ok()?,
        levels_db,
    })
}

fn parse_log_row(row: &csv::StringRecord) -> Option<LogRecord> {
    let number = |index: usize| row.get(index)?.parse::<f64>().ok();
    let marker_id = match row.get(2)? {
        "" => None,
        id => Some(id.parse().ok()?),
    };
    Some(LogRecord {
        t_s: number(0)?,
        utc: OffsetDateTime::parse(row.get(1)?, &Rfc3339).ok()?,
        marker_id,
        clipped: row.get(3)? == "1",
        laf_db: number(4)?,
        lcf_db: number(5)?,
        lzf_db: number(6)?,
    })
}

/// Writes `path` through a synced sibling temporary file and a rename.
fn replace(
    path: &Path,
    write: impl FnOnce(&Path) -> Result<(), SessionError>,
) -> Result<(), SessionError> {
    let mut temporary_name = path.file_name().unwrap_or_default().to_owned();
    temporary_name.push(".tmp");
    let temporary = path.with_file_name(temporary_name);
    write(&temporary)?;
    fs::rename(&temporary, path).map_err(|source| io_error(path, source))
}

fn write_synced(path: &Path, text: &str) -> Result<(), SessionError> {
    use std::io::Write as _;
    let mut file = File::create(path).map_err(|source| io_error(path, source))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|source| io_error(path, source))
}

fn io_error(path: &Path, source: std::io::Error) -> SessionError {
    SessionError::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests;

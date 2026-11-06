//! Persistent, append-only calibration history (specification 4.9, steps 6–8).
//!
//! Records live in one TOML file (`calibrations.toml`, settings schema
//! version [`SCHEMA_VERSION`]). A calibration belongs to a profile, the pair
//! (device id, channel); the profile's active calibration is its most recently
//! saved record. Restoring an older record appends a copy, so history is never
//! rewritten.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use spl_dsp::calibration::CalibrationProposal;
use spl_session::{Calibration, CalibrationType};
use time::OffsetDateTime;

use crate::atomic_file::write_atomically;

/// Settings schema version written to `calibrations.toml`.
///
/// Readers accept any file with this major version and ignore unknown keys;
/// migrations from older versions belong in this module.
pub const SCHEMA_VERSION: u32 = 1;

/// The input a calibration applies to.
///
/// Only `device_id` and `channel_index` identify the profile; the display
/// name and sample rate are recorded for the user's reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputProfile {
    /// Stable backend device identifier.
    pub device_id: String,
    /// Human-readable device name at the time of calibration.
    pub device_name: String,
    /// Zero-based input channel.
    pub channel_index: u16,
    /// Negotiated stream sample rate in hertz.
    pub sample_rate_hz: u32,
}

/// How a calibration offset was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CalibrationKind {
    /// Measured from an acoustic calibrator tone.
    Calibrator,
    /// Entered by the user.
    Manual,
}

/// One saved calibration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRecord {
    /// When this record was saved (or restored).
    #[serde(with = "time::serde::rfc3339")]
    pub saved_utc: OffsetDateTime,
    /// When the calibration itself was measured or entered.
    #[serde(with = "time::serde::rfc3339")]
    pub calibrated_utc: OffsetDateTime,
    /// Stable backend device identifier.
    pub device_id: String,
    /// Device display name at calibration time.
    pub device_name: String,
    /// Zero-based input channel.
    pub channel_index: u16,
    /// Stream sample rate in hertz at calibration time.
    pub sample_rate_hz: u32,
    /// Origin of the offset.
    pub kind: CalibrationKind,
    /// Calibration offset `C` in dB (SPL = dBFS + C).
    pub offset_db: f64,
    /// Calibrator reference level in dB SPL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibrator_level_db: Option<f64>,
    /// Detected calibrator tone frequency in hertz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_frequency_hz: Option<f64>,
    /// Measured raw tone level in dBFS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_level_dbfs: Option<f64>,
    /// Standard deviation of one-second tone levels in dB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stability_db: Option<f64>,
    /// Tone-to-background power ratio in dB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_to_background_db: Option<f64>,
    /// Largest raw sample during capture in dBFS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_dbfs: Option<f64>,
    /// Free-text note.
    #[serde(default)]
    pub note: String,
}

impl CalibrationRecord {
    /// Creates a record for an approved calibrator capture.
    #[must_use]
    pub fn from_proposal(
        input: &InputProfile,
        proposal: &CalibrationProposal,
        note: impl Into<String>,
        saved_utc: OffsetDateTime,
    ) -> Self {
        Self {
            calibrator_level_db: Some(proposal.calibrator_level_db),
            tone_frequency_hz: Some(proposal.frequency_hz),
            tone_level_dbfs: Some(proposal.measured_level_dbfs),
            stability_db: Some(proposal.stability_db),
            tone_to_background_db: Some(proposal.tone_to_background_db),
            peak_dbfs: Some(proposal.peak_dbfs),
            note: note.into(),
            ..Self::base(
                input,
                CalibrationKind::Calibrator,
                proposal.offset_db,
                saved_utc,
            )
        }
    }

    /// Creates a record for a manually entered offset.
    #[must_use]
    pub fn manual(input: &InputProfile, offset_db: f64, saved_utc: OffsetDateTime) -> Self {
        Self::base(input, CalibrationKind::Manual, offset_db, saved_utc)
    }

    /// Returns a copy re-saved at `saved_utc`, keeping the original measurement time.
    #[must_use]
    pub fn restored(&self, saved_utc: OffsetDateTime) -> Self {
        Self {
            saved_utc,
            ..self.clone()
        }
    }

    /// Returns whether this record belongs to `input`'s profile.
    #[must_use]
    pub fn belongs_to(&self, input: &InputProfile) -> bool {
        self.device_id == input.device_id && self.channel_index == input.channel_index
    }

    /// Converts the record into the metadata applied to measurements.
    #[must_use]
    pub fn calibration(&self) -> Calibration {
        Calibration {
            offset_db: self.offset_db,
            calibration_type: match self.kind {
                CalibrationKind::Calibrator => CalibrationType::Calibrator,
                CalibrationKind::Manual => CalibrationType::Manual,
            },
            calibrator_level_db: self.calibrator_level_db,
            calibrator_freq_hz: self.tone_frequency_hz,
            calibrated_utc: Some(self.calibrated_utc),
        }
    }

    fn base(
        input: &InputProfile,
        kind: CalibrationKind,
        offset_db: f64,
        saved_utc: OffsetDateTime,
    ) -> Self {
        Self {
            saved_utc,
            calibrated_utc: saved_utc,
            device_id: input.device_id.clone(),
            device_name: input.device_name.clone(),
            channel_index: input.channel_index,
            sample_rate_hz: input.sample_rate_hz,
            kind,
            offset_db,
            calibrator_level_db: None,
            tone_frequency_hz: None,
            tone_level_dbfs: None,
            stability_db: None,
            tone_to_background_db: None,
            peak_dbfs: None,
            note: String::new(),
        }
    }
}

/// Failure reading or writing the calibration history.
#[derive(Debug, thiserror::Error)]
pub enum CalibrationStoreError {
    /// The file exists but could not be read.
    #[error("could not read {path}: {source}")]
    Read {
        /// File that failed.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The file is not valid calibration TOML.
    #[error("could not parse {path}: {source}")]
    Parse {
        /// File that failed.
        path: PathBuf,
        /// Underlying TOML error.
        source: toml::de::Error,
    },
    /// The file was written by a newer, incompatible version.
    #[error("calibration file version {0} is newer than this application supports")]
    UnsupportedVersion(u32),
    /// The records could not be encoded as TOML.
    #[error("could not encode calibration history: {0}")]
    Serialize(toml::ser::Error),
    /// The history could not be written.
    #[error("could not save calibration history to {path}: {source}")]
    Write {
        /// File that failed.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct CalibrationFile {
    version: u32,
    #[serde(default, rename = "calibration")]
    records: Vec<CalibrationRecord>,
}

/// Append-only calibration history backed by `calibrations.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationStore {
    path: Option<PathBuf>,
    records: Vec<CalibrationRecord>,
}

impl CalibrationStore {
    /// Opens the history at `path`; a missing file is an empty history.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed, or was written
    /// by a newer major version. The file is never modified by opening it.
    pub fn open(path: &Path) -> Result<Self, CalibrationStoreError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(CalibrationStoreError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        let records = if text.trim().is_empty() {
            Vec::new()
        } else {
            let file: CalibrationFile =
                toml::from_str(&text).map_err(|source| CalibrationStoreError::Parse {
                    path: path.to_owned(),
                    source,
                })?;
            if file.version > SCHEMA_VERSION {
                return Err(CalibrationStoreError::UnsupportedVersion(file.version));
            }
            file.records
        };
        Ok(Self {
            path: Some(path.to_owned()),
            records,
        })
    }

    /// Creates a history that is kept in memory only.
    ///
    /// Used when the file cannot be opened, so an unreadable file is never
    /// overwritten.
    #[must_use]
    pub fn unsaved() -> Self {
        Self {
            path: None,
            records: Vec::new(),
        }
    }

    /// Returns the history file, or `None` when records are kept in memory only.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Returns the active (most recently saved) record for `input`'s profile.
    #[must_use]
    pub fn active(&self, input: &InputProfile) -> Option<&CalibrationRecord> {
        self.records
            .iter()
            .rev()
            .find(|record| record.belongs_to(input))
    }

    /// Returns all records for `input`'s profile, newest first.
    #[must_use]
    pub fn history(&self, input: &InputProfile) -> Vec<&CalibrationRecord> {
        self.records
            .iter()
            .rev()
            .filter(|record| record.belongs_to(input))
            .collect()
    }

    /// Appends `record` and rewrites the file atomically.
    ///
    /// The record stays in memory even if writing fails, so the active
    /// calibration matches what the user just approved.
    ///
    /// # Errors
    ///
    /// Returns an error when the history cannot be encoded or written.
    pub fn append(&mut self, record: CalibrationRecord) -> Result<(), CalibrationStoreError> {
        self.records.push(record);
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = CalibrationFile {
            version: SCHEMA_VERSION,
            records: self.records.clone(),
        };
        let text = toml::to_string(&file).map_err(CalibrationStoreError::Serialize)?;
        write_atomically(path, &text).map_err(|source| CalibrationStoreError::Write {
            path: path.clone(),
            source,
        })
    }
}

#[cfg(test)]
mod tests;

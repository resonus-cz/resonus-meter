//! Engine side of the session journal and crash recovery (ADR 0010).
//!
//! Each running session journals into its own folder below a sessions
//! directory chosen by the application. `recovery.toml` holds the exact
//! recorder and meter state; the remaining files are a readable bundle.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use spl_session::{Device, RECOVERY_FILE, SessionError};
use time::OffsetDateTime;

use crate::recorder::{LevelsState, RecorderState};

/// Version of `recovery.toml`; a different version is not recovered.
pub const RECOVERY_VERSION: u32 = 1;

/// Failure writing, reading or recovering a session journal.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// A journal file could not be written or read.
    #[error(transparent)]
    Session(#[from] SessionError),
    /// The recovery state could not be encoded.
    #[error("could not encode the recovery state: {0}")]
    Encode(#[from] toml::ser::Error),
    /// `recovery.toml` could not be read.
    #[error("could not read {path}: {source}")]
    Read {
        /// File that failed.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// `recovery.toml` is not valid.
    #[error("could not parse {path}: {source}")]
    Decode {
        /// File that failed.
        path: PathBuf,
        /// Underlying TOML error.
        source: toml::de::Error,
    },
    /// The journal was written by an incompatible version.
    #[error("recovery file version {0} is not supported")]
    UnsupportedVersion(u32),
    /// The journal was recorded at another stream rate than the open input.
    #[error("the journal was recorded at {recorded_hz} Hz, the input runs at {input_hz} Hz")]
    SampleRateMismatch {
        /// Rate of the journal.
        recorded_hz: f64,
        /// Rate of the open input.
        input_hz: f64,
    },
    /// A journal folder could not be deleted.
    #[error("could not delete {path}: {source}")]
    Remove {
        /// Folder that failed.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// There is no session to journal.
    #[error("no active measurement session")]
    NoActiveSession,
}

/// The persisted engine state at a journal flush.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct RecoveryFile {
    pub(crate) version: u32,
    /// Meter Leq, peak and Fast maximum since reset.
    pub(crate) display: [LevelsState; 3],
    pub(crate) clipped: bool,
    pub(crate) recorder: RecorderState,
}

impl RecoveryFile {
    pub(crate) fn read(directory: &Path) -> Result<Self, JournalError> {
        let path = directory.join(RECOVERY_FILE);
        let text = fs::read_to_string(&path).map_err(|source| JournalError::Read {
            path: path.clone(),
            source,
        })?;
        let file: Self =
            toml::from_str(&text).map_err(|source| JournalError::Decode { path, source })?;
        if file.version != RECOVERY_VERSION {
            return Err(JournalError::UnsupportedVersion(file.version));
        }
        Ok(file)
    }
}

/// A journal left by a session that was not saved and closed.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoverableSession {
    /// Journal folder.
    pub directory: PathBuf,
    /// Measurement name.
    pub name: String,
    /// When the session started.
    pub started_utc: OffsetDateTime,
    /// Session time covered by the journal.
    pub recorded_until_s: f64,
    /// Input that recorded the session.
    pub device: Device,
    /// Whether anything was recorded or marked; a journal without data has
    /// nothing to recover.
    pub holds_data: bool,
}

impl RecoverableSession {
    /// Deletes the journal instead of recovering it.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder cannot be removed.
    pub fn discard(&self) -> Result<(), JournalError> {
        discard_journal_directory(&self.directory)
    }
}

/// Deletes a journal folder, readable or not.
///
/// # Errors
///
/// Returns an error when the folder cannot be removed.
pub fn discard_journal_directory(directory: &Path) -> Result<(), JournalError> {
    fs::remove_dir_all(directory).map_err(|source| JournalError::Remove {
        path: directory.to_owned(),
        source,
    })
}

/// Lists the journals in `sessions_directory`, newest first.
///
/// Folders whose recovery state cannot be read are returned with their error
/// so the application can report them; they are never deleted here.
#[must_use]
pub fn recoverable_sessions(
    sessions_directory: &Path,
) -> Vec<Result<RecoverableSession, (PathBuf, JournalError)>> {
    let Ok(entries) = fs::read_dir(sessions_directory) else {
        return Vec::new();
    };
    let mut directories = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join(RECOVERY_FILE).is_file())
        .collect::<Vec<_>>();
    // Folder names start with the session's start time in milliseconds.
    directories.sort_unstable_by(|left, right| right.cmp(left));
    directories
        .into_iter()
        .map(|directory| match RecoveryFile::read(&directory) {
            Ok(file) => Ok(RecoverableSession {
                name: file.recorder.config().name.clone(),
                started_utc: file.recorder.started_utc(),
                recorded_until_s: file.recorder.elapsed_s(),
                device: file.recorder.config().device.clone(),
                holds_data: file.recorder.holds_data(),
                directory,
            }),
            Err(error) => Err((directory, error)),
        })
        .collect()
}

/// Returns the journal folder of a session started at `started_utc`.
pub(crate) fn session_directory(sessions_directory: &Path, started_utc: OffsetDateTime) -> PathBuf {
    let started_ms = started_utc.unix_timestamp_nanos() / 1_000_000;
    sessions_directory.join(format!("session-{started_ms:015}"))
}

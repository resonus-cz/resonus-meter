//! Journaling the active session and recovering it after a crash.

use std::path::Path;

use spl_session::{CalibrationType, JournalRows, JournalWriter};
use time::OffsetDateTime;

use super::{Chain, Engine, active_offset_db};
use crate::JournalError;
use crate::journal::{RECOVERY_VERSION, RecoveryFile, session_directory};
use crate::recorder::{LevelsState, SessionRecorder};

impl Engine {
    /// Starts journaling the active session into a new folder below
    /// `sessions_directory` and writes it once.
    ///
    /// # Errors
    ///
    /// Returns an error when no session is active or the journal cannot be written.
    pub fn start_journal(&mut self, sessions_directory: &Path) -> Result<(), JournalError> {
        let recorder = self
            .recorder
            .as_ref()
            .ok_or(JournalError::NoActiveSession)?;
        let directory = session_directory(sessions_directory, recorder.started_utc());
        self.journal = Some(JournalWriter::create(&directory)?);
        self.sessions_directory = Some(sessions_directory.to_owned());
        self.flush_journal()
    }

    /// Writes new log rows and the current state to the journal, if one is active.
    ///
    /// # Errors
    ///
    /// Returns an error when writing fails, or reports an earlier failure to
    /// restart the journal after Reset.
    pub fn flush_journal(&mut self) -> Result<(), JournalError> {
        if let Some(error) = self.journal_error.take() {
            return Err(error);
        }
        let (Some(journal), Some(recorder)) = (&mut self.journal, &mut self.recorder) else {
            return Ok(());
        };
        let snapshot = recorder.journal_snapshot(active_offset_db(self.calibration.as_ref()));
        let rows = recorder.journal_rows();
        let band_rows = recorder.journal_band_rows();
        let state = RecoveryFile {
            version: RECOVERY_VERSION,
            display: [self.a.state(), self.c.state(), self.z.state()],
            clipped: self.clipped,
            recorder: recorder.recovery_state(
                journal.log_rows() + rows.len(),
                journal.band_rows() + band_rows.len(),
            ),
        };
        journal.flush(rows, band_rows, &snapshot, &toml::to_string(&state)?)?;
        let counts = (rows.len(), band_rows.len());
        recorder.journaled(counts.0, counts.1);
        Ok(())
    }

    /// Deletes the journal; the session is no longer recoverable.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal folder cannot be removed.
    pub fn discard_journal(&mut self) -> Result<(), JournalError> {
        self.sessions_directory = None;
        match self.journal.take() {
            Some(journal) => Ok(journal.remove()?),
            None => Ok(()),
        }
    }

    /// Returns the active journal folder.
    #[must_use]
    pub fn journal_directory(&self) -> Option<&Path> {
        self.journal.as_ref().map(JournalWriter::directory)
    }

    /// Returns whether anything was recorded or marked since the last save.
    #[must_use]
    pub fn has_unsaved_changes(&self) -> bool {
        self.recorder
            .as_ref()
            .is_some_and(SessionRecorder::has_unsaved_changes)
    }

    /// Replaces the active session with the one journaled in `directory`,
    /// paused, and continues journaling there.
    ///
    /// The calibration recorded with the session becomes active again. The
    /// journal of the replaced session is deleted.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be read, was recorded at
    /// another sample rate, or cannot be written again.
    pub fn recover_session(
        &mut self,
        directory: &Path,
        now_utc: OffsetDateTime,
    ) -> Result<(), JournalError> {
        let file = RecoveryFile::read(directory)?;
        let recorded_hz = file.recorder.sample_rate_hz();
        if (recorded_hz - self.sample_rate_hz).abs() > f64::EPSILON {
            return Err(JournalError::SampleRateMismatch {
                recorded_hz,
                input_hz: self.sample_rate_hz,
            });
        }
        let calibration = file.recorder.config().calibration.clone();
        let log_rows = file.recorder.log_rows;
        let band_rows = file.recorder.bands.as_ref().map_or(0, |bands| bands.rows);
        let mut recorder = SessionRecorder::restore(file.recorder, now_utc);
        let bands_hz = recorder.band_nominal_hz();
        let bands = bands_hz.as_deref().map(|bands_hz| (bands_hz, band_rows));
        // Validates the logs before the current session is given up.
        JournalRows::open(directory, log_rows)?.try_for_each(|row| row.map(drop))?;
        if let Some((bands_hz, rows)) = bands {
            JournalRows::open_bands(directory, bands_hz, rows)?
                .try_for_each(|row| row.map(drop))?;
        }
        self.discard_journal()?;
        self.calibration =
            (calibration.calibration_type != CalibrationType::None).then_some(calibration);
        let [a, c, z] = file.display;
        self.a.restore(a);
        self.c.restore(c);
        self.z.restore(z);
        self.clipped = file.clipped;
        let journal = JournalWriter::resume(directory, log_rows, bands)?;
        for row in journal.rows()? {
            recorder.restore_row(row?);
        }
        self.journal = Some(journal);
        self.recorder = Some(recorder);
        self.sessions_directory = directory.parent().map(Path::to_owned);
        self.flush_journal()
    }

    /// Writes the journal after the markers changed, so a crash cannot lose
    /// them; a failure is reported by the next [`Self::flush_journal`].
    pub(super) fn journal_marker_change(&mut self) {
        if let Err(error) = self.flush_journal() {
            self.journal_error = Some(error);
        }
    }

    /// Replaces the journal of a discarded recording after Reset.
    pub(super) fn restart_journal(&mut self) {
        let Some(sessions_directory) = self.sessions_directory.clone() else {
            return;
        };
        let result = self
            .discard_journal()
            .and_then(|()| self.start_journal(&sessions_directory));
        if let Err(error) = result {
            self.journal_error = Some(error);
        }
    }
}

impl Chain {
    fn state(&self) -> LevelsState {
        LevelsState::new(&self.accumulator, self.fast_max_dbfs)
    }

    fn restore(&mut self, state: LevelsState) {
        self.accumulator = state.level();
        self.fast_max_dbfs = state.fast_max_dbfs();
    }
}

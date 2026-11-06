use super::Engine;
use crate::CalibrationCaptureState;
use spl_dsp::calibration::{self, CalibrationError, CalibrationProposal};
use spl_session::{Calibration, CalibrationType};
use time::OffsetDateTime;

#[derive(Debug)]
pub(super) struct CalibrationCapture {
    calibrator_level_db: f64,
    target_frequency_hz: f64,
    target_samples: usize,
    samples: Vec<f32>,
    result: Option<Result<CalibrationProposal, CalibrationError>>,
    peak_amplitude: f64,
}

impl Engine {
    /// Starts collecting eight seconds of raw samples for calibration.
    ///
    /// The caller supplies the calibrator's stated SPL level and nominal
    /// frequency. The proposal remains inactive until
    /// [`Self::save_calibration_proposal`] is called.
    ///
    /// # Errors
    ///
    /// Returns an error if the frequency is not representable by this stream.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn start_calibration_capture(
        &mut self,
        calibrator_level_db: f64,
        target_frequency_hz: f64,
    ) -> Result<(), CalibrationError> {
        if !(target_frequency_hz.is_sign_positive()
            && target_frequency_hz < self.sample_rate_hz / 2.0)
        {
            return Err(CalibrationError::InvalidFrequency);
        }
        let target_samples = (self.sample_rate_hz * 8.0).round() as usize;
        self.calibration_capture = Some(CalibrationCapture {
            calibrator_level_db,
            target_frequency_hz,
            target_samples,
            samples: Vec::with_capacity(target_samples),
            result: None,
            peak_amplitude: 0.0,
        });
        Ok(())
    }

    /// Returns automatic calibration capture progress or its finished result.
    #[must_use]
    pub fn calibration_capture_state(&self) -> CalibrationCaptureState {
        let Some(capture) = &self.calibration_capture else {
            return CalibrationCaptureState::Idle;
        };
        if let Some(result) = capture.result {
            CalibrationCaptureState::Complete(result)
        } else {
            CalibrationCaptureState::Capturing {
                captured_samples: capture.samples.len(),
                target_samples: capture.target_samples,
                peak_dbfs: spl_dsp::units::amplitude_to_dbfs(capture.peak_amplitude),
            }
        }
    }

    /// Applies an approved automatic proposal and starts a fresh recording.
    ///
    /// Returns the applied proposal so the caller can persist a calibration
    /// record; the engine itself does not store calibration history.
    ///
    /// # Errors
    ///
    /// Returns the completed capture's quality failure, or an error when no
    /// capture has completed.
    pub fn save_calibration_proposal(&mut self) -> Result<CalibrationProposal, CalibrationError> {
        let result = self
            .calibration_capture
            .as_ref()
            .and_then(|capture| capture.result)
            .ok_or(CalibrationError::EmptyCapture)?;
        let proposal = result?;
        self.set_calibration(Some(Calibration {
            offset_db: proposal.offset_db,
            calibration_type: CalibrationType::Calibrator,
            calibrator_level_db: Some(proposal.calibrator_level_db),
            calibrator_freq_hz: Some(proposal.frequency_hz),
            calibrated_utc: Some(OffsetDateTime::now_utc()),
        }));
        self.calibration_capture = None;
        self.reset();
        Ok(proposal)
    }

    /// Returns the SPL level of a full-scale sine wave for the active calibration.
    #[must_use]
    pub fn full_scale_sine_spl_db(&self) -> Option<f64> {
        const FULL_SCALE_SINE_RMS_DBFS: f64 = -3.010_299_956_64;
        self.calibration_offset_db()
            .map(|offset_db| offset_db + FULL_SCALE_SINE_RMS_DBFS)
    }

    pub(super) fn capture_raw_samples(&mut self, samples: &[f32]) {
        let Some(capture) = &mut self.calibration_capture else {
            return;
        };
        if capture.result.is_some() {
            return;
        }
        let remaining = capture.target_samples.saturating_sub(capture.samples.len());
        let captured = &samples[..samples.len().min(remaining)];
        capture.peak_amplitude = captured
            .iter()
            .fold(capture.peak_amplitude, |peak, sample| {
                peak.max(f64::from(sample.abs()))
            });
        capture.samples.extend_from_slice(captured);
        if capture.samples.len() == capture.target_samples {
            capture.result = Some(calibration::propose(
                &capture.samples,
                self.sample_rate_hz,
                capture.target_frequency_hz,
                capture.calibrator_level_db,
            ));
        }
    }
}

//! Calibration offset calculation and raw-tone measurement.

use std::f64::consts::TAU;

use crate::units::{amplitude_to_dbfs, power_to_dbfs};

/// A proposed calibration result that must be explicitly saved by the caller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationProposal {
    /// Calibrator reference level in dB SPL.
    pub calibrator_level_db: f64,
    /// Measured raw tone level in dBFS.
    pub measured_level_dbfs: f64,
    /// Frequency used for the lock-in measurement in hertz.
    pub frequency_hz: f64,
    /// Calibration offset to apply to future levels.
    pub offset_db: f64,
    /// Standard deviation of one-second raw-tone levels in dB.
    pub stability_db: f64,
    /// Tone-to-background power ratio in decibels.
    pub tone_to_background_db: f64,
    /// Largest raw sample amplitude during capture, in dBFS.
    pub peak_dbfs: f64,
}

/// Failure returned when a calibration capture is unsuitable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalibrationError {
    /// No samples were supplied.
    EmptyCapture,
    /// The requested frequency cannot be represented by the sample rate.
    InvalidFrequency,
    /// No sufficiently distinct tone was found near the requested frequency.
    NoToneNearTarget,
    /// The measured signal is below the minimum useful capture level.
    SignalTooQuiet,
    /// A raw sample reached the overload threshold.
    Clipped,
    /// The lock-in component was not sufficiently stronger than total energy.
    TooMuchBackgroundNoise,
    /// One-second calibration levels vary by more than 0.1 dB.
    UnstableTone,
}

impl std::fmt::Display for CalibrationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyCapture => "no samples were captured",
            Self::InvalidFrequency => "calibration frequency is outside the valid range",
            Self::NoToneNearTarget => "no tone was found near the calibration frequency",
            Self::SignalTooQuiet => "calibration tone is below -60 dBFS",
            Self::Clipped => "the calibration capture clipped",
            Self::TooMuchBackgroundNoise => "background noise is too high for calibration",
            Self::UnstableTone => "calibration tone level is unstable",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CalibrationError {}

/// Extracts a raw RMS tone level near a known calibrator frequency.
///
/// A Hann-windowed narrow-band search locates the peak within three percent of
/// `frequency_hz`. A parabolic refinement is then measured with a per-second
/// lock-in, which rejects components not coherent with the detected tone.
///
/// # Errors
///
/// Returns a quality-check failure when the captured signal cannot produce a
/// reliable calibration proposal.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub fn propose(
    samples: &[f32],
    sample_rate_hz: f64,
    frequency_hz: f64,
    calibrator_level_db: f64,
) -> Result<CalibrationProposal, CalibrationError> {
    if samples.is_empty() {
        return Err(CalibrationError::EmptyCapture);
    }
    if !(frequency_hz.is_sign_positive() && frequency_hz < sample_rate_hz / 2.0) {
        return Err(CalibrationError::InvalidFrequency);
    }

    if samples
        .iter()
        .any(|sample| sample.abs() >= crate::units::CLIP_LEVEL)
    {
        return Err(CalibrationError::Clipped);
    }
    let raw_power = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / samples.len() as f64;
    if power_to_dbfs(raw_power) < -60.0 {
        return Err(CalibrationError::SignalTooQuiet);
    }
    let peak_dbfs = samples
        .iter()
        .map(|sample| amplitude_to_dbfs(f64::from(*sample)))
        .fold(f64::NEG_INFINITY, f64::max);

    let detected_frequency_hz = find_tone_frequency(samples, sample_rate_hz, frequency_hz)?;
    let one_second_len = sample_rate_hz.round() as usize;
    let mut tone_powers = Vec::new();
    let mut total_power = 0.0;
    let mut total_count = 0_usize;
    for window in samples.chunks_exact(one_second_len) {
        tone_powers.push(lock_in_power(window, sample_rate_hz, detected_frequency_hz));
        total_power += window
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>();
        total_count += window.len();
    }
    if tone_powers.is_empty() {
        return Err(CalibrationError::EmptyCapture);
    }
    let tone_power = tone_powers.iter().sum::<f64>() / tone_powers.len() as f64;
    let tone_level_dbfs = power_to_dbfs(tone_power);
    if tone_level_dbfs < -60.0 {
        return Err(CalibrationError::SignalTooQuiet);
    }

    let total_power = total_power / total_count as f64;
    let residual_power = (total_power - tone_power).max(f64::MIN_POSITIVE);
    let tone_to_background_db = 10.0 * (tone_power / residual_power).log10();
    if tone_to_background_db < 20.0 {
        return Err(CalibrationError::TooMuchBackgroundNoise);
    }
    let stability_db = standard_deviation_db(&tone_powers);
    if stability_db >= 0.1 {
        return Err(CalibrationError::UnstableTone);
    }

    Ok(CalibrationProposal {
        calibrator_level_db,
        measured_level_dbfs: tone_level_dbfs,
        frequency_hz: detected_frequency_hz,
        offset_db: calibrator_level_db - tone_level_dbfs,
        stability_db,
        tone_to_background_db,
        peak_dbfs,
    })
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn find_tone_frequency(
    samples: &[f32],
    sample_rate_hz: f64,
    target_frequency_hz: f64,
) -> Result<f64, CalibrationError> {
    let window_len = sample_rate_hz.round() as usize;
    let window = samples
        .get(..window_len)
        .ok_or(CalibrationError::EmptyCapture)?;
    let minimum_hz = target_frequency_hz * 0.97;
    let maximum_hz = target_frequency_hz * 1.03;
    let bin_width_hz = sample_rate_hz / window_len as f64;
    let first_bin = (minimum_hz / bin_width_hz).ceil() as usize;
    let last_bin = (maximum_hz / bin_width_hz).floor() as usize;
    if first_bin >= last_bin {
        return Err(CalibrationError::NoToneNearTarget);
    }

    let powers = (first_bin..=last_bin)
        .map(|bin| spectral_power(window, sample_rate_hz, bin as f64 * bin_width_hz))
        .collect::<Vec<_>>();
    let Some((peak_index, peak_power)) = powers
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
    else {
        return Err(CalibrationError::NoToneNearTarget);
    };
    if *peak_power <= f64::MIN_POSITIVE {
        return Err(CalibrationError::NoToneNearTarget);
    }
    let peak_bin = first_bin + peak_index;
    if peak_index == 0 || peak_index + 1 == powers.len() {
        return Ok(peak_bin as f64 * bin_width_hz);
    }
    let left = powers[peak_index - 1].max(f64::MIN_POSITIVE).ln();
    let centre = powers[peak_index].max(f64::MIN_POSITIVE).ln();
    let right = powers[peak_index + 1].max(f64::MIN_POSITIVE).ln();
    let denominator = left - 2.0 * centre + right;
    let adjustment_bins = if denominator.abs() > f64::MIN_POSITIVE {
        (0.5 * (left - right) / denominator).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    Ok((peak_bin as f64 + adjustment_bins) * bin_width_hz)
}

#[allow(clippy::cast_precision_loss)]
fn spectral_power(samples: &[f32], sample_rate_hz: f64, frequency_hz: f64) -> f64 {
    let sample_count = samples.len() as f64;
    let mut in_phase = 0.0;
    let mut quadrature = 0.0;
    for (index, sample) in samples.iter().enumerate() {
        let phase = TAU * frequency_hz * index as f64 / sample_rate_hz;
        let hann = 0.5 - 0.5 * (TAU * index as f64 / (sample_count - 1.0)).cos();
        let weighted = f64::from(*sample) * hann;
        in_phase += weighted * phase.cos();
        quadrature += weighted * phase.sin();
    }
    in_phase.mul_add(in_phase, quadrature * quadrature)
}

#[allow(clippy::cast_precision_loss)]
fn lock_in_power(samples: &[f32], sample_rate_hz: f64, frequency_hz: f64) -> f64 {
    let mut in_phase = 0.0;
    let mut quadrature = 0.0;
    for (index, sample) in samples.iter().enumerate() {
        let phase = TAU * frequency_hz * index as f64 / sample_rate_hz;
        in_phase += f64::from(*sample) * phase.cos();
        quadrature += f64::from(*sample) * phase.sin();
    }
    2.0 * (in_phase.mul_add(in_phase, quadrature * quadrature)) / (samples.len() as f64).powi(2)
}

fn standard_deviation_db(powers: &[f64]) -> f64 {
    let mut count = 0.0;
    let mut mean = 0.0;
    let mut squared_deviation_sum = 0.0;
    for power in powers {
        let level_dbfs = power_to_dbfs(*power);
        count += 1.0;
        let delta = level_dbfs - mean;
        mean += delta / count;
        squared_deviation_sum += delta * (level_dbfs - mean);
    }
    if count > 1.0 {
        (squared_deviation_sum / (count - 1.0)).sqrt()
    } else {
        0.0
    }
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
mod tests {
    use super::{CalibrationError, propose};

    #[test]
    fn extracts_a_known_tone_and_calibration_offset() {
        let sample_rate_hz = 48_000.0;
        let samples = (0..(sample_rate_hz as usize * 2))
            .map(|index| {
                (std::f64::consts::TAU * 1_000.0 * index as f64 / sample_rate_hz).sin() as f32 * 0.1
            })
            .collect::<Vec<_>>();
        let proposal = propose(&samples, sample_rate_hz, 1_000.0, 94.0).expect("valid tone");

        assert!((proposal.measured_level_dbfs + 23.010_299_956_64).abs() < 1e-7);
        assert!((proposal.offset_db - 117.010_299_956_64).abs() < 1e-7);
        assert!((proposal.frequency_hz - 1_000.0).abs() < 0.01);
        assert!(proposal.tone_to_background_db > 100.0);
    }

    #[test]
    fn refines_a_tone_frequency_near_the_calibrator_target() {
        let sample_rate_hz = 48_000.0;
        let samples = (0..(sample_rate_hz as usize * 8))
            .map(|index| {
                (std::f64::consts::TAU * 1_007.3 * index as f64 / sample_rate_hz).sin() as f32 * 0.1
            })
            .collect::<Vec<_>>();

        let proposal = propose(&samples, sample_rate_hz, 1_000.0, 94.0).expect("valid tone");

        assert!((proposal.frequency_hz - 1_007.3).abs() < 0.1);
        assert!((proposal.measured_level_dbfs + 23.0103).abs() < 0.02);
    }

    #[test]
    fn clips_at_the_top_16_bit_code() {
        // CLIP_LEVEL = 1 - 2^-15: 32767/32768 clips, one code below does not.
        let sample_rate_hz = 48_000.0;
        let mut samples = (0..(sample_rate_hz as usize * 2))
            .map(|index| {
                (std::f64::consts::TAU * 1_000.0 * index as f64 / sample_rate_hz).sin() as f32 * 0.1
            })
            .collect::<Vec<_>>();
        samples[100] = -32_767.0 / 32_768.0;
        assert_eq!(
            propose(&samples, sample_rate_hz, 1_000.0, 94.0),
            Err(CalibrationError::Clipped)
        );
        samples[100] = 32_766.0 / 32_768.0;
        assert!(propose(&samples, sample_rate_hz, 1_000.0, 94.0).is_ok());
    }

    #[test]
    fn rejects_silence() {
        let samples = vec![0.0; 48_000];
        assert_eq!(
            propose(&samples, 48_000.0, 1_000.0, 94.0),
            Err(CalibrationError::SignalTooQuiet)
        );
    }

    #[test]
    fn rejects_a_tone_with_excessive_background_noise() {
        let samples = (0..48_000)
            .map(|index| {
                let tone = (std::f64::consts::TAU * f64::from(index) / 48.0).sin() * 0.01;
                let noise = if index % 2 == 0 { 0.1 } else { -0.1 };
                #[allow(clippy::cast_possible_truncation)]
                let sample = (tone + noise) as f32;
                sample
            })
            .collect::<Vec<_>>();

        assert_eq!(
            propose(&samples, 48_000.0, 1_000.0, 94.0),
            Err(CalibrationError::TooMuchBackgroundNoise)
        );
    }
}

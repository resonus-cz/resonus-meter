//! Time weighting, equivalent level, and sample-peak detectors.

use crate::units::{amplitude_to_dbfs, power_to_dbfs};

/// Exponential detector time constants defined by IEC 61672-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeWeighting {
    /// 125 ms exponential time weighting.
    Fast,
    /// 1 s exponential time weighting.
    Slow,
}

impl TimeWeighting {
    const fn time_constant_s(self) -> f64 {
        match self {
            Self::Fast => 0.125,
            Self::Slow => 1.0,
        }
    }
}

/// Stateful exponential mean-square detector.
#[derive(Debug, Clone)]
pub struct TimeDetector {
    alpha: f64,
    power: Option<f64>,
    maximum_dbfs: f64,
}

impl TimeDetector {
    /// Creates a detector for `sample_rate_hz`.
    ///
    /// # Panics
    ///
    /// Panics when `sample_rate_hz` is not positive.
    #[must_use]
    pub fn new(sample_rate_hz: f64, weighting: TimeWeighting) -> Self {
        assert!(sample_rate_hz.is_sign_positive());
        let alpha = 1.0 - (-1.0 / (sample_rate_hz * weighting.time_constant_s())).exp();
        Self {
            alpha,
            power: None,
            maximum_dbfs: f64::NEG_INFINITY,
        }
    }

    /// Processes a sample and returns the instantaneous weighted level in dBFS.
    pub fn process(&mut self, sample: f64) -> f64 {
        let sample_power = sample * sample;
        let power = self.power.get_or_insert(sample_power);
        *power += self.alpha * (sample_power - *power);
        let level_dbfs = power_to_dbfs(*power);
        self.maximum_dbfs = self.maximum_dbfs.max(level_dbfs);
        level_dbfs
    }

    /// Returns the current time-weighted level, or `None` before the first sample.
    #[must_use]
    pub fn level_dbfs(&self) -> Option<f64> {
        self.power.map(power_to_dbfs)
    }

    /// Returns the largest time-weighted level observed since reset.
    #[must_use]
    pub const fn maximum_dbfs(&self) -> f64 {
        self.maximum_dbfs
    }

    /// Clears the detector state.
    pub fn reset(&mut self) {
        self.power = None;
        self.maximum_dbfs = f64::NEG_INFINITY;
    }
}

/// The complete internal state of a [`LevelAccumulator`], so a recording can
/// be continued exactly after it was persisted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AccumulatorState {
    /// Kahan-compensated sum of squared samples.
    pub power_sum: f64,
    /// Kahan compensation term of `power_sum`.
    pub compensation: f64,
    /// Number of accumulated samples.
    pub sample_count: u64,
    /// Largest absolute sample.
    pub peak_amplitude: f64,
}

/// Kahan-compensated mean-square accumulator and sample-peak detector.
#[derive(Debug, Clone, Default)]
pub struct LevelAccumulator {
    power_sum: f64,
    compensation: f64,
    sample_count: u64,
    peak_amplitude: f64,
}

impl LevelAccumulator {
    /// Adds one weighted sample.
    pub fn process(&mut self, sample: f64) {
        let squared = sample * sample;
        let adjusted = squared - self.compensation;
        let total = self.power_sum + adjusted;
        self.compensation = (total - self.power_sum) - adjusted;
        self.power_sum = total;
        self.sample_count += 1;
        self.peak_amplitude = self.peak_amplitude.max(sample.abs());
    }

    /// Returns the equivalent level in dBFS, or `None` before the first sample.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn leq_dbfs(&self) -> Option<f64> {
        (self.sample_count > 0).then(|| power_to_dbfs(self.power_sum / self.sample_count as f64))
    }

    /// Returns the sample peak in dBFS, or `None` before the first sample.
    #[must_use]
    pub fn peak_dbfs(&self) -> Option<f64> {
        (self.sample_count > 0).then(|| amplitude_to_dbfs(self.peak_amplitude))
    }

    /// Returns the number of accumulated samples.
    #[must_use]
    pub const fn sample_count(&self) -> u64 {
        self.sample_count
    }

    /// Clears all accumulated values.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Returns the internal state for persisting.
    #[must_use]
    pub const fn state(&self) -> AccumulatorState {
        AccumulatorState {
            power_sum: self.power_sum,
            compensation: self.compensation,
            sample_count: self.sample_count,
            peak_amplitude: self.peak_amplitude,
        }
    }

    /// Rebuilds an accumulator from a state returned by [`Self::state`].
    #[must_use]
    pub const fn from_state(state: AccumulatorState) -> Self {
        Self {
            power_sum: state.power_sum,
            compensation: state.compensation,
            sample_count: state.sample_count,
            peak_amplitude: state.peak_amplitude,
        }
    }
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)]
mod tests {
    use super::{LevelAccumulator, TimeDetector, TimeWeighting};

    #[test]
    fn a_restored_accumulator_continues_exactly() {
        let samples = (0..1_000).map(|index| (f64::from(index) * 0.37).sin() * 0.3);
        let mut uninterrupted = LevelAccumulator::default();
        let mut first = LevelAccumulator::default();
        for (index, sample) in samples.clone().enumerate() {
            uninterrupted.process(sample);
            if index < 600 {
                first.process(sample);
            }
        }
        let mut restored = LevelAccumulator::from_state(first.state());
        for sample in samples.skip(600) {
            restored.process(sample);
        }

        assert_eq!(restored.state(), uninterrupted.state());
    }
    use proptest::prelude::*;

    #[test]
    fn fast_step_reaches_sixty_three_percent_at_time_constant() {
        let sample_rate_hz = 48_000.0;
        let mut detector = TimeDetector::new(sample_rate_hz, TimeWeighting::Fast);
        detector.process(0.0);
        for _ in 0..5_999 {
            detector.process(1.0);
        }

        let expected_dbfs = 10.0 * (1.0 - (-1.0_f64).exp()).log10();
        assert!((detector.process(1.0) - expected_dbfs).abs() < 0.001);
    }

    #[test]
    fn leq_and_peak_match_a_sine() {
        let mut accumulator = LevelAccumulator::default();
        for index in 0..48_000 {
            let sample = (std::f64::consts::TAU * f64::from(index) / 48.0).sin();
            accumulator.process(sample);
        }

        let leq = accumulator.leq_dbfs().expect("samples were processed");
        let peak = accumulator.peak_dbfs().expect("samples were processed");
        assert!((leq + 3.010_299_956_64).abs() < 1e-10);
        assert!((peak - leq - 3.010_299_956_64).abs() < 1e-10);
    }

    #[test]
    fn slow_step_reaches_sixty_three_percent_at_its_time_constant() {
        let sample_rate_hz = 48_000.0;
        let mut detector = TimeDetector::new(sample_rate_hz, TimeWeighting::Slow);
        detector.process(0.0);
        for _ in 0..47_999 {
            detector.process(1.0);
        }

        let expected_dbfs = 10.0 * (1.0 - (-1.0_f64).exp()).log10();
        assert!((detector.process(1.0) - expected_dbfs).abs() < 0.001);
    }

    #[test]
    fn leq_is_an_energy_mean_for_two_levels() {
        let mut accumulator = LevelAccumulator::default();
        for _ in 0..48_000 {
            accumulator.process(0.1);
        }
        for _ in 0..48_000 {
            accumulator.process(0.2);
        }

        let expected_dbfs = 10.0 * f64::midpoint(0.1_f64.powi(2), 0.2_f64.powi(2)).log10();
        assert!(
            (accumulator.leq_dbfs().expect("samples were processed") - expected_dbfs).abs() < 1e-10
        );
    }

    proptest! {
        #[test]
        fn detectors_do_not_emit_nan_or_infinity(
            samples in proptest::collection::vec(-1.0_f64..=1.0, 1..4096)
        ) {
            let mut fast = TimeDetector::new(48_000.0, TimeWeighting::Fast);
            let mut accumulator = LevelAccumulator::default();
            for sample in samples {
                prop_assert!(fast.process(sample).is_finite());
                accumulator.process(sample);
            }
            prop_assert!(accumulator.leq_dbfs().is_some_and(f64::is_finite));
            prop_assert!(accumulator.peak_dbfs().is_some_and(f64::is_finite));
        }
    }
}

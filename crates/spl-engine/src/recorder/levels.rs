//! Final values of one weighting for the open segment.

use serde::{Deserialize, Serialize};
use spl_dsp::{
    detector::{AccumulatorState, LevelAccumulator},
    units::calibrated_level_db,
};
use spl_session::LevelFinal;

/// Leq and sample peak of the weighted samples, plus the largest Fast level
/// seen after any of them.
#[derive(Debug, Clone)]
pub(super) struct SegmentLevels {
    level: LevelAccumulator,
    fast_max_dbfs: f64,
}

impl Default for SegmentLevels {
    fn default() -> Self {
        Self {
            level: LevelAccumulator::default(),
            fast_max_dbfs: f64::NEG_INFINITY,
        }
    }
}

impl SegmentLevels {
    /// Adds a weighted sample and the Fast level after it, in dBFS.
    pub(super) fn process(&mut self, sample: f64, fast_dbfs: f64) {
        self.level.process(sample);
        self.fast_max_dbfs = self.fast_max_dbfs.max(fast_dbfs);
    }

    pub(super) const fn sample_count(&self) -> u64 {
        self.level.sample_count()
    }

    /// Returns the calibrated values; all are `None` without samples.
    pub(super) fn final_values(&self, calibration_offset_db: f64) -> LevelFinal {
        let calibrated = |dbfs: f64| calibrated_level_db(dbfs, calibration_offset_db);
        LevelFinal {
            leq_db: self.level.leq_dbfs().map(calibrated),
            peak_db: self.level.peak_dbfs().map(calibrated),
            fast_max_db: (self.sample_count() > 0).then(|| calibrated(self.fast_max_dbfs)),
        }
    }
}

/// Persisted form of [`SegmentLevels`] and of the meter's running values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct LevelsState {
    power_sum: f64,
    compensation: f64,
    sample_count: u64,
    peak_amplitude: f64,
    /// `-inf` before the first sample.
    fast_max_dbfs: f64,
}

impl LevelsState {
    pub(crate) const fn new(level: &LevelAccumulator, fast_max_dbfs: f64) -> Self {
        let state = level.state();
        Self {
            power_sum: state.power_sum,
            compensation: state.compensation,
            sample_count: state.sample_count,
            peak_amplitude: state.peak_amplitude,
            fast_max_dbfs,
        }
    }

    pub(crate) const fn level(self) -> LevelAccumulator {
        LevelAccumulator::from_state(AccumulatorState {
            power_sum: self.power_sum,
            compensation: self.compensation,
            sample_count: self.sample_count,
            peak_amplitude: self.peak_amplitude,
        })
    }

    pub(crate) const fn fast_max_dbfs(self) -> f64 {
        self.fast_max_dbfs
    }
}

impl SegmentLevels {
    pub(super) const fn state(&self) -> LevelsState {
        LevelsState::new(&self.level, self.fast_max_dbfs)
    }

    pub(super) const fn from_state(state: LevelsState) -> Self {
        Self {
            level: state.level(),
            fast_max_dbfs: state.fast_max_dbfs(),
        }
    }
}

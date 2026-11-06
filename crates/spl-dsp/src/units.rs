//! Level conversion helpers using the SPL reference pressure.

/// Sound-pressure reference in pascals: 20 µPa.
pub const REFERENCE_PRESSURE_PA: f64 = 20e-6;

/// Clipping threshold for normalised samples: `|x| >= CLIP_LEVEL` counts as clipped.
///
/// `1 - 2^-15`, the top code of a 16-bit converter (32767/32768), so any converter of
/// 16 bits or more reaches it at full scale. One value for the Re:sonus stack (user
/// decision 2026-11-05, as resonus-core `calibration.CLIP_LEVEL`); a design choice, not
/// a normative value. Exactly representable in `f32`.
pub const CLIP_LEVEL: f32 = 1.0 - 1.0 / 32768.0;
const MIN_POWER: f64 = f64::MIN_POSITIVE;
const MIN_AMPLITUDE: f64 = f64::MIN_POSITIVE;

/// Converts mean-square normalised samples to dBFS.
#[must_use]
pub fn power_to_dbfs(power: f64) -> f64 {
    10.0 * power.max(MIN_POWER).log10()
}

/// Converts a normalised sample amplitude to dBFS.
#[must_use]
pub fn amplitude_to_dbfs(amplitude: f64) -> f64 {
    20.0 * amplitude.abs().max(MIN_AMPLITUDE).log10()
}

/// Converts a dBFS level to linear mean-square power.
#[must_use]
pub fn dbfs_to_power(level_dbfs: f64) -> f64 {
    10.0_f64.powf(level_dbfs / 10.0)
}

/// Applies a calibration offset to a dBFS value.
#[must_use]
pub fn calibrated_level_db(level_dbfs: f64, offset_db: f64) -> f64 {
    level_dbfs + offset_db
}

#[cfg(test)]
mod tests {
    use super::{amplitude_to_dbfs, dbfs_to_power, power_to_dbfs};

    #[test]
    fn full_scale_sine_rms_is_minus_3_dbfs() {
        assert!((power_to_dbfs(0.5) + 3.010_299_956_64).abs() < 1e-10);
    }

    #[test]
    fn power_conversion_round_trips() {
        let level_dbfs = -42.75;
        assert!((power_to_dbfs(dbfs_to_power(level_dbfs)) - level_dbfs).abs() < 1e-10);
        assert!((amplitude_to_dbfs(0.5) + 6.020_599_913_28).abs() < 1e-10);
    }
}

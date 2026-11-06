//! Specification 9.1 item 5: centre sines and the white-noise band slope.
//!
//! The IEC 61260-1 class 1 attenuation limits are checked against a licensed
//! local copy of the standard's table, which this repository does not hold.

use std::f64::consts::TAU;

use super::{BandDetector, FilterBank};
use crate::bands::Bandwidth;

/// Leq of every band for `seconds` of `signal`, after one settling second.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn band_leq_dbfs(
    bank: &mut FilterBank,
    sample_rate_hz: f64,
    seconds: f64,
    mut signal: impl FnMut(u64) -> f64,
) -> Vec<f64> {
    let settle = sample_rate_hz as u64;
    let total = settle + (seconds * sample_rate_hz) as u64;
    for index in 0..total {
        if index == settle {
            bank.reset_leq();
        }
        bank.process(signal(index), true);
    }
    (0..bank.bands().len())
        .map(|band| bank.level_dbfs(band, BandDetector::Leq).expect("processed"))
        .collect()
}

#[test]
fn a_sine_at_each_band_centre_reads_its_level_in_that_band() {
    // 0.5 amplitude: −9.03 dBFS RMS.
    let expected_dbfs = 20.0 * (0.5 / 2_f64.sqrt()).log10();
    // Tones in every third (one-third octave) or second (octave) band at
    // once: bands that far apart leak less than −50 dB into each other.
    for (sample_rate_hz, bandwidth, spacing) in [
        (48_000.0, Bandwidth::ThirdOctave, 3),
        (44_100.0, Bandwidth::ThirdOctave, 3),
        (96_000.0, Bandwidth::ThirdOctave, 3),
        (48_000.0, Bandwidth::Octave, 2),
    ] {
        let reference = FilterBank::new(sample_rate_hz, bandwidth, 20.0, 20_000.0);
        for offset in 0..spacing {
            let tones = (offset..reference.bands().len())
                .step_by(spacing)
                .collect::<Vec<_>>();
            let steps = tones
                .iter()
                .map(|&index| TAU * reference.bands()[index].exact_hz / sample_rate_hz)
                .collect::<Vec<_>>();
            let mut bank = reference.clone();
            #[allow(clippy::cast_precision_loss)]
            let levels = band_leq_dbfs(&mut bank, sample_rate_hz, 1.0, |n| {
                steps.iter().map(|step| 0.5 * (step * n as f64).sin()).sum()
            });
            for &index in &tones {
                let tone_band = reference.bands()[index];
                let error = levels[index] - expected_dbfs;
                assert!(
                    error.abs() <= 0.2,
                    "{} Hz band at {sample_rate_hz} Hz: {error:+.3} dB",
                    tone_band.nominal_hz
                );
                // Sanity only, not the IEC limit: neighbours are well below it.
                for neighbour in [index.wrapping_sub(1), index + 1] {
                    if let Some(level) = levels.get(neighbour) {
                        assert!(
                            *level < levels[index] - 10.0,
                            "{} Hz neighbour at {sample_rate_hz} Hz",
                            tone_band.nominal_hz
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn white_noise_rises_one_decibel_per_third_octave() {
    let sample_rate_hz = 48_000.0;
    let mut bank = FilterBank::new(sample_rate_hz, Bandwidth::ThirdOctave, 200.0, 10_000.0);
    // Deterministic uniform white noise (xorshift64*), RMS 0.1·√3/3.
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let levels = band_leq_dbfs(&mut bank, sample_rate_hz, 20.0, |_| {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        #[allow(clippy::cast_precision_loss)]
        let unit = (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1_u64 << 53) as f64;
        0.1 * (2.0 * unit - 1.0)
    });

    // Band power ∝ bandwidth, which grows by G^(1/3) per band: 1.0 dB.
    let step_db = 10.0 * crate::bands::OCTAVE_RATIO.cbrt().log10();
    for (offset, level) in levels.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let expected = levels[0] + step_db * offset as f64;
        assert!(
            (level - expected).abs() < 0.4,
            "band {offset}: {level} vs {expected}"
        );
    }
    #[allow(clippy::cast_precision_loss)]
    let slope = (levels[levels.len() - 1] - levels[0]) / (levels.len() - 1) as f64;
    assert!((slope - step_db).abs() < 0.03, "slope {slope}");
}

#[test]
fn bands_above_nyquist_are_left_out() {
    let at_44k = FilterBank::new(44_100.0, Bandwidth::ThirdOctave, 20.0, 20_000.0);
    let at_48k = FilterBank::new(48_000.0, Bandwidth::ThirdOctave, 20.0, 20_000.0);

    // The 20 kHz band's upper edge, 22.4 kHz, is above 22.05 kHz.
    assert_eq!(
        at_44k.bands().last().map(|band| band.nominal_hz),
        Some(16_000.0)
    );
    assert_eq!(
        at_48k.bands().last().map(|band| band.nominal_hz),
        Some(20_000.0)
    );
}

#[test]
fn the_response_peaks_at_zero_db_in_every_band() {
    let bank = FilterBank::new(48_000.0, Bandwidth::ThirdOctave, 20.0, 20_000.0);

    for (index, band) in bank.bands().iter().enumerate() {
        let centre = bank.magnitude_db(index, band.exact_hz).expect("band");
        assert!(centre.abs() < 0.05, "{} Hz: {centre}", band.nominal_hz);
    }
}

/// Checks every band's response against the IEC 61260-1:2014 class 1
/// relative attenuation limits, read from a CSV with the columns
/// `normalised_frequency,min_db,max_db` (frequency relative to the exact
/// mid-band frequency, attenuation in dB, `max_db` blank when unbounded).
#[test]
#[ignore = "requires SPL_METER_IEC61260_TABLE_PATH pointing at a licensed local fixture"]
fn class_1_attenuation_limits_match_external_iec_fixture() {
    let path = std::env::var("SPL_METER_IEC61260_TABLE_PATH")
        .expect("set SPL_METER_IEC61260_TABLE_PATH to the local IEC 61260-1 CSV");
    let table = std::fs::read_to_string(path).expect("fixture is readable");
    for sample_rate_hz in [44_100.0, 48_000.0, 96_000.0] {
        let bank = FilterBank::new(sample_rate_hz, Bandwidth::ThirdOctave, 20.0, 20_000.0);
        for (index, band) in bank.bands().iter().enumerate() {
            for line in table.lines().skip(1) {
                let fields = line.split(',').collect::<Vec<_>>();
                let ratio: f64 = fields[0].trim().parse().expect("frequency ratio");
                let frequency = band.exact_hz * ratio;
                if frequency >= sample_rate_hz / 2.0 {
                    continue;
                }
                let attenuation = -bank.magnitude_db(index, frequency).expect("band");
                let min: f64 = fields[1].trim().parse().expect("minimum");
                assert!(
                    attenuation >= min,
                    "{} Hz at ×{ratio}: {attenuation}",
                    band.nominal_hz
                );
                if let Some(max) = fields
                    .get(2)
                    .and_then(|field| field.trim().parse::<f64>().ok())
                {
                    assert!(
                        attenuation <= max,
                        "{} Hz at ×{ratio}: {attenuation}",
                        band.nominal_hz
                    );
                }
            }
        }
    }
}

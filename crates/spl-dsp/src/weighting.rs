//! IEC 61672-1 A, C, and Z frequency weighting filters.

use std::f64::consts::{PI, SQRT_2};

use crate::sos::Biquad;

const F1_HZ: f64 = 20.598_997;
const F2_HZ: f64 = 107.652_65;
const F3_HZ: f64 = 737.862_23;
const F4_HZ: f64 = 12_194.217;
const MINIMUM_SAMPLE_RATE_HZ: f64 = 44_100.0;

/// Frequency weighting applied before level detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weighting {
    /// IEC 61672-1 A frequency weighting.
    A,
    /// IEC 61672-1 C frequency weighting.
    C,
    /// Flat response with a 3 Hz DC-removal high-pass filter.
    Z,
}

impl Weighting {
    /// The weighting's analog response at `frequency_hz` relative to 1 kHz, in
    /// dB (IEC 61672-1:2013, Annex E). Z is flat.
    #[must_use]
    pub fn response_db(self, frequency_hz: f64) -> f64 {
        let raw = |frequency: f64| -> f64 {
            let f2 = frequency * frequency;
            let low = (f2 + F1_HZ * F1_HZ) * (f2 + F4_HZ * F4_HZ);
            match self {
                Self::A => {
                    let middle = ((f2 + F2_HZ * F2_HZ) * (f2 + F3_HZ * F3_HZ)).sqrt();
                    20.0 * (F4_HZ * F4_HZ * f2 * f2 / (low * middle)).log10()
                }
                Self::C => 20.0 * (F4_HZ * F4_HZ * f2 / low).log10(),
                Self::Z => 0.0,
            }
        };
        raw(frequency_hz) - raw(1_000.0)
    }
}

/// Error returned when a requested sample rate cannot reproduce the weighting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnsupportedSampleRate {
    /// Requested rate in hertz.
    pub sample_rate_hz: f64,
}

impl std::fmt::Display for UnsupportedSampleRate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} Hz is unsupported; the meter requires at least {} Hz",
            self.sample_rate_hz, MINIMUM_SAMPLE_RATE_HZ
        )
    }
}

impl std::error::Error for UnsupportedSampleRate {}

/// A cascade of direct-form II transposed second-order sections.
#[derive(Debug, Clone)]
pub struct WeightingFilter {
    sections: Vec<Biquad>,
}

impl WeightingFilter {
    /// Designs an IEC weighting filter for the negotiated sample rate.
    ///
    /// # Errors
    ///
    /// Returns [`UnsupportedSampleRate`] when the rate is below 44.1 kHz.
    pub fn new(weighting: Weighting, sample_rate_hz: f64) -> Result<Self, UnsupportedSampleRate> {
        if sample_rate_hz < MINIMUM_SAMPLE_RATE_HZ {
            return Err(UnsupportedSampleRate { sample_rate_hz });
        }

        let w1 = angular_frequency(F1_HZ);
        let w2 = angular_frequency(F2_HZ);
        let w3 = angular_frequency(F3_HZ);
        let w4 = angular_frequency(F4_HZ);
        let mut sections = match weighting {
            Weighting::A => vec![
                Biquad::from_analog([1.0, 0.0, 0.0], [1.0, 2.0 * w1, w1 * w1], sample_rate_hz),
                Biquad::from_analog([1.0, 0.0, 0.0], [1.0, w2 + w3, w2 * w3], sample_rate_hz),
                Biquad::from_analog([0.0, 0.0, 1.0], [1.0, 2.0 * w4, w4 * w4], sample_rate_hz),
            ],
            Weighting::C => vec![
                Biquad::from_analog([1.0, 0.0, 0.0], [1.0, 2.0 * w1, w1 * w1], sample_rate_hz),
                Biquad::from_analog([0.0, 0.0, 1.0], [1.0, 2.0 * w4, w4 * w4], sample_rate_hz),
            ],
            Weighting::Z => vec![Biquad::from_analog(
                [1.0, 0.0, 0.0],
                [
                    1.0,
                    SQRT_2 * angular_frequency(3.0),
                    angular_frequency(3.0).powi(2),
                ],
                sample_rate_hz,
            )],
        };
        normalize_at_1khz(&mut sections, sample_rate_hz);
        Ok(Self { sections })
    }

    /// Processes one normalised sample.
    pub fn process(&mut self, mut sample: f64) -> f64 {
        for section in &mut self.sections {
            sample = section.process(sample);
        }
        sample
    }

    /// Resets the delay state of every section.
    pub fn reset(&mut self) {
        for section in &mut self.sections {
            section.reset();
        }
    }

    /// Evaluates the filter's magnitude response at `frequency_hz`.
    #[must_use]
    pub fn magnitude_db(&self, frequency_hz: f64, sample_rate_hz: f64) -> f64 {
        let angular = -2.0 * PI * frequency_hz / sample_rate_hz;
        self.sections
            .iter()
            .map(|section| section.magnitude(angular))
            .product::<f64>()
            .log10()
            * 20.0
    }
}

fn angular_frequency(frequency_hz: f64) -> f64 {
    2.0 * PI * frequency_hz
}

fn normalize_at_1khz(sections: &mut [Biquad], sample_rate_hz: f64) {
    let magnitude = sections
        .iter()
        .map(|section| section.magnitude(-2.0 * PI * 1_000.0 / sample_rate_hz))
        .product::<f64>();
    let first = &mut sections[0];
    first.b0 /= magnitude;
    first.b1 /= magnitude;
    first.b2 /= magnitude;
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use super::{Weighting, WeightingFilter};

    #[test]
    fn the_analytic_response_matches_the_specification_sanity_values() {
        let close = |weighting: Weighting, frequency: f64, expected: f64| {
            let actual = weighting.response_db(frequency);
            assert!(
                (actual - expected).abs() < 0.05,
                "{weighting:?} {frequency} Hz: {actual}"
            );
        };
        close(Weighting::A, 1_000.0, 0.0);
        close(Weighting::A, 100.0, -19.1);
        close(Weighting::A, 10_000.0, -2.5);
        close(Weighting::C, 100.0, -0.3);
        close(Weighting::C, 10_000.0, -4.4);
        close(Weighting::Z, 50.0, 0.0);
    }
    use proptest::prelude::*;

    #[test]
    fn a_weighting_matches_reference_sanity_values() {
        let filter = WeightingFilter::new(Weighting::A, 48_000.0).expect("valid rate");
        assert!(filter.magnitude_db(1_000.0, 48_000.0).abs() < 1e-10);
        assert!((filter.magnitude_db(100.0, 48_000.0) + 19.1).abs() < 0.15);
    }

    #[test]
    fn c_weighting_matches_reference_sanity_values() {
        let filter = WeightingFilter::new(Weighting::C, 48_000.0).expect("valid rate");
        assert!(filter.magnitude_db(1_000.0, 48_000.0).abs() < 1e-10);
        assert!((filter.magnitude_db(100.0, 48_000.0) + 0.3).abs() < 0.15);
    }

    #[test]
    fn rejects_sample_rates_below_the_supported_range() {
        assert!(WeightingFilter::new(Weighting::A, 32_000.0).is_err());
    }

    #[test]
    #[ignore = "requires SPL_METER_IEC_TABLE_PATH pointing at a licensed local fixture"]
    fn class_1_limits_match_external_iec_fixture() {
        let path = env::var("SPL_METER_IEC_TABLE_PATH")
            .expect("set SPL_METER_IEC_TABLE_PATH to the local IEC reference CSV");
        let fixture = fs::read_to_string(path).expect("read IEC reference CSV");

        for sample_rate_hz in [44_100.0, 48_000.0, 96_000.0] {
            for (line_number, line) in fixture.lines().skip(1).enumerate() {
                let columns = line.split(',').collect::<Vec<_>>();
                assert_eq!(
                    columns.len(),
                    6,
                    "malformed fixture line {}",
                    line_number + 2
                );
                let frequency_hz = columns[0].parse::<f64>().expect("frequency");
                let expected_levels = [
                    columns[1].parse::<f64>().expect("A weighting"),
                    columns[2].parse::<f64>().expect("C weighting"),
                    columns[3].parse::<f64>().expect("Z weighting"),
                ];
                let (upper_db, lower_db) = parse_limits(columns[4]);

                for (weighting, expected_db) in [Weighting::A, Weighting::C, Weighting::Z]
                    .into_iter()
                    .zip(expected_levels)
                {
                    let filter =
                        WeightingFilter::new(weighting, sample_rate_hz).expect("supported rate");
                    let error_db = filter.magnitude_db(frequency_hz, sample_rate_hz) - expected_db;
                    assert!(
                        error_db <= upper_db && error_db >= lower_db,
                        "{weighting:?} at {frequency_hz} Hz / {sample_rate_hz} Hz: {error_db} dB outside [{lower_db}, {upper_db}] dB"
                    );
                }
            }
        }
    }

    proptest! {
        #[test]
        fn processing_finite_samples_stays_finite(
            samples in proptest::collection::vec(-1.0_f64..=1.0, 1..4096)
        ) {
            let mut filter = WeightingFilter::new(Weighting::A, 48_000.0)
                .expect("48 kHz is supported");
            for sample in samples {
                prop_assert!(filter.process(sample).is_finite());
            }
        }
    }

    fn parse_limits(limits: &str) -> (f64, f64) {
        let mut values = limits.split('/').map(str::trim);
        let upper = values
            .next()
            .expect("upper limit")
            .trim_start_matches('+')
            .parse::<f64>()
            .expect("numeric upper limit");
        let lower_text = values.next().expect("lower limit").trim_start_matches('-');
        let lower = if lower_text == "inf" {
            f64::NEG_INFINITY
        } else {
            -lower_text.parse::<f64>().expect("numeric lower limit")
        };
        (upper, lower)
    }
}

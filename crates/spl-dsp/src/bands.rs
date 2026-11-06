//! Base-ten octave and one-third-octave bands (IEC 61260-1:2014, 5.2–5.5).

/// Octave frequency ratio of the base-ten system, `G = 10^(3/10)`
/// (IEC 61260-1:2014, 5.2).
pub const OCTAVE_RATIO: f64 = 1.995_262_314_968_879_5;

/// Nominal one-third-octave mid-band frequencies from 10 Hz to 20 kHz: the
/// ISO 266 preferred frequencies that IEC 61260-1 uses as band labels. Index
/// 20 is 1 kHz.
const NOMINAL_THIRD_OCTAVE_HZ: [f64; 34] = [
    10.0, 12.5, 16.0, 20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0,
    315.0, 400.0, 500.0, 630.0, 800.0, 1_000.0, 1_250.0, 1_600.0, 2_000.0, 2_500.0, 3_150.0,
    4_000.0, 5_000.0, 6_300.0, 8_000.0, 10_000.0, 12_500.0, 16_000.0, 20_000.0,
];
/// Index of 1 kHz in [`NOMINAL_THIRD_OCTAVE_HZ`].
const KILOHERTZ_INDEX: i32 = 20;
/// Index of 20 kHz, the last entry of [`NOMINAL_THIRD_OCTAVE_HZ`].
const LAST_INDEX: i32 = 33;

/// Band width as a fraction of an octave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bandwidth {
    /// Whole octaves (b = 1).
    Octave,
    /// One-third octaves (b = 3).
    ThirdOctave,
}

impl Bandwidth {
    /// The bandwidth designator `b` of IEC 61260-1.
    #[must_use]
    pub const fn designator(self) -> i32 {
        match self {
            Self::Octave => 1,
            Self::ThirdOctave => 3,
        }
    }
}

/// One fractional-octave band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    /// Nominal mid-band frequency used as the label.
    pub nominal_hz: f64,
    /// Exact mid-band frequency `1000 · G^(k/b)`.
    pub exact_hz: f64,
    /// Lower band edge `fm · G^(−1/(2b))`.
    pub lower_hz: f64,
    /// Upper band edge `fm · G^(1/(2b))`.
    pub upper_hz: f64,
}

/// Returns the bands whose nominal frequency lies in `min_hz..=max_hz`,
/// ascending. The range is limited to the 10 Hz – 20 kHz nominal table.
#[must_use]
pub fn bands(bandwidth: Bandwidth, min_hz: f64, max_hz: f64) -> Vec<Band> {
    let designator = bandwidth.designator();
    let thirds_per_band = 3 / designator;
    let first = -KILOHERTZ_INDEX / thirds_per_band;
    let last = (LAST_INDEX - KILOHERTZ_INDEX) / thirds_per_band;
    (first..=last)
        .filter_map(|index| {
            let table_index = usize::try_from(KILOHERTZ_INDEX + index * thirds_per_band).ok()?;
            let nominal_hz = *NOMINAL_THIRD_OCTAVE_HZ.get(table_index)?;
            (min_hz..=max_hz)
                .contains(&nominal_hz)
                .then(|| band(index, designator, nominal_hz))
        })
        .collect()
}

fn band(index: i32, designator: i32, nominal_hz: f64) -> Band {
    let exponent = f64::from(index) / f64::from(designator);
    let exact_hz = 1_000.0 * OCTAVE_RATIO.powf(exponent);
    let half_band = OCTAVE_RATIO.powf(1.0 / (2.0 * f64::from(designator)));
    Band {
        nominal_hz,
        exact_hz,
        lower_hz: exact_hz / half_band,
        upper_hz: exact_hz * half_band,
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Nominal labels are exact table values.
mod tests {
    use super::{Bandwidth, LAST_INDEX, NOMINAL_THIRD_OCTAVE_HZ, OCTAVE_RATIO, bands};

    #[test]
    fn the_last_index_names_the_table_end() {
        assert_eq!(
            usize::try_from(LAST_INDEX),
            Ok(NOMINAL_THIRD_OCTAVE_HZ.len() - 1)
        );
    }

    #[test]
    fn the_octave_ratio_is_ten_to_the_three_tenths() {
        assert!((OCTAVE_RATIO - 10_f64.powf(0.3)).abs() < 1e-15);
    }

    #[test]
    fn third_octaves_from_20_hz_to_20_khz_follow_the_base_ten_series() {
        let thirds = bands(Bandwidth::ThirdOctave, 20.0, 20_000.0);

        assert_eq!(thirds.len(), 31);
        assert_eq!(thirds[0].nominal_hz, 20.0);
        assert_eq!(thirds[30].nominal_hz, 20_000.0);
        let kilohertz = thirds
            .iter()
            .find(|band| band.nominal_hz == 1_000.0)
            .expect("1 kHz band");
        assert!((kilohertz.exact_hz - 1_000.0).abs() < 1e-12);
        for pair in thirds.windows(2) {
            // Adjacent bands share an edge and step by G^(1/3).
            assert!((pair[0].upper_hz - pair[1].lower_hz).abs() < 1e-9 * pair[1].lower_hz);
            assert!((pair[1].exact_hz / pair[0].exact_hz - OCTAVE_RATIO.cbrt()).abs() < 1e-12);
        }
        for band in &thirds {
            assert!(
                (band.lower_hz * band.upper_hz - band.exact_hz.powi(2)).abs()
                    < 1e-9 * band.exact_hz.powi(2)
            );
            // Nominal labels round the exact frequency to about 3 %.
            assert!(
                (band.nominal_hz / band.exact_hz - 1.0).abs() < 0.03,
                "{band:?}"
            );
        }
    }

    #[test]
    fn octaves_use_every_third_one_third_octave_label() {
        let octaves = bands(Bandwidth::Octave, 20.0, 20_000.0);

        let nominal = octaves
            .iter()
            .map(|band| band.nominal_hz)
            .collect::<Vec<_>>();
        assert_eq!(
            nominal,
            [
                31.5, 63.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0
            ]
        );
        assert!((octaves[5].upper_hz / octaves[5].lower_hz - OCTAVE_RATIO).abs() < 1e-12);
    }
}

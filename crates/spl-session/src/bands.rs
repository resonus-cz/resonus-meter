//! The band-level (RTA) log of a session: Z-weighted band Leq per log
//! interval and per segment (specification 6.4, file format 1.2).

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::Segment;

/// Band levels recorded with a session.
#[derive(Debug, Clone, PartialEq)]
pub struct BandLog {
    /// 1 for octave bands, 3 for one-third-octave bands.
    pub bands_per_octave: u8,
    /// Nominal mid-band frequencies, ascending.
    pub bands_hz: Vec<f64>,
    /// Rows not stored elsewhere (see the journal), one per log interval.
    pub rows: Vec<BandRow>,
    /// Band values of the bundle's segments, matched by segment number.
    pub segments: Vec<BandSegment>,
    /// Band `LZeq` over the bundle's segments.
    pub summary_db: Vec<Option<f64>>,
}

/// Band `LZeq` over one log interval, written with the `log.csv` row of the
/// same `t_s`.
#[derive(Debug, Clone, PartialEq)]
pub struct BandRow {
    /// End of the interval, as in `log.csv`.
    pub t_s: f64,
    /// Session-start UTC plus [`Self::t_s`].
    pub utc: OffsetDateTime,
    /// One level per band in dB; `None` for a band without samples.
    pub levels_db: Vec<Option<f64>>,
}

/// Band `LZeq` over one segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BandSegment {
    /// Number of the [`Segment`] these values belong to.
    pub number: u64,
    /// One level per band in dB; `None` for a band without samples.
    pub levels_db: Vec<Option<f64>>,
}

/// Combines the band values of `segments` by energy mean, each weighted by
/// its segment's sample count. Segments without band values are skipped.
#[must_use]
pub fn aggregate_band_segments(
    segments: &[Segment],
    band_segments: &[BandSegment],
    band_count: usize,
) -> Vec<Option<f64>> {
    let mut energy = vec![0.0; band_count];
    let mut weight = vec![0.0; band_count];
    for segment in segments {
        let Some(bands) = band_segments
            .iter()
            .find(|bands| bands.number == segment.number)
        else {
            continue;
        };
        #[allow(clippy::cast_precision_loss)]
        let samples = segment.sample_count as f64;
        for (index, level_db) in bands.levels_db.iter().enumerate().take(band_count) {
            if let Some(level_db) = level_db {
                energy[index] += samples * 10_f64.powf(level_db / 10.0);
                weight[index] += samples;
            }
        }
    }
    energy
        .iter()
        .zip(&weight)
        .map(|(&energy, &weight)| (weight > 0.0).then(|| 10.0 * (energy / weight).log10()))
        .collect()
}

/// Returns the column name of a band: `LZeq_` and its nominal frequency in
/// Hz, such as `LZeq_31.5` or `LZeq_1000`.
#[must_use]
pub fn band_column(nominal_hz: f64) -> String {
    if (nominal_hz - nominal_hz.round()).abs() < 1e-9 {
        format!("LZeq_{nominal_hz:.0}")
    } else {
        format!("LZeq_{nominal_hz:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::{BandSegment, aggregate_band_segments, band_column};
    use crate::{FinalValues, LevelFinal, Segment};

    fn segment(number: u64, sample_count: u64) -> Segment {
        Segment {
            number,
            start_marker_id: number,
            end_marker_id: number + 1,
            start_s: 0.0,
            end_s: 1.0,
            sample_count,
            clipped: false,
            values: FinalValues {
                a: LevelFinal::EMPTY,
                c: LevelFinal::EMPTY,
                z: LevelFinal::EMPTY,
            },
        }
    }

    #[test]
    fn band_values_combine_by_energy_weighted_with_the_segment_length() {
        let segments = [segment(1, 1_000), segment(2, 0), segment(3, 3_000)];
        let bands = [
            BandSegment {
                number: 1,
                levels_db: vec![Some(60.0), Some(40.0)],
            },
            BandSegment {
                number: 2,
                levels_db: vec![None, None],
            },
            BandSegment {
                number: 3,
                levels_db: vec![Some(50.0), None],
            },
        ];

        let summary = aggregate_band_segments(&segments, &bands, 2);

        // (1·10^6 + 3·10^5) / 4 = 3.25·10^5.
        let expected_db = 10.0 * 3.25e5_f64.log10();
        assert!((summary[0].expect("measured") - expected_db).abs() < 1e-9);
        assert!((summary[1].expect("measured") - 40.0).abs() < 1e-9);
        assert_eq!(
            aggregate_band_segments(&segments[1..2], &bands, 2),
            [None, None]
        );
    }

    #[test]
    fn band_columns_name_the_nominal_frequency() {
        assert_eq!(band_column(31.5), "LZeq_31.5");
        assert_eq!(band_column(1_000.0), "LZeq_1000");
        assert_eq!(band_column(12.5), "LZeq_12.5");
    }
}

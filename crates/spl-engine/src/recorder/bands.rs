//! Band `LZeq` per log interval and per segment (ADR 0015).
//!
//! A filter bank of its own analyses the unweighted input for the whole
//! session. Its band Leq accumulators are drained into the open interval and
//! segment at every log row and marker, so each row holds the band `LZeq` over
//! the measured time since the previous row, and each segment its own.

use serde::{Deserialize, Serialize};
use spl_dsp::{bands::Bandwidth, filterbank::FilterBank, units::calibrated_level_db};
use spl_session::{BandLog, BandRow, BandSegment, Segment, aggregate_band_segments};
use time::OffsetDateTime;

use crate::RtaConfig;

/// Squared-sample sum and sample count of one band.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Energy {
    sum: f64,
    count: u64,
}

impl Energy {
    fn add(&mut self, other: Self) {
        self.sum += other.sum;
        self.count += other.count;
    }

    #[allow(clippy::cast_precision_loss)]
    fn level_db(self, calibration_offset_db: f64) -> Option<f64> {
        // A band that saw only digital silence has no finite level either.
        (self.count > 0 && self.sum > 0.0).then(|| {
            calibrated_level_db(
                10.0 * (self.sum / self.count as f64).log10(),
                calibration_offset_db,
            )
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BandRecorder {
    config: RtaConfig,
    bank: FilterBank,
    interval: Vec<Energy>,
    segment: Vec<Energy>,
    /// The segment before a marker whose label is still being entered, and
    /// the energy recorded since; see `ReservedMarker`.
    reserved: Option<(Vec<Energy>, Vec<Energy>)>,
    /// Rows not yet in the journal.
    unjournaled: Vec<BandRow>,
    segments: Vec<BandSegment>,
}

/// Persisted form of a [`BandRecorder`]; the filters start afresh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BandRecorderState {
    pub(crate) config: RtaConfig,
    interval: Vec<Energy>,
    segment: Vec<Energy>,
    segments: Vec<BandSegment>,
    /// Number of `rta_log.csv` rows that belong to this state.
    pub(crate) rows: usize,
}

impl BandRecorder {
    pub(crate) fn new(config: RtaConfig, sample_rate_hz: f64) -> Self {
        let bank = FilterBank::new(
            sample_rate_hz,
            config.bandwidth,
            config.min_hz,
            config.max_hz,
        );
        let bands = bank.bands().len();
        Self {
            config,
            bank,
            interval: vec![Energy::default(); bands],
            segment: vec![Energy::default(); bands],
            reserved: None,
            unjournaled: Vec::new(),
            segments: Vec::new(),
        }
    }

    pub(crate) fn process(&mut self, sample: f64, measuring: bool) {
        self.bank.process(sample, measuring);
    }

    /// The band energy in the bank since the last drain.
    fn pending(&self) -> impl Iterator<Item = Energy> + '_ {
        (0..self.interval.len()).map(|index| {
            self.bank
                .leq_state(index)
                .map_or_else(Energy::default, |state| Energy {
                    sum: state.power_sum,
                    count: state.sample_count,
                })
        })
    }

    /// Moves the bank's band energy into the open interval and segment.
    fn drain(&mut self) {
        let pending = self.pending().collect::<Vec<_>>();
        for (index, energy) in pending.into_iter().enumerate() {
            self.interval[index].add(energy);
            self.segment[index].add(energy);
            if let Some((_, following)) = &mut self.reserved {
                following[index].add(energy);
            }
        }
        self.bank.reset_leq();
    }

    /// Ends the open interval with a row at `t_s`.
    pub(crate) fn log_row(&mut self, t_s: f64, utc: OffsetDateTime, calibration_offset_db: f64) {
        self.drain();
        let levels_db = levels(&self.interval, calibration_offset_db);
        self.interval.fill(Energy::default());
        self.unjournaled.push(BandRow {
            t_s,
            utc,
            levels_db,
        });
    }

    /// Closes the open segment as segment `number`.
    pub(crate) fn close_segment(&mut self, number: u64, calibration_offset_db: f64) {
        self.drain();
        self.reserved = None;
        self.segments.push(BandSegment {
            number,
            levels_db: levels(&self.segment, calibration_offset_db),
        });
        self.segment.fill(Energy::default());
    }

    /// Keeps the open segment for a marker placed later; recording goes on.
    pub(crate) fn reserve(&mut self) {
        self.drain();
        let following = vec![Energy::default(); self.segment.len()];
        self.reserved = Some((self.segment.clone(), following));
    }

    /// Closes the reserved segment as segment `number`; the energy since
    /// the reservation opens the next one.
    pub(crate) fn place_reserved(&mut self, number: u64, calibration_offset_db: f64) {
        self.drain();
        let Some((before, following)) = self.reserved.take() else {
            return;
        };
        self.segments.push(BandSegment {
            number,
            levels_db: levels(&before, calibration_offset_db),
        });
        self.segment = following;
    }

    pub(crate) fn cancel_reserved(&mut self) {
        self.reserved = None;
    }

    /// Rows the next journal flush appends.
    pub(crate) fn journal_rows(&self) -> &[BandRow] {
        &self.unjournaled
    }

    pub(crate) fn journaled(&mut self, count: usize) {
        self.unjournaled.drain(..count.min(self.unjournaled.len()));
    }

    /// The band log of a bundle holding `segments` (all closed).
    pub(crate) fn band_log(&self, segments: &[Segment]) -> BandLog {
        let bands_hz = self.nominal_hz();
        BandLog {
            bands_per_octave: match self.config.bandwidth {
                Bandwidth::Octave => 1,
                Bandwidth::ThirdOctave => 3,
            },
            summary_db: aggregate_band_segments(segments, &self.segments, bands_hz.len()),
            bands_hz,
            rows: self.unjournaled.clone(),
            segments: self.segments.clone(),
        }
    }

    pub(crate) fn nominal_hz(&self) -> Vec<f64> {
        self.bank
            .bands()
            .iter()
            .map(|band| band.nominal_hz)
            .collect()
    }

    /// The state a journal flush persists, after `rows` rows in total. A
    /// reserved marker is not part of it, as for the broadband values.
    pub(crate) fn state(&self, rows: usize) -> BandRecorderState {
        let mut interval = self.interval.clone();
        let mut segment = self.segment.clone();
        for (index, energy) in self.pending().enumerate() {
            interval[index].add(energy);
            segment[index].add(energy);
        }
        BandRecorderState {
            config: self.config,
            interval,
            segment,
            segments: self.segments.clone(),
            rows,
        }
    }

    pub(crate) fn restore(state: BandRecorderState, sample_rate_hz: f64) -> Self {
        Self {
            interval: state.interval,
            segment: state.segment,
            segments: state.segments,
            ..Self::new(state.config, sample_rate_hz)
        }
    }
}

fn levels(energy: &[Energy], calibration_offset_db: f64) -> Vec<Option<f64>> {
    energy
        .iter()
        .map(|energy| energy.level_db(calibration_offset_db))
        .collect()
}

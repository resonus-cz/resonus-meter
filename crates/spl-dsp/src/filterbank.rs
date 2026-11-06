//! The RTA filter bank (specification 4.6).
//!
//! Each band is a 6th-order Butterworth band-pass designed at the rate it
//! runs at. The signal is decimated by two per octave: a stage processes the
//! bands whose upper edge is at most [`band_rate_fraction`] of its rate, then
//! low-passes and halves the rate for the next stage. Low bands therefore run
//! at low rates, where their filters are numerically well conditioned.
//!
//! Every band runs far enough below its stage's Nyquist frequency that the
//! bilinear transform's frequency warping and the next anti-aliasing filter
//! leave its skirts intact. The highest bands would not fit at the input rate,
//! so the input is first oversampled by two as often as needed (specification
//! 4.2's remedy for the same weak spot of the weighting filters).

use crate::bands::{Band, Bandwidth, bands};
use crate::butterworth::{band_pass, low_pass};
use crate::detector::{AccumulatorState, LevelAccumulator, TimeDetector, TimeWeighting};
use crate::sos::{Biquad, cascade_magnitude};

/// Poles per band edge (IEC 61260-1 bands as in pyfilterbank).
const BAND_ORDER_PER_EDGE: usize = 3;
/// Anti-aliasing low-pass order before each decimation by two.
const ANTI_ALIAS_ORDER: usize = 10;
/// Anti-aliasing cutoff as a fraction of the stage rate. The next stage's
/// bands end below 1/8 of this rate (loss < 0.01 dB there), while content that
/// would alias into them lies above 3/8 of it (attenuated by more than 65 dB).
const ANTI_ALIAS_CUTOFF: f64 = 0.17;
/// Interpolation low-pass cutoff as a fraction of the oversampled rate: flat
/// to within 0.03 dB at 20 kHz from a 48 kHz input, and over 35 dB down on the
/// images of a 20 kHz tone.
const INTERPOLATION_CUTOFF: f64 = 0.27;

/// A stage keeps bands whose upper edge is at most this fraction of its rate.
/// Band skirts reach about 1.5 (one-third octave) or 2.5 (octave) times the
/// upper edge at −40 dB; both then stay below the previous stage's
/// anti-aliasing cutoff and in the range where warping is small.
const fn band_rate_fraction(bandwidth: Bandwidth) -> f64 {
    match bandwidth {
        Bandwidth::Octave => 1.0 / 16.0,
        Bandwidth::ThirdOctave => 1.0 / 8.0,
    }
}

/// The band level reported by [`FilterBank::level_dbfs`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandDetector {
    /// Fast time weighting (125 ms).
    Fast,
    /// Slow time weighting (1 s).
    Slow,
    /// Equivalent level since the last reset.
    Leq,
}

/// Octave or one-third-octave band levels of a sample stream.
#[derive(Debug, Clone)]
pub struct FilterBank {
    bands: Vec<Band>,
    /// Interpolation low-passes of the 2× oversampling steps, first to last.
    oversampling: Vec<Vec<Biquad>>,
    stages: Vec<Stage>,
    /// Stage and channel index of each band.
    locations: Vec<(usize, usize)>,
}

#[derive(Debug, Clone)]
struct Stage {
    rate_hz: f64,
    channels: Vec<Channel>,
    /// Empty on the last stage.
    anti_alias: Vec<Biquad>,
    /// Whether the next filtered sample is passed on (decimation phase).
    pass_next: bool,
}

#[derive(Debug, Clone)]
struct Channel {
    filter: Vec<Biquad>,
    fast: TimeDetector,
    slow: TimeDetector,
    leq: LevelAccumulator,
}

impl FilterBank {
    /// Creates the bank for the bands with nominal frequency in
    /// `min_hz..=max_hz` that lie below the Nyquist frequency.
    #[must_use]
    pub fn new(sample_rate_hz: f64, bandwidth: Bandwidth, min_hz: f64, max_hz: f64) -> Self {
        let bands = bands(bandwidth, min_hz, max_hz)
            .into_iter()
            .filter(|band| band.upper_hz < sample_rate_hz / 2.0)
            .collect::<Vec<_>>();
        let fraction = band_rate_fraction(bandwidth);
        let highest_hz = bands.last().map_or(0.0, |band| band.upper_hz);
        let mut oversampling = Vec::new();
        let mut input_rate_hz = sample_rate_hz;
        while highest_hz > fraction * input_rate_hz {
            input_rate_hz *= 2.0;
            oversampling.push(low_pass(
                ANTI_ALIAS_ORDER,
                INTERPOLATION_CUTOFF * input_rate_hz,
                input_rate_hz,
            ));
        }
        let sample_rate_hz = input_rate_hz;
        let levels = bands
            .iter()
            .map(|band| stage_level(band.upper_hz, sample_rate_hz, fraction))
            .collect::<Vec<_>>();
        let stage_count = levels.iter().max().map_or(1, |level| level + 1);
        let mut stages = (0..stage_count)
            .map(|level| {
                let rate_hz = stage_rate(sample_rate_hz, level);
                Stage {
                    rate_hz,
                    channels: Vec::new(),
                    anti_alias: if level + 1 < stage_count {
                        low_pass(ANTI_ALIAS_ORDER, ANTI_ALIAS_CUTOFF * rate_hz, rate_hz)
                    } else {
                        Vec::new()
                    },
                    pass_next: true,
                }
            })
            .collect::<Vec<_>>();
        let mut locations = Vec::with_capacity(bands.len());
        for (band, &level) in bands.iter().zip(&levels) {
            let stage = &mut stages[level];
            let rate_hz = stage.rate_hz;
            stage.channels.push(Channel {
                filter: band_pass(BAND_ORDER_PER_EDGE, band.lower_hz, band.upper_hz, rate_hz),
                fast: TimeDetector::new(rate_hz, TimeWeighting::Fast),
                slow: TimeDetector::new(rate_hz, TimeWeighting::Slow),
                leq: LevelAccumulator::default(),
            });
            locations.push((level, stage.channels.len() - 1));
        }
        Self {
            bands,
            oversampling,
            stages,
            locations,
        }
    }

    /// Returns the analysed bands, ascending.
    #[must_use]
    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    /// Processes one sample at the bank's input rate without allocating.
    /// Fast and Slow always follow the input; `measuring` also adds the
    /// sample to the band Leq, so a paused recording holds it.
    pub fn process(&mut self, sample: f64, measuring: bool) {
        oversample(&mut self.oversampling, &mut self.stages, sample, measuring);
    }

    /// Returns band `index`'s level in dBFS, or `None` before it has a sample.
    #[must_use]
    pub fn level_dbfs(&self, index: usize, detector: BandDetector) -> Option<f64> {
        let channel = self.channel(index)?;
        match detector {
            BandDetector::Fast => channel.fast.level_dbfs(),
            BandDetector::Slow => channel.slow.level_dbfs(),
            BandDetector::Leq => channel.leq.leq_dbfs(),
        }
    }

    /// Returns band `index`'s Leq accumulator since the last
    /// [`Self::reset_leq`].
    #[must_use]
    pub fn leq_state(&self, index: usize) -> Option<AccumulatorState> {
        Some(self.channel(index)?.leq.state())
    }

    /// Clears the band Leq accumulators.
    pub fn reset_leq(&mut self) {
        for channel in self.stages.iter_mut().flat_map(|stage| &mut stage.channels) {
            channel.leq.reset();
        }
    }

    /// Returns the magnitude response in dB of band `index`, including the
    /// interpolation filters and the anti-aliasing filters of the stages
    /// before it, at `frequency_hz` below
    /// its stage's Nyquist frequency. Aliasing is not modelled.
    #[must_use]
    pub fn magnitude_db(&self, index: usize, frequency_hz: f64) -> Option<f64> {
        let &(level, channel) = self.locations.get(index)?;
        let first_rate_hz = self.stages[0].rate_hz;
        let interpolation = self
            .oversampling
            .iter()
            .rev()
            .enumerate()
            .map(|(step, sections)| {
                let rate_hz = first_rate_hz / f64::from(1_u32 << step);
                cascade_magnitude(sections, frequency_hz, rate_hz)
            })
            .product::<f64>();
        let decimation = interpolation
            * self.stages[..level]
                .iter()
                .map(|stage| cascade_magnitude(&stage.anti_alias, frequency_hz, stage.rate_hz))
                .product::<f64>();
        let stage = &self.stages[level];
        let band = cascade_magnitude(&stage.channels[channel].filter, frequency_hz, stage.rate_hz);
        Some(20.0 * (decimation * band).log10())
    }

    fn channel(&self, index: usize) -> Option<&Channel> {
        let &(level, channel) = self.locations.get(index)?;
        self.stages.get(level)?.channels.get(channel)
    }
}

/// Feeds `sample` through the remaining 2× oversampling steps, then into
/// the decimation stages. Zero-stuffing halves the level, so each step's
/// first sample is doubled.
fn oversample(steps: &mut [Vec<Biquad>], stages: &mut [Stage], sample: f64, measuring: bool) {
    let Some((step, rest)) = steps.split_first_mut() else {
        run_stages(stages, sample, measuring);
        return;
    };
    for input in [2.0 * sample, 0.0] {
        let mut output = input;
        for section in step.iter_mut() {
            output = section.process(output);
        }
        oversample(rest, stages, output, measuring);
    }
}

/// Processes one sample at the first stage's rate through all stages.
fn run_stages(stages: &mut [Stage], sample: f64, measuring: bool) {
    let mut input = sample;
    for stage in stages {
        for channel in &mut stage.channels {
            let mut output = input;
            for section in &mut channel.filter {
                output = section.process(output);
            }
            channel.fast.process(output);
            channel.slow.process(output);
            if measuring {
                channel.leq.process(output);
            }
        }
        if stage.anti_alias.is_empty() {
            return;
        }
        for section in &mut stage.anti_alias {
            input = section.process(input);
        }
        stage.pass_next = !stage.pass_next;
        if stage.pass_next {
            return;
        }
    }
}

/// The decimation stage for a band: the lowest rate at which its upper edge
/// is at most `fraction` of the rate, or the first rate.
fn stage_level(upper_hz: f64, sample_rate_hz: f64, fraction: f64) -> usize {
    (0..32)
        .take_while(|&level| upper_hz <= fraction * stage_rate(sample_rate_hz, level))
        .last()
        .unwrap_or(0)
}

fn stage_rate(sample_rate_hz: f64, level: usize) -> f64 {
    sample_rate_hz / f64::from(1_u32 << level.min(31))
}

#[cfg(test)]
mod tests;

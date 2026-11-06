//! The real-time analyser (specification 4.6), on only while its view needs it.

use spl_dsp::{
    filterbank::{BandDetector, FilterBank},
    units::calibrated_level_db,
    weighting::Weighting,
};

use super::{Engine, active_offset_db};
use crate::{RtaBandLevel, RtaConfig, RtaFrame};

/// An enabled analyser and the configuration it was built for.
#[derive(Debug, Clone)]
pub(super) struct Rta {
    config: RtaConfig,
    pub(super) bank: FilterBank,
}

impl Engine {
    /// Turns the analyser on with `config`, or off with `None`. Spectrum
    /// producers run only while their view is visible (specification 3.3);
    /// turning it on again starts its band Leq afresh.
    pub fn set_rta(&mut self, config: Option<RtaConfig>) {
        match config {
            Some(config) if self.rta.as_ref().is_some_and(|rta| rta.config == config) => {}
            Some(config) => {
                self.rta = Some(Rta {
                    bank: FilterBank::new(
                        self.sample_rate_hz,
                        config.bandwidth,
                        config.min_hz,
                        config.max_hz,
                    ),
                    config,
                });
            }
            None => self.rta = None,
        }
    }

    /// Returns the band levels in dB (SPL when calibrated) for `detector`,
    /// with `weighting` applied as a per-band constant at each exact
    /// mid-band frequency, or `None` while the analyser is off.
    #[must_use]
    pub fn rta_frame(&self, detector: BandDetector, weighting: Weighting) -> Option<RtaFrame> {
        let rta = self.rta.as_ref()?;
        let offset_db = active_offset_db(self.calibration.as_ref());
        let bands = rta
            .bank
            .bands()
            .iter()
            .enumerate()
            .map(|(index, band)| RtaBandLevel {
                nominal_hz: band.nominal_hz,
                exact_hz: band.exact_hz,
                level_db: rta.bank.level_dbfs(index, detector).map(|level| {
                    calibrated_level_db(level, offset_db) + weighting.response_db(band.exact_hz)
                }),
            })
            .collect();
        Some(RtaFrame { bands })
    }
}

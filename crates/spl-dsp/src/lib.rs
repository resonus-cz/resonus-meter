#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Pure, allocation-free DSP primitives for Re:sonus Meter.

pub mod bands;
mod butterworth;
pub mod calibration;
pub mod detector;
pub mod filterbank;
mod sos;
pub mod units;
pub mod weighting;

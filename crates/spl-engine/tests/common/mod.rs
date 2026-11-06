//! Helpers shared by the engine integration tests.

// Each test binary uses only some of them.
#![allow(dead_code)]

use std::fs;

use spl_engine::SessionConfig;
use spl_session::{Calibration, CalibrationType, Device};

pub fn session_config() -> SessionConfig {
    SessionConfig {
        band_log: None,
        name: "Engine test".to_owned(),
        software: "SPL Meter test".to_owned(),
        device: Device {
            id: "test.input".to_owned(),
            name: "Test input".to_owned(),
            backend: "test".to_owned(),
            channel: 1,
            sample_rate_hz: 48_000,
        },
        calibration: Calibration {
            offset_db: 100.0,
            calibration_type: CalibrationType::Manual,
            calibrator_level_db: None,
            calibrator_freq_hz: None,
            calibrated_utc: None,
        },
        log_interval_s: 1.0,
        author: None,
        tags: Vec::new(),
        description: String::new(),
    }
}

/// A scratch folder unique to one test, removed when dropped.
pub struct Scratch(pub std::path::PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("spl-engine-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        Self(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn sine_block(seconds: u32, amplitude: f32) -> Vec<f32> {
    (0..seconds * 48_000)
        .map(|index| {
            #[allow(clippy::cast_possible_truncation)]
            let sample = (std::f64::consts::TAU * f64::from(index) / 48.0).sin() as f32;
            sample * amplitude
        })
        .collect()
}

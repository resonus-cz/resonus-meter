//! Application settings stored in `settings.toml` (specification ADR-4).
//!
//! The file carries a settings schema version ([`SETTINGS_VERSION`]); unknown
//! keys are ignored and a newer major version is rejected. Migrations from
//! older versions belong in this module.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use spl_dsp::{bands::Bandwidth, filterbank::BandDetector, weighting::Weighting};

use crate::atomic_file::write_atomically;

/// Settings schema version written to `settings.toml`.
pub const SETTINGS_VERSION: u32 = 1;

/// Number of previously used inputs kept, most recent first.
pub const RECENT_INPUT_LIMIT: usize = 5;

/// Choices for how often the session journal is written, in seconds.
pub const JOURNAL_INTERVAL_CHOICES_S: [u32; 3] = [10, 60, 300];
/// Default journal write interval: at most a minute is lost in a crash.
pub const DEFAULT_JOURNAL_INTERVAL_S: u32 = 60;

/// RTA preferences (specification 5.4): the analysed bands and the RTA
/// tab's initial detector, weighting and peak hold.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RtaSettings {
    /// Octave or one-third-octave bands.
    #[serde(with = "BandwidthSetting")]
    pub bandwidth: Bandwidth,
    /// Lowest nominal mid-band frequency.
    pub min_hz: f64,
    /// Highest nominal mid-band frequency.
    pub max_hz: f64,
    /// Detector the RTA tab starts with.
    #[serde(with = "DetectorSetting")]
    pub detector: BandDetector,
    /// Display weighting the RTA tab starts with.
    #[serde(with = "WeightingSetting")]
    pub weighting: Weighting,
    /// Whether the RTA tab starts with peak hold on.
    pub peak_hold: bool,
    /// Whether sessions record band `LZeq` per log interval and segment
    /// (ADR 0015); applies from the next Reset or New Session.
    pub record: bool,
}

impl Default for RtaSettings {
    /// One-third octaves over 20 Hz – 20 kHz with Fast Z-weighted levels
    /// (specification 4.6).
    fn default() -> Self {
        Self {
            bandwidth: Bandwidth::ThirdOctave,
            min_hz: 20.0,
            max_hz: 20_000.0,
            detector: BandDetector::Fast,
            weighting: Weighting::Z,
            peak_hold: false,
            record: true,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Bandwidth", rename_all = "snake_case")]
pub(crate) enum BandwidthSetting {
    Octave,
    ThirdOctave,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "BandDetector", rename_all = "snake_case")]
enum DetectorSetting {
    Fast,
    Slow,
    Leq,
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Weighting")]
enum WeightingSetting {
    A,
    C,
    Z,
}

/// An input the user selected, identified like a calibration profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentInput {
    /// Stable backend device identifier.
    pub device_id: String,
    /// Zero-based input channel.
    pub channel_index: u16,
    /// Requested stream sample rate in hertz.
    pub sample_rate_hz: u32,
}

impl RecentInput {
    fn is_same_input(&self, other: &Self) -> bool {
        self.device_id == other.device_id && self.channel_index == other.channel_index
    }
}

/// Failure reading or writing the settings file.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// The file exists but could not be read.
    #[error("could not read {path}: {source}")]
    Read {
        /// File that failed.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The file is not valid settings TOML.
    #[error("could not parse {path}: {source}")]
    Parse {
        /// File that failed.
        path: PathBuf,
        /// Underlying TOML error.
        source: toml::de::Error,
    },
    /// The file was written by a newer, incompatible version.
    #[error("settings file version {0} is newer than this application supports")]
    UnsupportedVersion(u32),
    /// The settings could not be encoded as TOML.
    #[error("could not encode settings: {0}")]
    Serialize(toml::ser::Error),
    /// The settings could not be written.
    #[error("could not save settings to {path}: {source}")]
    Write {
        /// File that failed.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    #[serde(default = "default_journal_interval_s")]
    journal_interval_s: u32,
    #[serde(default)]
    rta: RtaSettings,
    #[serde(default, rename = "recent_input")]
    recent_inputs: Vec<RecentInput>,
}

/// Application settings backed by `settings.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    path: Option<PathBuf>,
    journal_interval_s: u32,
    rta: RtaSettings,
    recent_inputs: Vec<RecentInput>,
}

const fn default_journal_interval_s() -> u32 {
    DEFAULT_JOURNAL_INTERVAL_S
}

impl Settings {
    /// Opens the settings at `path`; a missing file gives the defaults.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed, or was written
    /// by a newer major version. The file is never modified by opening it.
    pub fn open(path: &Path) -> Result<Self, SettingsError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(SettingsError::Read {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        let (journal_interval_s, rta, recent_inputs) = if text.trim().is_empty() {
            (
                DEFAULT_JOURNAL_INTERVAL_S,
                RtaSettings::default(),
                Vec::new(),
            )
        } else {
            let file: SettingsFile =
                toml::from_str(&text).map_err(|source| SettingsError::Parse {
                    path: path.to_owned(),
                    source,
                })?;
            if file.version > SETTINGS_VERSION {
                return Err(SettingsError::UnsupportedVersion(file.version));
            }
            (
                valid_journal_interval_s(file.journal_interval_s),
                valid_rta(file.rta),
                file.recent_inputs,
            )
        };
        Ok(Self {
            path: Some(path.to_owned()),
            journal_interval_s,
            rta,
            recent_inputs,
        })
    }

    /// Creates default settings kept in memory only, so an unreadable file
    /// is never overwritten.
    #[must_use]
    pub fn unsaved() -> Self {
        Self {
            path: None,
            journal_interval_s: DEFAULT_JOURNAL_INTERVAL_S,
            rta: RtaSettings::default(),
            recent_inputs: Vec::new(),
        }
    }

    /// Returns previously used inputs, most recent first.
    #[must_use]
    pub fn recent_inputs(&self) -> &[RecentInput] {
        &self.recent_inputs
    }

    /// Moves `input` to the front of the recent inputs and saves the file.
    ///
    /// The change stays in memory even if writing fails.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be encoded or written.
    pub fn remember_input(&mut self, input: RecentInput) -> Result<(), SettingsError> {
        self.recent_inputs
            .retain(|recent| !recent.is_same_input(&input));
        self.recent_inputs.insert(0, input);
        self.recent_inputs.truncate(RECENT_INPUT_LIMIT);
        self.save()
    }

    /// Returns how often the session journal is written, in seconds.
    #[must_use]
    pub const fn journal_interval_s(&self) -> u32 {
        self.journal_interval_s
    }

    /// Sets the journal write interval and saves the file. Values other than
    /// [`JOURNAL_INTERVAL_CHOICES_S`] fall back to the default.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be encoded or written.
    pub fn set_journal_interval_s(&mut self, interval_s: u32) -> Result<(), SettingsError> {
        self.journal_interval_s = valid_journal_interval_s(interval_s);
        self.save()
    }

    /// Returns the RTA preferences.
    #[must_use]
    pub const fn rta(&self) -> RtaSettings {
        self.rta
    }

    /// Sets the RTA preferences and saves the file. A range whose lowest
    /// band is not below its highest falls back to the default range.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be encoded or written.
    pub fn set_rta(&mut self, rta: RtaSettings) -> Result<(), SettingsError> {
        self.rta = valid_rta(rta);
        self.save()
    }

    fn save(&self) -> Result<(), SettingsError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = SettingsFile {
            version: SETTINGS_VERSION,
            journal_interval_s: self.journal_interval_s,
            rta: self.rta,
            recent_inputs: self.recent_inputs.clone(),
        };
        let text = toml::to_string(&file).map_err(SettingsError::Serialize)?;
        write_atomically(path, &text).map_err(|source| SettingsError::Write {
            path: path.clone(),
            source,
        })
    }
}

fn valid_rta(mut rta: RtaSettings) -> RtaSettings {
    if !(rta.min_hz.is_finite() && rta.max_hz.is_finite() && rta.min_hz < rta.max_hz) {
        let default = RtaSettings::default();
        rta.min_hz = default.min_hz;
        rta.max_hz = default.max_hz;
    }
    rta
}

fn valid_journal_interval_s(interval_s: u32) -> u32 {
    if JOURNAL_INTERVAL_CHOICES_S.contains(&interval_s) {
        interval_s
    } else {
        DEFAULT_JOURNAL_INTERVAL_S
    }
}

#[cfg(test)]
mod tests;

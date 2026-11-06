#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! UI-independent orchestration of audio, DSP, and session recording.

mod atomic_file;
mod calibration_store;
#[cfg(feature = "cpal-backend")]
mod capture_runner;
mod demo;
mod engine;
mod journal;
mod recorder;
mod settings;

/// App name written to saved bundles as `software: "<name> <version>"` (ADR 0016;
/// "SPL Meter" before 2026-11-05, so older bundles carry that name).
pub const SOFTWARE_NAME: &str = "Re:sonus Meter";

pub use spl_dsp::bands::{Band, Bandwidth, bands as rta_bands};
pub use spl_dsp::calibration::{CalibrationError, CalibrationProposal};
pub use spl_dsp::filterbank::BandDetector;
use spl_dsp::weighting::UnsupportedSampleRate;
pub use spl_dsp::weighting::Weighting;
use spl_session::{LogRecord, SessionError};

pub use calibration_store::{
    CalibrationKind, CalibrationRecord, CalibrationStore, CalibrationStoreError, InputProfile,
};
#[cfg(feature = "cpal-backend")]
pub use capture_runner::{CaptureError, CaptureRunner};
pub use demo::{DEMO_DEVICE_ID, DemoRunner};
pub use engine::Engine;
pub use journal::{
    JournalError, RECOVERY_VERSION, RecoverableSession, discard_journal_directory,
    recoverable_sessions,
};
pub use recorder::SessionConfig;
pub use settings::{
    DEFAULT_JOURNAL_INTERVAL_S, JOURNAL_INTERVAL_CHOICES_S, RECENT_INPUT_LIMIT, RecentInput,
    RtaSettings, SETTINGS_VERSION, Settings, SettingsError,
};
pub use spl_session::{
    Attachment, Calibration, CalibrationType, Device, Marker, MarkerKind, SegmentAggregate,
};

/// The continuously calculated values for one frequency weighting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeightingLevels {
    /// Fast time-weighted level.
    pub fast_db: f64,
    /// Slow time-weighted level.
    pub slow_db: f64,
    /// Equivalent level since the last reset.
    pub leq_db: Option<f64>,
    /// Maximum sample peak since the last reset.
    pub peak_db: Option<f64>,
    /// Maximum Fast level since the last reset.
    pub fast_max_db: f64,
}

/// A non-blocking read model published by the measurement engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapshot {
    /// A-weighted levels.
    pub a: WeightingLevels,
    /// C-weighted levels.
    pub c: WeightingLevels,
    /// Z-weighted levels.
    pub z: WeightingLevels,
    /// Whether any processed raw sample reached the overload threshold.
    pub clipped: bool,
    /// Whether a calibration offset was supplied.
    pub calibrated: bool,
    /// Samples accumulated since the last reset.
    pub sample_count: u64,
    /// Whether the recording is paused (Leq, peak and maximum are frozen).
    pub paused: bool,
}

/// The observable state of an automatic calibrator capture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalibrationCaptureState {
    /// No automatic capture has been started.
    Idle,
    /// Raw samples are being collected outside the audio callback.
    Capturing {
        /// Number of collected raw samples.
        captured_samples: usize,
        /// Number of raw samples required to evaluate the capture.
        target_samples: usize,
        /// Largest raw sample seen so far, in dBFS.
        peak_dbfs: f64,
    },
    /// A capture has completed and awaits the user's approval or retry.
    Complete(Result<CalibrationProposal, CalibrationError>),
}

/// Recorded Fast-level history and marker information for the Log view.
#[derive(Debug, Clone, PartialEq)]
pub struct LogHistory {
    /// Fast A/C/Z records of the requested window, one per completed logging
    /// interval, or overview points that each combine several.
    pub records: Vec<LogRecord>,
    /// Number of log rows recorded in the session.
    pub record_count: usize,
    /// Markers delimiting the saved measurement segments.
    pub markers: Vec<Marker>,
    /// Elapsed duration since the recording session started.
    pub duration_s: f64,
    /// Active interval between completed log records.
    pub interval_s: f64,
}

/// The bands the real-time analyser covers.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RtaConfig {
    /// Octave or one-third-octave bands.
    #[serde(with = "settings::BandwidthSetting")]
    pub bandwidth: Bandwidth,
    /// Lowest nominal mid-band frequency shown.
    pub min_hz: f64,
    /// Highest nominal mid-band frequency shown.
    pub max_hz: f64,
}

/// One band of an [`RtaFrame`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RtaBandLevel {
    /// Nominal mid-band frequency, the band's label.
    pub nominal_hz: f64,
    /// Exact mid-band frequency.
    pub exact_hz: f64,
    /// Band level in dB, `None` before the first sample.
    pub level_db: Option<f64>,
}

/// The analyser's band levels for one detector and display weighting.
#[derive(Debug, Clone, PartialEq)]
pub struct RtaFrame {
    /// Bands in ascending frequency.
    pub bands: Vec<RtaBandLevel>,
}

/// User-provided metadata applied to one exported measurement bundle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveMetadata {
    /// Required display name for the exported bundle.
    pub name: String,
    /// Optional Markdown description.
    pub description: String,
    /// Part of the session to save.
    pub range: SaveRange,
    /// Files copied into the bundle's `attachments/` folder.
    pub attachments: Vec<Attachment>,
}

/// The markers delimiting the saved part of a session.
///
/// `None` means the session's start, or the stop marker the save adds; the
/// default saves the whole session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SaveRange {
    /// Marker the saved part starts at.
    pub start_marker_id: Option<u64>,
    /// Marker the saved part ends at.
    pub end_marker_id: Option<u64>,
}

/// Error constructing a measurement engine.
#[derive(Debug)]
pub enum EngineError {
    /// The DSP layer rejected the requested stream rate.
    UnsupportedSampleRate(UnsupportedSampleRate),
    /// The recorder was asked to use an invalid log interval.
    InvalidLogInterval,
    /// There is no active session recorder.
    NoActiveSession,
    /// A portable measurement bundle could not be written.
    Session(SessionError),
    /// A marker timestamp is already reserved while its label is being entered.
    MarkerTimestampAlreadyReserved,
    /// A marker timestamp was not reserved by the active session.
    InvalidMarkerTimestamp,
    /// Pause was requested while the recording is already paused.
    AlreadyPaused,
    /// Resume was requested while the recording is not paused.
    NotPaused,
    /// The requested save range is not delimited by markers.
    InvalidRange(spl_session::RangeError),
    /// The session has no marker with this id.
    UnknownMarker(u64),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSampleRate(error) => error.fmt(formatter),
            Self::InvalidLogInterval => {
                formatter.write_str("log interval must be finite and positive")
            }
            Self::NoActiveSession => formatter.write_str("no active measurement session"),
            Self::Session(error) => error.fmt(formatter),
            Self::MarkerTimestampAlreadyReserved => {
                formatter.write_str("a marker timestamp is already reserved")
            }
            Self::InvalidMarkerTimestamp => {
                formatter.write_str("marker timestamp was not reserved by this session")
            }
            Self::AlreadyPaused => formatter.write_str("the recording is already paused"),
            Self::NotPaused => formatter.write_str("the recording is not paused"),
            Self::InvalidRange(error) => error.fmt(formatter),
            Self::UnknownMarker(id) => write!(formatter, "there is no marker {id}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<UnsupportedSampleRate> for EngineError {
    fn from(error: UnsupportedSampleRate) -> Self {
        Self::UnsupportedSampleRate(error)
    }
}

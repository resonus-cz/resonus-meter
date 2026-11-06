use std::fs;
use std::path::PathBuf;

use spl_dsp::calibration::CalibrationProposal;
use spl_session::CalibrationType;
use time::{Duration, OffsetDateTime};

use super::{
    CalibrationKind, CalibrationRecord, CalibrationStore, CalibrationStoreError, InputProfile,
};

/// A scratch file path unique to one test, removed when dropped.
struct ScratchFile(PathBuf);

impl ScratchFile {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "spl-calibration-store-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        Self(directory.join("calibrations.toml"))
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        if let Some(directory) = self.0.parent() {
            let _ = fs::remove_dir_all(directory);
        }
    }
}

fn input(device_id: &str, channel_index: u16) -> InputProfile {
    InputProfile {
        device_id: device_id.to_owned(),
        device_name: format!("Device {device_id}"),
        channel_index,
        sample_rate_hz: 48_000,
    }
}

fn at(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + Duration::seconds(seconds)
}

const PROPOSAL: CalibrationProposal = CalibrationProposal {
    calibrator_level_db: 94.0,
    measured_level_dbfs: -23.0,
    frequency_hz: 1_000.2,
    offset_db: 117.0,
    stability_db: 0.01,
    tone_to_background_db: 45.0,
    peak_dbfs: -20.0,
};

#[test]
fn missing_file_opens_an_empty_store() {
    let file = ScratchFile::new("missing");

    let store = CalibrationStore::open(&file.0).expect("missing file is not an error");

    assert!(store.active(&input("mic", 0)).is_none());
    assert!(!file.0.exists(), "opening must not create the file");
}

#[test]
fn appended_records_persist_and_the_latest_per_profile_is_active() {
    let file = ScratchFile::new("persist");
    let mut store = CalibrationStore::open(&file.0).expect("open");
    store
        .append(CalibrationRecord::from_proposal(
            &input("mic", 0),
            &PROPOSAL,
            "first",
            at(10),
        ))
        .expect("append");
    store
        .append(CalibrationRecord::manual(&input("mic", 1), 90.0, at(20)))
        .expect("append");
    store
        .append(CalibrationRecord::manual(&input("mic", 0), 118.5, at(30)))
        .expect("append");

    let reopened = CalibrationStore::open(&file.0).expect("reopen");

    let active = reopened
        .active(&input("mic", 0))
        .expect("profile has a record");
    assert_eq!(active.kind, CalibrationKind::Manual);
    assert!((active.offset_db - 118.5).abs() < f64::EPSILON);
    let history = reopened.history(&input("mic", 0));
    assert_eq!(history.len(), 2, "other channels are a separate profile");
    assert_eq!(history[0].saved_utc, at(30), "history is newest first");
    assert_eq!(history[1].note, "first");
    assert_eq!(history[1].tone_frequency_hz, Some(1_000.2));
    assert!(reopened.active(&input("other", 0)).is_none());
}

#[test]
fn profile_identity_ignores_display_name_and_rate() {
    let mut store = CalibrationStore::unsaved();
    store
        .append(CalibrationRecord::manual(&input("mic", 0), 100.0, at(0)))
        .expect("in-memory append");
    let renamed = InputProfile {
        device_name: "Renamed by PipeWire".to_owned(),
        sample_rate_hz: 96_000,
        ..input("mic", 0)
    };

    assert!(store.active(&renamed).is_some());
}

#[test]
fn restoring_appends_a_copy_that_becomes_active() {
    let mut store = CalibrationStore::unsaved();
    let calibrator =
        CalibrationRecord::from_proposal(&input("mic", 0), &PROPOSAL, "seated", at(10));
    store.append(calibrator.clone()).expect("append");
    store
        .append(CalibrationRecord::manual(&input("mic", 0), 120.0, at(20)))
        .expect("append");

    store.append(calibrator.restored(at(30))).expect("restore");

    let active = store.active(&input("mic", 0)).expect("active record");
    assert_eq!(active.kind, CalibrationKind::Calibrator);
    assert_eq!(active.saved_utc, at(30));
    assert_eq!(
        active.calibrated_utc,
        at(10),
        "restore keeps the measurement time"
    );
    assert_eq!(
        store.history(&input("mic", 0)).len(),
        3,
        "history is append-only"
    );
}

#[test]
fn records_convert_to_session_calibration_metadata() {
    let calibrator =
        CalibrationRecord::from_proposal(&input("mic", 0), &PROPOSAL, "", at(10)).calibration();
    assert_eq!(calibrator.calibration_type, CalibrationType::Calibrator);
    assert_eq!(calibrator.calibrator_level_db, Some(94.0));
    assert_eq!(calibrator.calibrator_freq_hz, Some(1_000.2));
    assert_eq!(calibrator.calibrated_utc, Some(at(10)));

    let manual = CalibrationRecord::manual(&input("mic", 0), 99.0, at(20)).calibration();
    assert_eq!(manual.calibration_type, CalibrationType::Manual);
    assert_eq!(manual.calibrator_level_db, None);
    assert!((manual.offset_db - 99.0).abs() < f64::EPSILON);
}

#[test]
fn newer_major_version_is_rejected() {
    let file = ScratchFile::new("version");
    fs::create_dir_all(file.0.parent().expect("parent")).expect("create directory");
    fs::write(&file.0, "version = 2\n").expect("write fixture");

    let error = CalibrationStore::open(&file.0).expect_err("version 2 is unsupported");

    assert!(matches!(
        error,
        CalibrationStoreError::UnsupportedVersion(2)
    ));
}

#[test]
fn unreadable_file_is_reported_and_left_untouched() {
    let file = ScratchFile::new("corrupt");
    fs::create_dir_all(file.0.parent().expect("parent")).expect("create directory");
    fs::write(&file.0, "this is not toml = = =\n").expect("write fixture");

    let error = CalibrationStore::open(&file.0).expect_err("corrupt file");

    assert!(matches!(error, CalibrationStoreError::Parse { .. }));
    assert_eq!(
        fs::read_to_string(&file.0).expect("still readable"),
        "this is not toml = = =\n"
    );
}

#[test]
fn unknown_keys_from_newer_minor_versions_are_tolerated() {
    let file = ScratchFile::new("unknown-keys");
    let mut store = CalibrationStore::open(&file.0).expect("open");
    store
        .append(CalibrationRecord::manual(&input("mic", 0), 100.0, at(0)))
        .expect("append");
    let mut text = fs::read_to_string(&file.0).expect("read");
    text.push_str("future_setting = true\n");
    text = text.replace(
        "kind = \"manual\"",
        "kind = \"manual\"\ninput_gain_db = 12.0",
    );
    fs::write(&file.0, text).expect("write");

    let reopened = CalibrationStore::open(&file.0).expect("unknown keys are ignored");

    assert!(reopened.active(&input("mic", 0)).is_some());
}

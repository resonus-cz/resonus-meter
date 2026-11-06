//! Engine behaviour exercised through the public API with the demo source.

use std::fs;
use std::path::Path;
use std::time::Duration;

use spl_session::{Calibration, CalibrationType};
use time::OffsetDateTime;

mod common;

use common::{Scratch, session_config, sine_block};
use spl_engine::{
    CalibrationCaptureState, DemoRunner, Engine, EngineError, SaveMetadata, SaveRange,
};

fn manual(offset_db: f64) -> Calibration {
    Calibration {
        offset_db,
        calibration_type: CalibrationType::Manual,
        calibrator_level_db: None,
        calibrator_freq_hz: None,
        calibrated_utc: None,
    }
}

#[test]
fn engine_reports_calibrated_sine_level_and_clip() {
    let mut engine = Engine::new(48_000.0, Some(manual(120.0))).expect("valid rate");
    let samples = (0..48_000)
        .map(|index| {
            #[allow(clippy::cast_possible_truncation)]
            let sample = (std::f64::consts::TAU * f64::from(index) / 48.0).sin() as f32;
            sample * 0.1
        })
        .collect::<Vec<_>>();
    engine.process_block(&samples);
    let snapshot = engine.snapshot();

    assert!(snapshot.calibrated);
    assert!(!snapshot.clipped);
    assert!((snapshot.z.leq_db.expect("processed samples") - 96.9897).abs() < 0.02);
}

#[test]
fn demo_calibration_offset_updates_the_full_scale_equivalence() {
    let mut demo = DemoRunner::new(48_000.0, None).expect("valid rate");

    assert_eq!(demo.calibration_offset_db(), None);
    assert_eq!(demo.full_scale_sine_spl_db(), None);

    demo.set_calibration(Some(manual(120.0)));

    assert_eq!(demo.calibration_offset_db(), Some(120.0));
    assert!((demo.full_scale_sine_spl_db().expect("manual offset") - 116.9897).abs() < 0.001);
}

#[test]
fn automatic_capture_collects_raw_samples_before_applying_a_proposal() {
    let mut engine = Engine::new(48_000.0, None).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session configuration");
    engine
        .start_calibration_capture(94.0, 1_000.0)
        .expect("valid capture");
    let samples = (0..(48_000 * 8))
        .map(|index| {
            #[allow(clippy::cast_possible_truncation)]
            let sample =
                (std::f64::consts::TAU * 1_000.0 * f64::from(index) / 48_000.0).sin() as f32;
            sample * 0.1
        })
        .collect::<Vec<_>>();

    engine.process_block(&samples[..48_000]);
    let CalibrationCaptureState::Capturing {
        captured_samples,
        target_samples,
        peak_dbfs,
    } = engine.calibration_capture_state()
    else {
        panic!("capture should still be in progress");
    };
    assert_eq!(captured_samples, 48_000);
    assert_eq!(target_samples, 384_000);
    assert!((peak_dbfs + 20.0).abs() < 0.001);
    engine.process_block(&samples[48_000..]);

    let CalibrationCaptureState::Complete(Ok(proposal)) = engine.calibration_capture_state() else {
        panic!("capture should produce a proposal");
    };
    assert!((proposal.offset_db - 117.0103).abs() < 0.01);
    assert_eq!(engine.calibration_offset_db(), None);
    engine.save_calibration_proposal().expect("proposal saves");
    assert!(
        (engine.calibration_offset_db().expect("saved offset") - proposal.offset_db).abs()
            < f64::EPSILON
    );
    let history = engine.log_history(None).expect("active session");
    assert!(history.records.is_empty());
    assert_eq!(history.markers.len(), 1);
    assert_eq!(history.markers[0].kind, spl_session::MarkerKind::Reset);
    assert_eq!(
        engine.calibration_capture_state(),
        CalibrationCaptureState::Idle
    );
    let root = std::env::current_dir()
        .expect("current directory is available")
        .join(format!(
            "spl-engine-calibration-test-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&root);
    engine.save_measurement(&root).expect("session saves");
    let measurement =
        fs::read_to_string(root.join("measurement.md")).expect("read calibration metadata");
    assert!(measurement.contains("type: calibrator"));
    assert!(measurement.contains("calibrator_level_db: 94.00"));
    assert!(measurement.contains("calibrator_freq_hz: 1000.00"));
    fs::remove_dir_all(root).expect("clean up session bundle");
}

#[test]
fn reset_starts_a_fresh_recording_history() {
    let mut engine = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session configuration");
    let samples = vec![0.1; 48_000];
    engine.process_block(&samples);
    engine.reset();
    engine.process_block(&samples);

    let root = std::env::current_dir()
        .expect("current directory is available")
        .join(format!("spl-engine-recorder-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    engine.save_measurement(&root).expect("session saves");

    let log = fs::read_to_string(root.join("log.csv")).expect("read log");
    let markers = fs::read_to_string(root.join("markers.csv")).expect("read markers");
    let segments = fs::read_to_string(root.join("segments.csv")).expect("read segment values");
    assert_eq!(log.lines().count(), 2);
    assert!(log.lines().skip(1).all(|line| {
        let fields = line.split(',').collect::<Vec<_>>();
        fields.len() == 7
            && fields[4].parse::<f64>().is_ok()
            && fields[5].parse::<f64>().is_ok()
            && fields[6].parse::<f64>().is_ok()
    }));
    assert!(markers.contains(",reset,Reset,"));
    assert_eq!(markers.lines().count(), 3);
    assert!(
        markers.lines().nth(1).is_some_and(|line| {
            line.contains(",reset,Reset,") && !line.contains(",start,Start,")
        })
    );
    assert_eq!(segments.lines().count(), 2);

    fs::remove_dir_all(root).expect("clean up session bundle");
}

#[test]
fn exported_metadata_overrides_the_live_session_defaults() {
    let mut engine = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session configuration");
    engine.process_block(&vec![0.1; 48_000]);

    let root = std::env::current_dir()
        .expect("current directory is available")
        .join(format!(
            "spl-engine-export-metadata-test-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&root);
    engine
        .save_measurement_with_metadata(
            &root,
            SaveMetadata {
                name: "Courtyard survey".to_owned(),
                description: "Measured with a closed window.".to_owned(),
                ..SaveMetadata::default()
            },
        )
        .expect("session saves");

    let measurement =
        fs::read_to_string(root.join("measurement.md")).expect("read measurement metadata");
    assert!(measurement.contains("name: \"Courtyard survey\""));
    assert!(measurement.contains("# Courtyard survey"));
    assert!(measurement.contains("Measured with a closed window."));

    fs::remove_dir_all(root).expect("clean up session bundle");
}

#[test]
fn recorder_exposes_markers_and_records_interval_changes() {
    let mut engine = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session configuration");
    engine.set_log_interval(0.5).expect("valid interval change");
    engine.add_marker("Door opened").expect("active session");
    engine.process_block(&vec![0.1; 24_000]);

    let history = engine.log_history(None).expect("active log history");

    assert!((history.interval_s - 0.5).abs() < f64::EPSILON);
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.markers.len(), 3);
    assert_eq!(
        history.markers[1].kind,
        spl_session::MarkerKind::IntervalChange
    );
    assert_eq!(history.markers[2].label, "Door opened");
}

#[test]
fn reset_preserves_the_selected_log_interval() {
    let mut engine = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session configuration");
    engine.set_log_interval(5.0).expect("valid interval change");
    engine.reset();

    engine.process_block(&vec![0.1; 239_999]);
    let history = engine.log_history(None).expect("active log history");
    assert!((history.interval_s - 5.0).abs() < f64::EPSILON);
    assert!(history.records.is_empty());

    engine.process_block(&[0.1]);
    let history = engine.log_history(None).expect("active log history");
    assert_eq!(history.records.len(), 1);
}

#[test]
fn reserved_marker_uses_click_time_after_label_entry_delay() {
    let mut engine = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session configuration");
    engine.process_block(&vec![0.1; 72_000]);
    let marker_time_s = engine
        .reserve_marker_timestamp()
        .expect("marker timestamp reserves");
    engine.process_block(&vec![0.1; 48_000]);
    engine
        .add_marker_at("Door opened", marker_time_s)
        .expect("reserved timestamp inserts marker");

    let history = engine.log_history(None).expect("active log history");

    assert!((marker_time_s - 1.5).abs() < f64::EPSILON);
    assert_eq!(history.markers.len(), 2);
    assert!((history.markers[1].t_s - marker_time_s).abs() < f64::EPSILON);
    assert_eq!(history.markers[1].label, "Door opened");
    assert_eq!(history.records[1].marker_id, Some(history.markers[1].id));
}

#[test]
fn runner_keeps_calibrator_metadata_supplied_at_construction() {
    // Regression: opening a new input passed only the offset, so bundles
    // saved after a device switch reported a calibrator calibration as manual.
    let calibrated_utc = OffsetDateTime::UNIX_EPOCH;
    let calibration = Calibration {
        offset_db: 117.0,
        calibration_type: CalibrationType::Calibrator,
        calibrator_level_db: Some(94.0),
        calibrator_freq_hz: Some(1_000.0),
        calibrated_utc: Some(calibrated_utc),
    };
    let mut demo = DemoRunner::new(48_000.0, Some(calibration)).expect("valid rate");
    demo.catch_up(Duration::from_millis(100));
    let root = std::env::current_dir()
        .expect("current directory is available")
        .join(format!("spl-engine-profile-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);

    demo.save_measurement(&root).expect("session saves");

    let measurement = fs::read_to_string(root.join("measurement.md")).expect("read metadata");
    fs::remove_dir_all(&root).expect("clean up session bundle");
    assert!(measurement.contains("type: calibrator"));
    assert!(measurement.contains("calibrator_level_db: 94.00"));
    assert!(measurement.contains("1970-01-01T00:00:00.000Z"));
}

/// A 1 kHz sine at 48 kHz; the weighting filters remove DC, so tests need a tone.
#[test]
fn pause_freezes_measured_values_while_fast_follows_the_input() {
    let mut engine = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    engine.process_block(&sine_block(1, 0.01));
    let before = engine.snapshot();

    engine.pause().expect("recording runs");
    engine.process_block(&sine_block(1, 0.5));
    let paused = engine.snapshot();

    assert!(paused.paused);
    assert_eq!(paused.z.leq_db, before.z.leq_db);
    assert_eq!(paused.z.peak_db, before.z.peak_db);
    // Frozen, not merely close: compare bit patterns.
    assert_eq!(
        paused.z.fast_max_db.to_bits(),
        before.z.fast_max_db.to_bits()
    );
    assert!(
        paused.z.fast_db > before.z.fast_db + 30.0,
        "Fast stays live"
    );
    assert!(matches!(engine.pause(), Err(EngineError::AlreadyPaused)));

    engine.resume().expect("recording is paused");
    assert!(!engine.snapshot().paused);
    assert!(matches!(engine.resume(), Err(EngineError::NotPaused)));
    let history = engine.log_history(None).expect("active session");
    assert!(
        (history.duration_s - 2.0).abs() < 1e-9,
        "paused time counts"
    );
}

#[test]
fn pause_requires_an_active_session() {
    let mut engine = Engine::new(48_000.0, None).expect("valid rate");

    assert!(matches!(engine.pause(), Err(EngineError::NoActiveSession)));
}

/// A scratch folder unique to one test, removed when dropped.
#[test]
fn a_crashed_session_is_recovered_paused_from_its_journal() {
    let sessions = Scratch::new("recover");
    let started = OffsetDateTime::UNIX_EPOCH;
    let mut crashed = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    crashed
        .start_session(session_config(), started)
        .expect("valid session");
    crashed.start_journal(&sessions.0).expect("journal starts");
    crashed.process_block(&sine_block(2, 0.1));
    crashed.flush_journal().expect("flushes");
    let before = crashed.snapshot();
    drop(crashed);

    let found = spl_engine::recoverable_sessions(&sessions.0);
    assert_eq!(found.len(), 1);
    let session = found[0].as_ref().expect("readable journal");
    assert_eq!(session.name, "Engine test");
    assert!((session.recorded_until_s - 2.0).abs() < 1e-9);

    let mut engine = Engine::new(48_000.0, None).expect("valid rate");
    engine
        .start_session(session_config(), started + time::Duration::seconds(30))
        .expect("valid session");
    engine
        .start_journal(&sessions.0)
        .expect("fresh journal starts");
    let fresh = engine.journal_directory().expect("journal").to_owned();
    engine
        .recover_session(&session.directory, started + time::Duration::seconds(30))
        .expect("recovers");

    let snapshot = engine.snapshot();
    assert!(snapshot.paused);
    assert_eq!(
        snapshot.z.leq_db, before.z.leq_db,
        "meter values are restored"
    );
    assert_eq!(engine.calibration_offset_db(), Some(100.0));
    assert!(engine.has_unsaved_changes());
    assert!(!fresh.exists(), "the replaced session's journal is removed");
    assert_eq!(
        engine.journal_directory(),
        Some(session.directory.as_path())
    );
    let history = engine.log_history(None).expect("session");
    assert_eq!(history.records.len(), 2);
    assert!((history.duration_s - 30.0).abs() < 1e-6);

    engine.reset();
    engine.flush_journal().expect("reset journal is written");
    assert!(
        !session.directory.exists(),
        "reset discards the old journal"
    );
    assert_eq!(spl_engine::recoverable_sessions(&sessions.0).len(), 1);
    engine.discard_journal().expect("discards");
    assert!(spl_engine::recoverable_sessions(&sessions.0).is_empty());
}

#[test]
fn a_journal_from_another_sample_rate_is_not_recovered() {
    let sessions = Scratch::new("rate");
    let mut recorded = Engine::new(44_100.0, None).expect("valid rate");
    recorded
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    recorded.start_journal(&sessions.0).expect("journal starts");
    let directory = recorded.journal_directory().expect("journal").to_owned();

    let mut engine = Engine::new(48_000.0, None).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");

    assert!(matches!(
        engine.recover_session(&directory, OffsetDateTime::UNIX_EPOCH),
        Err(spl_engine::JournalError::SampleRateMismatch { .. })
    ));
    assert!(
        directory.exists(),
        "a journal that was not recovered is kept"
    );
}

#[test]
fn markers_and_pauses_are_journaled_immediately() {
    let sessions = Scratch::new("immediate");
    let mut engine = Engine::new(48_000.0, None).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    engine.start_journal(&sessions.0).expect("journal starts");
    engine.process_block(&sine_block(1, 0.1));
    engine.add_marker("Door opened").expect("session");
    engine.process_block(&sine_block(1, 0.1));
    let elapsed_s = engine.reserve_marker_timestamp().expect("none pending");
    engine.add_marker_at("Later", elapsed_s).expect("reserved");
    engine.pause().expect("running");
    // No explicit flush: a crash now must not lose the markers.
    drop(engine);

    let found = spl_engine::recoverable_sessions(&sessions.0);
    let session = found[0].as_ref().expect("readable journal");
    assert!((session.recorded_until_s - 2.0).abs() < 1e-9);
    let mut recovered = Engine::new(48_000.0, None).expect("valid rate");
    recovered
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    recovered
        .recover_session(&session.directory, OffsetDateTime::UNIX_EPOCH)
        .expect("recovers");
    let labels = recovered
        .log_history(None)
        .expect("session")
        .markers
        .into_iter()
        .map(|marker| marker.label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["Start", "Door opened", "Later", "Pause"]);
}

#[test]
fn the_demo_clock_follows_elapsed_time_without_drift() {
    let mut demo = DemoRunner::new(48_000.0, None).expect("valid rate");

    // Display refreshes every 125 ms: eight of them are one second.
    let mut snapshot = demo.catch_up(Duration::ZERO);
    for tick in 1..=8 {
        snapshot = demo.catch_up(Duration::from_millis(125 * tick));
    }
    assert_eq!(snapshot.sample_count, 48_000);

    // Fractional sample periods carry over instead of being lost.
    demo.catch_up(Duration::from_secs_f64(1.0 + 1.5 / 48_000.0));
    let snapshot = demo.catch_up(Duration::from_secs_f64(1.0 + 3.0 / 48_000.0));
    assert_eq!(snapshot.sample_count, 48_003);
}

#[test]
fn the_demo_catches_up_after_a_stall_one_second_per_tick() {
    let mut demo = DemoRunner::new(48_000.0, None).expect("valid rate");
    let stalled = Duration::from_secs(3);

    assert_eq!(demo.catch_up(stalled).sample_count, 48_000);
    demo.catch_up(stalled);
    assert_eq!(demo.catch_up(stalled).sample_count, 144_000);
    assert_eq!(demo.catch_up(stalled).sample_count, 144_000);
}

#[test]
fn saving_a_marker_range_writes_only_that_part_and_matches_the_preview() {
    let sessions = Scratch::new("range");
    let mut engine = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    engine.process_block(&sine_block(2, 0.01));
    engine.add_marker("Machine on").expect("session");
    engine.process_block(&sine_block(2, 0.3));
    engine.add_marker("Machine off").expect("session");
    engine.process_block(&sine_block(2, 0.01));
    let range = SaveRange {
        start_marker_id: Some(2),
        end_marker_id: Some(3),
    };
    let preview = engine.range_summary(range).expect("valid range");

    let bundle = sessions.0.join("machine");
    engine
        .save_measurement_with_metadata(
            &bundle,
            SaveMetadata {
                name: "Machine".to_owned(),
                range,
                ..SaveMetadata::default()
            },
        )
        .expect("saves");

    let markers = fs::read_to_string(bundle.join("markers.csv")).expect("reads");
    let labels = markers
        .lines()
        .skip(1)
        .map(|line| line.split(',').nth(4).expect("label"))
        .collect::<Vec<_>>();
    assert_eq!(labels, ["Machine on", "Machine off"]);
    let rows = fs::read_to_string(bundle.join("log.csv")).expect("reads");
    let times = rows
        .lines()
        .skip(1)
        .map(|line| line.split(',').next().expect("t_s"))
        .collect::<Vec<_>>();
    assert_eq!(times, ["3.000", "4.000"]);
    let measurement = fs::read_to_string(bundle.join("measurement.md")).expect("reads");
    assert!(measurement.contains("range: { start_s: 2.000, end_s: 4.000 }"));
    let leq = preview.values.z.leq_db.expect("recorded");
    assert!(
        measurement.contains(&format!("Z: {{ Leq_db: {leq:.2},")),
        "{measurement}"
    );
    // A 0.3 amplitude sine: 20·log10(0.3/√2) + 100 dB.
    assert!((leq - 86.532).abs() < 0.05, "{leq}");

    let whole = engine
        .range_summary(SaveRange::default())
        .expect("whole session");
    assert!(whole.values.z.leq_db < preview.values.z.leq_db);
    assert!(matches!(
        engine.range_summary(SaveRange {
            start_marker_id: Some(3),
            end_marker_id: Some(2),
        }),
        Err(EngineError::InvalidRange(_))
    ));
}

#[test]
fn a_new_session_starts_a_fresh_recording_with_its_own_journal() {
    let sessions = Scratch::new("new-session");
    let mut engine = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    engine.start_journal(&sessions.0).expect("journal starts");
    engine.process_block(&sine_block(1, 0.1));
    engine.add_marker("Before").expect("session");
    let old_journal = engine.journal_directory().expect("journal").to_owned();

    let later = OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1);
    engine.new_session(later).expect("session");

    let history = engine.log_history(None).expect("session");
    assert_eq!(history.markers.len(), 1);
    assert_eq!(history.markers[0].kind, spl_engine::MarkerKind::Start);
    assert_eq!(history.markers[0].utc, later);
    assert!(history.records.is_empty());
    assert_eq!(engine.snapshot().z.leq_db, None);
    assert!(!engine.has_unsaved_changes());
    assert_eq!(engine.calibration_offset_db(), Some(100.0));
    assert!(
        !old_journal.exists(),
        "the discarded session's journal is removed"
    );
    assert!(engine.journal_directory().is_some_and(Path::exists));
}

#[test]
fn markers_can_be_relabelled_and_annotated() {
    let sessions = Scratch::new("edit-marker");
    let mut engine = Engine::new(48_000.0, None).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    engine.process_block(&sine_block(1, 0.1));
    engine.add_marker("Marker").expect("session");
    let saved = sessions.0.join("before");
    engine.save_measurement(&saved).expect("saves");
    assert!(!engine.has_unsaved_changes());

    engine
        .edit_marker(2, "Door opened", "Chiller, \"north\" side")
        .expect("marker exists");

    assert!(engine.has_unsaved_changes(), "an edit is a change");
    let marker = &engine.log_history(None).expect("session").markers[1];
    assert_eq!(
        (marker.label.as_str(), marker.note.as_str()),
        ("Door opened", "Chiller, \"north\" side")
    );
    let bundle = sessions.0.join("after");
    engine.save_measurement(&bundle).expect("saves");
    let markers = fs::read_to_string(bundle.join("markers.csv")).expect("reads");
    assert!(
        markers.contains(",user,Door opened,\"Chiller, \"\"north\"\" side\"\n"),
        "{markers}"
    );
    assert!(matches!(
        engine.edit_marker(99, "x", ""),
        Err(EngineError::UnknownMarker(99))
    ));
}

fn sine_at(frequency_hz: f64, seconds: u32, amplitude: f32) -> Vec<f32> {
    (0..seconds * 48_000)
        .map(|index| {
            #[allow(clippy::cast_possible_truncation)]
            let sample =
                (std::f64::consts::TAU * frequency_hz * f64::from(index) / 48_000.0).sin() as f32;
            sample * amplitude
        })
        .collect()
}

#[test]
fn the_rta_reports_calibrated_band_levels_with_display_weighting() {
    use spl_engine::{BandDetector, Bandwidth, RtaConfig, Weighting};
    let mut engine = Engine::new(48_000.0, Some(manual(100.0))).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    assert!(
        engine.rta_frame(BandDetector::Leq, Weighting::Z).is_none(),
        "off by default"
    );
    engine.set_rta(Some(RtaConfig {
        bandwidth: Bandwidth::ThirdOctave,
        min_hz: 20.0,
        max_hz: 20_000.0,
    }));
    engine.process_block(&sine_at(100.0, 2, 0.1));
    engine.reset();
    engine.process_block(&sine_at(100.0, 2, 0.1));

    let level = |engine: &Engine, weighting| {
        engine
            .rta_frame(BandDetector::Leq, weighting)
            .expect("RTA on")
            .bands
            .into_iter()
            .find(|band| (band.nominal_hz - 100.0).abs() < 1e-9)
            .and_then(|band| band.level_db)
            .expect("100 Hz band")
    };
    // 0.1 amplitude: 20·log10(0.1/√2) + 100 dB.
    let expected = 20.0 * (0.1 / 2_f64.sqrt()).log10() + 100.0;
    assert!((level(&engine, Weighting::Z) - expected).abs() < 0.2);
    let exact_hz = engine
        .rta_frame(BandDetector::Leq, Weighting::Z)
        .expect("on")
        .bands
        .iter()
        .find(|band| (band.nominal_hz - 100.0).abs() < 1e-9)
        .expect("100 Hz band")
        .exact_hz;
    let a_weighting = Weighting::A.response_db(exact_hz);
    assert!(
        (level(&engine, Weighting::A) - level(&engine, Weighting::Z) - a_weighting).abs() < 1e-9
    );
    assert!((a_weighting + 19.1).abs() < 0.2);

    engine.pause().expect("running");
    engine.process_block(&sine_at(100.0, 1, 0.9));
    assert!(
        (level(&engine, Weighting::Z) - expected).abs() < 0.2,
        "a pause holds band Leq"
    );

    engine.set_rta(None);
    assert!(engine.rta_frame(BandDetector::Fast, Weighting::Z).is_none());
}

#[test]
fn recoverable_sessions_say_whether_they_hold_data_and_can_all_be_discarded() {
    let sessions = Scratch::new("holds-data");
    let mut empty = Engine::new(48_000.0, None).expect("valid rate");
    empty
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    empty.start_journal(&sessions.0).expect("journal starts");
    let mut recorded = Engine::new(48_000.0, None).expect("valid rate");
    recorded
        .start_session(
            session_config(),
            OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1),
        )
        .expect("valid session");
    recorded.start_journal(&sessions.0).expect("journal starts");
    recorded.process_block(&sine_block(1, 0.1));
    recorded.flush_journal().expect("flushes");
    let unreadable = sessions.0.join("session-broken");
    fs::create_dir_all(&unreadable).expect("creates");
    fs::write(unreadable.join("recovery.toml"), "version = [").expect("writes");

    let found = spl_engine::recoverable_sessions(&sessions.0);
    let holds_data = found
        .iter()
        .filter_map(|entry| entry.as_ref().ok())
        .map(|session| session.holds_data)
        .collect::<Vec<_>>();
    assert_eq!(holds_data, [true, false], "newest first");
    assert_eq!(found.iter().filter(|entry| entry.is_err()).count(), 1);

    for entry in &found {
        let directory = match entry {
            Ok(session) => &session.directory,
            Err((directory, _)) => directory,
        };
        spl_engine::discard_journal_directory(directory).expect("discards");
    }
    assert!(spl_engine::recoverable_sessions(&sessions.0).is_empty());
}

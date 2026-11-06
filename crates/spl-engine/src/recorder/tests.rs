//! Segment combination checks (specification 9.1 item 9).
//!
//! Weighted samples are fed straight into the recorder so the reference
//! values are exact; the weighting filters have their own tests in `spl-dsp`.

use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};
use spl_dsp::detector::LevelAccumulator;
use spl_session::{Calibration, CalibrationType, Device, MarkerKind};
use time::OffsetDateTime;

use super::{SessionConfig, SessionRecorder};

const SAMPLE_RATE_HZ: f64 = 48_000.0;
const OFFSET_DB: f64 = 100.0;
/// Segment Leq values pass through `10·log10` and back when combined, so
/// agreement is limited by f64 rounding rather than being bit-exact.
const LEQ_TOLERANCE_DB: f64 = 1e-9;

fn recorder() -> SessionRecorder {
    let config = SessionConfig {
        band_log: None,
        name: "Segment test".to_owned(),
        software: "SPL Meter test".to_owned(),
        device: Device {
            id: "test.input".to_owned(),
            name: "Test input".to_owned(),
            backend: "test".to_owned(),
            channel: 1,
            sample_rate_hz: 48_000,
        },
        calibration: Calibration {
            offset_db: OFFSET_DB,
            calibration_type: CalibrationType::Manual,
            calibrator_level_db: None,
            calibrator_freq_hz: None,
            calibrated_utc: None,
        },
        log_interval_s: 1.0,
        author: None,
        tags: Vec::new(),
        description: String::new(),
    };
    SessionRecorder::new(config, SAMPLE_RATE_HZ, OffsetDateTime::UNIX_EPOCH)
}

/// Feeds `samples` as the A, C and Z weighted signals (Z scaled by 0.5 so the
/// three weightings differ) and inserts a user marker before each index in
/// `marker_positions`.
fn record(samples: &[f64], marker_positions: &[usize]) -> SessionRecorder {
    let mut recorder = recorder();
    let mut markers = marker_positions.iter().peekable();
    for (index, &sample) in samples.iter().enumerate() {
        while markers.next_if(|&&position| position == index).is_some() {
            recorder.marker(MarkerKind::User, "Marker", OFFSET_DB);
        }
        recorder.process_sample(
            sample,
            sample,
            -sample,
            0.5 * sample,
            false,
            [0.0; 3],
            OFFSET_DB,
        );
    }
    recorder
}

fn reference(samples: &[f64], scale: f64) -> LevelAccumulator {
    let mut accumulator = LevelAccumulator::default();
    for &sample in samples {
        accumulator.process(scale * sample);
    }
    accumulator
}

fn deterministic_config() -> Config {
    Config {
        cases: 128,
        rng_seed: RngSeed::Fixed(0x5e6_0e47),
        failure_persistence: None,
        ..Config::default()
    }
}

proptest! {
    #![proptest_config(deterministic_config())]

    #[test]
    fn combined_segments_reproduce_whole_session_leq_and_peak(
        samples in proptest::collection::vec(-1.0_f64..=1.0, 1..2_000),
        raw_positions in proptest::collection::vec(0_usize..2_000, 0..12),
    ) {
        let mut positions: Vec<usize> =
            raw_positions.into_iter().map(|position| position % samples.len()).collect();
        positions.sort_unstable();

        let bundle = record(&samples, &positions).bundle(OFFSET_DB, None);

        prop_assert_eq!(bundle.summary.sample_count, samples.len() as u64);
        let summary = bundle.summary.values;
        for (combined, scale) in [(summary.a, 1.0), (summary.c, -1.0), (summary.z, 0.5)] {
            let whole = reference(&samples, scale);
            match (combined.leq_db, whole.leq_dbfs()) {
                (Some(combined_db), Some(whole_dbfs)) => {
                    prop_assert!((combined_db - (whole_dbfs + OFFSET_DB)).abs() < LEQ_TOLERANCE_DB);
                }
                (combined_db, whole_dbfs) => prop_assert_eq!(combined_db, whole_dbfs),
            }
            prop_assert_eq!(combined.peak_db, whole.peak_dbfs().map(|peak| peak + OFFSET_DB));
        }
    }
}

#[test]
fn two_known_levels_give_per_segment_and_combined_values() {
    // A square wave of amplitude `a` has mean square a², so Leq = peak = 20·log10(a).
    const FIRST_AMPLITUDE: f64 = 0.1; // −20 dBFS
    const SECOND_AMPLITUDE: f64 = 0.01; // −40 dBFS
    const FIRST_SAMPLES: usize = 48_000;
    const SECOND_SAMPLES: usize = 144_000;
    let square = |amplitude: f64, count: usize| {
        (0..count).map(move |index| {
            if index % 2 == 0 {
                amplitude
            } else {
                -amplitude
            }
        })
    };
    let samples: Vec<f64> = square(FIRST_AMPLITUDE, FIRST_SAMPLES)
        .chain(square(SECOND_AMPLITUDE, SECOND_SAMPLES))
        .collect();

    let bundle = record(&samples, &[FIRST_SAMPLES]).bundle(OFFSET_DB, None);

    let [first, second] = bundle.segments.as_slice() else {
        panic!("expected two segments, got {}", bundle.segments.len());
    };
    let assert_close = |actual: Option<f64>, expected: f64| {
        let actual = actual.expect("segment holds samples");
        assert!(
            (actual - expected).abs() < LEQ_TOLERANCE_DB,
            "{actual} != {expected}"
        );
    };
    assert_eq!(first.sample_count, FIRST_SAMPLES as u64);
    assert_close(first.values.a.leq_db, OFFSET_DB - 20.0);
    assert_close(first.values.a.peak_db, OFFSET_DB - 20.0);
    assert_eq!(second.sample_count, SECOND_SAMPLES as u64);
    assert_close(second.values.a.leq_db, OFFSET_DB - 40.0);
    assert_close(
        second.values.z.leq_db,
        OFFSET_DB - 40.0 - 20.0 * 2.0_f64.log10(),
    );

    // Energy mean: (1·10⁻² + 3·10⁻⁴) / 4 in mean square.
    let combined_dbfs = 10.0 * ((1e-2 + 3.0 * 1e-4) / 4.0_f64).log10();
    assert_close(bundle.summary.values.a.leq_db, OFFSET_DB + combined_dbfs);
    assert_close(bundle.summary.values.a.peak_db, OFFSET_DB - 20.0);
}

mod pause {
    use super::{OFFSET_DB, SAMPLE_RATE_HZ, recorder};
    use spl_session::MarkerKind;
    use time::{Duration, OffsetDateTime};

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn feed(recorder: &mut super::SessionRecorder, seconds: f64, sample: f64) {
        for _ in 0..(seconds * SAMPLE_RATE_HZ) as u64 {
            recorder.process_sample(sample, sample, sample, sample, false, [0.0; 3], OFFSET_DB);
        }
    }

    /// One second at 0.1, a two-second pause with loud input, one more second at 0.1.
    fn paused_recording() -> super::SessionRecorder {
        let mut recorder = recorder();
        feed(&mut recorder, 1.0, 0.1);
        recorder.pause(OFFSET_DB);
        feed(&mut recorder, 2.0, 0.9);
        recorder.resume(OFFSET_DB);
        feed(&mut recorder, 1.0, 0.1);
        recorder
    }

    #[test]
    fn paused_time_advances_timestamps_but_records_no_energy() {
        let bundle = paused_recording().bundle(OFFSET_DB, None);

        let kinds = bundle
            .markers
            .iter()
            .map(|marker| (marker.kind, marker.t_s))
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            [
                (MarkerKind::Start, 0.0),
                (MarkerKind::Pause, 1.0),
                (MarkerKind::Resume, 3.0),
                (MarkerKind::Stop, 4.0),
            ]
        );
        assert_eq!(
            bundle.markers[2].utc,
            OffsetDateTime::UNIX_EPOCH + Duration::seconds(3)
        );
        assert_eq!(
            bundle.metadata.end_utc,
            OffsetDateTime::UNIX_EPOCH + Duration::seconds(4)
        );

        let paused = &bundle.segments[1];
        assert_eq!((paused.start_s, paused.end_s), (1.0, 3.0));
        assert_eq!(paused.sample_count, 0);
        assert_eq!(paused.values.a.leq_db, None);

        // A constant 0.1 signal: Leq and peak are both 20·log10(0.1) + offset.
        let expected_db = -20.0 + OFFSET_DB;
        let summary = bundle.summary.values.a;
        assert!((summary.leq_db.expect("recorded samples") - expected_db).abs() < 1e-9);
        assert!((summary.peak_db.expect("recorded samples") - expected_db).abs() < 1e-9);
    }

    #[test]
    fn no_log_rows_are_written_while_paused() {
        let recorder = paused_recording();

        let times = recorder
            .plot_rows(None)
            .0
            .iter()
            .map(|record| record.t_s)
            .collect::<Vec<_>>();
        // The interval grid restarts at the resume marker.
        assert_eq!(times, [1.0, 4.0]);
    }

    #[test]
    fn reset_ends_a_pause() {
        let mut recorder = recorder();
        recorder.pause(OFFSET_DB);
        recorder.reset();

        assert!(!recorder.is_paused());
        feed(&mut recorder, 1.0, 0.1);
        assert_eq!(recorder.plot_rows(None).1, 1);
    }
}

mod fast_max {
    use super::{OFFSET_DB, SAMPLE_RATE_HZ, recorder};
    use spl_session::MarkerKind;

    /// One second of a steady -50 dBFS Fast level with a single excursion.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn second_with_spike(recorder: &mut super::SessionRecorder, spike_dbfs: f64) {
        for index in 0..SAMPLE_RATE_HZ as u32 {
            let fast_dbfs = if index == 1_000 { spike_dbfs } else { -50.0 };
            recorder.process_sample(0.1, 0.1, 0.1, 0.1, false, [fast_dbfs; 3], OFFSET_DB);
        }
    }

    #[test]
    fn segments_keep_their_own_fast_maximum_and_pauses_add_none() {
        let mut recorder = recorder();
        second_with_spike(&mut recorder, -20.0);
        recorder.marker(MarkerKind::User, "Marker", OFFSET_DB);
        second_with_spike(&mut recorder, -35.0);
        recorder.pause(OFFSET_DB);
        second_with_spike(&mut recorder, -5.0);
        recorder.resume(OFFSET_DB);
        let elapsed_s = recorder.reserve_marker_timestamp().expect("none pending");
        // Audio after the click belongs to the segment the marker opens.
        second_with_spike(&mut recorder, -30.0);
        recorder
            .marker_at("Later", elapsed_s, OFFSET_DB)
            .expect("reserved");
        second_with_spike(&mut recorder, -40.0);

        let bundle = recorder.bundle(OFFSET_DB, None);
        let maxima = bundle
            .segments
            .iter()
            .map(|segment| segment.values.a.fast_max_db)
            .collect::<Vec<_>>();
        assert_eq!(
            maxima,
            [
                Some(OFFSET_DB - 20.0),
                Some(OFFSET_DB - 35.0),
                None,
                None,
                Some(OFFSET_DB - 30.0),
            ]
        );
        assert_eq!(bundle.summary.values.z.fast_max_db, Some(OFFSET_DB - 20.0));
    }
}

mod recovery {
    use super::{OFFSET_DB, SAMPLE_RATE_HZ, recorder};
    use crate::recorder::{RecorderState, SessionRecorder};
    use spl_session::MarkerKind;
    use time::{Duration, OffsetDateTime};

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn feed(recorder: &mut SessionRecorder, seconds: f64, start: u64) {
        for index in start..start + (seconds * SAMPLE_RATE_HZ) as u64 {
            #[allow(clippy::cast_precision_loss)]
            let sample = 0.2 * (index as f64 * 0.05).sin();
            recorder.process_sample(sample, sample, sample, sample, false, [-30.0; 3], OFFSET_DB);
        }
    }

    /// 1.5 s, a user marker, then 1 s: the open segment holds data.
    fn running() -> SessionRecorder {
        let mut recorder = recorder();
        feed(&mut recorder, 1.5, 0);
        recorder.marker(MarkerKind::User, "Marker", OFFSET_DB);
        feed(&mut recorder, 1.0, 72_000);
        recorder
    }

    fn through_toml(state: &RecorderState) -> RecorderState {
        let text = toml::to_string(state).expect("state encodes");
        toml::from_str(&text).expect("state decodes")
    }

    #[test]
    fn a_recovered_recording_matches_one_paused_at_the_flush() {
        let original = running();
        let state = through_toml(&original.recovery_state(original.journal_rows().len(), 0));
        let now = OffsetDateTime::UNIX_EPOCH + Duration::seconds(60);

        let mut recovered = SessionRecorder::restore(state, now);
        for &row in original.journal_rows() {
            recovered.restore_row(row);
        }
        let mut reference = original.clone();
        reference.pause(OFFSET_DB);

        assert!(recovered.is_paused());
        let pause = recovered.markers().last().expect("pause marker");
        assert_eq!(pause.kind, MarkerKind::Pause);
        assert!((pause.t_s - 2.5).abs() < 1e-12);
        assert_eq!(
            pause.utc,
            OffsetDateTime::UNIX_EPOCH + Duration::milliseconds(2_500)
        );
        assert!(
            (recovered.current_elapsed_s() - 60.0).abs() < 1e-6,
            "clock moved on"
        );
        assert_eq!(recovered.plot_rows(None), reference.plot_rows(None));

        let recovered_bundle = recovered.bundle(OFFSET_DB, None);
        let reference_bundle = reference.bundle(OFFSET_DB, None);
        assert_eq!(
            recovered_bundle.segments[..2],
            reference_bundle.segments[..2],
            "closed and interrupted segments are exact"
        );
        assert_eq!(recovered_bundle.summary, reference_bundle.summary);

        recovered.resume(OFFSET_DB);
        let resume = recovered.markers().last().expect("resume");
        assert!((resume.t_s - 60.0).abs() < 1e-6);
    }

    #[test]
    fn a_recording_paused_before_the_crash_gets_no_extra_marker() {
        let mut original = running();
        original.pause(OFFSET_DB);
        let markers = original.markers().len();

        let recovered = SessionRecorder::restore(
            through_toml(&original.recovery_state(0, 0)),
            OffsetDateTime::UNIX_EPOCH,
        );

        assert_eq!(recovered.markers().len(), markers);
        assert!(recovered.is_paused());
    }

    #[test]
    fn unsaved_changes_follow_recording_and_saving() {
        let mut recorder = recorder();
        assert!(!recorder.has_unsaved_changes());
        feed(&mut recorder, 0.1, 0);
        assert!(recorder.has_unsaved_changes());

        let directory =
            std::env::temp_dir().join(format!("spl-recorder-unsaved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        recorder
            .write(&directory, OFFSET_DB, None, None)
            .expect("saves");
        std::fs::remove_dir_all(&directory).expect("cleans up");
        assert!(!recorder.has_unsaved_changes());

        recorder.pause(OFFSET_DB);
        assert!(recorder.has_unsaved_changes(), "a marker is a change");

        let recovered =
            SessionRecorder::restore(recorder.recovery_state(0, 0), OffsetDateTime::UNIX_EPOCH);
        assert!(recovered.has_unsaved_changes(), "recovered data is unsaved");
    }
}

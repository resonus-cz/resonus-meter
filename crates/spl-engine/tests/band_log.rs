//! Band `LZeq` per log interval and segment (ADR 0015).

mod common;

use std::fs;

use common::{Scratch, session_config};
use spl_engine::{Bandwidth, Engine, RtaConfig};
use time::OffsetDateTime;

const OCTAVES: RtaConfig = RtaConfig {
    bandwidth: Bandwidth::Octave,
    min_hz: 250.0,
    max_hz: 4_000.0,
};

fn sine(frequency_hz: f64, seconds: f64, amplitude: f64) -> Vec<f32> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = (seconds * 48_000.0) as u32;
    (0..count)
        .map(|index| {
            let phase = std::f64::consts::TAU * frequency_hz * f64::from(index) / 48_000.0;
            #[allow(clippy::cast_possible_truncation)]
            let sample = (amplitude * phase.sin()) as f32;
            sample
        })
        .collect()
}

fn engine_with_bands(sessions: &std::path::Path) -> Engine {
    let mut engine = Engine::new(48_000.0, None).expect("valid rate");
    let mut config = session_config();
    config.band_log = Some(OCTAVES);
    engine
        .start_session(config, OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    engine.start_journal(sessions).expect("journal starts");
    engine
}

fn csv_rows(path: &std::path::Path) -> Vec<Vec<String>> {
    fs::read_to_string(path)
        .expect("reads")
        .lines()
        .map(|line| line.split(',').map(str::to_owned).collect())
        .collect()
}

fn level(cell: &str) -> f64 {
    cell.parse().expect("a level")
}

#[test]
fn a_tone_reads_at_its_level_in_its_band_per_row_and_segment() {
    let scratch = Scratch::new("band-log");
    let mut engine = engine_with_bands(&scratch.0.join("sessions"));
    // 0.1 FS rms·√2 at 1 kHz, then 0.01 at 1 kHz: 20 dB lower.
    engine.process_block(&sine(1_000.0, 2.0, 0.1 * 2_f64.sqrt()));
    engine.flush_journal().expect("flushes");
    engine.add_marker("Quieter").expect("session");
    engine.process_block(&sine(1_000.0, 2.0, 0.01 * 2_f64.sqrt()));
    let bundle = scratch.0.join("saved");
    engine.save_measurement(&bundle).expect("saves");

    let rows = csv_rows(&bundle.join("rta_log.csv"));
    assert_eq!(
        rows[0],
        [
            "t_s",
            "utc",
            "LZeq_250",
            "LZeq_500",
            "LZeq_1000",
            "LZeq_2000",
            "LZeq_4000"
        ]
    );
    assert_eq!(rows.len(), 5, "a header and four rows");
    // Uncalibrated, 0.1 FS rms reads −20 dBFS. The first second includes the
    // filters settling.
    assert!((level(&rows[2][4]) + 20.0).abs() < 0.2, "{:?}", rows[2]);
    assert!((level(&rows[4][4]) + 40.0).abs() < 0.2, "{:?}", rows[4]);
    assert!(
        level(&rows[2][3]) < -20.0 - 15.0,
        "the 500 Hz band rejects it"
    );

    let segments = csv_rows(&bundle.join("rta_segments.csv"));
    assert_eq!(segments.len(), 3, "a header and two segments");
    assert!(
        (level(&segments[1][8]) + 20.0).abs() < 0.2,
        "{:?}",
        segments[1]
    );
    // The band filter's decay from the louder tone adds about 0.2 dB.
    assert!(
        (level(&segments[2][8]) + 40.0).abs() < 0.3,
        "{:?}",
        segments[2]
    );
    let markdown = fs::read_to_string(bundle.join("measurement.md")).expect("reads");
    assert!(markdown.contains("  bands_hz: [250, 500, 1000, 2000, 4000]\n"));
    // Equal halves at −20 and −40 dB: 10·log10((10^−2 + 10^−4) / 2) ≈ −22.97 dB.
    let summary = markdown
        .lines()
        .find_map(|line| line.strip_prefix("rta_LZeq_db: ["))
        .expect("band summary");
    let summary_1000 = level(summary.split(", ").nth(2).expect("1 kHz"));
    assert!((summary_1000 + 22.97).abs() < 0.2, "{summary}");
}

#[test]
fn a_recovered_session_keeps_its_band_log() {
    let scratch = Scratch::new("band-log-recover");
    let sessions = scratch.0.join("sessions");
    let mut crashed = engine_with_bands(&sessions);
    crashed.process_block(&sine(1_000.0, 2.5, 0.1 * 2_f64.sqrt()));
    crashed.flush_journal().expect("flushes");
    let directory = crashed.journal_directory().expect("journal").to_owned();
    drop(crashed);

    let mut engine = Engine::new(48_000.0, None).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    engine
        .recover_session(
            &directory,
            OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(5),
        )
        .expect("recovers");
    engine.resume().expect("paused");
    engine.process_block(&sine(1_000.0, 1.0, 0.1 * 2_f64.sqrt()));
    let bundle = scratch.0.join("saved");
    engine.save_measurement(&bundle).expect("saves");

    let rows = csv_rows(&bundle.join("rta_log.csv"));
    let times = rows[1..]
        .iter()
        .map(|row| row[0].as_str())
        .collect::<Vec<_>>();
    assert_eq!(times, ["1.000", "2.000", "6.000"]);
    let segments = csv_rows(&bundle.join("rta_segments.csv"));
    // The recovered calibration (100 dB) applies. Recorded, interrupted (pause marker) and resumed segments.
    assert_eq!(segments.len(), 4);
    assert!(
        (level(&segments[1][8]) - 80.0).abs() < 0.3,
        "{:?}",
        segments[1]
    );
    assert_eq!(segments[2][8], "", "nothing is recorded while paused");
    assert!(
        (level(&segments[3][8]) - 80.0).abs() < 0.3,
        "{:?}",
        segments[3]
    );
}

#[test]
fn a_marker_labelled_later_splits_the_bands_at_its_click() {
    let scratch = Scratch::new("band-log-reserved");
    let mut engine = engine_with_bands(&scratch.0.join("sessions"));
    engine.process_block(&sine(1_000.0, 2.0, 0.1 * 2_f64.sqrt()));
    let reserved_s = engine.reserve_marker_timestamp().expect("none pending");
    // Audio goes on while the label is entered.
    engine.process_block(&sine(1_000.0, 2.0, 0.01 * 2_f64.sqrt()));
    engine
        .add_marker_at("Quieter", reserved_s)
        .expect("reserved");
    engine.process_block(&sine(1_000.0, 1.0, 0.01 * 2_f64.sqrt()));
    let bundle = scratch.0.join("saved");
    engine.save_measurement(&bundle).expect("saves");

    let segments = csv_rows(&bundle.join("rta_segments.csv"));
    assert_eq!(segments.len(), 3, "a header and two segments");
    assert_eq!(segments[1][4], "2.000");
    assert!(
        (level(&segments[1][8]) + 20.0).abs() < 0.2,
        "{:?}",
        segments[1]
    );
    assert!(
        (level(&segments[2][8]) + 40.0).abs() < 0.3,
        "{:?}",
        segments[2]
    );
}

#[test]
fn a_session_saves_when_bands_start_after_its_journal() {
    // The app's order: the journal starts, then the band choice is applied.
    let scratch = Scratch::new("band-log-late");
    let mut engine = Engine::new(48_000.0, None).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    engine
        .start_journal(&scratch.0.join("sessions"))
        .expect("journal starts");
    engine.set_band_log(Some(RtaConfig {
        min_hz: 500.0,
        ..OCTAVES
    }));
    engine.process_block(&sine(1_000.0, 2.0, 0.1 * 2_f64.sqrt()));

    let bundle = scratch.0.join("saved");
    engine
        .save_measurement(&bundle)
        .expect("saves before the next flush");
    assert_eq!(csv_rows(&bundle.join("rta_log.csv")).len(), 3);

    // A band choice changed before anything was recorded rewrites the header.
    engine.set_band_log(Some(OCTAVES));
    engine.reset();
    engine.set_band_log(Some(RtaConfig {
        min_hz: 500.0,
        ..OCTAVES
    }));
    engine.flush_journal().expect("flushes");
    engine.process_block(&sine(1_000.0, 1.0, 0.1));
    engine.flush_journal().expect("flushes");
    let again = scratch.0.join("again");
    engine.save_measurement(&again).expect("saves");
    assert_eq!(
        csv_rows(&again.join("rta_log.csv"))[0][2..],
        ["LZeq_500", "LZeq_1000", "LZeq_2000", "LZeq_4000"]
    );
}

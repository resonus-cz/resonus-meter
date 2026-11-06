//! Log rows leave memory once journaled, and saves read them back (ADR 0014).

mod common;

use std::fs;

use common::{Scratch, session_config, sine_block};
use spl_engine::Engine;
use time::OffsetDateTime;

fn saved_log(engine: &Engine, scratch: &Scratch, name: &str) -> Vec<String> {
    let bundle = scratch.0.join(name);
    engine.save_measurement(&bundle).expect("saves");
    fs::read_to_string(bundle.join("log.csv"))
        .expect("reads the log")
        .lines()
        .skip(1)
        .map(str::to_owned)
        .collect()
}

fn marker_column(row: &str) -> &str {
    row.split(',').nth(2).expect("marker column")
}

#[test]
fn a_save_holds_journaled_and_newer_rows_with_their_markers() {
    let scratch = Scratch::new("journal-log");
    let mut engine = Engine::new(48_000.0, None).expect("valid rate");
    engine
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    engine
        .start_journal(&scratch.0.join("sessions"))
        .expect("journal starts");
    engine.process_block(&sine_block(2, 0.1));
    engine.flush_journal().expect("flushes");
    // A label dialog is open across a flush: the rows after its time wait.
    let reserved_s = engine.reserve_marker_timestamp().expect("none pending");
    engine.process_block(&sine_block(2, 0.1));
    engine.flush_journal().expect("flushes");
    engine.add_marker_at("Door", reserved_s).expect("reserved");
    engine.process_block(&sine_block(1, 0.1));

    let rows = saved_log(&engine, &scratch, "saved");

    assert_eq!(rows.len(), 5);
    assert!(rows[0].starts_with("1.000,"));
    assert!(rows[4].starts_with("5.000,"));
    assert_eq!(marker_column(&rows[0]), "1", "the start marker");
    assert_eq!(
        marker_column(&rows[2]),
        "2",
        "the first row after the marker"
    );
    let history = engine.log_history(None).expect("session");
    assert_eq!(history.record_count, 5);
    assert_eq!(history.records[2].marker_id, Some(2));
}

#[test]
fn a_recovered_session_saves_its_journaled_rows() {
    let scratch = Scratch::new("journal-log-recover");
    let sessions = scratch.0.join("sessions");
    let mut crashed = Engine::new(48_000.0, None).expect("valid rate");
    crashed
        .start_session(session_config(), OffsetDateTime::UNIX_EPOCH)
        .expect("valid session");
    crashed.start_journal(&sessions).expect("journal starts");
    crashed.process_block(&sine_block(3, 0.1));
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
            OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(10),
        )
        .expect("recovers");
    engine.resume().expect("paused");
    engine.process_block(&sine_block(1, 0.1));

    let rows = saved_log(&engine, &scratch, "saved");

    let times = rows
        .iter()
        .map(|row| row.split(',').next().expect("time"))
        .collect::<Vec<_>>();
    assert_eq!(times, ["1.000", "2.000", "3.000", "11.000"]);
}

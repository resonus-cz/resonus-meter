use std::fs;
use std::path::PathBuf;

use super::{JournalWriter, RECOVERY_FILE};
use crate::LogRecord;
use crate::test_support::{bundle, timestamp};

/// A scratch folder unique to one test, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("spl-journal-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        Self(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn row(second: u8, laf_db: f64) -> LogRecord {
    LogRecord {
        t_s: f64::from(second) - 3.0,
        utc: timestamp(second),
        marker_id: (second == 4).then_some(1),
        clipped: second == 5,
        laf_db,
        lcf_db: laf_db + 1.0,
        lzf_db: laf_db + 2.0,
    }
}

fn read_all(journal: &JournalWriter) -> Vec<LogRecord> {
    journal
        .rows()
        .expect("opens")
        .collect::<Result<_, _>>()
        .expect("reads")
}

#[test]
fn flushes_append_new_log_rows_and_replace_the_snapshot() {
    let scratch = Scratch::new("flush");
    let mut journal = JournalWriter::create(&scratch.0).expect("creates");
    journal
        .flush(&[row(4, 40.0)], &[], &bundle(), "state = 1\n")
        .expect("flushes");
    journal
        .flush(&[row(5, 41.5)], &[], &bundle(), "state = 2\n")
        .expect("flushes");

    assert_eq!(
        fs::read_to_string(scratch.0.join("log.csv")).expect("reads"),
        "t_s,utc,marker_id,clip,LAF,LCF,LZF\n\
         1.000,2026-10-16T09:12:04.250Z,1,0,40.00,41.00,42.00\n\
         2.000,2026-10-16T09:12:05.250Z,,1,41.50,42.50,43.50\n"
    );
    assert_eq!(journal.log_rows(), 2);
    assert_eq!(read_all(&journal), [row(4, 40.0), row(5, 41.5)]);
    assert_eq!(
        fs::read_to_string(scratch.0.join(RECOVERY_FILE)).expect("reads"),
        "state = 2\n"
    );
    for file in ["markers.csv", "segments.csv", "measurement.md"] {
        assert!(scratch.0.join(file).exists(), "{file} written");
    }
    let leftovers = fs::read_dir(&scratch.0)
        .expect("lists")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn resuming_keeps_the_recorded_rows_and_cuts_off_an_unfinished_flush() {
    let scratch = Scratch::new("resume");
    let mut journal = JournalWriter::create(&scratch.0).expect("creates");
    journal
        .flush(&[row(4, 40.0), row(5, 41.5)], &[], &bundle(), "")
        .expect("flushes");
    // A crash during the next flush may leave extra and partly written rows.
    let path = scratch.0.join("log.csv");
    let mut text = fs::read_to_string(&path).expect("reads");
    text.push_str("3.000,2026-10-16T09:12:06.250Z,,0,4");
    fs::write(&path, text).expect("writes");
    assert!(
        JournalWriter::resume(&scratch.0, 4, None).is_err(),
        "missing rows are an error"
    );

    let mut resumed = JournalWriter::resume(&scratch.0, 2, None).expect("resumes");
    resumed
        .flush(&[row(6, 43.0)], &[], &bundle(), "")
        .expect("flushes");

    assert_eq!(
        read_all(&resumed),
        [row(4, 40.0), row(5, 41.5), row(6, 43.0)]
    );
    resumed.remove().expect("removes");
    assert!(!scratch.0.exists());
}

#[test]
fn an_empty_journal_resumes() {
    let scratch = Scratch::new("resume-empty");
    JournalWriter::create(&scratch.0).expect("creates");

    let mut resumed = JournalWriter::resume(&scratch.0, 0, None).expect("resumes");
    resumed
        .flush(&[row(4, 40.0)], &[], &bundle(), "")
        .expect("flushes");

    assert_eq!(read_all(&resumed), [row(4, 40.0)]);
}

#[test]
fn band_rows_are_journaled_and_resumed_like_log_rows() {
    let scratch = Scratch::new("bands");
    let mut snapshot = bundle();
    snapshot.bands = Some(crate::test_support::band_log());
    let band_row = |t_s: f64| crate::BandRow {
        t_s,
        utc: timestamp(4),
        levels_db: vec![Some(40.0 + t_s), None],
    };
    let mut journal = JournalWriter::create(&scratch.0).expect("creates");
    journal
        .flush(&[], &[band_row(1.0), band_row(2.0)], &snapshot, "")
        .expect("flushes");
    assert!(scratch.0.join("rta_segments.csv").exists());
    let path = scratch.0.join("rta_log.csv");
    let mut text = fs::read_to_string(&path).expect("reads");
    text.push_str("3.000,2026-10-16T09:12:06.250Z,4");
    fs::write(&path, text).expect("writes");

    let bands_hz = [500.0, 1_000.0];
    let resumed = JournalWriter::resume(&scratch.0, 0, Some((&bands_hz, 2))).expect("resumes");

    let rows = resumed
        .band_log_rows(&bands_hz)
        .expect("opens")
        .collect::<Result<Vec<_>, _>>()
        .expect("reads");
    assert_eq!(rows, [band_row(1.0), band_row(2.0)]);
    assert!(
        resumed.band_log_rows(&[500.0]).is_err(),
        "other bands are refused"
    );
}

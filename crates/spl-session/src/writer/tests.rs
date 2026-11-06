use std::fs;

use super::*;
use crate::test_support::bundle;
use crate::{Attachment, LogRecord};

#[test]
fn writer_creates_canonical_files_with_specified_csv_format() {
    let root = std::env::temp_dir().join(format!("spl-session-writer-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);

    BundleWriter.write(&root, &bundle()).expect("bundle writes");

    assert_eq!(
        fs::read_to_string(root.join("log.csv")).expect("read log"),
        "t_s,utc,marker_id,clip,LAF,LCF,LZF\n\
         1.000,2026-10-16T09:12:04.250Z,,0,41.05,49.00,54.90\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("segments.csv")).expect("read segments"),
        "segment,start_marker,end_marker,start_s,end_s,duration_s,clip,LAeq,LApeak,LCeq,LCpeak,LZeq,LZpeak,LAFmax,LCFmax,LZFmax\n\
         1,1,2,0.000,1.000,1.000,0,41.05,66.10,49.00,70.40,54.90,72.30,44.20,52.00,57.50\n"
    );
    let markdown = fs::read_to_string(root.join("measurement.md")).expect("read Markdown");
    assert!(markdown.contains("format: splmeter-session\nversion: \"1.2\"\n"));
    assert!(
        markdown.contains("  log: log.csv\n  markers: markers.csv\n  segments: segments.csv\n")
    );
    assert!(markdown.contains("A: { Leq_db: 41.05, peak_db: 66.10, Fmax_db: 44.20 }\n"));
    assert!(!root.join("measurement.md.").exists());

    fs::remove_dir_all(root).expect("clean up test output");
}

#[test]
fn attachments_are_copied_hashed_and_listed() {
    let root = std::env::temp_dir().join(format!("spl-session-attachments-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let sources = root.join("sources");
    fs::create_dir_all(sources.join("other")).expect("creates sources");
    fs::write(sources.join("photo.jpg"), "abc").expect("writes");
    fs::write(sources.join("other").join("photo.jpg"), "").expect("writes");
    let mut bundle = bundle();
    bundle.attachments = vec![
        Attachment {
            source: sources.join("photo.jpg"),
            title: "Room \"2.14\"".to_owned(),
        },
        Attachment {
            source: sources.join("other").join("photo.jpg"),
            title: "Empty".to_owned(),
        },
    ];
    let saved = root.join("bundle");

    BundleWriter.write(&saved, &bundle).expect("bundle writes");

    assert_eq!(
        fs::read_to_string(saved.join("attachments").join("photo.jpg")).expect("copied"),
        "abc"
    );
    assert!(saved.join("attachments").join("photo-2.jpg").exists());
    let markdown = fs::read_to_string(saved.join("measurement.md")).expect("read Markdown");
    // SHA-256 of "abc" and of the empty file (FIPS 180-2 test vectors).
    assert!(markdown.contains(
        "  attachments:\n    - file: \"attachments/photo.jpg\"\n      title: \"Room \\\"2.14\\\"\"\n      \
         sha256: \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"\n    \
         - file: \"attachments/photo-2.jpg\"\n      title: \"Empty\"\n      \
         sha256: \"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"\n---\n"
    ), "{markdown}");

    fs::remove_dir_all(root).expect("clean up test output");
}

#[test]
fn a_missing_attachment_fails_before_measurement_md_is_written() {
    let root = std::env::temp_dir().join(format!("spl-session-missing-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut bundle = bundle();
    bundle.attachments = vec![Attachment {
        source: root.join("does-not-exist.pdf"),
        title: "Datasheet".to_owned(),
    }];

    assert!(BundleWriter.write(&root, &bundle).is_err());
    assert!(!root.join("measurement.md").exists());

    fs::remove_dir_all(root).expect("clean up test output");
}

#[test]
fn a_save_copies_the_journaled_rows_before_the_rows_in_memory() {
    let root = std::env::temp_dir().join(format!("spl-session-journaled-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut bundle = bundle();
    let row = |t_s: f64, marker_id| LogRecord {
        t_s,
        marker_id,
        laf_db: 40.0 + t_s,
        ..bundle.log[0]
    };
    let mut journal = JournalWriter::create(&root.join("journal")).expect("creates");
    // The row at 0 s lies before the segments; marker 9 is not in the bundle.
    journal
        .flush(&[row(0.0, None), row(0.5, Some(9))], &[], &bundle, "")
        .expect("flushes");
    bundle.log = vec![row(1.0, Some(2)), row(1.5, None)];

    BundleWriter
        .write_with_journal(&root.join("saved"), &bundle, &journal)
        .expect("bundle writes");

    assert_eq!(
        fs::read_to_string(root.join("saved").join("log.csv")).expect("read log"),
        "t_s,utc,marker_id,clip,LAF,LCF,LZF\n\
         0.500,2026-10-16T09:12:04.250Z,,0,40.50,49.00,54.90\n\
         1.000,2026-10-16T09:12:04.250Z,2,0,41.00,49.00,54.90\n"
    );
    fs::remove_dir_all(root).expect("clean up test output");
}

#[test]
fn band_levels_are_saved_with_journaled_rows_first() {
    let root = std::env::temp_dir().join(format!("spl-session-bands-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut bundle = bundle();
    bundle.bands = Some(crate::test_support::band_log());
    let early = crate::BandRow {
        t_s: 0.5,
        utc: bundle.metadata.start_utc,
        levels_db: vec![Some(30.0), Some(31.0)],
    };
    let mut journal = JournalWriter::create(&root.join("journal")).expect("creates");
    journal.flush(&[], &[early], &bundle, "").expect("flushes");

    BundleWriter
        .write_with_journal(&root.join("saved"), &bundle, &journal)
        .expect("bundle writes");

    let saved = root.join("saved");
    assert_eq!(
        fs::read_to_string(saved.join("rta_log.csv")).expect("read band log"),
        "t_s,utc,LZeq_500,LZeq_1000\n\
         0.500,2026-10-16T09:12:03.250Z,30.00,31.00\n\
         1.000,2026-10-16T09:12:04.250Z,38.50,\n"
    );
    assert_eq!(
        fs::read_to_string(saved.join("rta_segments.csv")).expect("read band segments"),
        "segment,start_marker,end_marker,start_s,end_s,duration_s,LZeq_500,LZeq_1000\n\
         1,1,2,0.000,1.000,1.000,38.50,35.25\n"
    );
    let markdown = fs::read_to_string(saved.join("measurement.md")).expect("read Markdown");
    assert!(markdown.contains(
        "rta:\n  quantity: LZeq\n  bands_per_octave: 1\n  bands_hz: [500, 1000]\nfiles:\n"
    ));
    assert!(markdown.contains("  rta_log: rta_log.csv\n  rta_segments: rta_segments.csv\n"));
    assert!(markdown.contains("rta_LZeq_db: [38.50, 35.25]\n"));
    fs::remove_dir_all(root).expect("clean up test output");
}

//! Round trip through the independent Python reader (specification 6.5).
//!
//! The test skips, with a message, when `python3` is not installed.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use spl_engine::{Bandwidth, DemoRunner, RtaConfig, SaveMetadata};

fn reader_script() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/read_session.py")
}

/// Advances the demo's clock to `seconds` in display-refresh steps.
fn run_until(demo: &mut DemoRunner, seconds: u64) {
    for step in 1..=seconds * 8 {
        demo.catch_up(Duration::from_millis(125 * step));
    }
}

/// Extracts a number following `"key": ` in the reader's JSON output.
fn number(output: &str, key: &str) -> f64 {
    let start = output.find(&format!("\"{key}\": ")).expect("key present") + key.len() + 4;
    let rest = &output[start..];
    let end = rest.find([',', '}']).expect("value ends");
    rest[..end].trim().parse().expect("number")
}

/// Saves a 9 s demo measurement with a marker and a pause.
fn saved_bundle(name: &str) -> std::path::PathBuf {
    let mut demo = DemoRunner::new(48_000.0, None).expect("valid rate");
    demo.set_band_log(Some(RtaConfig {
        bandwidth: Bandwidth::Octave,
        min_hz: 125.0,
        max_hz: 8_000.0,
    }));
    run_until(&mut demo, 3);
    demo.add_marker("Door opened").expect("session");
    run_until(&mut demo, 5);
    demo.pause().expect("running");
    run_until(&mut demo, 7);
    demo.resume().expect("paused");
    run_until(&mut demo, 9);
    let bundle =
        std::env::temp_dir().join(format!("spl-python-reader-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&bundle);
    demo.save_measurement_with_metadata(
        &bundle,
        SaveMetadata {
            name: "Reader \"round trip\"".to_owned(),
            description: "Saved by the Python reader test.".to_owned(),
            ..SaveMetadata::default()
        },
    )
    .expect("saves");
    bundle
}

/// Runs the reader on `bundle`, or returns `None` when python3 is missing.
fn run_reader(bundle: &Path) -> Option<std::process::Output> {
    match Command::new("python3")
        .arg("-I")
        .arg(reader_script())
        .arg(bundle)
        .output()
    {
        Ok(output) => Some(output),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipped: python3 is not installed");
            None
        }
        Err(error) => panic!("could not run python3: {error}"),
    }
}

#[test]
fn the_python_reader_reads_a_saved_bundle() {
    let bundle = saved_bundle("round-trip");

    let result = run_reader(&bundle);
    std::fs::remove_dir_all(&bundle).expect("cleans up");
    let Some(output) = result else { return };
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(
        output.status.success(),
        "reader failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        stdout.contains(r#""name": "Reader \"round trip\"""#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""version": "1.2""#), "{stdout}");
    assert!(
        stdout.contains(r#""markers": ["start", "user", "pause", "resume", "stop"]"#),
        "{stdout}"
    );
    // Rows at 1 s intervals: 1–5 s before the pause, 8–9 s after it.
    assert!(stdout.contains(r#""log_rows": 7,"#), "{stdout}");
    // Segment values are rounded to 0.01 dB, so the recombined Leq may differ
    // from the summary by one rounding step.
    let combined = number(&stdout, "LAeq_combined");
    assert!(
        (combined - number(&stdout, "LAeq_summary")).abs() <= 0.011,
        "{stdout}"
    );
    assert!(
        stdout.contains(r#""rta_bands_hz": ["125", "250", "500", "1000", "2000", "4000", "8000"]"#),
        "{stdout}"
    );
    assert!(stdout.contains(r#""rta_log_rows": 7,"#), "{stdout}");
    let band_combined = number(&stdout, "LZeq_1000_combined");
    assert!(
        (band_combined - number(&stdout, "LZeq_1000_summary")).abs() <= 0.011,
        "{stdout}"
    );
}

#[test]
fn the_python_reader_rejects_a_newer_major_version() {
    let bundle = saved_bundle("version");
    let path = bundle.join("measurement.md");
    let text = std::fs::read_to_string(&path).expect("reads");
    std::fs::write(&path, text.replace("version: \"1.2\"", "version: \"2.0\"")).expect("writes");

    let result = run_reader(&bundle);
    std::fs::remove_dir_all(&bundle).expect("cleans up");
    let Some(output) = result else { return };

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsupported major version 2.0"));
}

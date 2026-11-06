use std::fs;
use std::path::PathBuf;

use super::{DEFAULT_JOURNAL_INTERVAL_S, RECENT_INPUT_LIMIT, RecentInput, Settings, SettingsError};

/// A scratch file path unique to one test, removed when dropped.
struct ScratchFile(PathBuf);

impl ScratchFile {
    fn new(name: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("spl-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        Self(directory.join("settings.toml"))
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        if let Some(directory) = self.0.parent() {
            let _ = fs::remove_dir_all(directory);
        }
    }
}

fn input(device_id: &str, channel_index: u16) -> RecentInput {
    RecentInput {
        device_id: device_id.to_owned(),
        channel_index,
        sample_rate_hz: 48_000,
    }
}

#[test]
fn a_missing_file_has_no_recent_inputs() {
    let file = ScratchFile::new("missing");

    let settings = Settings::open(&file.0).expect("missing file opens");

    assert!(settings.recent_inputs().is_empty());
    assert!(!file.0.exists(), "opening must not create the file");
}

#[test]
fn remembered_inputs_persist_most_recent_first() {
    let file = ScratchFile::new("persist");
    let mut settings = Settings::open(&file.0).expect("opens");

    settings.remember_input(input("a", 0)).expect("saves");
    settings.remember_input(input("b", 1)).expect("saves");

    let reopened = Settings::open(&file.0).expect("reopens");
    assert_eq!(reopened.recent_inputs(), [input("b", 1), input("a", 0)]);
}

#[test]
fn reusing_an_input_moves_it_to_the_front_with_its_new_rate() {
    let mut settings = Settings::unsaved();
    settings.remember_input(input("a", 0)).expect("in memory");
    settings.remember_input(input("b", 0)).expect("in memory");

    let mut again = input("a", 0);
    again.sample_rate_hz = 96_000;
    settings.remember_input(again.clone()).expect("in memory");

    assert_eq!(settings.recent_inputs(), [again, input("b", 0)]);
}

#[test]
fn channels_of_one_device_are_separate_inputs() {
    let mut settings = Settings::unsaved();
    settings.remember_input(input("a", 0)).expect("in memory");
    settings.remember_input(input("a", 1)).expect("in memory");

    assert_eq!(settings.recent_inputs(), [input("a", 1), input("a", 0)]);
}

#[test]
fn only_the_most_recent_inputs_are_kept() {
    let mut settings = Settings::unsaved();
    for index in 0..=RECENT_INPUT_LIMIT {
        settings
            .remember_input(input(&format!("device-{index}"), 0))
            .expect("in memory");
    }

    let recent = settings.recent_inputs();
    assert_eq!(recent.len(), RECENT_INPUT_LIMIT);
    assert_eq!(recent[0].device_id, format!("device-{RECENT_INPUT_LIMIT}"));
    assert!(recent.iter().all(|input| input.device_id != "device-0"));
}

#[test]
fn a_newer_major_version_is_rejected_and_left_untouched() {
    let file = ScratchFile::new("version");
    fs::create_dir_all(file.0.parent().expect("has parent")).expect("creates directory");
    fs::write(&file.0, "version = 2\n").expect("writes fixture");

    let error = Settings::open(&file.0).expect_err("newer version is rejected");

    assert!(matches!(error, SettingsError::UnsupportedVersion(2)));
    assert_eq!(fs::read_to_string(&file.0).expect("reads"), "version = 2\n");
}

#[test]
fn unknown_keys_are_tolerated() {
    let file = ScratchFile::new("unknown");
    fs::create_dir_all(file.0.parent().expect("has parent")).expect("creates directory");
    fs::write(
        &file.0,
        "version = 1\nfuture = true\n\n[[recent_input]]\ndevice_id = \"a\"\n\
         channel_index = 0\nsample_rate_hz = 48000\nlabel = \"x\"\n",
    )
    .expect("writes fixture");

    let settings = Settings::open(&file.0).expect("opens");

    assert_eq!(settings.recent_inputs(), [input("a", 0)]);
}

#[test]
fn the_journal_interval_defaults_to_a_minute_and_persists() {
    let file = ScratchFile::new("journal");
    let mut settings = Settings::open(&file.0).expect("opens");
    assert_eq!(settings.journal_interval_s(), DEFAULT_JOURNAL_INTERVAL_S);

    settings.set_journal_interval_s(10).expect("saves");

    assert_eq!(
        Settings::open(&file.0)
            .expect("reopens")
            .journal_interval_s(),
        10
    );
}

#[test]
fn an_unsupported_journal_interval_falls_back_to_the_default() {
    let file = ScratchFile::new("journal-invalid");
    fs::create_dir_all(file.0.parent().expect("has parent")).expect("creates directory");
    fs::write(&file.0, "version = 1\njournal_interval_s = 0\n").expect("writes fixture");

    let settings = Settings::open(&file.0).expect("opens");

    assert_eq!(settings.journal_interval_s(), DEFAULT_JOURNAL_INTERVAL_S);
}

#[test]
fn rta_settings_default_to_third_octaves_and_persist() {
    use super::RtaSettings;
    use spl_dsp::{bands::Bandwidth, filterbank::BandDetector, weighting::Weighting};
    let file = ScratchFile::new("rta");
    let mut settings = Settings::open(&file.0).expect("opens");
    assert_eq!(settings.rta(), RtaSettings::default());
    assert_eq!(settings.rta().bandwidth, Bandwidth::ThirdOctave);

    let chosen = RtaSettings {
        bandwidth: Bandwidth::Octave,
        min_hz: 63.0,
        max_hz: 8_000.0,
        detector: BandDetector::Leq,
        weighting: Weighting::A,
        peak_hold: true,
        record: false,
    };
    assert!(settings.rta().record, "band recording is on by default");
    settings.set_rta(chosen).expect("saves");

    assert_eq!(Settings::open(&file.0).expect("reopens").rta(), chosen);
}

#[test]
fn an_inverted_rta_range_falls_back_to_the_default_range() {
    let mut settings = Settings::unsaved();
    let mut rta = settings.rta();
    rta.min_hz = 8_000.0;
    rta.max_hz = 100.0;

    settings.set_rta(rta).expect("in memory");

    assert!(settings.rta().min_hz < settings.rta().max_hz);
}

//! Manually verifies a cpal input stream and its engine integration.

use std::{thread, time::Duration};

use spl_audio::{AudioSource, CpalSource, StreamConfig};
use spl_engine::CaptureRunner;

fn main() {
    let source = CpalSource::new();
    let device = source
        .list_devices()
        .expect("cpal host must enumerate input devices")
        .into_iter()
        .find(|device| {
            let requested_name = std::env::var("SPL_METER_CAPTURE_DEVICE").ok();
            requested_name
                .as_deref()
                .is_none_or(|name| device.display_name.contains(name))
                && device.sample_rates_hz.contains(&48_000)
        })
        .expect("an input device must support 48 kHz");
    let mut runner = CaptureRunner::open(
        StreamConfig {
            device_id: device.id,
            channel_index: 0,
            sample_rate_hz: 48_000,
            buffer_frames: 0,
        },
        None,
    )
    .expect("cpal must open the selected input");
    thread::sleep(Duration::from_secs(2));
    let snapshot = runner.tick();
    println!(
        "samples={} dropped={} errors={} LZF={:.1} dBFS",
        snapshot.sample_count,
        runner.dropped_samples(),
        runner.stream_error_count(),
        snapshot.z.fast_db,
    );
    runner.close();
}

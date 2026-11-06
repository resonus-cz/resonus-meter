//! Choosing the input opened when the application starts.

use crate::runner::MeasurementRunner;
use spl_engine::{CalibrationStore, DEMO_DEVICE_ID, Device, Settings};
#[cfg(feature = "cpal-backend")]
use {
    crate::runner::open_capture,
    spl_audio::{AudioSource, CpalSource, DeviceInfo},
    spl_engine::RecentInput,
};

/// Opens the most recently used input that is still available.
///
/// Inputs are tried in the order they were last used, so an unplugged
/// microphone falls back to the one used before it, and finally to the demo
/// source. The returned notice explains a fallback.
pub(crate) fn open_startup_input(
    settings: &Settings,
    store: &CalibrationStore,
    demo_only: bool,
) -> (MeasurementRunner, Option<String>) {
    if demo_only {
        return (MeasurementRunner::demo(store), None);
    }
    #[cfg(feature = "cpal-backend")]
    {
        let devices = match CpalSource::new().list_devices() {
            Ok(devices) => devices,
            Err(error) => {
                return (
                    MeasurementRunner::demo(store),
                    Some(format!("Could not enumerate input devices: {error}")),
                );
            }
        };
        open_recent_input(settings.recent_inputs(), &devices, store)
    }
    #[cfg(not(feature = "cpal-backend"))]
    {
        let _ = settings;
        (MeasurementRunner::demo(store), None)
    }
}

/// Opens the input a journaled session was recorded with, at its rate.
pub(crate) fn open_recorded_input(
    device: &Device,
    store: &CalibrationStore,
) -> Result<MeasurementRunner, String> {
    if device.id == DEMO_DEVICE_ID {
        return Ok(MeasurementRunner::demo(store));
    }
    #[cfg(feature = "cpal-backend")]
    {
        let devices = CpalSource::new()
            .list_devices()
            .map_err(|error| format!("Could not enumerate input devices: {error}"))?;
        let available = devices
            .iter()
            .find(|candidate| candidate.id == device.id)
            .ok_or_else(|| format!("{} is not connected", device.name))?;
        let channel_index = device
            .channel
            .checked_sub(1)
            .and_then(|index| u16::try_from(index).ok())
            .ok_or_else(|| format!("channel {} is invalid", device.channel))?;
        open_capture(available, channel_index, device.sample_rate_hz, store)
    }
    #[cfg(not(feature = "cpal-backend"))]
    Err(format!(
        "{} cannot be opened: hardware capture is not part of this build",
        device.name
    ))
}

#[cfg(feature = "cpal-backend")]
fn open_recent_input(
    recent: &[RecentInput],
    devices: &[DeviceInfo],
    store: &CalibrationStore,
) -> (MeasurementRunner, Option<String>) {
    let Some(last_used) = recent.first() else {
        return (MeasurementRunner::demo(store), None);
    };
    let opened =
        startup_inputs(recent, devices)
            .into_iter()
            .find_map(|candidate| match candidate {
                StartupInput::Demo => Some(MeasurementRunner::demo(store)),
                StartupInput::Live {
                    device,
                    channel_index,
                    sample_rate_hz,
                } => open_capture(device, channel_index, sample_rate_hz, store).ok(),
            });
    match opened {
        Some(runner) => {
            let opened_last_used = runner.recent_input().device_id == last_used.device_id
                && runner.recent_input().channel_index == last_used.channel_index;
            let notice = (!opened_last_used).then(|| {
                format!(
                    "The last used input is not available; opened {}.",
                    runner.subtitle()
                )
            });
            (runner, notice)
        }
        None => (
            MeasurementRunner::demo(store),
            Some("No previously used input is available; using the demo source.".to_owned()),
        ),
    }
}

/// A remembered input resolved against the devices present now.
#[cfg(feature = "cpal-backend")]
#[derive(Debug, PartialEq)]
enum StartupInput<'a> {
    Demo,
    Live {
        device: &'a DeviceInfo,
        channel_index: u16,
        sample_rate_hz: u32,
    },
}

/// Returns the remembered inputs that can be opened, most recent first.
///
/// A device that is missing, has fewer channels, or no longer offers the
/// remembered rate is skipped.
#[cfg(feature = "cpal-backend")]
fn startup_inputs<'a>(recent: &[RecentInput], devices: &'a [DeviceInfo]) -> Vec<StartupInput<'a>> {
    recent
        .iter()
        .filter_map(|input| {
            if input.device_id == DEMO_DEVICE_ID {
                return Some(StartupInput::Demo);
            }
            let device = devices.iter().find(|device| device.id == input.device_id)?;
            (input.channel_index < device.channels
                && supported_device_rates(device).contains(&input.sample_rate_hz))
            .then_some(StartupInput::Live {
                device,
                channel_index: input.channel_index,
                sample_rate_hz: input.sample_rate_hz,
            })
        })
        .collect()
}

/// Returns the device's rates that the DSP supports, ascending.
#[cfg(feature = "cpal-backend")]
pub(crate) fn supported_device_rates(device: &DeviceInfo) -> Vec<u32> {
    let mut rates = device
        .sample_rates_hz
        .iter()
        .copied()
        .filter(|rate| matches!(rate, 44_100 | 48_000 | 96_000))
        .collect::<Vec<_>>();
    rates.sort_unstable();
    rates.dedup();
    rates
}

#[cfg(all(test, feature = "cpal-backend"))]
mod tests {
    use super::{StartupInput, startup_inputs, supported_device_rates};
    use spl_audio::{AudioBackend, DeviceInfo};
    use spl_engine::{DEMO_DEVICE_ID, RecentInput};

    fn device(id: &str, channels: u16) -> DeviceInfo {
        DeviceInfo {
            id: id.to_owned(),
            display_name: format!("Device {id}"),
            backend: AudioBackend::Simulated,
            channels,
            sample_rates_hz: vec![44_100, 48_000],
        }
    }

    fn recent(device_id: &str, channel_index: u16, sample_rate_hz: u32) -> RecentInput {
        RecentInput {
            device_id: device_id.to_owned(),
            channel_index,
            sample_rate_hz,
        }
    }

    #[test]
    fn device_rate_choices_are_limited_to_dsp_rates() {
        let mut device = device("test", 2);
        device.sample_rates_hz = vec![8_000, 44_100, 48_000, 96_000, 192_000];

        assert_eq!(
            supported_device_rates(&device),
            vec![44_100, 48_000, 96_000]
        );
    }

    #[test]
    fn available_inputs_are_tried_in_order_of_last_use() {
        let devices = [device("usb", 2), device("builtin", 1)];
        let recent = [
            recent("unplugged", 0, 48_000),
            recent("builtin", 0, 44_100),
            recent(DEMO_DEVICE_ID, 0, 48_000),
            recent("usb", 1, 48_000),
        ];

        assert_eq!(
            startup_inputs(&recent, &devices),
            [
                StartupInput::Live {
                    device: &devices[1],
                    channel_index: 0,
                    sample_rate_hz: 44_100,
                },
                StartupInput::Demo,
                StartupInput::Live {
                    device: &devices[0],
                    channel_index: 1,
                    sample_rate_hz: 48_000,
                },
            ]
        );
    }

    #[test]
    fn inputs_whose_channel_or_rate_is_gone_are_skipped() {
        let devices = [device("usb", 1)];
        let recent = [recent("usb", 1, 48_000), recent("usb", 0, 96_000)];

        assert!(startup_inputs(&recent, &devices).is_empty());
    }
}

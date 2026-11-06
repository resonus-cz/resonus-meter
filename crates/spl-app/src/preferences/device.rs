use super::{PreferencesContext, read_only_row, string_list};
use adw::prelude::*;
use std::rc::Rc;
#[cfg(feature = "cpal-backend")]
use {
    crate::runner::{MeasurementRunner, open_capture, replace_runner},
    crate::startup::supported_device_rates,
    spl_audio::{AudioSource, CpalSource, DeviceInfo},
    std::{cell::Cell, time::Instant},
};

#[allow(clippy::too_many_lines)]
pub(crate) fn sound_device_page(
    context: &PreferencesContext,
    on_input_changed: &Rc<dyn Fn()>,
) -> adw::PreferencesPage {
    let PreferencesContext {
        runner,
        input_status,
        ..
    } = context;
    // Without capture support the input never changes, so the window state
    // used for switching inputs is not needed.
    #[cfg(not(feature = "cpal-backend"))]
    let _ = (
        &context.title,
        &context.reset_time,
        &context.settings,
        &context.sessions_directory,
        input_status,
        on_input_changed,
    );
    let page = adw::PreferencesPage::builder()
        .name("sound-device")
        .title("Sound Device")
        .icon_name("audio-input-microphone-symbolic")
        .build();

    let input = adw::PreferencesGroup::builder()
        .title("Input")
        .description("Changes open the selected input immediately.")
        .build();
    #[cfg(feature = "cpal-backend")]
    let devices = match CpalSource::new().list_devices() {
        Ok(devices) => devices,
        Err(error) => {
            let status = format!("Could not enumerate input devices: {error}");
            input_status.set_label(&status);
            input_status.set_visible(true);
            Vec::new()
        }
    };
    #[cfg(feature = "cpal-backend")]
    let mut sources = vec!["Demo sine source (1 kHz)".to_owned()];
    #[cfg(feature = "cpal-backend")]
    sources.extend(devices.iter().map(|device| device.display_name.clone()));
    #[cfg(not(feature = "cpal-backend"))]
    let sources = vec!["Demo sine source (1 kHz)".to_owned()];
    let source_model = string_list(&sources);
    #[cfg(feature = "cpal-backend")]
    let source_selected = runner
        .borrow()
        .live_configuration()
        .and_then(|(id, _, _)| {
            devices
                .iter()
                .position(|device| device.id == id)
                .and_then(|index| u32::try_from(index + 1).ok())
        })
        .unwrap_or(0);
    #[cfg(not(feature = "cpal-backend"))]
    let source_selected = 0;
    let source = adw::ComboRow::builder()
        .title("Source")
        .subtitle("Demo is always available")
        .model(&source_model)
        .selected(source_selected)
        .build();
    source.update_property(&[gtk::accessible::Property::Label("Input source")]);
    input.add(&source);
    let channel = adw::ComboRow::builder().title("Channel").build();
    channel.update_property(&[gtk::accessible::Property::Label("Input channel")]);
    input.add(&channel);
    let sample_rate = adw::ComboRow::builder().title("Sample rate").build();
    sample_rate.update_property(&[gtk::accessible::Property::Label("Requested sample rate")]);
    input.add(&sample_rate);
    let negotiated_rate = read_only_row(
        "Configured sample rate",
        &runner.borrow().configured_rate_label(),
    );
    input.add(&negotiated_rate);
    let capture_status = read_only_row("Capture status", &runner.borrow().subtitle());
    input.add(&capture_status);
    page.add(&input);

    let buffering = adw::PreferencesGroup::builder()
        .title("Buffering")
        .description("The M3 capture path uses the backend default buffer size.")
        .build();
    let buffer_size = adw::ActionRow::builder()
        .title("Buffer size")
        .subtitle("Use the audio backend default")
        .build();
    buffer_size.set_sensitive(false);
    buffer_size.update_property(&[gtk::accessible::Property::Label("Buffer size")]);
    buffering.add(&buffer_size);
    page.add(&buffering);

    #[cfg(feature = "cpal-backend")]
    {
        let update_options = {
            let devices = devices.clone();
            let source = source.clone();
            let channel = channel.clone();
            let sample_rate = sample_rate.clone();
            move || update_device_options(&source, &channel, &sample_rate, &devices)
        };
        update_options();
        if let Some((device_id, channel_index, rate)) = runner.borrow().live_configuration()
            && devices.iter().any(|device| device.id == device_id)
        {
            channel.set_selected(u32::from(channel_index));
            if let Some(device) = devices.iter().find(|device| device.id == device_id) {
                if let Some(rate_index) = supported_device_rates(device)
                    .iter()
                    .position(|candidate| *candidate == rate)
                    .and_then(|index| u32::try_from(index).ok())
                {
                    sample_rate.set_selected(rate_index);
                }
            }
        }

        let switch_input = {
            let devices = devices.clone();
            let source = source.clone();
            let channel = channel.clone();
            let sample_rate = sample_rate.clone();
            let context = context.clone();
            let on_input_changed = Rc::clone(on_input_changed);
            let negotiated_rate = negotiated_rate.clone();
            let capture_status = capture_status.clone();
            move || {
                switch_selected_input(
                    &InputRows {
                        source: &source,
                        channel: &channel,
                        sample_rate: &sample_rate,
                        negotiated_rate: &negotiated_rate,
                        capture_status: &capture_status,
                    },
                    &devices,
                    &context,
                );
                on_input_changed();
            }
        };
        let updating_device_options = Rc::new(Cell::new(false));
        let update_on_source = {
            let update_options = update_options.clone();
            let switch_input = switch_input.clone();
            let updating_device_options = Rc::clone(&updating_device_options);
            move || {
                updating_device_options.set(true);
                update_options();
                updating_device_options.set(false);
                switch_input();
            }
        };
        source.connect_notify_local(Some("selected"), move |_, _| update_on_source());
        let switch_channel = switch_input.clone();
        let updating_channel = Rc::clone(&updating_device_options);
        channel.connect_notify_local(Some("selected"), move |_, _| {
            if !updating_channel.get() {
                switch_channel();
            }
        });
        sample_rate.connect_notify_local(Some("selected"), move |_, _| {
            if !updating_device_options.get() {
                switch_input();
            }
        });
    }
    #[cfg(not(feature = "cpal-backend"))]
    {
        channel.set_model(Some(&string_list(&["Mono".to_owned()])));
        channel.set_selected(0);
        channel.set_sensitive(false);
        sample_rate.set_model(Some(&string_list(&["48,000 Hz".to_owned()])));
        sample_rate.set_selected(0);
        sample_rate.set_sensitive(false);
        capture_status.set_subtitle("Hardware capture was not included in this build");
    }
    page
}

#[cfg(feature = "cpal-backend")]
fn update_device_options(
    source: &adw::ComboRow,
    channel: &adw::ComboRow,
    sample_rate: &adw::ComboRow,
    devices: &[DeviceInfo],
) {
    let Some(device) = source
        .selected()
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| devices.get(index))
    else {
        channel.set_model(Some(&string_list(&["Mono".to_owned()])));
        channel.set_selected(0);
        channel.set_sensitive(false);
        sample_rate.set_model(Some(&string_list(&["48,000 Hz".to_owned()])));
        sample_rate.set_selected(0);
        sample_rate.set_sensitive(false);
        return;
    };

    let channels = (1..=device.channels)
        .map(|index| format!("Channel {index}"))
        .collect::<Vec<_>>();
    channel.set_model(Some(&string_list(&channels)));
    channel.set_selected(0);
    channel.set_sensitive(!channels.is_empty());

    let rates = supported_device_rates(device)
        .into_iter()
        .map(|rate| format!("{rate} Hz"))
        .collect::<Vec<_>>();
    sample_rate.set_model(Some(&string_list(&rates)));
    let preferred = rates
        .iter()
        .position(|rate| rate == "48000 Hz")
        .unwrap_or(0);
    #[allow(clippy::cast_possible_truncation)]
    sample_rate.set_selected(preferred as u32);
    sample_rate.set_sensitive(!rates.is_empty());
}

/// Sound Device rows read or updated when the input changes.
#[cfg(feature = "cpal-backend")]
struct InputRows<'a> {
    source: &'a adw::ComboRow,
    channel: &'a adw::ComboRow,
    sample_rate: &'a adw::ComboRow,
    negotiated_rate: &'a adw::ActionRow,
    capture_status: &'a adw::ActionRow,
}

#[cfg(feature = "cpal-backend")]
fn switch_selected_input(
    rows: &InputRows<'_>,
    devices: &[DeviceInfo],
    context: &PreferencesContext,
) {
    let InputRows {
        source,
        channel,
        sample_rate,
        negotiated_rate,
        capture_status,
    } = rows;
    let PreferencesContext {
        runner,
        store,
        settings,
        title,
        input_status,
        reset_time,
        ..
    } = context;
    let device = source
        .selected()
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| devices.get(index));
    let journal;
    let status = if let Some(device) = device {
        let Ok(channel_index) = u16::try_from(channel.selected()) else {
            show_capture_error(
                "The selected channel is invalid.",
                input_status,
                capture_status,
            );
            return;
        };
        let rate = supported_device_rates(device)
            .get(usize::try_from(sample_rate.selected()).unwrap_or(usize::MAX))
            .copied();
        let Some(sample_rate_hz) = rate else {
            show_capture_error(
                "No supported 44.1, 48, or 96 kHz rate was reported by this device.",
                input_status,
                capture_status,
            );
            return;
        };
        match open_capture(device, channel_index, sample_rate_hz, &store.borrow()) {
            Ok(new_runner) => {
                journal = activate_runner(context, new_runner);
                *reset_time.borrow_mut() = Instant::now();
                title.set_subtitle(&runner.borrow().subtitle());
                negotiated_rate.set_subtitle(&format!("{sample_rate_hz} Hz"));
                format!(
                    "Capturing {} on channel {}.",
                    device.display_name,
                    channel_index + 1
                )
            }
            Err(error) => {
                show_capture_error(&error, input_status, capture_status);
                return;
            }
        }
    } else {
        journal = activate_runner(context, MeasurementRunner::demo(&store.borrow()));
        *reset_time.borrow_mut() = Instant::now();
        title.set_subtitle(&runner.borrow().subtitle());
        negotiated_rate.set_subtitle("48,000 Hz (demo)");
        "Demo generator is active.".to_owned()
    };
    let status = match journal {
        Ok(()) => status,
        Err(error) => format!("{status} The journal could not be started: {error}"),
    };
    // The selection is opened again on the next start (see `startup`).
    let remembered = settings
        .borrow_mut()
        .remember_input(runner.borrow().recent_input());
    let status = match remembered {
        Ok(()) => status,
        Err(error) => format!("{status} It will not be reopened on the next start: {error}"),
    };
    capture_status.set_subtitle(&status);
    capture_status.update_property(&[gtk::accessible::Property::Description(&status)]);
    input_status.set_label("");
    input_status.set_visible(false);
}

/// Makes `next` active with a fresh journal; the replaced recording and its
/// journal are discarded, as switching input always started a new recording.
#[cfg(feature = "cpal-backend")]
fn activate_runner(
    context: &PreferencesContext,
    mut next: MeasurementRunner,
) -> Result<(), String> {
    let journal = next.start_journal(&context.sessions_directory);
    replace_runner(&context.runner, next).and(journal)
}

#[cfg(feature = "cpal-backend")]
fn show_capture_error(error: &str, input_status: &gtk::Label, capture_status: &adw::ActionRow) {
    let status = format!("Input error: {error}");
    input_status.set_label(&status);
    input_status.set_visible(true);
    capture_status.set_subtitle(&status);
    capture_status.update_property(&[gtk::accessible::Property::Description(&status)]);
}

#![forbid(unsafe_code)]
//! GNOME desktop application Re:sonus Meter (formerly SPL Meter).

use adw::prelude::*;
mod fonts;
mod journal;
mod layout;
mod log_view;
mod marker_editor;
mod meter;
mod migrate;
mod plot;
mod preferences;
mod rta_view;
mod runner;
mod save_dialog;
mod session_actions;
mod startup;
mod style;

use gtk::{gdk, glib};
use journal::{
    JournalSchedule, confirm_close, offer_recovery, sessions_directory, show_journal_result,
};
use log_view::{log_tab, present_marker_dialog, sync_interval_selector, update_log_view};
use meter::{
    ReadingSelection, install_meter_styles, meter_surface, selection_controls, update_display,
};
use preferences::{PreferencesContext, present_preferences};
use rta_view::{RtaView, rta_config};
use runner::MeasurementRunner;
use save_dialog::present_save_dialog;
use session_actions::{add_new_session_action, primary_menu};
use spl_engine::{CalibrationStore, Settings};
use startup::open_startup_input;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

/// Window title (window list, About dialog).
pub(crate) const WINDOW_TITLE: &str = "Re:sonus Meter";
/// Header-bar title: the app name without the brand, which stands left of it (ADR 0017).
const HEADER_TITLE: &str = "Meter";
/// Application id (Re:sonus decision D7, ADR 0016); also names the config and state folders.
pub(crate) const APPLICATION_ID: &str = "cz.resonus.SplMeter";
pub(crate) const DISPLAY_REFRESH: Duration = Duration::from_millis(125);
/// Width of the ribbon's calibration note in characters (three lines at most, the dB
/// value on its own line); "Uncalibrated" and "Calibrator" fit without breaking.
const CALIBRATION_NOTE_CHARS: i32 = 10;

fn main() -> glib::ExitCode {
    // `--demo` starts with the demo source instead of the last used input,
    // for example for the packaging smoke tests.
    let demo_only = std::env::args().any(|argument| argument == "--demo");
    // `--screenshot=PATH` renders the window to a PNG after
    // `--screenshot-delay=SECONDS` (default 2) and quits (README screenshots and
    // layout checks; ADR 0017).
    let screenshot = std::env::args().find_map(|argument| {
        argument
            .strip_prefix("--screenshot=")
            .map(std::path::PathBuf::from)
    });
    let delay_s = std::env::args()
        .find_map(|argument| {
            argument
                .strip_prefix("--screenshot-delay=")
                .and_then(|seconds| seconds.parse::<u32>().ok())
        })
        .unwrap_or(2);
    let arguments = std::env::args()
        .filter(|argument| argument != "--demo" && !argument.starts_with("--screenshot"))
        .collect::<Vec<_>>();
    migrate_legacy_folders();
    let application = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    application.connect_activate(move |application| {
        build_ui(application, demo_only);
        if let Some(path) = screenshot.clone() {
            schedule_screenshot(application, path, delay_s);
        }
    });
    application.run_with_args(&arguments)
}

#[allow(clippy::too_many_lines)]
fn build_ui(application: &adw::Application, demo_only: bool) {
    install_meter_styles();

    // Header bar: brand left, app name centred with the input as subtitle, main
    // menu right; all other controls are in the ribbon (ADR 0017).
    let title = adw::WindowTitle::builder()
        .title(HEADER_TITLE)
        .subtitle("Demo source")
        .build();
    let header_bar = adw::HeaderBar::builder().title_widget(&title).build();
    header_bar.pack_start(&layout::brand_label());
    header_bar.pack_end(&primary_menu());
    let reset = layout::ribbon_button("view-refresh-symbolic", "Reset");
    reset.set_tooltip_text(Some(
        "Reset measurement values; logging continues in a new segment",
    ));
    reset.update_property(&[gtk::accessible::Property::Label("Reset measurement values")]);
    let add_marker = layout::ribbon_button("bookmark-new-symbolic", "Add Marker");
    add_marker.set_action_name(Some("win.add-marker"));
    add_marker.set_tooltip_text(Some("Add marker to measurement log (M)"));
    add_marker.update_property(&[gtk::accessible::Property::Label(
        "Add marker to measurement log",
    )]);
    let pause = layout::ribbon_button("media-playback-pause-symbolic", "Pause");
    pause.set_action_name(Some("win.toggle-pause"));
    show_pause_state(&pause, false);
    let save = layout::ribbon_button("document-save-symbolic", "Save…");
    save.set_tooltip_text(Some("Save measurement bundle (Ctrl+S)"));
    save.set_action_name(Some("win.save-measurement"));
    save.update_property(&[gtk::accessible::Property::Label("Save measurement")]);
    let new_session = layout::ribbon_button("document-new-symbolic", "New Session");
    new_session.set_tooltip_text(Some("Start a new measurement session (Ctrl+N)"));
    new_session.set_action_name(Some("win.new-session"));
    let input = layout::ribbon_button("audio-input-microphone-symbolic", "Input…");
    input.set_tooltip_text(Some("Choose the sound device and channel"));
    let calibrate = layout::ribbon_button("preferences-system-symbolic", "Calibrate…");
    calibrate.set_tooltip_text(Some(
        "Calibrate with an acoustic calibrator or enter an offset",
    ));
    // Fixed width in characters across both ribbon rows, wrapped, so a longer text does
    // not shift or widen the ribbon (user, 2026-11-06).
    let calibration_status = gtk::Label::builder()
        .label("Uncalibrated")
        .xalign(0.0)
        .yalign(0.5)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .width_chars(CALIBRATION_NOTE_CHARS)
        .max_width_chars(CALIBRATION_NOTE_CHARS)
        .lines(3)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .valign(gtk::Align::Fill)
        .css_classes(["dim-label"])
        .build();
    calibration_status.update_property(&[gtk::accessible::Property::Label("Calibration")]);
    let measure_group = layout::ribbon_grid();
    measure_group.attach(&pause, 0, 0, 1, 1);
    measure_group.attach(&reset, 0, 1, 1, 1);
    measure_group.attach(&add_marker, 1, 0, 1, 1);
    let device_group = layout::ribbon_grid();
    device_group.attach(&input, 0, 0, 1, 1);
    device_group.attach(&calibrate, 0, 1, 1, 1);
    device_group.attach(&calibration_status, 1, 0, 1, 2);
    let files_group = layout::ribbon_grid();
    files_group.attach(&save, 0, 0, 1, 1);
    files_group.attach(&new_session, 0, 1, 1, 1);
    let selection = Rc::new(RefCell::new(ReadingSelection::DEFAULT));
    let ribbon = layout::ribbon(&[
        ("Measure", measure_group.upcast()),
        ("Reading", selection_controls(&selection).upcast()),
        ("Device", device_group.upcast()),
        ("Files", files_group.upcast()),
    ]);

    let meter = meter_surface();

    let (store, store_warning) = open_calibration_store();
    let (settings, settings_warning) = open_settings();
    let (mut runner, startup_notice) = open_startup_input(&settings, &store, demo_only);
    let sessions_directory: Rc<std::path::Path> = Rc::from(sessions_directory());
    // List earlier journals before this session writes its own.
    let recoverable = spl_engine::recoverable_sessions(&sessions_directory);
    let journal_notice = runner
        .start_journal(&sessions_directory)
        .err()
        .map(|error| format!("Journal not started; this measurement cannot be recovered: {error}"));
    let runner = Rc::new(RefCell::new(runner));
    let store = Rc::new(RefCell::new(store));
    let settings = Rc::new(RefCell::new(settings));
    let log = log_tab(&runner);
    let tab_stack = adw::ViewStack::new();
    tab_stack.add_titled_with_icon(
        &log.content,
        Some("log"),
        "Log",
        "utilities-system-monitor-symbolic",
    );
    let rta = RtaView::new(settings.borrow().rta(), &log.interval);
    tab_stack.add_titled_with_icon(&rta.content, Some("rta"), "RTA", "view-continuous-symbolic");
    for page in [&log.content, &rta.content] {
        page.set_margin_start(12);
        page.set_margin_end(12);
        page.set_margin_top(12);
        page.set_margin_bottom(12);
    }
    let (tab_switcher, tab_panel) = layout::tabbed_panel(&tab_stack);
    // Narrow windows: the tab switcher moves to a bar at the bottom (breakpoint below).
    let tab_bar = adw::ViewSwitcherBar::builder().stack(&tab_stack).build();
    let reset_time = Rc::new(RefCell::new(Instant::now()));
    // Refreshes the view after Reset or New Session started a fresh recording.
    let after_restart: Rc<dyn Fn(&str)> = {
        let runner = Rc::clone(&runner);
        let reset_time = Rc::clone(&reset_time);
        let plot = log.plot.clone();
        let plot_data = Rc::clone(&log.plot_data);
        let status = log.status.clone();
        let interval = log.interval.clone();
        let interval_syncing = Rc::clone(&log.interval_syncing);
        let rta = Rc::clone(&rta);
        Rc::new(move |message: &str| {
            rta.clear_hold();
            let window_s = plot_data.borrow().window.duration_s();
            if let Some(history) = runner.borrow().log_history(window_s) {
                sync_interval_selector(&interval, &interval_syncing, history.interval_s);
                plot_data.borrow_mut().update(&history);
                plot.queue_draw();
            }
            status.set_label(message);
            status.update_property(&[gtk::accessible::Property::Description(message)]);
            *reset_time.borrow_mut() = Instant::now();
        })
    };
    let runner_reset = Rc::clone(&runner);
    let reset_view = Rc::clone(&after_restart);
    reset.connect_clicked(move |_| {
        runner_reset.borrow_mut().reset();
        reset_view("Reset marker added; logging continues in a new segment.");
    });

    let reading_update = meter.reading.clone();
    let descriptor_update = meter.descriptor.clone();
    let unit_update = meter.unit.clone();
    let elapsed_update = meter.elapsed.clone();
    let uncalibrated_update = meter.uncalibrated.clone();
    let overload_update = meter.overload.clone();
    let fast_max_update = meter.fast_max.clone();
    let runner_update = Rc::clone(&runner);
    let title_update = title.clone();
    let reset_time_update = Rc::clone(&reset_time);
    let input_status = gtk::Label::builder()
        .wrap(true)
        .css_classes(["warning"])
        .margin_bottom(12)
        .margin_start(24)
        .margin_end(24)
        .build();
    input_status.update_property(&[gtk::accessible::Property::Label("Input status")]);
    let input_status_update = input_status.clone();
    let log_plot_update = log.plot.clone();
    let log_plot_data_update = Rc::clone(&log.plot_data);
    let log_status_update = log.status.clone();
    let settings_update = Rc::clone(&settings);
    let tabs_update = tab_stack.clone();
    let rta_update = Rc::clone(&rta);
    glib::timeout_add_local(DISPLAY_REFRESH, move || {
        let mut runner = runner_update.borrow_mut();
        // Before the tick's audio: applies at once to a recording still
        // empty (a new input), otherwise from the next Reset.
        let rta_settings = settings_update.borrow().rta();
        runner.set_band_log(rta_settings.record.then(|| rta_config(rta_settings)));
        let snapshot = runner.tick();
        update_display(
            snapshot,
            *selection.borrow(),
            reset_time_update.borrow().elapsed(),
            &reading_update,
            &descriptor_update,
            &unit_update,
            &elapsed_update,
            &uncalibrated_update,
            &overload_update,
            &fast_max_update,
        );
        title_update.set_subtitle(&runner.subtitle());
        calibration_status.set_label(&runner.calibration_status());
        show_pause_state(&pause, snapshot.paused);
        let status = runner.input_status().unwrap_or_default();
        input_status_update.set_label(&status);
        input_status_update.set_visible(!status.is_empty());
        update_log_view(
            &runner,
            snapshot,
            &log_plot_data_update,
            &log_plot_update,
            &log_status_update,
        );
        // The analyser runs only while its tab is shown (specification 3.3).
        let rta_shown = tabs_update.visible_child_name().as_deref() == Some("rta");
        runner.set_rta(rta_shown.then(|| rta_config(rta_settings)));
        if rta_shown {
            rta_update.show(
                runner.rta_frame(rta_update.detector(), rta_update.weighting()),
                rta_settings.bandwidth,
                snapshot.calibrated,
            );
        }
        glib::ControlFlow::Continue
    });

    let layout = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    layout.append(&header_bar);
    layout.append(&ribbon);
    layout.append(&meter.content);
    layout.append(&tab_switcher);
    layout.append(&tab_panel);
    let save_status = gtk::Label::builder()
        .wrap(true)
        .css_classes(["dim-label"])
        .margin_bottom(12)
        .margin_start(24)
        .margin_end(24)
        .build();
    save_status.update_property(&[gtk::accessible::Property::Label("Save status")]);
    layout.append(&save_status);
    layout.append(&input_status);
    layout.append(&tab_bar);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&layout));
    for notice in [
        settings_warning.as_deref(),
        startup_notice.as_deref(),
        journal_notice.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        toasts.add_toast(adw::Toast::builder().title(notice).timeout(10).build());
    }
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title(WINDOW_TITLE)
        .default_width(940)
        .default_height(860)
        .content(&toasts)
        .build();
    // Below 400 px the wide tab switcher no longer fits: use the bottom bar.
    let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        400.0,
        adw::LengthUnit::Sp,
    ));
    narrow.add_setter(&tab_switcher, "visible", Some(&false.to_value()));
    narrow.add_setter(&tab_bar, "reveal", Some(&true.to_value()));
    window.add_breakpoint(narrow);
    // Plots follow the light/dark style (style tokens, ADR 0017).
    let (log_plot, rta_plot) = (log.plot.clone(), rta.plot.clone());
    adw::StyleManager::default().connect_dark_notify(move |_| {
        log_plot.queue_draw();
        rta_plot.queue_draw();
    });
    let preferences_action = gtk::gio::SimpleAction::new("preferences", None);
    let preferences_window = window.downgrade();
    let preferences_context = PreferencesContext {
        runner: Rc::clone(&runner),
        store,
        store_warning,
        settings: Rc::clone(&settings),
        sessions_directory,
        on_rta_changed: {
            let rta = Rc::clone(&rta);
            Rc::new(move |settings| {
                rta.apply_defaults(settings);
                rta.clear_hold();
            })
        },
        title: title.clone(),
        input_status: input_status.clone(),
        reset_time: Rc::clone(&reset_time),
    };
    let context = preferences_context.clone();
    preferences_action.connect_activate(move |_, _| {
        if let Some(window) = preferences_window.upgrade() {
            present_preferences(&window, &context, None);
        }
    });
    for (button, page) in [(&input, "sound-device"), (&calibrate, "calibration")] {
        let window = window.downgrade();
        let context = preferences_context.clone();
        button.connect_clicked(move |_| {
            if let Some(window) = window.upgrade() {
                present_preferences(&window, &context, Some(page));
            }
        });
    }
    window.add_action(&preferences_action);
    add_save_action(&window, &runner, &toasts);
    add_new_session_action(&window, &runner, &after_restart);
    add_marker_action(&window, &runner, &log.status);
    add_pause_action(&window, &runner, &log.status);
    add_pause_key(&window);
    application.set_accels_for_action("win.preferences", &["<Control>comma"]);
    application.set_accels_for_action("win.save-measurement", &["<Control>s"]);
    application.set_accels_for_action("win.new-session", &["<Control>n"]);
    application.set_accels_for_action("win.add-marker", &["m", "M"]);
    write_journal_periodically(&runner, &settings, &save_status);
    confirm_close(&window, &runner);
    window.present();
    let on_recovered: Rc<dyn Fn()> = {
        let runner = Rc::clone(&runner);
        let interval = log.interval.clone();
        let syncing = Rc::clone(&log.interval_syncing);
        let time_window = log.time_window.clone();
        Rc::new(move || {
            if let Some(history) = runner.borrow().log_history(None) {
                sync_interval_selector(&interval, &syncing, history.interval_s);
            }
            // The clock moved on past the crash gap; show the recovered part too.
            time_window.set_selected(3);
        })
    };
    offer_recovery(
        &window,
        &toasts,
        &preferences_context,
        recoverable,
        on_recovered,
    );
}

/// Renders the active window to a PNG at `path` after `delay_s` seconds, then quits.
fn schedule_screenshot(application: &adw::Application, path: std::path::PathBuf, delay_s: u32) {
    let application = application.clone();
    glib::timeout_add_seconds_local_once(delay_s, move || {
        if let Some(window) = application.active_window() {
            let (width, height) = (window.width(), window.height());
            let paintable = gtk::WidgetPaintable::new(Some(&window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
            let renderer = window.native().and_then(|native| native.renderer());
            if let (Some(node), Some(renderer)) = (snapshot.to_node(), renderer) {
                #[allow(clippy::cast_precision_loss)]
                let area = gtk::graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
                let texture = renderer.render_texture(node, Some(&area));
                if let Err(error) = texture.save_to_png(&path) {
                    eprintln!("Could not save {}: {error}", path.display());
                }
            }
        }
        application.quit();
    });
}

/// Writes the journal at the interval chosen in Preferences.
fn write_journal_periodically(
    runner: &Rc<RefCell<MeasurementRunner>>,
    settings: &Rc<RefCell<Settings>>,
    status: &gtk::Label,
) {
    let runner = Rc::clone(runner);
    let settings = Rc::clone(settings);
    let status = status.clone();
    let mut schedule = JournalSchedule::new();
    glib::timeout_add_seconds_local(1, move || {
        if schedule.is_due(settings.borrow().journal_interval_s()) {
            show_journal_result(&status, runner.borrow_mut().flush_journal());
        }
        glib::ControlFlow::Continue
    });
}

/// Moves the config and state folders of the former application id once (ADR 0016).
///
/// Runs before anything reads them; a failed move is reported on stderr and the
/// app starts with the new, empty folders (the legacy ones stay untouched).
fn migrate_legacy_folders() {
    for base in [glib::user_config_dir(), journal::user_state_directory()] {
        match migrate::migrate_folder(&base, migrate::LEGACY_APPLICATION_ID, APPLICATION_ID) {
            Ok(Some(path)) => eprintln!(
                "Moved {} to {}",
                base.join(migrate::LEGACY_APPLICATION_ID).display(),
                path.display()
            ),
            Ok(None) => {}
            Err(error) => eprintln!(
                "Could not move {} to {}: {error}",
                base.join(migrate::LEGACY_APPLICATION_ID).display(),
                base.join(APPLICATION_ID).display()
            ),
        }
    }
}

/// Returns the path of a file in the application's XDG config directory.
fn config_file(name: &str) -> std::path::PathBuf {
    glib::user_config_dir().join(APPLICATION_ID).join(name)
}

/// Opens `settings.toml` in the XDG config directory.
///
/// An unreadable file is left untouched; defaults are used for this run and
/// the returned warning explains why.
fn open_settings() -> (Settings, Option<String>) {
    match Settings::open(&config_file("settings.toml")) {
        Ok(settings) => (settings, None),
        Err(error) => (
            Settings::unsaved(),
            Some(format!("Settings are not saved this session: {error}")),
        ),
    }
}

/// Opens `calibrations.toml` in the XDG config directory.
///
/// An unreadable history is left untouched: calibration then works for this
/// run only, and the returned warning explains why.
fn open_calibration_store() -> (CalibrationStore, Option<Rc<str>>) {
    match CalibrationStore::open(&config_file("calibrations.toml")) {
        Ok(store) => (store, None),
        Err(error) => (
            CalibrationStore::unsaved(),
            Some(Rc::from(error.to_string())),
        ),
    }
}

fn add_save_action(
    window: &adw::ApplicationWindow,
    runner: &Rc<RefCell<MeasurementRunner>>,
    toasts: &adw::ToastOverlay,
) {
    let save_demo_action = gtk::gio::SimpleAction::new("save-measurement", None);
    let save_runner = Rc::clone(runner);
    let save_window = window.downgrade();
    let toasts = toasts.clone();
    save_demo_action.connect_activate(move |_, _| {
        if let Some(window) = save_window.upgrade() {
            present_save_dialog(&window, &save_runner, &toasts);
        }
    });
    window.add_action(&save_demo_action);
}

fn add_marker_action(
    window: &adw::ApplicationWindow,
    runner: &Rc<RefCell<MeasurementRunner>>,
    log_status: &gtk::Label,
) {
    let action = gtk::gio::SimpleAction::new("add-marker", None);
    let marker_runner = Rc::clone(runner);
    let marker_status = log_status.clone();
    let marker_window = window.downgrade();
    action.connect_activate(move |_, _| {
        let Some(window) = marker_window.upgrade() else {
            return;
        };
        match marker_runner.borrow_mut().reserve_marker_timestamp() {
            Ok(elapsed_s) => {
                present_marker_dialog(&window, &marker_runner, &marker_status, elapsed_s);
            }
            Err(error) => {
                let message = format!("Could not add marker: {error}");
                marker_status.set_label(&message);
                marker_status.update_property(&[gtk::accessible::Property::Description(&message)]);
            }
        }
    });
    window.add_action(&action);
}

/// Shows Pause while recording and Resume while paused.
fn show_pause_state(button: &gtk::Button, paused: bool) {
    let (icon, label, text) = if paused {
        (
            "media-playback-start-symbolic",
            "Resume",
            "Resume recording",
        )
    } else {
        ("media-playback-pause-symbolic", "Pause", "Pause recording")
    };
    let Some(content) = button.child().and_downcast::<adw::ButtonContent>() else {
        return;
    };
    if content.icon_name() != icon {
        content.set_icon_name(icon);
        content.set_label(label);
        button.set_tooltip_text(Some(&format!("{text} (P)")));
        button.update_property(&[gtk::accessible::Property::Label(text)]);
    }
}

fn add_pause_action(
    window: &adw::ApplicationWindow,
    runner: &Rc<RefCell<MeasurementRunner>>,
    log_status: &gtk::Label,
) {
    let action = gtk::gio::SimpleAction::new("toggle-pause", None);
    let runner = Rc::clone(runner);
    let log_status = log_status.clone();
    action.connect_activate(move |_, _| {
        if let Err(error) = runner.borrow_mut().toggle_pause() {
            let message = format!("Could not pause or resume: {error}");
            log_status.set_label(&message);
            log_status.update_property(&[gtk::accessible::Property::Description(&message)]);
        }
    });
    window.add_action(&action);
}

/// Toggles pause with <kbd>P</kbd>, once per key press.
///
/// An application accelerator would also fire on key auto-repeat, so holding
/// the key would write a run of pause/resume markers. The window-level
/// controller sees the key only if no focused text field consumed it.
fn add_pause_key(window: &adw::ApplicationWindow) {
    let held = Rc::new(Cell::new(false));
    let controller = gtk::EventControllerKey::new();
    let pressed = Rc::clone(&held);
    let target = window.downgrade();
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        let command_modifiers = gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::ALT_MASK
            | gdk::ModifierType::SUPER_MASK
            | gdk::ModifierType::META_MASK;
        if key.to_lower() != gdk::Key::p || modifiers.intersects(command_modifiers) {
            return glib::Propagation::Proceed;
        }
        if !pressed.replace(true)
            && let Some(window) = target.upgrade()
        {
            ActionGroupExt::activate_action(&window, "toggle-pause", None);
        }
        glib::Propagation::Stop
    });
    let released = Rc::clone(&held);
    controller.connect_key_released(move |_, key, _, _| {
        if key.to_lower() == gdk::Key::p {
            released.set(false);
        }
    });
    window.add_controller(controller);
    // A release that happens while another window has focus is never seen.
    window.connect_is_active_notify(move |_| held.set(false));
}

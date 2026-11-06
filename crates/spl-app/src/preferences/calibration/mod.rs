//! Calibration preferences: the active input's offset, calibrator capture and
//! the saved history (specification 5.4 and 4.9).

mod capture;
mod history;

use super::{PreferencesContext, read_only_row, update_full_scale_equivalence};
use crate::DISPLAY_REFRESH;
use adw::prelude::*;
use capture::CaptureWidgets;
use gtk::glib;
use history::HistoryWidgets;
use spl_engine::{CalibrationRecord, CalibrationStoreError, InputProfile};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use time::OffsetDateTime;

/// A manual offset is saved once it has stopped changing for this long, so
/// stepping through values does not fill the history.
const MANUAL_SAVE_DELAY: Duration = Duration::from_millis(1_500);
/// Offsets closer than half the 0.01 dB entry step are the same setting.
const SAME_OFFSET_DB: f64 = 0.005;

/// Builds the page and a callback that reloads it after the input changes.
pub(crate) fn calibration_page(
    context: &PreferencesContext,
) -> (adw::PreferencesPage, Rc<dyn Fn()>) {
    let page = adw::PreferencesPage::builder()
        .name("calibration")
        .title("Calibration")
        .icon_name("preferences-system-symbolic")
        .build();
    let view = Rc::new(CalibrationView::new(context.clone()));
    page.add(&view.manual);
    page.add(&view.capture.group);
    page.add(&view.history.group);
    connect_handlers(&view);
    view.refresh();

    let refresh: Rc<dyn Fn()> = {
        let view = Rc::downgrade(&view);
        Rc::new(move || {
            if let Some(view) = view.upgrade() {
                view.refresh();
            }
        })
    };
    // The page owns the view; handlers hold weak references, so closing the
    // dialog releases both and stops the capture poll.
    page.connect_destroy(move |_| {
        let _ = &view;
    });
    (page, refresh)
}

struct CalibrationView {
    context: PreferencesContext,
    manual: adw::PreferencesGroup,
    offset: adw::SpinRow,
    /// Set while `refresh` writes the offset row, so it is not saved as manual input.
    offset_syncing: Cell<bool>,
    pending_manual_save: RefCell<Option<glib::SourceId>>,
    full_scale: adw::ActionRow,
    capture: CaptureWidgets,
    history: HistoryWidgets,
}

impl CalibrationView {
    fn new(context: PreferencesContext) -> Self {
        let manual = adw::PreferencesGroup::builder()
            .title("Calibration offset")
            .build();
        let offset = adw::SpinRow::with_range(-200.0, 200.0, 0.01);
        offset.set_title("Calibration offset (C)");
        offset.set_subtitle("SPL = dBFS + C, in dB. Edits are saved as a manual calibration.");
        offset.set_digits(2);
        offset.update_property(&[gtk::accessible::Property::Label(
            "Calibration offset in decibels",
        )]);
        offset.add_prefix(&nudge_button(
            &offset,
            -0.1,
            "list-remove-symbolic",
            "Decrease",
        ));
        offset.add_suffix(&nudge_button(&offset, 0.1, "list-add-symbolic", "Increase"));
        manual.add(&offset);
        let full_scale = read_only_row("Calibrated full-scale sine", "");
        full_scale.update_property(&[gtk::accessible::Property::Label(
            "Full-scale sine calibration equivalence",
        )]);
        manual.add(&full_scale);
        Self {
            context,
            manual,
            offset,
            offset_syncing: Cell::new(false),
            pending_manual_save: RefCell::new(None),
            full_scale,
            capture: CaptureWidgets::new(),
            history: HistoryWidgets::new(),
        }
    }

    /// Shows the active input's calibration and history.
    fn refresh(self: &Rc<Self>) {
        self.show_input(None);
    }

    /// Like [`Self::refresh`], reporting a failed history write if there was one.
    fn show_input(self: &Rc<Self>, save_error: Option<&str>) {
        let runner = self.context.runner.borrow();
        let profile = runner.input_profile();
        self.manual.set_description(Some(&format!(
            "Applies to {} · channel {}.",
            profile.device_name,
            profile.channel_index + 1
        )));
        self.offset_syncing.set(true);
        self.offset
            .set_value(runner.calibration_offset_db().unwrap_or(0.0));
        self.offset_syncing.set(false);
        update_full_scale_equivalence(&self.full_scale, &runner);
        drop(runner);
        self.show_history(&profile, save_error);
    }

    /// Lists the profile's records; `save_error` reports a failed write.
    fn show_history(self: &Rc<Self>, profile: &InputProfile, save_error: Option<&str>) {
        let store = self.context.store.borrow();
        let status = match (save_error, &self.context.store_warning, store.path()) {
            (Some(error), ..) => format!("Not saved: {error}"),
            (None, Some(warning), _) => format!("Not saved: {warning}"),
            (None, None, Some(path)) => path.display().to_string(),
            (None, None, None) => "Not saved (kept in memory)".to_owned(),
        };
        self.history.set_storage_status(&status);
        let view = Rc::downgrade(self);
        self.history.show(
            &store.history(profile),
            &move |record: CalibrationRecord| {
                if let Some(view) = view.upgrade() {
                    view.restore(&record);
                }
            },
        );
    }

    /// Applies an older record again and saves it as the newest.
    fn restore(self: &Rc<Self>, record: &CalibrationRecord) {
        self.cancel_manual_save();
        self.context
            .runner
            .borrow_mut()
            .set_calibration(Some(record.calibration()));
        let error = append_record(&self.context, record.restored(OffsetDateTime::now_utc()));
        self.show_input(error.as_deref());
    }

    /// Applies a typed offset at once and saves it once input settles.
    fn offset_changed(self: &Rc<Self>) {
        if self.offset_syncing.get() {
            return;
        }
        let offset_db = self.offset.value();
        let profile = {
            let mut runner = self.context.runner.borrow_mut();
            runner.set_manual_calibration(offset_db);
            update_full_scale_equivalence(&self.full_scale, &runner);
            runner.input_profile()
        };
        self.cancel_manual_save();
        // The save holds the shared state, not the view, so it still happens
        // if Preferences is closed before the delay ends.
        let context = self.context.clone();
        let view = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(MANUAL_SAVE_DELAY, move || {
            let error = save_manual(&context, &profile, offset_db);
            if let Some(view) = view.upgrade() {
                view.pending_manual_save.borrow_mut().take();
                let current = context.runner.borrow().input_profile();
                view.show_history(&current, error.as_deref());
            }
        });
        *self.pending_manual_save.borrow_mut() = Some(source);
    }

    fn cancel_manual_save(&self) {
        if let Some(source) = self.pending_manual_save.borrow_mut().take() {
            source.remove();
        }
    }

    /// Applies the completed capture, saves its record and resets the recording.
    fn save_proposal(self: &Rc<Self>) {
        self.cancel_manual_save();
        let (profile, result) = {
            let mut runner = self.context.runner.borrow_mut();
            (runner.input_profile(), runner.save_calibration_proposal())
        };
        match result {
            Ok(proposal) => {
                let note = self.capture.note.text().to_string();
                self.capture.note.set_text("");
                let record = CalibrationRecord::from_proposal(
                    &profile,
                    &proposal,
                    note,
                    OffsetDateTime::now_utc(),
                );
                let error = append_record(&self.context, record);
                self.capture.save.set_sensitive(false);
                self.capture.show_message(
                    "Calibration applied; measurement values and recording history were reset.",
                );
                self.show_input(error.as_deref());
            }
            Err(error) => self.capture.show_message(&error),
        }
    }
}

/// Appends `record`, returning a message if it could not be written.
fn append_record(context: &PreferencesContext, record: CalibrationRecord) -> Option<String> {
    let result = context.store.borrow_mut().append(record);
    result
        .err()
        .map(|error: CalibrationStoreError| error.to_string())
}

/// Saves a settled manual offset unless it equals the active record.
fn save_manual(
    context: &PreferencesContext,
    profile: &InputProfile,
    offset_db: f64,
) -> Option<String> {
    let active = context.store.borrow().active(profile).cloned();
    match active {
        // Stepping away and back again is not a new calibration: keep the
        // active record, including its calibrator metadata, if still current.
        Some(record) if (record.offset_db - offset_db).abs() < SAME_OFFSET_DB => {
            let mut runner = context.runner.borrow_mut();
            if record.belongs_to(&runner.input_profile()) {
                runner.set_calibration(Some(record.calibration()));
            }
            None
        }
        _ => append_record(
            context,
            CalibrationRecord::manual(profile, offset_db, OffsetDateTime::now_utc()),
        ),
    }
}

fn connect_handlers(view: &Rc<CalibrationView>) {
    let weak = Rc::downgrade(view);
    view.offset
        .connect_notify_local(Some("value"), move |_, _| {
            if let Some(view) = weak.upgrade() {
                view.offset_changed();
            }
        });
    let weak = Rc::downgrade(view);
    view.capture.button.connect_clicked(move |_| {
        let Some(view) = weak.upgrade() else { return };
        let result = view
            .context
            .runner
            .borrow_mut()
            .start_calibration_capture(view.capture.level.value(), view.capture.frequency.value());
        match result {
            Ok(()) => view.capture.show_started(),
            Err(error) => view
                .capture
                .show_message(&format!("Could not start capture: {error}")),
        }
    });
    let weak = Rc::downgrade(view);
    view.capture.save.connect_clicked(move |_| {
        if let Some(view) = weak.upgrade() {
            view.save_proposal();
        }
    });
    let weak = Rc::downgrade(view);
    glib::timeout_add_local(DISPLAY_REFRESH, move || {
        let Some(view) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let state = view.context.runner.borrow().calibration_capture_state();
        view.capture.show_state(state);
        glib::ControlFlow::Continue
    });
}

fn nudge_button(offset: &adw::SpinRow, step_db: f64, icon: &str, verb: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .valign(gtk::Align::Center)
        .tooltip_text(format!("{verb} calibration offset by 0.1 dB"))
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&format!(
        "{verb} calibration offset by 0.1 decibels"
    ))]);
    let offset = offset.clone();
    button.connect_clicked(move |_| {
        offset.set_value((offset.value() + step_db).clamp(-200.0, 200.0));
    });
    button
}

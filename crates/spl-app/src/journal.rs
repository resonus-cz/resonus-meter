//! The session journal in the app: where it lives, when it is written,
//! recovery at startup and confirmation before closing (ADR 0010).

use std::{
    cell::{Cell, RefCell},
    fmt::Write as _,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use adw::prelude::*;
use gtk::glib;
use spl_engine::{JournalError, RecoverableSession, discard_journal_directory};

use crate::APPLICATION_ID;
use crate::meter::format_elapsed;
use crate::preferences::PreferencesContext;
use crate::runner::{MeasurementRunner, replace_runner};
use crate::startup::open_recorded_input;

const JOURNAL_ERROR_PREFIX: &str = "Journal not written:";

/// Returns the folder holding session journals in the XDG state directory,
/// which survives reboots, unlike `/tmp`.
pub(crate) fn sessions_directory() -> PathBuf {
    user_state_directory().join(APPLICATION_ID).join("sessions")
}

/// `$XDG_STATE_HOME`, or `~/.local/state` when it is unset or relative (XDG
/// Base Directory Specification); Flatpak sets it to the sandbox's folder.
/// `glib::user_state_dir` would need a newer `GLib` feature than is enabled.
pub(crate) fn user_state_directory() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".local").join("state"))
}

/// Decides when the journal is written next.
pub(crate) struct JournalSchedule {
    last_flush: Instant,
}

impl JournalSchedule {
    pub(crate) fn new() -> Self {
        Self {
            last_flush: Instant::now(),
        }
    }

    /// Returns whether `interval_s` has passed since the last write, and if
    /// so starts the next interval.
    pub(crate) fn is_due(&mut self, interval_s: u32) -> bool {
        let due = self.last_flush.elapsed() >= Duration::from_secs(u64::from(interval_s));
        if due {
            self.last_flush = Instant::now();
        }
        due
    }
}

/// Reports a failed journal write in `status` and clears the report once
/// writing works again.
pub(crate) fn show_journal_result(status: &gtk::Label, result: Result<(), String>) {
    let message = match result {
        Ok(()) if status.label().starts_with(JOURNAL_ERROR_PREFIX) => String::new(),
        Ok(()) => return,
        Err(error) => format!(
            "{JOURNAL_ERROR_PREFIX} {error}. The measurement cannot be recovered after a \
             crash; save it to keep it."
        ),
    };
    status.set_label(&message);
    status.update_property(&[gtk::accessible::Property::Description(&message)]);
}

/// Journals left by earlier runs, sorted for the recovery prompt.
#[derive(Debug, Default)]
struct Candidates {
    /// Sessions with data, newest first.
    sessions: Vec<RecoverableSession>,
    /// Folders whose recovery state cannot be read, with the reason.
    unreadable: Vec<(PathBuf, String)>,
    /// Journals without recorded data or markers: nothing to recover.
    empty: Vec<PathBuf>,
}

fn sort_candidates(found: Vec<Result<RecoverableSession, (PathBuf, JournalError)>>) -> Candidates {
    let mut candidates = Candidates::default();
    for entry in found {
        match entry {
            Ok(session) if session.holds_data => candidates.sessions.push(session),
            Ok(session) => candidates.empty.push(session.directory),
            Err((directory, error)) => candidates.unreadable.push((directory, error.to_string())),
        }
    }
    candidates
}

/// Asks once about every journal left by earlier runs.
///
/// Journals without data are deleted silently. Otherwise one dialog offers
/// the newest session (or a choice when there are several): **Recover**
/// continues the chosen one and deletes the other journals, **Discard All**
/// deletes them all, and **Not Now** keeps them for the next start.
/// `on_recovered` runs after the recovered session became active.
pub(crate) fn offer_recovery(
    window: &adw::ApplicationWindow,
    toasts: &adw::ToastOverlay,
    context: &PreferencesContext,
    found: Vec<Result<RecoverableSession, (PathBuf, JournalError)>>,
    on_recovered: Rc<dyn Fn()>,
) {
    let candidates = sort_candidates(found);
    for directory in &candidates.empty {
        // A leftover empty journal is harmless; it is tried again next start.
        let _ = discard_journal_directory(directory);
    }
    if candidates.sessions.is_empty() && candidates.unreadable.is_empty() {
        return;
    }
    let choices = SessionChoices::new(&candidates.sessions);
    let dialog = adw::AlertDialog::builder()
        .heading(if candidates.sessions.is_empty() {
            "Discard Unreadable Journals?"
        } else {
            "Recover Unsaved Measurement?"
        })
        .body(recovery_body(&candidates))
        .close_response("later")
        .default_response(if candidates.sessions.is_empty() {
            "later"
        } else {
            "recover"
        })
        .build();
    if let Some(list) = &choices.list {
        dialog.set_extra_child(Some(list));
    }
    dialog.add_responses(&[("discard", "Discard All"), ("later", "Not Now")]);
    if !candidates.sessions.is_empty() {
        dialog.add_response("recover", "Recover");
        dialog.set_response_appearance("recover", adw::ResponseAppearance::Suggested);
    }
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    let context = context.clone();
    let toasts = toasts.clone();
    dialog.connect_response(None, move |_, response| match response {
        "recover" => {
            let chosen = choices.selected();
            let session = &candidates.sessions[chosen];
            match recover(&context, session) {
                Ok(()) => {
                    on_recovered();
                    discard_all(&toasts, &candidates, Some(chosen));
                    toast(
                        &toasts,
                        "Measurement recovered and paused. Press Resume to continue.",
                    );
                }
                Err(error) => toast(
                    &toasts,
                    &format!("Could not recover the measurement: {error}. The journals are kept."),
                ),
            }
        }
        "discard" => discard_all(&toasts, &candidates, None),
        _ => {}
    });
    dialog.present(Some(window));
}

/// Deletes every candidate journal except the recovered one, `keep`.
fn discard_all(toasts: &adw::ToastOverlay, candidates: &Candidates, keep: Option<usize>) {
    let directories = candidates
        .sessions
        .iter()
        .enumerate()
        .filter(|(index, _)| Some(*index) != keep)
        .map(|(_, session)| &session.directory)
        .chain(candidates.unreadable.iter().map(|(directory, _)| directory));
    for directory in directories {
        if let Err(error) = discard_journal_directory(directory) {
            toast(toasts, &format!("Could not discard a journal: {error}"));
        }
    }
}

fn recovery_body(candidates: &Candidates) -> String {
    let mut body = match candidates.sessions.as_slice() {
        [] => String::new(),
        [session] => recovery_description(session),
        sessions => format!(
            "{} measurements were not saved. Choose one to recover; Recover discards the \
             others. It reopens the input it was recorded with and continues paused.",
            sessions.len()
        ),
    };
    if !candidates.unreadable.is_empty() {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        let reasons = candidates
            .unreadable
            .iter()
            .map(|(directory, reason)| format!("{}: {reason}", directory.display()))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = write!(
            body,
            "{} journal(s) cannot be read and are discarded with Recover or Discard All:\n{reasons}",
            candidates.unreadable.len()
        );
    }
    body
}

/// Radio rows choosing which of several sessions to recover.
struct SessionChoices {
    list: Option<gtk::ListBox>,
    buttons: Vec<gtk::CheckButton>,
}

impl SessionChoices {
    fn new(sessions: &[RecoverableSession]) -> Self {
        if sessions.len() < 2 {
            return Self {
                list: None,
                buttons: Vec::new(),
            };
        }
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        list.update_property(&[gtk::accessible::Property::Label("Measurement to recover")]);
        let mut buttons: Vec<gtk::CheckButton> = Vec::new();
        for session in sessions {
            let button = gtk::CheckButton::builder()
                .active(buttons.is_empty())
                .valign(gtk::Align::Center)
                .build();
            if let Some(first) = buttons.first() {
                button.set_group(Some(first));
            }
            let row = adw::ActionRow::builder()
                .title(&session.name)
                .subtitle(session_summary(session))
                .activatable_widget(&button)
                .build();
            row.add_prefix(&button);
            list.append(&row);
            buttons.push(button);
        }
        Self {
            list: Some(list),
            buttons,
        }
    }

    /// Index of the chosen session; the newest when there is no choice.
    fn selected(&self) -> usize {
        self.buttons
            .iter()
            .position(gtk::prelude::CheckButtonExt::is_active)
            .unwrap_or(0)
    }
}

fn session_summary(session: &RecoverableSession) -> String {
    format!(
        "{} · {} · {}",
        local_time(session.started_utc),
        session.device.name,
        format_elapsed(Duration::from_secs_f64(session.recorded_until_s.max(0.0)))
    )
}

fn local_time(utc: time::OffsetDateTime) -> String {
    glib::DateTime::from_unix_utc(utc.unix_timestamp())
        .and_then(|date| date.to_local())
        .and_then(|date| date.format("%x %X"))
        .map_or_else(|_| format!("{utc} UTC"), String::from)
}

fn recovery_description(session: &RecoverableSession) -> String {
    format!(
        "“{}” started {} on {} and was not saved. {} of it is in the journal. \
         Recovering reopens that input and continues the measurement paused.",
        session.name,
        local_time(session.started_utc),
        session.device.name,
        format_elapsed(Duration::from_secs_f64(session.recorded_until_s.max(0.0))),
    )
}

/// Opens the recorded input, restores the session there and makes it active.
fn recover(context: &PreferencesContext, session: &RecoverableSession) -> Result<(), String> {
    let mut next = open_recorded_input(&session.device, &context.store.borrow())?;
    next.recover_session(&session.directory)?;
    let duration_s = next
        .log_history(None)
        .map_or(0.0, |history| history.duration_s);
    replace_runner(&context.runner, next)?;
    *context.reset_time.borrow_mut() = Instant::now()
        .checked_sub(Duration::from_secs_f64(duration_s))
        .unwrap_or_else(Instant::now);
    context
        .title
        .set_subtitle(&context.runner.borrow().subtitle());
    Ok(())
}

/// Asks before closing the window would discard unsaved data; a clean close
/// deletes the journal.
pub(crate) fn confirm_close(
    window: &adw::ApplicationWindow,
    runner: &Rc<RefCell<MeasurementRunner>>,
) {
    let confirmed = Rc::new(Cell::new(false));
    let runner = Rc::clone(runner);
    window.connect_close_request(move |window| {
        if confirmed.get() || !runner.borrow().has_unsaved_changes() {
            // A journal that cannot be deleted is offered again next time.
            let _ = runner.borrow_mut().discard_journal();
            return glib::Propagation::Proceed;
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Save Measurement Before Closing?")
            .body(
                "The measurement has changed since it was last saved. If you discard it, it \
                 cannot be recovered.",
            )
            .close_response("cancel")
            .default_response("save")
            .build();
        dialog.add_responses(&[
            ("cancel", "Cancel"),
            ("discard", "Discard"),
            ("save", "Save…"),
        ]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        let confirmed = Rc::clone(&confirmed);
        let target = window.downgrade();
        dialog.connect_response(None, move |_, response| {
            let Some(window) = target.upgrade() else {
                return;
            };
            match response {
                "discard" => {
                    confirmed.set(true);
                    window.close();
                }
                "save" => ActionGroupExt::activate_action(&window, "save-measurement", None),
                _ => {}
            }
        });
        dialog.present(Some(window));
        glib::Propagation::Stop
    });
}

fn toast(toasts: &adw::ToastOverlay, message: &str) {
    toasts.add_toast(adw::Toast::builder().title(message).timeout(10).build());
}

#[cfg(test)]
mod tests {
    use super::sort_candidates;
    use spl_engine::{Device, JournalError, RecoverableSession};
    use std::path::PathBuf;

    fn session(name: &str, holds_data: bool) -> RecoverableSession {
        RecoverableSession {
            directory: PathBuf::from(name),
            name: name.to_owned(),
            started_utc: time::OffsetDateTime::UNIX_EPOCH,
            recorded_until_s: 1.0,
            device: Device {
                id: "demo".to_owned(),
                name: "Demo".to_owned(),
                backend: "demo".to_owned(),
                channel: 1,
                sample_rate_hz: 48_000,
            },
            holds_data,
        }
    }

    #[test]
    fn candidates_separate_data_empty_and_unreadable_journals() {
        let found = vec![
            Ok(session("newest", true)),
            Ok(session("empty", false)),
            Err((PathBuf::from("broken"), JournalError::NoActiveSession)),
            Ok(session("older", true)),
        ];

        let candidates = sort_candidates(found);

        let names = candidates
            .sessions
            .iter()
            .map(|session| session.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["newest", "older"]);
        assert_eq!(candidates.empty, [PathBuf::from("empty")]);
        assert_eq!(candidates.unreadable.len(), 1);
    }
}

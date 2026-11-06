//! The primary menu and the New Session action (specification 5.1 and 5.3).

use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;

use crate::runner::MeasurementRunner;

/// The header's primary menu.
pub(crate) fn primary_menu() -> gtk::MenuButton {
    let menu = gtk::gio::Menu::new();
    let session = gtk::gio::Menu::new();
    session.append(Some("_New Session"), Some("win.new-session"));
    menu.append_section(None, &session);
    let application = gtk::gio::Menu::new();
    application.append(Some("_Preferences"), Some("win.preferences"));
    menu.append_section(None, &application);
    let button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main Menu")
        .menu_model(&menu)
        .primary(true)
        .build();
    button.update_property(&[gtk::accessible::Property::Label("Main menu")]);
    button
}

/// Adds `win.new-session`: discards the session, after confirmation when it
/// has unsaved changes, and starts a fresh one. `after_restart` refreshes the
/// view with a status message.
pub(crate) fn add_new_session_action(
    window: &adw::ApplicationWindow,
    runner: &Rc<RefCell<MeasurementRunner>>,
    after_restart: &Rc<dyn Fn(&str)>,
) {
    let action = gtk::gio::SimpleAction::new("new-session", None);
    let runner = Rc::clone(runner);
    let after_restart = Rc::clone(after_restart);
    let target = window.downgrade();
    action.connect_activate(move |_, _| {
        let Some(window) = target.upgrade() else {
            return;
        };
        let start = {
            let runner = Rc::clone(&runner);
            let after_restart = Rc::clone(&after_restart);
            move || {
                let result = runner.borrow_mut().new_session();
                match result {
                    Ok(()) => after_restart("New session started."),
                    Err(error) => after_restart(&format!("Could not start a new session: {error}")),
                }
            }
        };
        if !runner.borrow().has_unsaved_changes() {
            start();
            return;
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Start a New Session?")
            .body("The current measurement has unsaved changes, which are discarded.")
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("discard", "Discard and Start")]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.connect_response(Some("discard"), move |_, _| start());
        dialog.present(Some(&window));
    });
    window.add_action(&action);
}

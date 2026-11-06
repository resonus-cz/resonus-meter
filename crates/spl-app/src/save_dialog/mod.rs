//! The Save Measurement dialog (specification 5.5).

mod attachments;
mod range;

use crate::runner::MeasurementRunner;
use adw::prelude::*;
use attachments::AttachmentRows;
use gtk::glib;
use range::RangeRows;
use spl_engine::SaveMetadata;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

struct SaveDialog {
    dialog: adw::Dialog,
    name: adw::EntryRow,
    description: gtk::TextView,
    range: RangeRows,
    attachments: Rc<AttachmentRows>,
    folder: RefCell<PathBuf>,
    location: adw::ActionRow,
    choose_location: gtk::Button,
    /// Shows why a save failed, above the scrolling page.
    status: adw::Banner,
    save: gtk::Button,
}

pub(crate) fn present_save_dialog(
    window: &adw::ApplicationWindow,
    runner: &Rc<RefCell<MeasurementRunner>>,
    toasts: &adw::ToastOverlay,
) {
    let view = Rc::new(build(&runner.borrow()));
    connect_range(&view, runner);
    connect_folder_picker(window, &view);
    view.attachments.connect_add(window);
    connect_save(window, &view, runner, toasts);
    // The handlers hold the view weakly; the open dialog owns it.
    let owner = RefCell::new(Some(Rc::clone(&view)));
    view.dialog.connect_closed(move |_| {
        owner.borrow_mut().take();
    });
    view.dialog.present(Some(window));
    view.name.grab_focus();
}

fn build(runner: &MeasurementRunner) -> SaveDialog {
    let name = adw::EntryRow::builder()
        .title("Name")
        .text("Measurement")
        .build();
    name.update_property(&[gtk::accessible::Property::Label("Measurement name")]);
    let description = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .height_request(100)
        .top_margin(8)
        .bottom_margin(8)
        .left_margin(8)
        .right_margin(8)
        .css_classes(["card"])
        .build();
    description.update_property(&[gtk::accessible::Property::Label(
        "Measurement description in Markdown",
    )]);
    let measurement = adw::PreferencesGroup::builder()
        .title("Measurement")
        .build();
    measurement.add(&name);
    let description_group = adw::PreferencesGroup::builder()
        .title("Description")
        .description("Markdown is allowed.")
        .build();
    description_group.add(&description);

    let range = RangeRows::new(runner.log_history(None).as_ref());
    let attachments = AttachmentRows::new();

    let folder = default_save_folder();
    let location = adw::ActionRow::builder()
        .title("Folder")
        .subtitle(folder.display().to_string())
        .subtitle_selectable(true)
        .build();
    let choose_location = gtk::Button::builder()
        .label("Choose…")
        .valign(gtk::Align::Center)
        .build();
    choose_location.update_property(&[gtk::accessible::Property::Label(
        "Choose measurement save folder",
    )]);
    location.add_suffix(&choose_location);
    let location_group = adw::PreferencesGroup::builder()
        .title("Location")
        .description("The measurement is saved as a folder named after it.")
        .build();
    location_group.add(&location);

    // Error texts hold paths, which are not markup.
    let status = adw::Banner::builder().use_markup(false).build();
    status.update_property(&[gtk::accessible::Property::Label("Save error")]);

    let page = adw::PreferencesPage::new();
    page.add(&measurement);
    page.add(&description_group);
    page.add(&range.group);
    page.add(&attachments.group);
    page.add(&location_group);

    let cancel = gtk::Button::builder().label("Cancel").build();
    let save = gtk::Button::builder()
        .label("Save")
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel);
    header.pack_end(&save);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&status);
    toolbar.set_content(Some(&page));
    let dialog = adw::Dialog::builder()
        .title("Save Measurement")
        .content_width(480)
        .content_height(640)
        .child(&toolbar)
        .build();
    let close = dialog.clone();
    cancel.connect_clicked(move |_| {
        close.close();
    });
    SaveDialog {
        dialog,
        name,
        description,
        range,
        attachments,
        folder: RefCell::new(folder),
        location,
        choose_location,
        status,
        save,
    }
}

/// Keeps the preview and the Save button in step with the name and range.
fn connect_range(view: &Rc<SaveDialog>, runner: &Rc<RefCell<MeasurementRunner>>) {
    let update = {
        let view = Rc::downgrade(view);
        let runner = Rc::clone(runner);
        move || {
            let Some(view) = view.upgrade() else { return };
            let range = view.range.selected();
            view.range
                .show_preview(range.map(|range| runner.borrow().range_summary(range)));
            view.save
                .set_sensitive(range.is_some() && !view.name.text().trim().is_empty());
        }
    };
    update();
    let on_from = update.clone();
    view.range.from.connect_selected_notify(move |_| on_from());
    let on_to = update.clone();
    view.range.to.connect_selected_notify(move |_| on_to());
    view.name.connect_changed(move |_| update());
}

fn connect_folder_picker(window: &adw::ApplicationWindow, view: &Rc<SaveDialog>) {
    let weak = Rc::downgrade(view);
    let window = window.clone();
    view.choose_location.connect_clicked(move |_| {
        let Some(view) = weak.upgrade() else { return };
        let file_dialog = gtk::FileDialog::builder()
            .title("Choose Measurement Folder")
            .initial_folder(&gtk::gio::File::for_path(&*view.folder.borrow()))
            .build();
        let weak = Rc::downgrade(&view);
        file_dialog.select_folder(
            Some(&window),
            None::<&gtk::gio::Cancellable>,
            move |result| {
                let Some(view) = weak.upgrade() else { return };
                if let Some(folder) = result.ok().and_then(|folder| folder.path()) {
                    view.location.set_subtitle(&folder.display().to_string());
                    *view.folder.borrow_mut() = folder;
                }
            },
        );
    });
}

fn connect_save(
    window: &adw::ApplicationWindow,
    view: &Rc<SaveDialog>,
    runner: &Rc<RefCell<MeasurementRunner>>,
    toasts: &adw::ToastOverlay,
) {
    let save = {
        let weak = Rc::downgrade(view);
        let runner = Rc::clone(runner);
        let toasts = toasts.clone();
        let window = window.downgrade();
        move || {
            let Some(view) = weak.upgrade() else { return };
            if let Err(error) = save_measurement(&view, &runner.borrow()) {
                view.status.set_title(&error);
                view.status.set_revealed(true);
                return;
            }
            toasts.add_toast(saved_toast(&view.saved_path(), window.clone()));
            view.dialog.close();
        }
    };
    let on_click = save.clone();
    view.save.connect_clicked(move |_| on_click());
    let weak = Rc::downgrade(view);
    view.name.connect_entry_activated(move |_| {
        if weak.upgrade().is_some_and(|view| view.save.is_sensitive()) {
            save();
        }
    });
}

fn save_measurement(view: &SaveDialog, runner: &MeasurementRunner) -> Result<(), String> {
    let name = view.name.text().trim().to_owned();
    let path = bundle_directory(&view.folder.borrow(), &name)?;
    let range = view
        .range
        .selected()
        .ok_or("The range must end after it starts.")?;
    let buffer = view.description.buffer();
    let description = buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string();
    runner
        .save_measurement_with_metadata(
            &path,
            SaveMetadata {
                name,
                description,
                range,
                attachments: view.attachments.attachments(),
            },
        )
        .map_err(|error| format!("Could not save: {error}"))
}

/// A toast confirming the save, with a button that opens the bundle folder.
fn saved_toast(
    path: &std::path::Path,
    window: glib::WeakRef<adw::ApplicationWindow>,
) -> adw::Toast {
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let toast = adw::Toast::builder()
        .title(format!("Saved “{name}”"))
        .button_label("Open Folder")
        .timeout(10)
        .build();
    let folder = gtk::gio::File::for_path(path);
    toast.connect_button_clicked(move |_| {
        let launcher = gtk::FileLauncher::new(Some(&folder));
        launcher.launch(
            window.upgrade().as_ref(),
            None::<&gtk::gio::Cancellable>,
            |_| {},
        );
    });
    toast
}

impl SaveDialog {
    fn saved_path(&self) -> PathBuf {
        self.folder.borrow().join(self.name.text().trim())
    }
}

fn default_save_folder() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Documents).unwrap_or_else(glib::home_dir)
}

pub(crate) fn bundle_directory(folder: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() {
        return Err("Measurement name is required.".to_owned());
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err("Measurement name cannot contain path separators.".to_owned());
    }
    Ok(folder.join(name))
}

#[cfg(test)]
mod tests {
    use super::bundle_directory;

    #[test]
    fn bundle_directory_accepts_a_name_and_rejects_path_components() {
        let folder = std::path::Path::new("/tmp/measurements");

        assert_eq!(
            bundle_directory(folder, "Courtyard survey"),
            Ok(folder.join("Courtyard survey"))
        );
        assert!(bundle_directory(folder, "").is_err());
        assert!(bundle_directory(folder, "../outside").is_err());
        assert!(bundle_directory(folder, "nested/name").is_err());
    }
}

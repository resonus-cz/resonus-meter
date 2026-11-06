//! The Attachments group: files saved with the measurement.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use adw::prelude::*;
use spl_engine::Attachment;

/// The chosen files and their rows.
pub(super) struct AttachmentRows {
    pub(super) group: adw::PreferencesGroup,
    add: gtk::Button,
    files: RefCell<Vec<PathBuf>>,
    rows: RefCell<Vec<adw::ActionRow>>,
}

impl AttachmentRows {
    pub(super) fn new() -> Rc<Self> {
        let add = gtk::Button::builder()
            .label("Add Files…")
            .valign(gtk::Align::Center)
            .build();
        add.update_property(&[gtk::accessible::Property::Label("Add attachment files")]);
        let group = adw::PreferencesGroup::builder()
            .title("Attachments")
            .description("Photos, datasheets or any other files, copied into the measurement.")
            .header_suffix(&add)
            .build();
        let rows = Rc::new(Self {
            group,
            add,
            files: RefCell::new(Vec::new()),
            rows: RefCell::new(Vec::new()),
        });
        rows.refresh();
        rows
    }

    /// Opens the file chooser from `window` when Add Files… is pressed.
    pub(super) fn connect_add(self: &Rc<Self>, window: &adw::ApplicationWindow) {
        let weak = Rc::downgrade(self);
        let window = window.clone();
        self.add.connect_clicked(move |_| {
            let dialog = gtk::FileDialog::builder().title("Add Attachments").build();
            let weak = weak.clone();
            dialog.open_multiple(
                Some(&window),
                None::<&gtk::gio::Cancellable>,
                move |result| {
                    let (Some(rows), Ok(files)) = (weak.upgrade(), result) else {
                        return;
                    };
                    let chosen = (0..files.n_items())
                        .filter_map(|index| files.item(index))
                        .filter_map(|item| item.downcast::<gtk::gio::File>().ok())
                        .filter_map(|file| file.path());
                    rows.files.borrow_mut().extend(chosen);
                    rows.refresh();
                },
            );
        });
    }

    /// Returns the files to save, titled by their file names without extension.
    pub(super) fn attachments(&self) -> Vec<Attachment> {
        self.files
            .borrow()
            .iter()
            .map(|path| Attachment {
                title: path
                    .file_stem()
                    .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
                source: path.clone(),
            })
            .collect()
    }

    fn refresh(self: &Rc<Self>) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
        let files = self.files.borrow().clone();
        let mut rows = self.rows.borrow_mut();
        if files.is_empty() {
            let row = adw::ActionRow::builder().title("No attachments").build();
            self.group.add(&row);
            rows.push(row);
        }
        for (index, path) in files.iter().enumerate() {
            let name = path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            let row = adw::ActionRow::builder()
                .title(&name)
                .subtitle(
                    path.parent()
                        .map(|folder| folder.display().to_string())
                        .unwrap_or_default(),
                )
                .build();
            let remove = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text("Remove")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            remove.update_property(&[gtk::accessible::Property::Label(&format!(
                "Remove attachment {name}"
            ))]);
            let weak = Rc::downgrade(self);
            remove.connect_clicked(move |_| {
                let Some(rows) = weak.upgrade() else { return };
                rows.files.borrow_mut().remove(index);
                // Rebuild after this handler returns; it belongs to a removed row.
                gtk::glib::idle_add_local_once(move || rows.refresh());
            });
            row.add_suffix(&remove);
            self.group.add(&row);
            rows.push(row);
        }
    }
}

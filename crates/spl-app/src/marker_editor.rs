//! Editing marker labels and notes (specification 5.3).

use std::{cell::RefCell, rc::Rc, time::Duration};

use adw::prelude::*;
use spl_engine::{Marker, MarkerKind};

use crate::meter::format_elapsed;
use crate::runner::MeasurementRunner;

/// Lists the session's markers with editable label and note; `focus` opens
/// one of them, for example the marker clicked on the plot.
pub(crate) fn present_marker_editor(
    parent: &impl IsA<gtk::Widget>,
    runner: &Rc<RefCell<MeasurementRunner>>,
    focus: Option<u64>,
) {
    let markers = runner
        .borrow()
        .log_history(None)
        .map(|history| history.markers)
        .unwrap_or_default();
    let group = adw::PreferencesGroup::builder()
        .title("Markers")
        .description("Press Enter or the check button to apply a change.")
        .build();
    let mut focus_row = None;
    for marker in &markers {
        let (expander, label) = marker_rows(marker, runner, &group);
        if focus == Some(marker.id) {
            expander.set_expanded(true);
            focus_row = Some(label);
        }
        group.add(&expander);
    }
    let page = adw::PreferencesPage::new();
    page.add(&group);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page));
    let dialog = adw::Dialog::builder()
        .title("Markers")
        .content_width(440)
        .content_height(520)
        .child(&toolbar)
        .build();
    dialog.present(Some(parent));
    if let Some(row) = focus_row {
        row.grab_focus();
    }
}

/// Returns the expander row of `marker` and its label entry.
fn marker_rows(
    marker: &Marker,
    runner: &Rc<RefCell<MeasurementRunner>>,
    group: &adw::PreferencesGroup,
) -> (adw::ExpanderRow, adw::EntryRow) {
    let expander = adw::ExpanderRow::builder()
        .title(&marker.label)
        .subtitle(format!(
            "{} · {}",
            format_elapsed(Duration::from_secs_f64(marker.t_s.max(0.0))),
            kind_name(marker.kind)
        ))
        .build();
    let label = adw::EntryRow::builder()
        .title("Label")
        .text(&marker.label)
        .show_apply_button(true)
        .build();
    label.update_property(&[gtk::accessible::Property::Label(&format!(
        "Label of marker {}",
        marker.label
    ))]);
    let note = adw::EntryRow::builder()
        .title("Note")
        .text(&marker.note)
        .show_apply_button(true)
        .build();
    note.update_property(&[gtk::accessible::Property::Label(&format!(
        "Note of marker {}",
        marker.label
    ))]);
    let apply = {
        let (id, runner, group) = (marker.id, Rc::clone(runner), group.clone());
        let (expander, label, note) = (expander.clone(), label.clone(), note.clone());
        move || {
            let text = label.text();
            let result = if text.trim().is_empty() {
                Err("A marker needs a label.".to_owned())
            } else {
                runner
                    .borrow_mut()
                    .edit_marker(id, text.trim(), &note.text())
            };
            match result {
                Ok(()) => {
                    expander.set_title(text.trim());
                    group.set_description(Some("Saved with the measurement."));
                }
                Err(error) => group.set_description(Some(&format!("Not changed: {error}"))),
            }
        }
    };
    let apply_label = apply.clone();
    label.connect_apply(move |_| apply_label());
    note.connect_apply(move |_| apply());
    expander.add_row(&label);
    expander.add_row(&note);
    (expander, label)
}

const fn kind_name(kind: MarkerKind) -> &'static str {
    match kind {
        MarkerKind::Start => "start",
        MarkerKind::Stop => "stop",
        MarkerKind::User => "marker",
        MarkerKind::Reset => "reset",
        MarkerKind::IntervalChange => "interval change",
        MarkerKind::Pause => "pause",
        MarkerKind::Resume => "resume",
    }
}

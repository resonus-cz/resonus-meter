use super::{PreferencesContext, string_list};
use adw::prelude::*;
use spl_engine::JOURNAL_INTERVAL_CHOICES_S;

const JOURNAL_INTERVAL_LABELS: [&str; 3] = ["10 seconds", "1 minute", "5 minutes"];
const JOURNAL_SUBTITLE: &str =
    "At most this much recording is lost if the app or system stops unexpectedly.";

/// General preferences: how often the session journal is written.
pub(crate) fn general_page(context: &PreferencesContext) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .name("general")
        .title("General")
        .icon_name("preferences-other-symbolic")
        .build();
    let recording = adw::PreferencesGroup::builder()
        .title("Recording")
        .description(
            "Unsaved measurements are journaled to disk, so they can be recovered after a crash.",
        )
        .build();
    let labels = JOURNAL_INTERVAL_LABELS.map(str::to_owned);
    let current = context.settings.borrow().journal_interval_s();
    let selected = JOURNAL_INTERVAL_CHOICES_S
        .iter()
        .position(|&choice| choice == current)
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(1);
    let interval = adw::ComboRow::builder()
        .title("Write journal every")
        .subtitle(JOURNAL_SUBTITLE)
        .model(&string_list(&labels))
        .selected(selected)
        .build();
    interval.update_property(&[gtk::accessible::Property::Label("Journal write interval")]);
    let settings = std::rc::Rc::clone(&context.settings);
    interval.connect_selected_notify(move |row| {
        let Some(&choice) = usize::try_from(row.selected())
            .ok()
            .and_then(|index| JOURNAL_INTERVAL_CHOICES_S.get(index))
        else {
            return;
        };
        let subtitle = match settings.borrow_mut().set_journal_interval_s(choice) {
            Ok(()) => JOURNAL_SUBTITLE.to_owned(),
            Err(error) => format!("Applies until the app closes; not saved: {error}"),
        };
        row.set_subtitle(&subtitle);
    });
    recording.add(&interval);
    page.add(&recording);
    page
}

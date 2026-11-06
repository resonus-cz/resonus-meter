//! The Range group: the part of the session to save, with a preview.

use std::time::Duration;

use adw::prelude::*;
use spl_engine::{LogHistory, Marker, SaveRange, SegmentAggregate};

use crate::meter::format_elapsed;
use crate::preferences::string_list;

const END_OF_RECORDING: &str = "End of recording";

/// From/To selectors over the session's markers and the preview row.
pub(super) struct RangeRows {
    pub(super) group: adw::PreferencesGroup,
    pub(super) from: adw::ComboRow,
    pub(super) to: adw::ComboRow,
    pub(super) preview: adw::ActionRow,
    markers: Vec<Marker>,
    duration_s: f64,
}

impl RangeRows {
    pub(super) fn new(history: Option<&LogHistory>) -> Self {
        let markers = history
            .map(|history| history.markers.clone())
            .unwrap_or_default();
        let duration_s = history.map_or(0.0, |history| history.duration_s);
        let labels = markers.iter().map(marker_label).collect::<Vec<_>>();
        let from = adw::ComboRow::builder()
            .title("From")
            .model(&string_list(&labels))
            .build();
        from.update_property(&[gtk::accessible::Property::Label("Range start marker")]);
        let mut to_labels = labels;
        to_labels.push(END_OF_RECORDING.to_owned());
        let to = adw::ComboRow::builder()
            .title("To")
            .model(&string_list(&to_labels))
            .selected(u32::try_from(markers.len()).unwrap_or(0))
            .build();
        to.update_property(&[gtk::accessible::Property::Label("Range end marker")]);
        let preview = adw::ActionRow::builder().title("Saved values").build();
        preview.update_property(&[gtk::accessible::Property::Label("Saved range values")]);
        let group = adw::PreferencesGroup::builder()
            .title("Range")
            .description("Save the whole recording or only the part between two markers.")
            .build();
        group.add(&from);
        group.add(&to);
        group.add(&preview);
        Self {
            group,
            from,
            to,
            preview,
            markers,
            duration_s,
        }
    }

    /// Returns the selected range, or `None` when the end does not follow the start.
    pub(super) fn selected(&self) -> Option<SaveRange> {
        let from = usize::try_from(self.from.selected()).ok()?;
        let to = usize::try_from(self.to.selected()).ok()?;
        if to <= from {
            return None;
        }
        Some(SaveRange {
            start_marker_id: (from > 0)
                .then(|| self.markers.get(from).map(|marker| marker.id))
                .flatten(),
            end_marker_id: self.markers.get(to).map(|marker| marker.id),
        })
    }

    /// Shows the preview for `summary`, or why the range cannot be saved.
    pub(super) fn show_preview(&self, summary: Option<Result<SegmentAggregate, String>>) {
        let text = match summary {
            None => "The end must come after the start.".to_owned(),
            Some(Err(error)) => format!("Cannot save this range: {error}"),
            Some(Ok(summary)) => preview_text(&summary, self.span_s()),
        };
        self.preview.set_subtitle(&text);
        self.preview
            .update_property(&[gtk::accessible::Property::Description(&text)]);
    }

    fn span_s(&self) -> f64 {
        let time = |index: u32| {
            usize::try_from(index)
                .ok()
                .and_then(|index| self.markers.get(index))
                .map_or(self.duration_s, |marker| marker.t_s)
        };
        (time(self.to.selected()) - time(self.from.selected())).max(0.0)
    }
}

fn marker_label(marker: &Marker) -> String {
    format!(
        "{} · {}",
        marker.label,
        format_elapsed(Duration::from_secs_f64(marker.t_s.max(0.0)))
    )
}

fn preview_text(summary: &SegmentAggregate, span_s: f64) -> String {
    let level = |value: Option<f64>| value.map_or_else(|| "–".to_owned(), |db| format!("{db:.1}"));
    format!(
        "{} · LAeq {} dB · LCeq {} dB · LZeq {} dB{}",
        format_elapsed(Duration::from_secs_f64(span_s)),
        level(summary.values.a.leq_db),
        level(summary.values.c.leq_db),
        level(summary.values.z.leq_db),
        if summary.clipped { " · overload" } else { "" }
    )
}

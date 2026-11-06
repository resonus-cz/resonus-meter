//! Window layout of the Re:sonus stack (ADR 0017; the stack's app
//! architecture, section 2, decision D8): header brand and centred title, a ribbon of titled groups,
//! and a view switcher over the tab content in an accent-framed panel.
//!
//! The Rust counterpart of the app template's `gui/layout.py`; the CSS is the same.

use adw::prelude::*;

/// Ribbon, captions, brand and the accent frame around the tab content (the stack's
/// design choices, resonus-room D-20...D-23). The brand's 5px margin lines its text
/// up with the ribbon's first button.
const LAYOUT_CSS: &str = "
.resonus-ribbon { padding: 6px 12px 3px 12px; }
.resonus-ribbon .ribbon-caption { font-size: smaller; opacity: 0.6; }
.resonus-brand { font-family: Lexend; font-weight: 600; font-size: 1.2em; margin-left: 5px; }
.resonus-panel {
  border: 1px solid color-mix(in srgb, var(--accent-bg-color) 45%, transparent);
  border-radius: 8px;
}
";

/// Loads [`LAYOUT_CSS`] for the default display, once per process.
pub(crate) fn install_layout_css() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(LAYOUT_CSS);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}

/// Returns the header-bar brand: the plot style's brand text in Lexend semibold.
pub(crate) fn brand_label() -> gtk::Label {
    crate::fonts::install_fonts();
    install_layout_css();
    let label = gtk::Label::new(Some(&crate::style::palette(false).brand_text));
    label.add_css_class("resonus-brand");
    label
}

/// Returns a flat ribbon button with icon and label.
pub(crate) fn ribbon_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(
            &adw::ButtonContent::builder()
                .icon_name(icon_name)
                .label(label)
                .use_underline(true)
                .build(),
        )
        .build();
    button.add_css_class("flat");
    button.update_property(&[gtk::accessible::Property::Label(&label.replace('_', ""))]);
    button
}

/// Returns the grid of one ribbon group (rows of equal height).
pub(crate) fn ribbon_grid() -> gtk::Grid {
    gtk::Grid::builder()
        .row_spacing(3)
        .column_spacing(6)
        .row_homogeneous(true)
        .build()
}

/// Returns one ribbon group: `content` with the caption `title` below.
fn ribbon_group(title: &str, content: &impl IsA<gtk::Widget>) -> gtk::Box {
    let caption = gtk::Label::new(Some(title));
    caption.add_css_class("ribbon-caption");
    let group = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    group.append(content);
    group.append(&caption);
    group
}

/// Returns the ribbon: the `(title, content)` groups left to right with separators.
///
/// The contents share one vertical size group, so all have the height of the
/// tallest and the captions line up. A narrow window scrolls the ribbon
/// horizontally instead of squeezing it.
pub(crate) fn ribbon(groups: &[(&str, gtk::Widget)]) -> gtk::ScrolledWindow {
    install_layout_css();
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    row.add_css_class("resonus-ribbon");
    let sizes = gtk::SizeGroup::new(gtk::SizeGroupMode::Vertical);
    for (index, (title, content)) in groups.iter().enumerate() {
        if index > 0 {
            row.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        }
        sizes.add_widget(content);
        row.append(&ribbon_group(title, content));
    }
    gtk::ScrolledWindow::builder()
        .child(&row)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .build()
}

/// Returns the wide view switcher over `stack` and the panel holding the stack.
///
/// The panel has the CSS class `resonus-panel` and 12 px margins; the switcher is
/// left-aligned above it (at narrow widths a bottom switcher bar replaces it,
/// see `build_ui`).
pub(crate) fn tabbed_panel(stack: &adw::ViewStack) -> (adw::ViewSwitcher, gtk::Box) {
    install_layout_css();
    let switcher = adw::ViewSwitcher::builder()
        .stack(stack)
        .policy(adw::ViewSwitcherPolicy::Wide)
        .halign(gtk::Align::Start)
        .margin_start(12)
        .margin_end(12)
        .margin_top(3)
        .margin_bottom(6)
        .build();
    stack.set_vexpand(true);
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(12)
        .overflow(gtk::Overflow::Hidden)
        .build();
    panel.add_css_class("resonus-panel");
    panel.append(stack);
    (switcher, panel)
}

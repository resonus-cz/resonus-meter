//! Plot text and line widths in the stack's plot style (ADR 0017): Pango layouts in
//! the token font (Lexend) at the token sizes, so the meter's plots match the Python
//! apps' matplotlib plots.

use crate::style::Palette;

/// Device pixels per point: Pango-Cairo and GTK use 96 px per inch, a point is 1/72 in.
pub(crate) const PX_PER_PT: f64 = 96.0 / 72.0;

/// Horizontal or vertical anchor of a text: the fraction of its extent left of (or
/// above) the given point. 0 = left/top, 0.5 = centre, 1 = right/bottom.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor(pub(crate) f64, pub(crate) f64);

impl Anchor {
    pub(crate) const LEFT_TOP: Self = Self(0.0, 0.0);
    pub(crate) const LEFT_MIDDLE: Self = Self(0.0, 0.5);
    pub(crate) const RIGHT_MIDDLE: Self = Self(1.0, 0.5);
    pub(crate) const CENTRE_TOP: Self = Self(0.5, 0.0);
    pub(crate) const CENTRE_BOTTOM: Self = Self(0.5, 1.0);
    pub(crate) const RIGHT_BOTTOM: Self = Self(1.0, 1.0);
}

/// Returns a layout of `text` in the palette's font at `size_pt`, semibold if asked.
fn layout(
    context: &gtk::cairo::Context,
    palette: &Palette,
    text: &str,
    size_pt: f64,
    semibold: bool,
) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(context);
    let mut font = pango::FontDescription::new();
    font.set_family(&palette.font_family);
    #[allow(clippy::cast_possible_truncation)]
    font.set_size((size_pt * f64::from(pango::SCALE)).round() as i32);
    if semibold {
        font.set_weight(pango::Weight::Semibold);
    }
    layout.set_font_description(Some(&font));
    layout.set_text(text);
    layout
}

/// Returns the width and height of `text` in plot text size, in pixels.
#[cfg(test)]
fn text_size(context: &gtk::cairo::Context, palette: &Palette, text: &str) -> (f64, f64) {
    let (width, height) = layout(context, palette, text, palette.font_size_pt, false).pixel_size();
    (f64::from(width), f64::from(height))
}

/// Draws `text` in plot text size and the current source colour at (`x`, `y`) by `anchor`.
pub(crate) fn draw_text(
    context: &gtk::cairo::Context,
    palette: &Palette,
    text: &str,
    x: f64,
    y: f64,
    anchor: Anchor,
) {
    show(
        context,
        &layout(context, palette, text, palette.font_size_pt, false),
        x,
        y,
        anchor,
    );
}

/// Draws the plot mark (brand text, semibold, brand colour) with its bottom right
/// corner at (`x`, `y`); the plots put it in their bottom right corner, as
/// resonus-core `style.add_brand` does.
pub(crate) fn draw_mark(context: &gtk::cairo::Context, palette: &Palette, x: f64, y: f64) {
    palette.brand.source(context);
    let mark = layout(
        context,
        palette,
        &palette.brand_text,
        palette.brand_size_pt,
        true,
    );
    show(context, &mark, x, y, Anchor::RIGHT_BOTTOM);
}

fn show(context: &gtk::cairo::Context, layout: &pango::Layout, x: f64, y: f64, anchor: Anchor) {
    let (width, height) = layout.pixel_size();
    context.move_to(
        x - anchor.0 * f64::from(width),
        y - anchor.1 * f64::from(height),
    );
    pangocairo::functions::show_layout(context, layout);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_measured_in_the_token_size() {
        let surface =
            gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 200, 50).expect("surface");
        let context = gtk::cairo::Context::new(&surface).expect("context");
        let palette = crate::style::palette(false);
        let (width, height) = text_size(&context, palette, "-30");
        // 10 pt = 13.3 px: a line of text is taller than the size, a short label wider than 0
        assert!(width > 0.0);
        assert!(height >= palette.font_size_pt * PX_PER_PT, "{height}");
        let (wide, _) = text_size(&context, palette, "-30-30");
        assert!(wide > width);
    }
}

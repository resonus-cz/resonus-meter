//! Plot style of the Re:sonus stack: the style tokens file of resonus-core (ADR 0017).
//!
//! `data/style_tokens.toml` is a copy of `resonus_core/assets/style_tokens.toml` at
//! [`TOKENS_SOURCE`] (schema version 1, resonus-core decision D-T1). Never edit
//! the copy: change the file in resonus-core, then copy it again and update the pin.

use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

/// resonus-core tag the vendored tokens file was copied from (checked by a test).
#[cfg(test)]
const TOKENS_SOURCE: &str = "v0.10.0";
/// Path of the tokens file in the resonus-core repository.
#[cfg(test)]
const TOKENS_PATH_IN_CORE: &str = "resonus_core/assets/style_tokens.toml";
/// The vendored tokens file.
pub(crate) const TOKENS_TOML: &str = include_str!("../data/style_tokens.toml");
/// Newest tokens schema this app reads.
const SCHEMA_VERSION: i64 = 1;

/// An sRGB colour with components in 0...1, for Cairo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rgb {
    pub(crate) r: f64,
    pub(crate) g: f64,
    pub(crate) b: f64,
}

impl Rgb {
    /// Parses `#rrggbb` (either case).
    fn parse(text: &str) -> Result<Self, String> {
        let hex = text
            .strip_prefix('#')
            .filter(|hex| hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| format!("style tokens: {text:?} is not a colour '#rrggbb'"))?;
        let channel =
            |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map(|v| f64::from(v) / 255.0);
        Ok(Self {
            r: channel(0).map_err(|e| e.to_string())?,
            g: channel(2).map_err(|e| e.to_string())?,
            b: channel(4).map_err(|e| e.to_string())?,
        })
    }

    /// Sets this colour as the Cairo source.
    pub(crate) fn source(self, context: &gtk::cairo::Context) {
        context.set_source_rgb(self.r, self.g, self.b);
    }
}

/// One variant (light or dark) of the plot style.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Palette {
    /// Font family of all plot text (`"Lexend"`).
    pub(crate) font_family: String,
    /// Trace colours in order.
    pub(crate) traces: Vec<Rgb>,
    /// Plot background.
    pub(crate) background: Rgb,
    /// Grid lines.
    pub(crate) grid: Rgb,
    /// Axes, tick labels and other text.
    pub(crate) axes: Rgb,
    /// Averages, references and markers.
    pub(crate) neutral: Rgb,
    /// Plot mark.
    pub(crate) brand: Rgb,
    /// Default plot-mark text (`"Re:sonus"`).
    pub(crate) brand_text: String,
    /// Text size in points.
    pub(crate) font_size_pt: f64,
    /// Plot-mark size in points.
    pub(crate) brand_size_pt: f64,
    /// Trace width in points.
    pub(crate) line_width: f64,
    /// Grid line width in points.
    pub(crate) grid_width: f64,
}

#[derive(Deserialize)]
struct File {
    version: i64,
    font_family: String,
    trace_colors: Vec<String>,
    tokens: BTreeMap<String, toml::Value>,
    #[serde(default)]
    dark: Option<DarkTable>,
}

#[derive(Deserialize)]
struct DarkTable {
    #[serde(default)]
    trace_colors: Option<Vec<String>>,
    #[serde(default)]
    tokens: BTreeMap<String, toml::Value>,
}

/// Reads a tokens file (`dark`: with the `[dark]` overrides).
///
/// # Errors
///
/// Returns a message for a parse error, a newer schema version, or a missing or
/// malformed token.
pub(crate) fn parse(text: &str, dark: bool) -> Result<Palette, String> {
    let file: File = toml::from_str(text).map_err(|e| format!("style tokens: {e}"))?;
    if file.version > SCHEMA_VERSION {
        return Err(format!(
            "style tokens: version {} is newer than {SCHEMA_VERSION}",
            file.version
        ));
    }
    let mut tokens = file.tokens;
    let mut traces = file.trace_colors;
    if dark && let Some(table) = file.dark {
        tokens.extend(table.tokens);
        if let Some(colours) = table.trace_colors {
            traces = colours;
        }
    }
    let text = |key: &str| {
        tokens
            .get(key)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("style tokens: missing tokens.{key}"))
    };
    let colour = |key: &str| text(key).and_then(|value| Rgb::parse(&value));
    let size = |key: &str| {
        tokens
            .get(key)
            .and_then(|v| {
                v.as_float().or_else(|| {
                    v.as_integer()
                        .and_then(|i| i32::try_from(i).ok())
                        .map(f64::from)
                })
            })
            .filter(|v| *v > 0.0)
            .ok_or_else(|| format!("style tokens: tokens.{key} must be a positive number"))
    };
    if traces.is_empty() {
        return Err("style tokens: trace_colors must be a non-empty list".to_owned());
    }
    Ok(Palette {
        font_family: file.font_family,
        traces: traces
            .iter()
            .map(|c| Rgb::parse(c))
            .collect::<Result<_, _>>()?,
        background: colour("background")?,
        grid: colour("grid_color")?,
        axes: colour("axes_color")?,
        neutral: colour("neutral_color")?,
        brand: colour("brand_color")?,
        brand_text: text("brand_text")?,
        font_size_pt: size("font_size")?,
        brand_size_pt: size("brand_size")?,
        line_width: size("line_width")?,
        grid_width: size("grid_width")?,
    })
}

/// The bundled style: light or dark.
pub(crate) fn palette(dark: bool) -> &'static Palette {
    static LIGHT: OnceLock<Palette> = OnceLock::new();
    static DARK: OnceLock<Palette> = OnceLock::new();
    let cell = if dark { &DARK } else { &LIGHT };
    cell.get_or_init(|| parse(TOKENS_TOML, dark).expect("the bundled style tokens are valid"))
}

/// The style for the current light/dark appearance of libadwaita.
pub(crate) fn current() -> &'static Palette {
    palette(adw::StyleManager::default().is_dark())
}

#[cfg(test)]
mod tests {
    use super::{Rgb, TOKENS_PATH_IN_CORE, TOKENS_SOURCE, TOKENS_TOML, palette, parse};

    fn hex(text: &str) -> Rgb {
        Rgb::parse(text).expect("valid colour")
    }

    #[test]
    fn the_bundled_light_and_dark_styles() {
        let light = palette(false);
        assert_eq!(light.font_family, "Lexend");
        assert_eq!(light.traces.len(), 7);
        assert_eq!(light.traces[3], hex("#e66100"));
        assert_eq!(light.background, hex("#ffffff"));
        assert_eq!(light.neutral, hex("#3d3846"));
        assert_eq!(light.brand_text, "Re:sonus");
        assert!((light.font_size_pt - 10.0).abs() < 1e-12);
        let dark = palette(true);
        assert_eq!(dark.background, hex("#1d1d20"));
        assert_eq!(dark.axes, hex("#deddda"));
        assert_eq!(dark.traces[4], hex("#c061cb"));
        assert_eq!(dark.traces[0], light.traces[0]);
        assert!((dark.line_width - light.line_width).abs() < 1e-12);
    }

    #[test]
    fn colours_parse_exactly() {
        assert_eq!(
            hex("#FF0080"),
            Rgb {
                r: 1.0,
                g: 0.0,
                b: 128.0 / 255.0
            }
        );
        assert!(Rgb::parse("red").is_err());
        assert!(Rgb::parse("#12345").is_err());
    }

    #[test]
    fn a_newer_schema_is_rejected() {
        let newer = TOKENS_TOML.replacen("version = 1", "version = 2", 1);
        assert!(parse(&newer, false).unwrap_err().contains("newer"));
    }

    /// The vendored copy equals resonus-core's file at the pinned commit, when
    /// the resonus-core repository is checked out next to this one.
    #[test]
    fn the_copy_matches_resonus_core_at_the_pin() {
        let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../resonus-core");
        if !core.join(".git").exists() {
            eprintln!("resonus-core not found next to this repository; check skipped");
            return;
        }
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&core)
            .arg("show")
            .arg(format!("{TOKENS_SOURCE}:{TOKENS_PATH_IN_CORE}"))
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "pin {TOKENS_SOURCE} not found in resonus-core"
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), TOKENS_TOML);
    }
}

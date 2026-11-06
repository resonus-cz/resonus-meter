//! The bundled Lexend fonts (SIL OFL 1.1, `data/fonts/lexend`), registered with Pango.
//!
//! Pango loads fonts from files only, so the embedded files are written once to
//! the user cache folder and added to the default font map (Pango >= 1.56,
//! ADR 0017). Without them, GTK falls back to the default font.

use std::path::{Path, PathBuf};

use gtk::glib;
use pango::prelude::FontMapExt;

/// The embedded font files: name and contents.
const FONT_FILES: [(&str, &[u8]); 3] = [
    (
        "Lexend-Regular.ttf",
        include_bytes!("../data/fonts/lexend/Lexend-Regular.ttf"),
    ),
    (
        "Lexend-Medium.ttf",
        include_bytes!("../data/fonts/lexend/Lexend-Medium.ttf"),
    ),
    (
        "Lexend-SemiBold.ttf",
        include_bytes!("../data/fonts/lexend/Lexend-SemiBold.ttf"),
    ),
];

/// Writes the embedded font files to `folder` unless a file of the same size is there.
///
/// # Errors
///
/// Returns the error of creating the folder or writing a file.
pub(crate) fn write_fonts(folder: &Path) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(folder)?;
    FONT_FILES
        .iter()
        .map(|(name, bytes)| {
            let path = folder.join(name);
            let current = std::fs::metadata(&path).map(|m| m.len()).ok();
            if current != Some(bytes.len() as u64) {
                std::fs::write(&path, bytes)?;
            }
            Ok(path)
        })
        .collect()
}

/// Registers the bundled fonts with Pango's default font map, once per process.
pub(crate) fn install_fonts() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let folder = glib::user_cache_dir()
            .join(crate::APPLICATION_ID)
            .join("fonts");
        let font_map = pangocairo::FontMap::default();
        match write_fonts(&folder) {
            Ok(paths) => {
                for path in paths {
                    if let Err(error) = font_map.add_font_file(&path) {
                        eprintln!("Font {} not loaded: {error}", path.display());
                    }
                }
            }
            Err(error) => eprintln!("Fonts not written to {}: {error}", folder.display()),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::write_fonts;

    #[test]
    fn writes_the_three_weights_once() {
        let folder = std::env::temp_dir().join(format!("spl-app-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let paths = write_fonts(&folder).expect("write");
        let names: Vec<_> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "Lexend-Regular.ttf",
                "Lexend-Medium.ttf",
                "Lexend-SemiBold.ttf"
            ]
        );
        let modified = std::fs::metadata(&paths[0]).unwrap().modified().unwrap();
        write_fonts(&folder).expect("second write");
        assert_eq!(
            std::fs::metadata(&paths[0]).unwrap().modified().unwrap(),
            modified
        );
        std::fs::remove_dir_all(folder).expect("clean up");
    }
}

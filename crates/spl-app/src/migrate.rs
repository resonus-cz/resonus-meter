//! One-time move of the folders kept under the former application id (ADR 0016).
//!
//! The application id changed from `io.github.vyhyb.SPLMeter` to
//! `cz.resonus.SplMeter` before the first release. Settings, the calibration
//! history (config folder) and session journals (state folder) are kept in
//! folders named after the id, so they are moved once at startup.

use std::{
    io,
    path::{Path, PathBuf},
};

/// Application id used before ADR 0016; only read for the move.
pub(crate) const LEGACY_APPLICATION_ID: &str = "io.github.vyhyb.SPLMeter";

/// Moves `base/legacy` to `base/current` when only the legacy folder exists.
///
/// Returns the new path when a folder was moved, `None` when there was nothing
/// to move or the new folder already exists (it is never overwritten; the
/// legacy folder is then left in place).
///
/// # Errors
///
/// Returns the error of [`std::fs::rename`], for example when the two folders
/// are on different file systems.
pub(crate) fn migrate_folder(
    base: &Path,
    legacy: &str,
    current: &str,
) -> io::Result<Option<PathBuf>> {
    let old = base.join(legacy);
    let new = base.join(current);
    if !old.is_dir() || new.exists() {
        return Ok(None);
    }
    std::fs::rename(&old, &new)?;
    Ok(Some(new))
}

#[cfg(test)]
mod tests {
    use super::migrate_folder;
    use std::fs;

    fn scratch(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("spl-app-migrate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create scratch folder");
        root
    }

    #[test]
    fn moves_the_legacy_folder_with_its_files() {
        let root = scratch("move");
        fs::create_dir(root.join("old.App")).expect("legacy folder");
        fs::write(
            root.join("old.App").join("calibrations.toml"),
            "version = 1\n",
        )
        .expect("file");

        let moved = migrate_folder(&root, "old.App", "new.App").expect("move");

        assert_eq!(moved, Some(root.join("new.App")));
        assert!(!root.join("old.App").exists());
        assert_eq!(
            fs::read_to_string(root.join("new.App").join("calibrations.toml")).expect("read"),
            "version = 1\n"
        );
        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn never_overwrites_an_existing_folder() {
        let root = scratch("keep");
        fs::create_dir(root.join("old.App")).expect("legacy folder");
        fs::create_dir(root.join("new.App")).expect("current folder");
        fs::write(root.join("new.App").join("settings.toml"), "new").expect("file");

        assert_eq!(
            migrate_folder(&root, "old.App", "new.App").expect("no move"),
            None
        );
        assert!(root.join("old.App").is_dir());
        assert_eq!(
            fs::read_to_string(root.join("new.App").join("settings.toml")).expect("read"),
            "new"
        );
        fs::remove_dir_all(root).expect("clean up");
    }

    #[test]
    fn nothing_to_move() {
        let root = scratch("none");
        assert_eq!(
            migrate_folder(&root, "old.App", "new.App").expect("no move"),
            None
        );
        assert!(!root.join("new.App").exists());
        fs::remove_dir_all(root).expect("clean up");
    }
}

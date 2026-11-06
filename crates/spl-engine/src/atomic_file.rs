//! Crash-safe replacement of small files.

use std::fs;
use std::io::Write;
use std::path::Path;

/// Writes to a sibling temporary file, fsyncs it, then renames it over `path`,
/// so a crash leaves either the old or the new contents, never a mix.
pub(crate) fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)?;
    }
    let mut temporary_name = path.file_name().unwrap_or_default().to_owned();
    temporary_name.push(".tmp");
    let temporary = path.with_file_name(temporary_name);
    let mut file = fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}

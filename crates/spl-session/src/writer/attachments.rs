//! Copying attachments into a bundle and hashing them.

use std::{
    collections::HashSet,
    fmt::Write as _,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

use sha2::{Digest, Sha256};

use super::SessionError;
use crate::Attachment;

/// An attachment as listed under `files: attachments:`.
pub(crate) struct WrittenAttachment {
    /// Path relative to the bundle folder.
    pub(crate) file: String,
    pub(crate) title: String,
    /// Lower-case hexadecimal SHA-256 of the copied file.
    pub(crate) sha256: String,
}

/// Copies `attachments` into `bundle_path/attachments/`, renaming clashing
/// file names `name-2.ext`, `name-3.ext`, …
pub(super) fn copy_attachments(
    bundle_path: &Path,
    attachments: &[Attachment],
) -> Result<Vec<WrittenAttachment>, SessionError> {
    if attachments.is_empty() {
        return Ok(Vec::new());
    }
    let folder = bundle_path.join("attachments");
    fs::create_dir_all(&folder).map_err(|source| io_error(&folder, source))?;
    let mut used = HashSet::new();
    attachments
        .iter()
        .map(|attachment| {
            let name = unique_name(&attachment.source, &mut used);
            let sha256 = copy_hashed(&attachment.source, &folder.join(&name))?;
            Ok(WrittenAttachment {
                file: format!("attachments/{name}"),
                title: attachment.title.clone(),
                sha256,
            })
        })
        .collect()
}

fn unique_name(source: &Path, used: &mut HashSet<String>) -> String {
    let name = source.file_name().map_or_else(
        || "attachment".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let stem = source
        .file_stem()
        .map_or_else(|| name.clone(), |stem| stem.to_string_lossy().into_owned());
    let extension = source
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let mut candidate = name;
    let mut number = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{stem}-{number}{extension}");
        number += 1;
    }
    candidate
}

/// Copies `source` to `target`, syncs it, and returns its SHA-256.
fn copy_hashed(source: &Path, target: &Path) -> Result<String, SessionError> {
    let mut input = File::open(source).map_err(|error| io_error(source, error))?;
    let mut output = File::create(target).map_err(|error| io_error(target, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|error| io_error(source, error))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|error| io_error(target, error))?;
    }
    output.sync_all().map_err(|error| io_error(target, error))?;
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        // Writing to a String cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

fn io_error(path: &Path, source: std::io::Error) -> SessionError {
    SessionError::Io {
        path: path.to_owned(),
        source,
    }
}

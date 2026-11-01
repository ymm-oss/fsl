// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Publish complete artifact bytes without truncating the existing output.
/// Parent creation and error framing remain the caller's responsibility.
pub(super) fn publish(path: &Path, content: impl AsRef<[u8]>) -> io::Result<()> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let permissions = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            // Do not follow a final symlink or replace a FIFO/device/directory.
            if !metadata.file_type().is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "output target is not a regular file",
                ));
            }
            let permissions = metadata.permissions();
            if permissions.readonly() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "output target is read-only",
                ));
            }
            // Replacement must not bypass the existing file's write access.
            // Opening without truncate checks access without changing bytes.
            OpenOptions::new().write(true).open(path)?;
            Some(permissions)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(
        ".fslc-output-{}-{sequence}.tmp",
        std::process::id()
    ));
    // create_new establishes ownership: never remove a pre-existing entry if
    // creation fails. Default creation permissions retain the ordinary umask.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let prepared = file.write_all(content.as_ref()).and_then(|()| {
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        Ok(())
    });
    // Close before rename or cleanup, including on platforms that require it.
    drop(file);
    let published = prepared.and_then(|()| fs::rename(&temporary, path));
    if published.is_err() {
        // Cleanup is best-effort; retain the original write/permission/rename
        // error even if removing our own temporary file also fails.
        let _ = fs::remove_file(&temporary);
    }
    published
}

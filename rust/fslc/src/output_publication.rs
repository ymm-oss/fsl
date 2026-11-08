// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
            ensure_writable(path, &permissions)?;
            Some(permissions)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let temporary = temporary_path(path, std::process::id(), nanos, sequence);
    // create_new establishes ownership: never remove a pre-existing entry if
    // creation fails.
    let file = open_temporary(&temporary, permissions.is_some())?;
    let prepared = write_private(&file, permissions, content.as_ref());
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

/// Refuse to replace an existing regular file this process cannot write.
/// A rename onto the destination needs only parent-directory access, so every
/// replacing writer (this helper and `migrate --write`) checks the file itself.
pub(super) fn ensure_writable(path: &Path, permissions: &fs::Permissions) -> io::Result<()> {
    if permissions.readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "output target is read-only",
        ));
    }
    // Opening without truncate checks access without changing bytes.
    OpenOptions::new().write(true).open(path)?;
    Ok(())
}

fn temporary_path(path: &Path, process_id: u32, nanos: u128, sequence: u64) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = parent.join(format!(".fslc-output-{process_id}-{nanos}-{sequence}.tmp"));
    // Never prepare bytes at the destination, even if its name matches ours.
    if temporary.file_name() == path.file_name() {
        temporary.as_mut_os_string().push(".tmp");
    }
    temporary
}

// There is no retry, so a stale entry is named for whoever has to remove it.
// Other creation errors keep the callers' existing message.
fn open_temporary(path: &Path, replacing: bool) -> io::Result<File> {
    create_temporary(path, replacing).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            io::Error::new(
                error.kind(),
                format!("temporary file '{}' already exists", path.display()),
            )
        } else {
            error
        }
    })
}

// A destination that exists can be narrower than the default mode, so its
// replacement is private from creation. A new output has no such bound, and
// creating it with the default mode is how its umask-derived bits are learned.
#[cfg(unix)]
fn create_temporary(path: &Path, replacing: bool) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    if replacing {
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(not(unix))]
fn create_temporary(path: &Path, _replacing: bool) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

// Partial bytes stay owner-only until the final bits are applied before rename.
// Only the permission bits carry over: a replacement is new content, and a
// direct write would have cleared setuid and setgid.
#[cfg(unix)]
fn write_private(
    mut file: &File,
    existing: Option<fs::Permissions>,
    content: &[u8],
) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = match existing {
        Some(permissions) => permissions.mode(),
        None => file.metadata()?.permissions().mode(),
    };
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(content)?;
    file.set_permissions(fs::Permissions::from_mode(mode & 0o777))
}

#[cfg(not(unix))]
fn write_private(
    mut file: &File,
    existing: Option<fs::Permissions>,
    content: &[u8],
) -> io::Result<()> {
    file.write_all(content)?;
    match existing {
        Some(permissions) => file.set_permissions(permissions),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../target/output-publication-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn temporary_name_excludes_the_destination_basename() {
        let ordinary = temporary_path(Path::new("out/artifact"), 7, 8, 9);
        assert_eq!(ordinary, Path::new("out/.fslc-output-7-8-9.tmp"));
        let matching = temporary_path(ordinary.as_path(), 7, 8, 9);
        assert_eq!(matching, Path::new("out/.fslc-output-7-8-9.tmp.tmp"));
        assert_ne!(matching, ordinary);
    }

    #[test]
    fn temporary_name_differs_between_restarts_of_one_process_id() {
        let first = temporary_path(Path::new("artifact"), 1, 100, 0);
        let restarted = temporary_path(Path::new("artifact"), 1, 200, 0);
        assert_ne!(first, restarted);
    }

    // Narrowing after creation would leave a window for a reader to keep an open
    // descriptor, so a replacement must be private from the moment it exists.
    #[cfg(unix)]
    #[test]
    fn replacement_temporary_is_private_from_creation() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = scratch("private");
        let temporary = directory.join("private.tmp");
        let file = create_temporary(&temporary, true).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn existing_temporary_entry_is_named_kept_and_not_written() {
        let directory = scratch("stale");
        let stale = directory.join("stale.tmp");
        fs::write(&stale, b"stale").unwrap();
        let error = open_temporary(&stale, false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(
            error.to_string().contains(&*stale.to_string_lossy()),
            "{error}"
        );
        assert_eq!(fs::read(&stale).unwrap(), b"stale");
        fs::remove_dir_all(&directory).unwrap();
    }
}

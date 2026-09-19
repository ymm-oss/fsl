// SPDX-License-Identifier: Apache-2.0

//! Placing a symbolic link, replacing one this tool made.
//!
//! Its own module because both the `current` pointer and the per-skill links
//! need it, and because it is the one place the Windows limit lives.

use std::path::Path;

/// Whether this platform provides the links a user install is built from.
///
/// A constant rather than a probe: [`place`] itself refuses on `not(unix)`,
/// so the two cannot disagree, and a caller can ask before it writes.
pub(super) const SUPPORTED: bool = cfg!(unix);

/// Point `link` at `target`, replacing whatever is there.
///
/// `replace_any` is what `--force` passes. Without it only a link this tool
/// could have made is replaced, so a real directory somebody else put there
/// is reported rather than removed.
///
/// # Errors
///
/// When something that is not a link is already at `link` and `replace_any`
/// is false, when the old entry cannot be removed, or when the platform has
/// no symbolic links here.
pub(super) fn place(link: &Path, target: &Path, replace_any: bool) -> Result<(), String> {
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    match std::fs::symlink_metadata(link) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::remove_file(link)
            .map_err(|error| format!("failed to replace {}: {error}", link.display()))?,
        Ok(metadata) if replace_any => {
            let removed = if metadata.is_dir() {
                std::fs::remove_dir_all(link)
            } else {
                std::fs::remove_file(link)
            };
            removed.map_err(|error| format!("failed to replace {}: {error}", link.display()))?;
        }
        Ok(_) => return Err(format!("{} is not a link of ours", link.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("failed to inspect {}: {error}", link.display())),
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
            .map_err(|error| format!("failed to link {}: {error}", link.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = target;
        Err(format!(
            "creating {} needs symbolic links, which this platform does not provide here",
            link.display()
        ))
    }
}

// `place` refuses outright on a platform without symbolic links here.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("fslc-symlink-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("create");
        base
    }

    #[test]
    fn a_link_is_created_where_nothing_was() {
        let base = temp_dir("create");
        let link = base.join("nested/link");
        place(&link, Path::new("/somewhere"), false).expect("place");
        assert_eq!(
            std::fs::read_link(&link).expect("read"),
            Path::new("/somewhere"),
            "a missing parent is created on the way"
        );
    }

    #[test]
    fn an_existing_link_is_repointed() {
        let base = temp_dir("repoint");
        let link = base.join("link");
        place(&link, Path::new("/first"), false).expect("place");
        place(&link, Path::new("/second"), false).expect("repoint");
        assert_eq!(
            std::fs::read_link(&link).expect("read"),
            Path::new("/second")
        );
    }

    /// A path behind a directory nobody may look into cannot be inspected.
    #[test]
    fn a_path_that_cannot_be_inspected_is_reported() {
        use std::os::unix::fs::PermissionsExt as _;

        let base = temp_dir("unreadable");
        let locked = base.join("locked");
        std::fs::create_dir_all(&locked).expect("create");
        let mut mode = std::fs::metadata(&locked).expect("read").permissions();
        mode.set_mode(0o000);
        std::fs::set_permissions(&locked, mode).expect("set");

        let outcome = place(&locked.join("link"), Path::new("/somewhere"), false);

        let mut mode = std::fs::metadata(&locked).expect("read").permissions();
        mode.set_mode(0o700);
        std::fs::set_permissions(&locked, mode).expect("restore");

        // Running as root defeats the setup; then there is nothing to assert.
        if let Err(error) = outcome {
            assert!(
                error.contains("failed to inspect") || error.contains("failed to create"),
                "{error}"
            );
        }
    }

    /// With `--force` behind it, a real directory is replaced.
    #[test]
    fn a_real_directory_is_replaced_when_the_caller_says_to() {
        let base = temp_dir("replace-dir");
        let path = base.join("skill");
        std::fs::create_dir_all(path.join("nested")).expect("create");
        std::fs::write(path.join("nested/theirs.md"), "mine\n").expect("write");

        place(&path, Path::new("/somewhere"), true).expect("force replaces it");
        assert_eq!(
            std::fs::read_link(&path).expect("read"),
            Path::new("/somewhere")
        );
    }

    /// A real file is somebody else's, so it is reported rather than replaced.
    #[test]
    fn a_real_file_is_refused_rather_than_replaced() {
        let base = temp_dir("refuse");
        let path = base.join("not-a-link");
        std::fs::write(&path, "mine\n").expect("write");

        let error = place(&path, Path::new("/somewhere"), false).expect_err("a file is not ours");
        assert!(error.contains("not a link"), "{error}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "mine\n",
            "the file must survive the refusal"
        );
    }
}

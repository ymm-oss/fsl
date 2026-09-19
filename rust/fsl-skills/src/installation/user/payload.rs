// SPDX-License-Identifier: Apache-2.0

//! The versioned payload a user install links into.
//!
//! Separate from the links themselves because the two answer different
//! questions. This module owns what is on disk under the data directory, and
//! which release `current` names. [`super`] owns where `~/.claude/skills`
//! points.

use std::path::{Path, PathBuf};

use crate::embedded::EMBEDDED_SKILL_FILES;

use super::symlink;

/// What writing the payload changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Outcome {
    /// How many payload files were written into the release directory.
    pub written: usize,
    /// Whether `current` was moved to this release.
    pub repointed: bool,
}

/// One release's payload under the data directory, and the `current` pointer.
pub(super) struct Payload {
    release_dir: PathBuf,
    skills_dir: PathBuf,
    current_link: PathBuf,
}

impl Payload {
    /// The payload for `release` under `data`.
    #[must_use]
    pub(super) fn new(data: &Path, release: &str, skills_subdir: &str) -> Self {
        let release_dir = data.join("releases").join(release);
        Self {
            skills_dir: release_dir.join(skills_subdir),
            current_link: data.join("current"),
            release_dir,
        }
    }

    /// Where this release's skills live.
    #[must_use]
    pub(super) fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    /// Where a link should point to reach the active release's skills.
    #[must_use]
    pub(super) fn current_skills(data: &Path, skills_subdir: &str) -> PathBuf {
        data.join("current").join(skills_subdir)
    }

    /// Write the payload and point `current` at it.
    ///
    /// # Errors
    ///
    /// When the payload cannot be written, or when `current` belongs to
    /// another release and `force` was not given.
    pub(super) fn place(&self, force: bool) -> Result<Outcome, String> {
        // Ask whether `current` will accept this release before writing a
        // single byte. Writing first left a whole payload behind on every
        // refusal, and removal is manifest-gated, so nothing could take it
        // back: one orphan per attempt, forever.
        self.check_current(force)?;
        // And whether this platform can point it at all. Writing the payload
        // first, then failing at the link, left a whole payload behind on
        // every attempt on a platform without symbolic links here.
        if !symlink::SUPPORTED {
            return Err(
                "a user install is built from symbolic links, which this platform does not \
                 provide here; install into a project instead, or pass --dir"
                    .to_owned(),
            );
        }
        std::fs::create_dir_all(&self.skills_dir)
            .map_err(|error| format!("failed to create {}: {error}", self.skills_dir.display()))?;
        let written = self.write_files()?;
        let repointed = self.point_current(force)?;
        Ok(Outcome { written, repointed })
    }

    /// Whether `current` already names this release, without changing it.
    ///
    /// # Errors
    ///
    /// When it names another release and `force` was not given, or when
    /// something that is not a link is in its place.
    pub(super) fn check_current(&self, force: bool) -> Result<bool, String> {
        match std::fs::symlink_metadata(&self.current_link) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let existing = std::fs::read_link(&self.current_link).map_err(|error| {
                    format!("failed to read {}: {error}", self.current_link.display())
                })?;
                let existing = self.resolve_from_current(&existing);
                if existing == self.release_dir {
                    return Ok(false);
                }
                // `current` has two owners. `install.sh` links
                // `~/.local/bin/fslc` at `current/bin/fslc`, so moving the
                // pointer to a release that holds no `bin/` leaves the command
                // itself unreachable. `--force` is permission to change which
                // skills every project sees, not to uninstall the binary.
                if existing.join("bin").is_dir() && !self.release_dir.join("bin").is_dir() {
                    return Err(format!(
                        "{} points at {}, which holds the installed commands, and this \
                         fslc has no payload beside them there. Re-run install.sh to move \
                         both, or install the skills into a project instead.",
                        self.current_link.display(),
                        existing.display()
                    ));
                }
                if !force {
                    return Err(format!(
                        "{} points at {}, not at this fslc's payload; re-link it with --force \
                         once you mean to move every project on this machine to this version",
                        self.current_link.display(),
                        existing.display()
                    ));
                }
                Ok(true)
            }
            Ok(_) => Err(format!(
                "{} exists and is not a link; move it and re-run",
                self.current_link.display()
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(format!(
                "failed to inspect {}: {error}",
                self.current_link.display()
            )),
        }
    }

    /// How many payload files a run would write.
    ///
    /// `write_files` skips a file already holding those bytes, so this is the
    /// number that differ. A dry run reports it, and reporting a 0/1 flag
    /// instead made the dry run disagree with the run it exists to predict.
    #[must_use]
    pub(super) fn pending_count(&self) -> usize {
        EMBEDDED_SKILL_FILES
            .iter()
            .filter(|(relative, contents)| {
                !std::fs::read(self.skills_dir.join(relative))
                    .is_ok_and(|current| current == contents.as_bytes())
            })
            .count()
    }

    /// Write the embedded files, skipping ones already holding those bytes.
    fn write_files(&self) -> Result<usize, String> {
        let mut written = 0;
        for (relative, contents) in EMBEDDED_SKILL_FILES {
            let path = self.skills_dir.join(relative);
            if let Ok(current) = std::fs::read(&path)
                && current == contents.as_bytes()
            {
                continue;
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
            }
            std::fs::write(&path, contents.as_bytes())
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            written += 1;
        }
        Ok(written)
    }

    /// Remove the skills this payload wrote, tolerating their absence.
    ///
    /// Only `skills/` under the release directory. `bin/` beside it and the
    /// `current` pointer belong to whatever installed the binaries.
    ///
    /// # Errors
    ///
    /// When the directory exists but cannot be removed.
    pub(super) fn remove(&self) -> Result<(), String> {
        match std::fs::remove_dir_all(&self.skills_dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "failed to remove {}: {error}",
                self.skills_dir.display()
            )),
        }
    }

    /// A `current` target as an absolute path.
    ///
    /// `install.sh:298` writes `ln -s "releases/$RELEASE_NAME" current`, a
    /// relative target, while this module names the release absolutely.
    /// Comparing the two as written makes every install.sh-made `current`
    /// look like another release's, which refuses the delegation `install.sh`
    /// performs on every run.
    fn resolve_from_current(&self, target: &Path) -> PathBuf {
        if target.is_absolute() {
            return target.to_path_buf();
        }
        self.current_link
            .parent()
            .unwrap_or(Path::new(""))
            .join(target)
    }

    /// Point `current` at this release, refusing to move someone else's.
    ///
    /// Moving `current` changes the skills of every project on the machine, so
    /// an existing pointer aimed elsewhere is an error naming both paths.
    fn point_current(&self, force: bool) -> Result<bool, String> {
        if !self.check_current(force)? {
            return Ok(false);
        }
        match std::fs::symlink_metadata(&self.current_link) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let existing = std::fs::read_link(&self.current_link).map_err(|error| {
                    format!("failed to read {}: {error}", self.current_link.display())
                })?;
                if self.resolve_from_current(&existing) == self.release_dir {
                    return Ok(false);
                }
                if !force {
                    return Err(format!(
                        "{} points at {}, not at this fslc's payload; re-link it with --force \
                         once you mean to move every project on this machine to this version",
                        self.current_link.display(),
                        existing.display()
                    ));
                }
            }
            Ok(_) => {
                return Err(format!(
                    "{} exists and is not a link; move it and re-run",
                    self.current_link.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "failed to inspect {}: {error}",
                    self.current_link.display()
                ));
            }
        }
        // `current` is a pointer, never a directory somebody owns.
        symlink::place(&self.current_link, &self.release_dir, false)?;
        Ok(true)
    }
}

// Symbolic links are what this module places, and creating one needs
// privilege on Windows, so the whole module's tests are unix-only.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("fslc-payload-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("create the sandbox");
        base
    }

    /// `install.sh:298` writes `ln -s "releases/$RELEASE_NAME" current`.
    ///
    /// Comparing that relative target against an absolute release directory
    /// makes it look like another release's, and the delegation `install.sh`
    /// performs on every run is refused.
    #[test]
    fn a_relative_current_is_recognized_as_this_release() {
        let data = sandbox("relative");
        let payload = Payload::new(&data, "v4.6.0-abcdef123456", "skills");
        std::fs::create_dir_all(data.join("releases/v4.6.0-abcdef123456")).expect("create");
        std::os::unix::fs::symlink("releases/v4.6.0-abcdef123456", data.join("current"))
            .expect("link");

        let outcome = payload.place(false).expect("a relative current is ours");
        assert!(!outcome.repointed, "an equivalent target is left alone");
        assert_eq!(
            std::fs::read_link(data.join("current")).expect("read"),
            Path::new("releases/v4.6.0-abcdef123456"),
            "the relative form is preserved"
        );
    }

    #[test]
    fn an_absolute_current_for_this_release_is_also_recognized() {
        let data = sandbox("absolute");
        let release = data.join("releases/v4.6.0-abcdef123456");
        let payload = Payload::new(&data, "v4.6.0-abcdef123456", "skills");
        std::fs::create_dir_all(&release).expect("create");
        std::os::unix::fs::symlink(&release, data.join("current")).expect("link");

        assert!(!payload.place(false).expect("place").repointed);
    }

    #[test]
    fn a_current_for_another_release_is_refused_without_force() {
        let data = sandbox("other");
        let payload = Payload::new(&data, "v4.6.0-abcdef123456", "skills");
        std::fs::create_dir_all(data.join("releases/v4.5.0-000000000000")).expect("create");
        std::os::unix::fs::symlink("releases/v4.5.0-000000000000", data.join("current"))
            .expect("link");

        let error = payload
            .place(false)
            .expect_err("another release is refused");
        assert!(error.contains("v4.5.0-000000000000"), "{error}");
        assert!(error.contains("--force"), "{error}");
        assert!(payload.place(true).expect("forced").repointed);
    }

    #[test]
    fn a_real_directory_at_current_is_refused_even_with_force() {
        let data = sandbox("real-dir");
        let payload = Payload::new(&data, "v4.6.0-abcdef123456", "skills");
        std::fs::create_dir_all(data.join("current")).expect("create");

        let error = payload
            .place(true)
            .expect_err("a directory is not ours to move");
        assert!(error.contains("not a link"), "{error}");
    }

    /// A payload that was never written is nothing to take back.
    #[test]
    fn removing_a_payload_that_is_not_there_is_not_a_failure() {
        let data = sandbox("absent-payload");
        Payload::new(&data, "v4.6.0-abcdef123456", "skills")
            .remove()
            .expect("absence is not a failure");
    }

    /// `current` behind a directory nobody may look into cannot be inspected,
    /// and that is a fault rather than a verdict.
    #[test]
    fn a_current_that_cannot_be_inspected_is_an_error() {
        use std::os::unix::fs::PermissionsExt as _;

        let data = sandbox("unreadable").join("locked");
        std::fs::create_dir_all(&data).expect("create");
        let payload = Payload::new(&data, "v4.6.0-abcdef123456", "skills");
        let parent = data.clone();
        let mut mode = std::fs::metadata(&parent).expect("read").permissions();
        mode.set_mode(0o000);
        std::fs::set_permissions(&parent, mode).expect("set");

        let outcome = payload.check_current(false);
        // Whether the mode actually took effect. Root ignores it, and so do
        // some filesystems. Deciding that from the outcome instead let this
        // test pass on the very regression it guards: an `Ok` looked exactly
        // like running as root.
        let enforced = std::fs::read_dir(&parent).is_err();

        let mut mode = std::fs::metadata(&parent).expect("read").permissions();
        mode.set_mode(0o700);
        std::fs::set_permissions(&parent, mode).expect("restore");

        if enforced {
            let error = outcome.expect_err("an uninspectable `current` is a fault");
            assert!(error.contains("failed to inspect"), "{error}");
        }
    }

    /// The pending count is what a run would write, and a dry run reports it.
    ///
    /// Reporting a 0/1 flag made a dry run say `1` where the run it predicts
    /// said `21`.
    #[test]
    fn the_pending_count_is_the_number_of_files_a_run_would_write() {
        let data = sandbox("written");
        let payload = Payload::new(&data, "v4.6.0-abcdef123456", "skills");
        let all = EMBEDDED_SKILL_FILES.len();
        assert_eq!(payload.pending_count(), all, "nothing is written yet");

        let written = payload.place(false).expect("place").written;
        assert_eq!(written, all, "the run writes every file");
        assert_eq!(payload.pending_count(), 0, "and then none are pending");

        std::fs::write(payload.skills_dir().join("fsl/SKILL.md"), "changed\n").expect("write");
        assert_eq!(payload.pending_count(), 1, "one file short is one pending");
    }

    #[test]
    fn writing_the_payload_twice_writes_nothing_the_second_time() {
        let data = sandbox("twice");
        let payload = Payload::new(&data, "v4.6.0-abcdef123456", "skills");
        let first = payload.place(false).expect("place");
        assert!(first.written > 0);
        assert_eq!(
            payload.place(false).expect("place again").written,
            0,
            "files already holding the embedded bytes are left alone"
        );
    }
}

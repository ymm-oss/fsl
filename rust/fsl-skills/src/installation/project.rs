// SPDX-License-Identifier: Apache-2.0

//! A project's `.claude/skills`, holding copies of the embedded skills.
//!
//! Copies rather than links: this is the version the project pinned, and it
//! stays put when another project selects a different `fslc`.

pub mod location;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::embedded::EMBEDDED_SKILL_FILES;
use crate::installation::manifest::{Manifest, Scope, digest};
use crate::installation::plan::{Action, EntryState, FilePlan, Plan, PlannedFile};

/// The digest a manifest recorded for `path`, if it recorded one.
fn recorded<'a>(manifest: Option<&'a Manifest>, path: &str) -> Option<&'a str> {
    manifest
        .and_then(|manifest| manifest.entries.get(path))
        .map(String::as_str)
}

/// A project's skills directory, holding copies.
pub struct Installation {
    skills_dir: PathBuf,
    force: bool,
}

impl Installation {
    /// Open the installation at `skills_dir`, reading any manifest there.
    ///
    /// # Errors
    ///
    /// When a manifest is present but unreadable.
    pub fn open(skills_dir: impl Into<PathBuf>, force: bool) -> Result<Self, String> {
        let skills_dir = skills_dir.into();
        // Read once to reject an unusable manifest early, then drop it. A
        // cached copy goes stale the moment `install` writes a new one, and a
        // plan made from the stale copy silently plans nothing.
        Manifest::read_for(&skills_dir, Scope::Project)?;
        Ok(Self { skills_dir, force })
    }

    /// Where this installation lives.
    #[must_use]
    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    /// Plan writing the embedded skills here.
    ///
    /// # Errors
    ///
    /// When a path exists but cannot be read.
    pub fn plan_install(&self) -> Result<FilePlan, String> {
        let manifest = Manifest::read_for(&self.skills_dir, Scope::Project)?;
        let mut entries = Vec::with_capacity(EMBEDDED_SKILL_FILES.len());
        // A path this binary no longer embeds is still ours, and an install
        // that only ever adds would leave a retired skill in place forever
        // while quietly dropping it from the manifest that records it.
        for relative in Self::retired(manifest.as_ref()) {
            let path = self.contained(&relative)?;
            let state = Self::classify(&path, recorded(manifest.as_ref(), &relative))?;
            let action = match state {
                EntryState::Managed => Action::Remove,
                EntryState::Modified => self.blocked_unless_forced(Action::Remove),
                EntryState::Absent | EntryState::Foreign => Action::Skip,
            };
            entries.push(PlannedFile {
                path: relative,
                state,
                action,
            });
        }
        for (relative, contents) in EMBEDDED_SKILL_FILES {
            let path = self.contained(relative)?;
            let state = Self::classify(&path, recorded(manifest.as_ref(), relative))?;
            let action = match state {
                EntryState::Absent => Action::Write,
                EntryState::Managed => Self::write_if_stale(&path, contents)?,
                // Somebody else's file sits where this skill goes. `--force`
                // is how a user says they want fslc's copy instead, and the
                // alternative is deleting the directory by hand.
                EntryState::Modified | EntryState::Foreign => {
                    self.blocked_unless_forced(Action::Write)
                }
            };
            entries.push(PlannedFile {
                path: (*relative).to_owned(),
                state,
                action,
            });
        }
        Ok(Plan::of(entries))
    }

    /// Plan taking this installation back out.
    ///
    /// # Errors
    ///
    /// When a path exists but cannot be read.
    pub fn plan_uninstall(&self) -> Result<FilePlan, String> {
        let Some(manifest) = Manifest::read_for(&self.skills_dir, Scope::Project)? else {
            return Ok(Plan::empty());
        };
        let mut entries = Vec::with_capacity(manifest.entries.len());
        for (relative, recorded) in &manifest.entries {
            let path = self.contained(relative)?;
            let state = Self::classify(&path, Some(recorded))?;
            let action = match state {
                EntryState::Managed => Action::Remove,
                EntryState::Modified => self.blocked_unless_forced(Action::Remove),
                EntryState::Absent | EntryState::Foreign => Action::Skip,
            };
            entries.push(PlannedFile {
                path: relative.clone(),
                state,
                action,
            });
        }
        Ok(Plan::of(entries))
    }

    /// Carry out an install plan, recording what it wrote.
    ///
    /// # Errors
    ///
    /// When a directory or file cannot be created or written.
    pub fn install(&self, plan: &FilePlan, version: &str) -> Result<(), String> {
        // The manifest goes here even when the plan writes nothing, so this
        // does not rely on some file's parent happening to create it.
        std::fs::create_dir_all(&self.skills_dir)
            .map_err(|error| format!("failed to create {}: {error}", self.skills_dir.display()))?;
        let embedded: BTreeMap<&str, &str> = EMBEDDED_SKILL_FILES.iter().copied().collect();
        let mut manifest = Manifest::new(version, Scope::Project);
        let retired: Vec<&PlannedFile> = plan
            .entries()
            .iter()
            .filter(|entry| entry.action == Action::Remove)
            .collect();
        if !retired.is_empty() {
            self.remove_files(&retired)?;
        }
        // A write that fails halfway leaves the files before it on disk.
        // Returning without a manifest left them unrecorded, so the next run
        // classified this run's own writes as foreign and refused them, and
        // `uninstall` reported `not_installed`. Record what was placed, then
        // return the failure.
        let previous = Manifest::read_for(&self.skills_dir, Scope::Project)?;
        let mut failure = None;
        for entry in plan.entries() {
            let Some(contents) = embedded.get(entry.path.as_str()) else {
                continue;
            };
            match entry.action {
                Action::Write => {
                    if let Err(error) = self.write_file(&entry.path, contents) {
                        failure = Some(error);
                        break;
                    }
                }
                Action::Keep => {}
                Action::Skip | Action::Blocked | Action::Remove => continue,
            }
            manifest
                .entries
                .insert(entry.path.clone(), digest(contents.as_bytes()));
        }
        if failure.is_some() {
            // The entries this run never reached are still on disk, written
            // and recorded by the run before. Writing only the prefix this
            // one managed disowned them, and then `uninstall` reported
            // success while leaving every one of them behind.
            self.carry_over(&mut manifest, previous);
        }
        manifest.write(&self.skills_dir)?;
        failure.map_or(Ok(()), Err)
    }

    /// Keep recording what a previous run left on disk and this one did not
    /// reach.
    ///
    /// Only a path that is still there. One this run removed as retired is
    /// gone, and recording it would ask a later removal to take back nothing.
    fn carry_over(&self, manifest: &mut Manifest, previous: Option<Manifest>) {
        let Some(previous) = previous else {
            return;
        };
        for (path, recorded) in previous.entries {
            if manifest.entries.contains_key(&path) {
                continue;
            }
            if self.contained(&path).is_ok_and(|path| path.exists()) {
                manifest.entries.insert(path, recorded);
            }
        }
    }

    /// Carry out a removal plan, pruning the directories it empties.
    ///
    /// # Errors
    ///
    /// When a file or the manifest cannot be removed.
    pub fn uninstall(&self, plan: &FilePlan) -> Result<usize, String> {
        let removing: Vec<&PlannedFile> = plan
            .entries()
            .iter()
            .filter(|entry| entry.action == Action::Remove)
            .collect();
        let removed = self.remove_files(&removing)?;
        Manifest::remove_from(&self.skills_dir)?;
        Ok(removed)
    }

    /// Remove the named files and the directories that leaves empty.
    fn remove_files(&self, entries: &[&PlannedFile]) -> Result<usize, String> {
        let mut removed = Vec::new();
        for entry in entries {
            let path = self.contained(&entry.path)?;
            match std::fs::remove_file(&path) {
                Ok(()) => removed.push(entry.path.clone()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("failed to remove {}: {error}", path.display())),
            }
        }
        self.prune_emptied(&removed);
        Ok(removed.len())
    }

    /// Paths a previous install wrote that this binary no longer embeds.
    fn retired(manifest: Option<&Manifest>) -> Vec<String> {
        let embedded: std::collections::BTreeSet<&str> =
            EMBEDDED_SKILL_FILES.iter().map(|(path, _)| *path).collect();
        manifest.map_or_else(Vec::new, |manifest| {
            manifest
                .entries
                .keys()
                .filter(|path| !embedded.contains(path.as_str()))
                .cloned()
                .collect()
        })
    }

    const fn blocked_unless_forced(&self, intended: Action) -> Action {
        if self.force {
            intended
        } else {
            Action::Blocked
        }
    }

    /// The path an entry names, refused when a parent would lead out.
    ///
    /// [`Manifest::is_contained`] checks the text of an entry, and text is not
    /// enough. A symbolic link at any directory component makes a
    /// contained-looking path resolve somewhere else, and `create_dir_all`,
    /// `write` and `remove_file` all follow it. A repository can carry both
    /// halves, so `fslc skills install` in a fresh clone would write outside
    /// the directory it names, and `uninstall` would delete outside it.
    ///
    /// `--force` does not reach this. It means "replace what is at the path I
    /// am about to write". A directory somebody linked elsewhere was never
    /// this installation's to write at all.
    fn contained(&self, relative: &str) -> Result<PathBuf, String> {
        let mut path = self.skills_dir.clone();
        let mut components = Path::new(relative).components().peekable();
        while let Some(component) = components.next() {
            path.push(component);
            // The leaf is the caller's own business: `classify` reports a link
            // there as foreign, and `write_file` replaces it under `--force`.
            if components.peek().is_none() {
                break;
            }
            if std::fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                return Err(format!(
                    "{} is a symbolic link, so {relative} would leave {}; move it and re-run",
                    path.display(),
                    self.skills_dir.display()
                ));
            }
        }
        Ok(path)
    }

    fn classify(path: &Path, recorded: Option<&str>) -> Result<EntryState, String> {
        // Ask about the path itself before reading through it. `read` follows
        // symbolic links, so a link pointing at a missing file reads as
        // absent, and writing it would land wherever the link aims. A project
        // install only ever writes plain files, so a link here is never ours.
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Ok(EntryState::Foreign);
            }
            // `read` on a directory fails with `EISDIR`, which reported a
            // failed read for something nobody was reading. Name the cause.
            Ok(metadata) if metadata.is_dir() => {
                return Err(format!(
                    "{} is a directory, and a skill file goes there; move it and re-run",
                    path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(EntryState::Absent);
            }
            Err(error) => return Err(format!("failed to inspect {}: {error}", path.display())),
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(EntryState::Absent);
            }
            Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
        };
        let Some(recorded) = recorded else {
            return Ok(EntryState::Foreign);
        };
        if digest(&bytes) == recorded {
            Ok(EntryState::Managed)
        } else {
            Ok(EntryState::Modified)
        }
    }

    fn write_if_stale(path: &Path, contents: &str) -> Result<Action, String> {
        let current = std::fs::read(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        if current == contents.as_bytes() {
            Ok(Action::Keep)
        } else {
            Ok(Action::Write)
        }
    }

    fn write_file(&self, relative: &str, contents: &str) -> Result<(), String> {
        let path = self.contained(relative)?;
        // Planning classified this path, but the filesystem can change under
        // it. Never write through a link: removing the link leaves whatever it
        // aimed at untouched, while writing would land there instead.
        if let Ok(metadata) = std::fs::symlink_metadata(&path)
            && metadata.file_type().is_symlink()
        {
            if !self.force {
                return Err(format!(
                    "{} is a symbolic link; move it and re-run",
                    path.display()
                ));
            }
            std::fs::remove_file(&path)
                .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        std::fs::write(&path, contents.as_bytes())
            .map_err(|error| format!("failed to write {}: {error}", path.display()))
    }

    /// Remove the directories a removal emptied, deepest first.
    ///
    /// A directory that still holds anything is left alone, so a hand-written
    /// file beside ours keeps its parent.
    fn prune_emptied(&self, removed: &[String]) {
        let mut directories: Vec<PathBuf> = removed
            .iter()
            .filter_map(|relative| Path::new(relative).parent().map(Path::to_path_buf))
            .filter(|relative| !relative.as_os_str().is_empty())
            .collect();
        // Sort by path as well as by depth: `dedup` only drops neighbours, and
        // two directories at the same depth are not neighbours under a
        // depth-only ordering.
        directories.sort();
        directories.dedup();
        directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for relative in directories {
            let mut current = Some(relative);
            while let Some(path) = current {
                if path.as_os_str().is_empty() {
                    break;
                }
                if std::fs::remove_dir(self.skills_dir.join(&path)).is_err() {
                    break;
                }
                current = path.parent().map(Path::to_path_buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("fslc-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("create");
        base
    }

    /// A directory where a skill file goes is a fault, and says which.
    ///
    /// `read` on a directory fails with `EISDIR`, and reporting that verbatim
    /// said "failed to read" for something nobody was reading. `--force` does
    /// not reach it either, because the fault happens while planning.
    #[test]
    fn a_directory_in_a_file_s_place_is_a_fault_not_a_verdict() {
        let dir = temp_dir("dir-for-file").join("skills");
        std::fs::create_dir_all(dir.join("fsl/SKILL.md")).expect("create");

        for force in [false, true] {
            let error = Installation::open(&dir, force)
                .expect("open")
                .plan_install()
                .expect_err("a directory cannot be read as a file");
            assert!(error.contains("is a directory"), "{error}");
            assert!(!error.contains("failed to read"), "{error}");
        }
    }

    /// A path nobody may read is a fault, not an absence.
    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_read_is_a_fault() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = temp_dir("unreadable").join("skills");
        std::fs::create_dir_all(dir.join("fsl")).expect("create");
        let path = dir.join("fsl/SKILL.md");
        std::fs::write(&path, "mine\n").expect("write");
        let mut mode = std::fs::metadata(&path).expect("read").permissions();
        mode.set_mode(0o000);
        std::fs::set_permissions(&path, mode).expect("set");

        let outcome = Installation::open(&dir, false)
            .expect("open")
            .plan_install();
        // Whether the mode actually took effect. Root ignores it, and so do
        // some filesystems. Deciding that from the outcome instead let this
        // test pass on the very regression it guards: an `Ok` looked exactly
        // like running as root.
        let enforced = std::fs::read(&path).is_err();

        let mut mode = std::fs::metadata(&path).expect("read").permissions();
        mode.set_mode(0o600);
        std::fs::set_permissions(&path, mode).expect("restore");

        if enforced {
            let error = outcome.expect_err("an unreadable file is a fault");
            assert!(error.contains("failed to read"), "{error}");
        }
    }

    /// Pruning stops at the first directory that will not go, and says nothing
    /// about it: an install put nothing there to report.
    #[test]
    fn pruning_leaves_a_directory_that_is_not_empty() {
        let dir = temp_dir("prune").join("skills");
        let installation = Installation::open(&dir, false).expect("open");
        installation
            .install(&installation.plan_install().expect("plan"), "4.6.0")
            .expect("install");
        std::fs::write(dir.join("fsl/theirs.md"), "notes\n").expect("write");

        let reopened = Installation::open(&dir, false).expect("reopen");
        let plan = reopened.plan_uninstall().expect("plan");
        reopened.uninstall(&plan).expect("uninstall");

        assert!(dir.join("fsl/theirs.md").exists());
        assert!(dir.join("fsl").exists(), "its parent stays with it");
        assert!(
            !dir.join("fsl-design").exists(),
            "a directory this install emptied still goes"
        );
    }
}

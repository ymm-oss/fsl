// SPDX-License-Identifier: Apache-2.0

//! What an install recorded, so a later run can tell its own work from yours.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Schema of the manifest written beside the installed skills.
const MANIFEST_SCHEMA: &str = "fslc.skills.v1";
/// Manifest file name, inside the skills directory.
const MANIFEST_FILE: &str = ".fslc-skills.json";

/// Which scope an install targeted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// A project's own `.claude/skills`, holding copies.
    Project,
    /// The user-wide `~/.claude/skills`, holding links.
    User,
}

impl Scope {
    /// The word this scope reports as.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
        }
    }
}

/// What a previous install put on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Always [`MANIFEST_SCHEMA`]. Present so a future shape can be told apart.
    pub schema: String,
    /// The `fslc` version whose payload was written.
    pub fslc_version: String,
    /// Which scope wrote it.
    pub scope: Scope,
    /// For a user install, the release directory the links point into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
    /// Files this install wrote.
    ///
    /// Key: the path relative to the skills directory. Value: the SHA-256 of
    /// the bytes written there, as lower-case hex.
    ///
    /// ```text
    /// "fsl/SKILL.md" -> "b1946ac9...",
    /// ```
    ///
    /// The digest is what a later run compares the file against, to tell one
    /// it wrote and nobody touched from one somebody has edited.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub entries: BTreeMap<String, String>,
    /// Links this install placed.
    ///
    /// Key: the skill name, which is also the link's file name. Value: the
    /// path the link was pointed at.
    ///
    /// ```text
    /// "fsl" -> "/home/me/.local/share/fsl/current/skills/fsl",
    /// ```
    ///
    /// Recording the target is what lets removal tell a link this tool made
    /// from one somebody else put there.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub links: BTreeMap<String, String>,
}

impl Manifest {
    /// An empty manifest for `version` in `scope`.
    #[must_use]
    pub fn new(version: &str, scope: Scope) -> Self {
        Self {
            schema: MANIFEST_SCHEMA.to_owned(),
            fslc_version: version.to_owned(),
            scope,
            release: None,
            entries: BTreeMap::new(),
            links: BTreeMap::new(),
        }
    }

    /// Read the manifest in `skills_dir`, or `None` when there is not one.
    ///
    /// # Errors
    ///
    /// When the file exists but cannot be read or parsed. An unreadable
    /// manifest is not treated as absent: that would make the next install
    /// mistake its own earlier files for someone else's.
    pub fn read(skills_dir: &Path) -> Result<Option<Self>, String> {
        let path = skills_dir.join(MANIFEST_FILE);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
        };
        let manifest: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
        if manifest.schema != MANIFEST_SCHEMA {
            return Err(format!(
                "{} has schema '{}'; this fslc writes '{MANIFEST_SCHEMA}'",
                path.display(),
                manifest.schema
            ));
        }
        for key in manifest.entries.keys() {
            if !is_contained(key) {
                return Err(format!(
                    "{} records '{key}', which points outside the skills directory",
                    path.display()
                ));
            }
        }
        // A link is one skill, so its key is one component. `is_contained`
        // allows `a/b`, which would let a recorded key reach through a
        // symbolic link somebody put inside the skills directory.
        for key in manifest.links.keys() {
            if !is_one_component(key) {
                return Err(format!(
                    "{} records the link '{key}', which is not a skill name",
                    path.display()
                ));
            }
        }
        // `release` names a directory under `<data>/releases`, and `join`
        // with an absolute or `..`-bearing component walks out of it. An
        // uninstall removes what this field names, so an unchecked one is a
        // recursive removal anywhere the user can write.
        if let Some(release) = &manifest.release
            && !is_one_component(release)
        {
            return Err(format!(
                "{} records the release '{release}', which is not a release name",
                path.display()
            ));
        }
        Ok(Some(manifest))
    }

    /// Write this manifest into `skills_dir`.
    ///
    /// # Errors
    ///
    /// When the file cannot be serialized or written.
    pub fn write(&self, skills_dir: &Path) -> Result<(), String> {
        let path = skills_dir.join(MANIFEST_FILE);
        let mut bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("failed to serialize {}: {error}", path.display()))?;
        bytes.push(b'\n');
        // Write beside it and rename. `fs::write` truncates first, so a run
        // that dies mid-write leaves a manifest the next one cannot parse,
        // and the files it records look like nobody's.
        //
        // `create_new` is what makes the staging file ours. `fs::write`
        // follows a symbolic link, and the name was the process id, so
        // anyone able to write here could plant `.fslc-skills.json.<pid>`
        // aimed at a file of their choosing: the write landed there, and the
        // rename then moved the link into place as the manifest. The manifest
        // is what decides which paths a removal deletes, so that was a
        // permanent redirection, not only one overwritten file. The clock
        // makes the name unguessable and `create_new` refuses whatever is
        // already there, link or not.
        let staging = skills_dir.join(format!(
            "{MANIFEST_FILE}.{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.subsec_nanos())
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|error| format!("failed to write {}: {error}", staging.display()))?;
        std::io::Write::write_all(&mut file, &bytes)
            .map_err(|error| format!("failed to write {}: {error}", staging.display()))?;
        drop(file);
        std::fs::rename(&staging, &path).map_err(|error| {
            let _ = std::fs::remove_file(&staging);
            format!("failed to write {}: {error}", path.display())
        })
    }

    /// Read the manifest here, requiring it to belong to `scope`.
    ///
    /// A manifest the other scope wrote is an error, not a manifest. Acting on
    /// it replaces the record of an install that is still on disk, and every
    /// file it named becomes unremovable.
    ///
    /// # Errors
    ///
    /// When the manifest is unreadable, or records the other scope.
    pub(super) fn read_for(skills_dir: &Path, scope: Scope) -> Result<Option<Self>, String> {
        let Some(manifest) = Self::read(skills_dir)? else {
            return Ok(None);
        };
        if manifest.scope != scope {
            return Err(format!(
                "{} was installed with {} scope; re-run with {}",
                skills_dir.display(),
                manifest.scope.as_str(),
                match manifest.scope {
                    Scope::User => "--user",
                    Scope::Project => "--dir, or from inside the project",
                }
            ));
        }
        Ok(Some(manifest))
    }

    /// Remove the manifest from `skills_dir`, tolerating its absence.
    pub(super) fn remove_from(skills_dir: &Path) -> Result<(), String> {
        let path = skills_dir.join(MANIFEST_FILE);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("failed to remove {}: {error}", path.display())),
        }
    }
}

/// Whether a recorded key stays inside the directory it is relative to.
///
/// A manifest is a file on disk, so it is input, not a fact. One checked into
/// a repository and cloned by somebody else can name `../../secrets` or an
/// absolute path, and removal would take exactly what it was told to. Only
/// plain relative components are accepted: no root, no prefix, no `..`, and
/// no bare `.`.
pub(super) fn is_contained(key: &str) -> bool {
    !key.is_empty()
        && Path::new(key)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Whether a recorded key is a single plain path component.
///
/// Stricter than [`is_contained`], for the fields that name one directory
/// rather than a path: a skill's link, and the release the payload lives in.
fn is_one_component(key: &str) -> bool {
    let mut components = Path::new(key).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

/// [`Manifest::read_for`], for a test that has to forge the other scope.
#[doc(hidden)]
///
/// # Errors
///
/// Whatever [`Manifest::read_for`] returns.
pub fn read_for_test(skills_dir: &Path, scope: Scope) -> Result<Option<Manifest>, String> {
    Manifest::read_for(skills_dir, scope)
}

/// Hex SHA-256 of some bytes, for a test that has to forge a manifest entry.
#[doc(hidden)]
#[must_use]
pub fn digest_for_test(bytes: &[u8]) -> String {
    digest(bytes)
}

/// Hex SHA-256 of some bytes.
pub(super) fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let base =
            std::env::temp_dir().join(format!("fslc-manifest-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("create");
        base
    }

    /// `release` names one directory under `<data>/releases`, and an
    /// uninstall removes what it names.
    ///
    /// `Path::join` with an absolute component drops the prefix, so an
    /// unchecked field was a recursive removal anywhere the user can write.
    #[test]
    fn a_release_that_is_not_a_release_name_is_refused() {
        let dir = temp_dir("release-key");
        for release in ["/etc", "../../elsewhere", "a/b", "..", "."] {
            std::fs::write(
                dir.join(MANIFEST_FILE),
                format!(
                    r#"{{"schema":"{MANIFEST_SCHEMA}","fslc_version":"4.6.0","scope":"user","release":"{release}"}}"#
                ),
            )
            .expect("write");
            let error = Manifest::read(&dir).expect_err("{release} must be refused");
            assert!(
                error.contains("is not a release name"),
                "{release}: {error}"
            );
        }

        std::fs::write(
            dir.join(MANIFEST_FILE),
            format!(
                r#"{{"schema":"{MANIFEST_SCHEMA}","fslc_version":"4.6.0","scope":"user","release":"v4.6.0-abcdef123456"}}"#
            ),
        )
        .expect("write");
        let manifest = Manifest::read(&dir).expect("read").expect("a manifest");
        assert_eq!(manifest.release.as_deref(), Some("v4.6.0-abcdef123456"));
    }

    /// A link is one skill, so its key is one component.
    ///
    /// `is_contained` accepts `a/b`, which reached through a symbolic link
    /// somebody put inside the skills directory: `uninstall` then removed a
    /// file outside it.
    #[test]
    fn a_link_key_that_is_not_a_skill_name_is_refused() {
        let dir = temp_dir("link-key");
        std::fs::write(
            dir.join(MANIFEST_FILE),
            format!(
                r#"{{"schema":"{MANIFEST_SCHEMA}","fslc_version":"4.6.0","scope":"user","links":{{"sub/victim":"/etc/hosts"}}}}"#
            ),
        )
        .expect("write");
        let error = Manifest::read(&dir).expect_err("a two-component link must be refused");
        assert!(error.contains("is not a skill name"), "{error}");
    }

    /// A manifest that is not there is nothing to remove.
    #[test]
    fn removing_a_manifest_that_is_absent_is_not_a_failure() {
        let dir = temp_dir("absent");
        Manifest::remove_from(&dir).expect("absence is not a failure");
    }

    /// A write that cannot land reports, and leaves no staging file behind.
    #[test]
    fn a_write_that_cannot_land_leaves_nothing_behind() {
        let dir = temp_dir("unwritable").join("missing");
        let error = Manifest::new("4.6.0", Scope::Project)
            .write(&dir)
            .expect_err("a directory that is not there cannot hold a manifest");
        assert!(error.contains("failed to write"), "{error}");
        assert!(!dir.exists(), "and nothing is created on the way");
    }

    /// Only plain relative components name something inside the directory.
    #[test]
    fn containment_accepts_a_plain_relative_path_and_nothing_else() {
        assert!(is_contained("fsl/SKILL.md"));
        assert!(is_contained("fsl"));
        for outside in ["", "..", "../x", "a/../../b", "/etc/hosts", "./x"] {
            assert!(!is_contained(outside), "{outside} must be refused");
        }
    }
}

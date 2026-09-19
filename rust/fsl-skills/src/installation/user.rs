// SPDX-License-Identifier: Apache-2.0

//! The user-wide `~/.claude/skills`, holding links into a release payload.
//!
//! Links rather than copies, through `current`, which is the shape
//! `install.sh` already produces. An upgrade is then one repointed pointer
//! rather than one relinked skill per skill.

pub mod location;
pub(crate) mod payload;
pub(super) mod symlink;

use std::path::{Path, PathBuf};

use crate::installation::manifest::{Manifest, Scope};
use crate::installation::plan::{Action, LinkPlan, LinkState, Plan, PlannedLink};
use crate::installation::{AGENT_DIR, SKILLS_SUBDIR};

use location::release_name;
use payload::{Outcome, Payload};

/// The user-wide skills directory and the payload behind it.
pub(crate) struct Installation {
    data: PathBuf,
    skills_dir: PathBuf,
    current_skills: PathBuf,
    payload: Payload,
    release: String,
    force: bool,
}

impl Installation {
    /// Open the user-wide installation for a binary with this digest.
    ///
    /// # Errors
    ///
    /// When a manifest is present but unreadable.
    pub(crate) fn open(
        home: &Path,
        data: &Path,
        version: &str,
        binary_digest: &str,
        force: bool,
    ) -> Result<Self, String> {
        let release = release_name(version, binary_digest);
        let skills_dir = home.join(AGENT_DIR).join(SKILLS_SUBDIR);
        Manifest::read_for(&skills_dir, Scope::User)?;
        Ok(Self {
            data: data.to_path_buf(),
            current_skills: Payload::current_skills(data, SKILLS_SUBDIR),
            payload: Payload::new(data, &release, SKILLS_SUBDIR),
            skills_dir,
            release,
            force,
        })
    }

    /// Where the links live.
    #[must_use]
    pub(crate) fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    /// Where the payload for this binary lives.
    #[must_use]
    pub(crate) fn payload_dir(&self) -> &Path {
        self.payload.skills_dir()
    }

    /// What a run would have to change beyond the links, without changing it.
    ///
    /// A dry run reported `up_to_date` whenever the links happened to match,
    /// because it assumed a payload it never looked at and a `current` it
    /// never asked about. The real run then refused.
    ///
    /// # Errors
    ///
    /// When `current` names another release and `--force` was not given.
    pub(crate) fn pending_payload(&self) -> Result<Outcome, String> {
        let repointed = self.payload.check_current(self.force)?;
        Ok(Outcome {
            written: self.payload.pending_count(),
            repointed,
        })
    }

    /// The release directory name for this binary.
    #[must_use]
    pub(crate) fn release(&self) -> &str {
        &self.release
    }

    /// Plan a link for every embedded skill.
    ///
    /// # Errors
    ///
    /// When a path exists but its kind cannot be determined.
    pub(crate) fn plan_install(&self) -> Result<LinkPlan, String> {
        let manifest = Manifest::read_for(&self.skills_dir, Scope::User)?;
        let names = crate::embedded::embedded_skill_names();
        // A link resolving says only that something is there. `install.sh`
        // uses `status`'s exit code to decide whether the install settled, so
        // a payload emptied behind the links has to read as work to do.
        let payload_intact = self.payload.pending_count() == 0;
        let mut entries = Vec::with_capacity(names.len());
        // A skill a previous `fslc` linked and this one no longer carries is
        // still ours, and an install that only ever adds leaves it behind
        // while dropping it from the manifest that named it. The link then
        // points into a payload that no longer holds it, and no later
        // `uninstall` can take it back. The project scope already removes its
        // retired files; this is the same rule for links.
        for skill in Self::retired(manifest.as_ref(), &names) {
            let target = manifest
                .as_ref()
                .and_then(|manifest| manifest.links.get(&skill))
                .cloned()
                .unwrap_or_default();
            let state = Self::classify(&self.skills_dir.join(&skill), &target, Some(&target))?;
            let action = match state {
                LinkState::Managed | LinkState::Broken => Action::Remove,
                LinkState::Diverged => self.blocked_unless_forced(Action::Remove),
                LinkState::Absent | LinkState::Foreign | LinkState::Occupied => Action::Skip,
            };
            entries.push(PlannedLink {
                skill,
                target,
                state,
                action,
            });
        }
        for name in &names {
            let target = self
                .current_skills
                .join(name)
                .to_string_lossy()
                .into_owned();
            let recorded = manifest
                .as_ref()
                .and_then(|manifest| manifest.links.get(*name))
                .map(String::as_str);
            let state = Self::classify(&self.skills_dir.join(name), &target, recorded)?;
            let action = match state {
                LinkState::Managed if payload_intact => Action::Keep,
                LinkState::Absent | LinkState::Broken | LinkState::Managed => Action::Write,
                // A link of ours pointing elsewhere is the version mismatch
                // this command refuses to repair quietly.
                // A real directory or somebody's own link where this skill
                // goes. `--force` replaces it, the same as a diverged one.
                LinkState::Diverged | LinkState::Foreign | LinkState::Occupied => {
                    self.blocked_unless_forced(Action::Write)
                }
            };
            entries.push(PlannedLink {
                skill: (*name).to_owned(),
                target,
                state,
                action,
            });
        }
        Ok(Plan::of(entries))
    }

    /// Plan removing the links the manifest records.
    ///
    /// # Errors
    ///
    /// When a path exists but its kind cannot be determined.
    pub(crate) fn plan_uninstall(&self) -> Result<LinkPlan, String> {
        let Some(manifest) = Manifest::read_for(&self.skills_dir, Scope::User)? else {
            return Ok(Plan::empty());
        };
        let mut entries = Vec::with_capacity(manifest.links.len());
        for (skill, target) in &manifest.links {
            let state = Self::classify(&self.skills_dir.join(skill), target, Some(target))?;
            let action = match state {
                LinkState::Managed | LinkState::Broken => Action::Remove,
                LinkState::Diverged => self.blocked_unless_forced(Action::Remove),
                LinkState::Absent | LinkState::Foreign | LinkState::Occupied => Action::Skip,
            };
            entries.push(PlannedLink {
                skill: skill.clone(),
                target: target.clone(),
                state,
                action,
            });
        }
        Ok(Plan::of(entries))
    }

    /// Write the payload, point `current` at it, and place the links.
    ///
    /// # Errors
    ///
    /// When the payload cannot be written, when `current` belongs to another
    /// release and `--force` was not given, or when a link cannot be placed.
    pub(crate) fn install(&self, plan: &LinkPlan, version: &str) -> Result<Outcome, String> {
        let outcome = self.payload.place(self.force)?;
        std::fs::create_dir_all(&self.skills_dir)
            .map_err(|error| format!("failed to create {}: {error}", self.skills_dir.display()))?;
        let mut manifest = Manifest::new(version, Scope::User);
        manifest.release = Some(self.release.clone());
        // A link that fails halfway leaves the ones before it on disk, and
        // `current` already moved. Record what was placed before returning the
        // failure, so a later removal can still take it back.
        let mut failure = None;
        for entry in plan.entries() {
            match entry.action {
                Action::Write => {
                    if let Err(error) = symlink::place(
                        &self.skills_dir.join(&entry.skill),
                        Path::new(&entry.target),
                        self.force,
                    ) {
                        failure = Some(error);
                        break;
                    }
                }
                Action::Keep => {}
                // A skill this binary no longer carries. Take the link out
                // rather than leaving it to dangle once `current` moves, and
                // leave it out of the manifest this run writes.
                Action::Remove => {
                    let path = self.skills_dir.join(&entry.skill);
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            failure = Some(format!("failed to remove {}: {error}", path.display()));
                            break;
                        }
                    }
                    continue;
                }
                Action::Skip | Action::Blocked => continue,
            }
            manifest
                .links
                .insert(entry.skill.clone(), entry.target.clone());
        }
        manifest.write(&self.skills_dir)?;
        failure.map_or(Ok(outcome), Err)
    }

    /// Remove the links a removal plan names.
    ///
    /// # Errors
    ///
    /// When a link or the manifest cannot be removed.
    pub(crate) fn uninstall(&self, plan: &LinkPlan) -> Result<usize, String> {
        // Read the recorded release before the manifest goes. The payload to
        // take back is the one the manifest names, which is not always this
        // binary's: uninstalling with a different `fslc` than the one that
        // installed removed this binary's payload, left the recorded one on
        // disk, and then removed the manifest that named it. Removal is
        // manifest-gated, so nothing could ever take it back.
        let recorded = Manifest::read_for(&self.skills_dir, Scope::User)?
            .and_then(|manifest| manifest.release);
        let mut removed = 0;
        for entry in plan.entries() {
            if entry.action != Action::Remove {
                continue;
            }
            let path = self.skills_dir.join(&entry.skill);
            match std::fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("failed to remove {}: {error}", path.display())),
            }
        }
        Manifest::remove_from(&self.skills_dir)?;
        // The payload is this command's too. `current` and `bin/` beside it
        // belong to whatever installed the binaries, so they stay.
        match recorded {
            Some(release) if release != self.release => {
                Payload::new(&self.data, &release, SKILLS_SUBDIR).remove()?;
            }
            _ => self.payload.remove()?,
        }
        Ok(removed)
    }

    /// `intended` under `--force`, a refusal otherwise.
    ///
    /// The caller passes what it actually wants done. Hard-coding `Write` here
    /// made a forced removal plan ask to write a link, which `uninstall`
    /// ignores, so the link survived and the run still reported success.
    const fn blocked_unless_forced(&self, intended: Action) -> Action {
        if self.force {
            intended
        } else {
            Action::Blocked
        }
    }

    /// What is at `path`, against where it should point and what was recorded.
    ///
    /// A link already pointing at `expected` is ours to record, manifest or
    /// not. Without that, an install made by `install.sh` before this command
    /// existed reads as somebody else's: its links get skipped, nothing
    /// records them, and a later removal leaves every one of them behind.
    /// Skills a previous run linked that this binary no longer carries.
    fn retired(manifest: Option<&Manifest>, embedded: &[&str]) -> Vec<String> {
        manifest.map_or_else(Vec::new, |manifest| {
            manifest
                .links
                .keys()
                .filter(|skill| !embedded.contains(&skill.as_str()))
                .cloned()
                .collect()
        })
    }

    fn classify(path: &Path, expected: &str, recorded: Option<&str>) -> Result<LinkState, String> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LinkState::Absent);
            }
            Err(error) => return Err(format!("failed to inspect {}: {error}", path.display())),
        };
        if !metadata.file_type().is_symlink() {
            return Ok(LinkState::Occupied);
        }
        let target = std::fs::read_link(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        if target == Path::new(expected) {
            // The link text is right. Whether it reaches anything is a
            // separate question, and the one `status` has to answer.
            if std::fs::metadata(path).is_err() {
                return Ok(LinkState::Broken);
            }
            return Ok(LinkState::Managed);
        }
        if recorded.is_some() {
            Ok(LinkState::Diverged)
        } else {
            Ok(LinkState::Foreign)
        }
    }
}

// The machine-wide scope is built out of symbolic links.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A home and a data directory nobody else is using.
    fn sandbox(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("fslc-user-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("home")).expect("create the sandbox");
        std::fs::create_dir_all(base.join("data")).expect("create the sandbox");
        base
    }

    fn open(base: &Path, force: bool) -> Installation {
        Installation::open(
            &base.join("home"),
            &base.join("data"),
            "4.6.0",
            "0123456789ab",
            force,
        )
        .expect("open")
    }

    /// A skill a previous `fslc` linked and this one does not carry is taken
    /// out, not left to dangle.
    ///
    /// Leaving it was worse than untidy. The manifest this run writes holds
    /// only the skills this binary carries, so the forgotten link could never
    /// be taken back by `uninstall` either, and it pointed into a payload
    /// that no longer held it once `current` moved.
    #[test]
    fn a_skill_this_binary_no_longer_carries_is_unlinked_on_install() {
        let base = sandbox("retired-link");
        let installation = open(&base, false);
        installation
            .install(&installation.plan_install().expect("plan"), "4.6.0")
            .expect("install");

        // What a previous `fslc` that carried one more skill would have left.
        let skills_dir = installation.skills_dir().to_path_buf();
        let target = installation.current_skills.join("fsl-retired");
        std::fs::create_dir_all(&target).expect("create");
        std::os::unix::fs::symlink(&target, skills_dir.join("fsl-retired")).expect("link");
        let mut manifest = Manifest::read_for(&skills_dir, Scope::User)
            .expect("read")
            .expect("a manifest");
        manifest.links.insert(
            "fsl-retired".to_owned(),
            target.to_string_lossy().into_owned(),
        );
        manifest.write(&skills_dir).expect("write");

        let plan = installation.plan_install().expect("plan");
        let entry = plan
            .entries()
            .iter()
            .find(|entry| entry.skill == "fsl-retired")
            .expect("the retired skill is planned");
        assert_eq!(entry.action, Action::Remove);
        assert!(!plan.is_settled(), "there is work to do");

        installation.install(&plan, "4.6.0").expect("install");
        assert!(
            std::fs::symlink_metadata(skills_dir.join("fsl-retired")).is_err(),
            "the link must be gone"
        );
        let manifest = Manifest::read_for(&skills_dir, Scope::User)
            .expect("read")
            .expect("a manifest");
        assert!(!manifest.links.contains_key("fsl-retired"));
    }

    /// Point one link somewhere else, the way a stale install leaves it.
    fn diverge(base: &Path, skill: &str) {
        let link = base
            .join("home")
            .join(AGENT_DIR)
            .join(SKILLS_SUBDIR)
            .join(skill);
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("create");
        std::fs::remove_file(&link).expect("remove the link");
        symlink::place(&link, &elsewhere, false).expect("repoint");
    }

    /// A removal plan that asks to write is the defect this guards.
    ///
    /// `uninstall` acts only on `Remove`, so a `Write` here is skipped: the
    /// link survives, the manifest is deleted, and the run reports success.
    #[test]
    fn a_forced_removal_plan_never_asks_to_write() {
        let base = sandbox("forced-removal");
        let names = crate::embedded::embedded_skill_names();
        let fresh = open(&base, false);
        fresh
            .install(&fresh.plan_install().expect("plan"), "4.6.0")
            .expect("install");
        diverge(&base, "fsl");

        let forced = open(&base, true);
        let plan = forced.plan_uninstall().expect("plan");
        assert!(
            plan.entries()
                .iter()
                .all(|entry| entry.action != Action::Write),
            "a removal plan must not ask to write"
        );

        let diverged = plan
            .entries()
            .iter()
            .find(|entry| entry.skill == "fsl")
            .expect("the diverged link is planned");
        assert_eq!(diverged.state, LinkState::Diverged);
        assert_eq!(diverged.action, Action::Remove);

        assert_eq!(forced.uninstall(&plan).expect("uninstall"), names.len());
        let skills_dir = base.join("home").join(AGENT_DIR).join(SKILLS_SUBDIR);
        let left = std::fs::read_dir(&skills_dir).expect("read").count();
        assert_eq!(left, 0, "nothing may be left behind");
    }

    /// The plain path: links placed, payload written, `current` pointed.
    #[test]
    fn a_fresh_install_places_links_a_payload_and_current() {
        let base = sandbox("fresh");
        let installation = open(&base, false);
        let plan = installation.plan_install().expect("plan");
        let outcome = installation.install(&plan, "4.6.0").expect("install");

        assert!(outcome.written > 0, "the payload is written");
        assert!(outcome.repointed, "current is pointed at this release");
        assert_eq!(
            std::fs::read_link(base.join("data").join("current")).expect("read"),
            base.join("data")
                .join("releases")
                .join(installation.release()),
        );

        let names = crate::embedded::embedded_skill_names();
        for name in &names {
            let link = base
                .join("home")
                .join(AGENT_DIR)
                .join(SKILLS_SUBDIR)
                .join(name);
            assert_eq!(
                std::fs::read_link(&link).expect("read"),
                base.join("data")
                    .join("current")
                    .join(SKILLS_SUBDIR)
                    .join(name),
                "{name} must point through current"
            );
        }
        let manifest = Manifest::read(installation.skills_dir())
            .expect("read")
            .expect("a manifest");
        assert_eq!(manifest.links.len(), names.len());
        assert_eq!(manifest.scope, Scope::User);
        assert_eq!(manifest.release.as_deref(), Some(installation.release()));
    }

    /// A second install changes nothing, and says so.
    #[test]
    fn a_second_install_settles() {
        let base = sandbox("settled");
        let first = open(&base, false);
        first
            .install(&first.plan_install().expect("plan"), "4.6.0")
            .expect("install");

        let second = open(&base, false);
        let plan = second.plan_install().expect("plan");
        assert!(plan.is_settled());
        assert_eq!(plan.writes(), 0);
        let outcome = second.install(&plan, "4.6.0").expect("install");
        assert_eq!(outcome.written, 0);
        assert!(!outcome.repointed);
    }

    /// Removal takes back the links, the manifest, and the payload it wrote.
    ///
    /// `current` and `bin/` beside the payload belong to whatever installed
    /// the binaries, so they stay.
    #[test]
    fn uninstall_removes_the_links_the_manifest_and_the_payload() {
        let base = sandbox("removal");
        let installation = open(&base, false);
        installation
            .install(&installation.plan_install().expect("plan"), "4.6.0")
            .expect("install");
        let payload = installation.payload_dir().to_path_buf();
        let binaries = payload.parent().expect("a release directory").join("bin");
        std::fs::create_dir_all(&binaries).expect("create");

        let plan = installation.plan_uninstall().expect("plan");
        let removed = installation.uninstall(&plan).expect("uninstall");

        assert_eq!(removed, crate::embedded::embedded_skill_names().len());
        assert_eq!(
            std::fs::read_dir(installation.skills_dir())
                .expect("read")
                .count(),
            0,
            "the manifest goes with the links"
        );
        assert!(!payload.exists(), "the payload is this command's too");
        assert!(binaries.exists(), "the binaries are not");
        assert!(
            base.join("data").join("current").exists(),
            "current belongs to whatever installed the binaries"
        );
    }

    /// A layout `install.sh` made, before this command existed, must be adopted.
    ///
    /// Those links already point at `current/skills/<name>`, which is exactly
    /// where this command would point them. Reading them as somebody else's
    /// would skip all of them, record none, and leave every one behind on the
    /// next removal.
    #[test]
    fn links_already_pointing_where_we_would_point_them_are_adopted() {
        let base = sandbox("adopt");
        let names = crate::embedded::embedded_skill_names();
        let skill_count = names.len();
        let skills_dir = base.join("home").join(AGENT_DIR).join(SKILLS_SUBDIR);
        std::fs::create_dir_all(&skills_dir).expect("create");
        let current_skills = base.join("data").join("current").join(SKILLS_SUBDIR);
        // Two of the skills, linked by hand, with no manifest anywhere.
        for name in names.iter().take(2) {
            symlink::place(&skills_dir.join(name), &current_skills.join(name), false)
                .expect("link");
        }

        let installation = open(&base, false);
        let plan = installation.plan_install().expect("plan");
        assert!(
            plan.entries()
                .iter()
                .all(|entry| entry.action != Action::Skip),
            "an existing correct link is ours, not a stranger's"
        );
        assert!(plan.blocked().is_empty());

        installation.install(&plan, "4.6.0").expect("install");
        let manifest = Manifest::read(&skills_dir)
            .expect("read")
            .expect("a manifest");
        assert_eq!(
            manifest.links.len(),
            skill_count,
            "every link must be recorded, including the adopted ones"
        );
    }

    /// Without `--force`, the same link is a refusal rather than a removal.
    #[test]
    fn an_unforced_removal_refuses_a_diverged_link() {
        let base = sandbox("unforced-removal");
        let fresh = open(&base, false);
        fresh
            .install(&fresh.plan_install().expect("plan"), "4.6.0")
            .expect("install");
        diverge(&base, "fsl");

        let plan = open(&base, false).plan_uninstall().expect("plan");
        assert_eq!(plan.blocked(), vec!["fsl"]);
    }
}

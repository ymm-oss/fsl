// SPDX-License-Identifier: Apache-2.0

//! Where a project install writes, and what it refuses to overwrite.

use std::path::{Path, PathBuf};

use fsl_skills::installation::plan::Action as PlanAction;
use fsl_skills::installation::project::Installation;
use fsl_skills::installation::project::location::{ResolveError, resolve_dir};
use fsl_skills::installation::user::location::{data_dir, release_name};
use fsl_skills::{Action, EMBEDDED_SKILL_FILES, EntryState, Manifest, Scope};

fn temp_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("fslc-skills-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("create the temporary directory");
    base
}

/// The text a user actually sees for a refusal.
fn error_text(error: &ResolveError) -> String {
    error.to_string()
}

/// A resolver that reports the listed paths, and only those, as present.
fn present(paths: &[PathBuf]) -> impl Fn(&Path) -> bool + '_ {
    move |candidate: &Path| paths.iter().any(|path| path == candidate)
}

#[test]
fn an_existing_agent_directory_wins_over_the_repository_root() {
    let home = PathBuf::from("/home/someone");
    let root = PathBuf::from("/home/someone/work/repo");
    let cwd = root.join("rust/fslc");
    let existing = root.join(".claude");
    let resolved = resolve_dir(
        &cwd,
        Some(&root),
        &home,
        &present(std::slice::from_ref(&existing)),
    )
    .expect("resolve");
    assert_eq!(resolved, existing.join("skills"));
}

#[test]
fn the_nearest_agent_directory_wins() {
    let home = PathBuf::from("/home/someone");
    let root = PathBuf::from("/home/someone/work/repo");
    let cwd = root.join("packages/app");
    let nearest = cwd.join(".claude");
    let outer = root.join(".claude");
    let resolved = resolve_dir(
        &cwd,
        Some(&root),
        &home,
        &present(&[nearest.clone(), outer]),
    )
    .expect("resolve");
    assert_eq!(resolved, nearest.join("skills"));
}

#[test]
fn without_an_agent_directory_the_repository_root_gets_one() {
    let home = PathBuf::from("/home/someone");
    let root = PathBuf::from("/home/someone/work/repo");
    let cwd = root.join("rust/fslc");
    let resolved = resolve_dir(&cwd, Some(&root), &home, &present(&[])).expect("resolve");
    assert_eq!(resolved, root.join(".claude").join("skills"));
}

/// The trap this bound exists for. `~/.claude` is present for every Claude
/// Code user, so an unbounded walk from a directory under the home directory
/// always reaches it, turning a project install into a user-wide one.
#[test]
fn the_walk_stops_at_the_repository_root_instead_of_reaching_home() {
    let home = PathBuf::from("/home/someone");
    let root = PathBuf::from("/home/someone/work/repo");
    let cwd = root.join("rust/fslc");
    let user_scope = home.join(".claude");
    let resolved = resolve_dir(
        &cwd,
        Some(&root),
        &home,
        &present(std::slice::from_ref(&user_scope)),
    )
    .expect("resolve");
    assert_eq!(resolved, root.join(".claude").join("skills"));
    assert!(!resolved.starts_with(&user_scope));
}

#[test]
fn landing_on_the_user_directory_is_refused() {
    let home = PathBuf::from("/home/someone");
    let user_scope = home.join(".claude");
    let error = resolve_dir(
        &home,
        Some(&home),
        &home,
        &present(std::slice::from_ref(&user_scope)),
    )
    .expect_err("the user-wide directory must be refused");
    assert_eq!(
        error,
        ResolveError::WouldWriteUserScope { path: user_scope }
    );
}

#[test]
fn outside_a_repository_without_an_agent_directory_it_refuses_to_guess() {
    let home = PathBuf::from("/home/someone");
    let cwd = PathBuf::from("/tmp/scratch");
    let error = resolve_dir(&cwd, None, &home, &present(&[]))
        .expect_err("no repository root means no answer");
    match error {
        ResolveError::OutsideRepository { looked_at } => {
            assert_eq!(looked_at, cwd.join(".claude"));
            // The message must not promise a search that never happened.
            let rendered = error_text(&ResolveError::OutsideRepository {
                looked_at: cwd.join(".claude"),
            });
            assert!(rendered.contains("no .claude directory at"), "{rendered}");
            assert!(!rendered.contains("looked at:"), "{rendered}");
        }
        ResolveError::WouldWriteUserScope { path } => {
            panic!("unexpected refusal at {}", path.display())
        }
    }
}

#[test]
fn the_data_directory_follows_the_installer() {
    let home = PathBuf::from("/home/someone");
    let explicit = PathBuf::from("/opt/fsl");
    let xdg = PathBuf::from("/home/someone/.xdg");
    assert_eq!(data_dir(Some(&explicit), Some(&xdg), &home), explicit);
    assert_eq!(data_dir(None, Some(&xdg), &home), xdg.join("fsl"));
    assert_eq!(
        data_dir(None, None, &home),
        home.join(".local").join("share").join("fsl")
    );
}

#[test]
fn the_release_name_matches_the_installer_shape() {
    let name = release_name("4.6.0", "0123456789abcdeffedcba9876543210");
    assert_eq!(name, "v4.6.0-0123456789ab");
}

#[test]
fn a_fresh_install_writes_every_embedded_file() {
    let dir = temp_dir("fresh").join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    let plan = installation.plan_install().expect("plan");
    assert_eq!(plan.entries().len(), EMBEDDED_SKILL_FILES.len());
    assert!(
        plan.entries()
            .iter()
            .all(|entry| entry.action == Action::Write)
    );

    installation.install(&plan, "4.6.0").expect("install");

    // Read the manifest back rather than trusting a return value: what the
    // next run sees is the file on disk.
    let manifest = Manifest::read(&dir).expect("read").expect("a manifest");
    assert_eq!(manifest.entries.len(), EMBEDDED_SKILL_FILES.len());
    assert_eq!(manifest.scope, Scope::Project);

    for (relative, contents) in EMBEDDED_SKILL_FILES {
        let written = std::fs::read_to_string(dir.join(relative)).expect("read back");
        assert_eq!(&written, contents, "{relative} differs");
    }
}

#[test]
fn installing_twice_changes_nothing_the_second_time() {
    let dir = temp_dir("twice").join("skills");
    let first = Installation::open(&dir, false).expect("open");
    first
        .install(&first.plan_install().expect("plan"), "4.6.0")
        .expect("install");

    let second = Installation::open(&dir, false).expect("reopen");
    let plan = second.plan_install().expect("plan");
    assert!(plan.is_settled(), "a repeated install should settle");
    assert_eq!(plan.writes(), 0);
}

#[test]
fn a_hand_edited_file_blocks_the_install_and_survives() {
    let dir = temp_dir("edited").join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    installation
        .install(&installation.plan_install().expect("plan"), "4.6.0")
        .expect("install");

    let edited = dir.join("fsl/SKILL.md");
    std::fs::write(&edited, "mine now\n").expect("edit");

    let reopened = Installation::open(&dir, false).expect("reopen");
    let plan = reopened.plan_install().expect("plan");
    assert_eq!(plan.blocked(), vec!["fsl/SKILL.md"]);
    assert_eq!(
        std::fs::read_to_string(&edited).expect("read"),
        "mine now\n",
        "planning must not write anything"
    );

    let forced = Installation::open(&dir, true).expect("reopen forced");
    assert!(
        forced.plan_install().expect("plan").blocked().is_empty(),
        "--force clears the refusal"
    );
}

/// Somebody else's file where a skill goes is a refusal, not a silent skip.
///
/// Skipping it reports success while the skill is not installed. `--force` is
/// how a user says they want fslc's copy instead of what is there.
#[test]
fn a_file_we_never_wrote_is_refused_until_forced() {
    let dir = temp_dir("foreign").join("skills");
    std::fs::create_dir_all(dir.join("fsl")).expect("create");
    std::fs::write(dir.join("fsl/SKILL.md"), "someone else's\n").expect("write");

    let installation = Installation::open(&dir, false).expect("open");
    let plan = installation.plan_install().expect("plan");
    let entry = plan
        .entries()
        .iter()
        .find(|entry| entry.path == "fsl/SKILL.md")
        .expect("the path is planned");
    assert_eq!(entry.state, EntryState::Foreign);
    assert_eq!(entry.action, Action::Blocked);
    assert_eq!(plan.blocked().len(), 1, "a foreign file is a refusal");

    let forced = Installation::open(&dir, true).expect("reopen forced");
    let plan = forced.plan_install().expect("plan");
    assert!(plan.blocked().is_empty(), "--force clears the refusal");
    forced.install(&plan, "4.6.0").expect("install");
    assert_ne!(
        std::fs::read_to_string(dir.join("fsl/SKILL.md")).expect("read"),
        "someone else's\n",
        "--force must replace it with ours"
    );
}

/// A skipped entry means ours is not there, so the set is not settled.
///
/// Counting a skip as settled reports an install that never happened as up to
/// date, and `status` is meant to work as a check.
#[test]
fn a_skipped_entry_leaves_the_set_unsettled() {
    let dir = temp_dir("unsettled").join("skills");
    std::fs::create_dir_all(dir.join("fsl")).expect("create");
    std::fs::write(dir.join("fsl/SKILL.md"), "someone else's\n").expect("write");

    let installation = Installation::open(&dir, false).expect("open");
    let plan = installation.plan_install().expect("plan");
    installation.install(&plan, "4.6.0").expect("install");

    let reopened = Installation::open(&dir, false).expect("reopen");
    let settled = reopened.plan_install().expect("plan");
    assert!(
        !settled.is_settled(),
        "a foreign file means the skill it displaces is not installed"
    );
}

#[test]
fn uninstall_takes_back_only_what_it_wrote() {
    let dir = temp_dir("uninstall").join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    installation
        .install(&installation.plan_install().expect("plan"), "4.6.0")
        .expect("install");

    let mine = dir.join("mine.md");
    std::fs::write(&mine, "handwritten\n").expect("write");

    let reopened = Installation::open(&dir, false).expect("reopen");
    let plan = reopened.plan_uninstall().expect("plan");
    let removed = reopened.uninstall(&plan).expect("uninstall");

    assert_eq!(removed, EMBEDDED_SKILL_FILES.len());
    assert!(mine.exists(), "a handwritten file must survive");
    assert!(!dir.join("fsl/SKILL.md").exists());
    assert!(!dir.join("fsl").exists(), "an emptied directory is pruned");
    assert!(
        Manifest::read(&dir).expect("read").is_none(),
        "the manifest goes with the files"
    );
}

#[test]
fn an_edited_file_blocks_removal_too() {
    let dir = temp_dir("edited-removal").join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    installation
        .install(&installation.plan_install().expect("plan"), "4.6.0")
        .expect("install");

    std::fs::write(dir.join("fsl/SKILL.md"), "mine now\n").expect("edit");
    let reopened = Installation::open(&dir, false).expect("reopen");
    assert_eq!(
        reopened.plan_uninstall().expect("plan").blocked(),
        vec!["fsl/SKILL.md"]
    );
}

#[test]
fn a_manifest_round_trips() {
    let dir = temp_dir("manifest");
    let mut manifest = Manifest::new("4.6.0", Scope::User);
    manifest.release = Some("v4.6.0-0123456789ab".to_owned());
    manifest
        .links
        .insert("fsl".to_owned(), "/data/current/skills/fsl".to_owned());
    manifest.write(&dir).expect("write");

    let read = Manifest::read(&dir).expect("read").expect("a manifest");
    assert_eq!(read, manifest);
}

#[test]
fn a_manifest_from_another_schema_is_an_error_not_an_absence() {
    let dir = temp_dir("schema");
    std::fs::write(
        dir.join(".fslc-skills.json"),
        br#"{"schema":"fslc.skills.v99","fslc_version":"9.9.9","scope":"project"}"#,
    )
    .expect("write");
    let error = Manifest::read(&dir).expect_err("an unknown schema must be reported");
    assert!(error.contains("fslc.skills.v99"), "{error}");
}

/// A manifest is a file, so it is input. One checked into a repository can
/// name anything, and removal would take exactly what it was told to.
#[test]
fn a_manifest_naming_a_path_outside_the_directory_is_refused() {
    let base = temp_dir("escape");
    let dir = base.join("skills");
    let outside = base.join("outside");
    std::fs::create_dir_all(&dir).expect("create");
    std::fs::create_dir_all(&outside).expect("create");
    let precious = outside.join("precious.txt");
    std::fs::write(&precious, "keep me\n").expect("write");

    for key in ["../outside/precious.txt", "/etc/hosts", "a/../../b"] {
        std::fs::write(
            dir.join(".fslc-skills.json"),
            format!(
                r#"{{"schema":"fslc.skills.v1","fslc_version":"4.6.0","scope":"project","entries":{{"{key}":"0"}}}}"#
            ),
        )
        .expect("write");
        let error = Manifest::read(&dir).expect_err("{key} must be refused");
        assert!(error.contains("outside the skills directory"), "{error}");
    }
    assert!(precious.exists(), "nothing outside may be reached");
}

/// `read` follows symbolic links, so a link to a missing file reads as absent.
/// Writing it would land wherever the link aims.
#[cfg(unix)]
#[test]
fn a_symbolic_link_is_never_written_through() {
    let base = temp_dir("through");
    let dir = base.join("skills");
    let victim = base.join("victim/escaped.md");
    std::fs::create_dir_all(dir.join("fsl")).expect("create");
    std::fs::create_dir_all(base.join("victim")).expect("create");
    std::os::unix::fs::symlink(&victim, dir.join("fsl/SKILL.md")).expect("link");

    let installation = Installation::open(&dir, false).expect("open");
    let plan = installation.plan_install().expect("plan");
    let entry = plan
        .entries()
        .iter()
        .find(|entry| entry.path == "fsl/SKILL.md")
        .expect("planned");
    assert_eq!(entry.state, EntryState::Foreign);
    assert_eq!(entry.action, PlanAction::Blocked);

    installation.install(&plan, "4.6.0").expect("install");
    assert!(!victim.exists(), "nothing may be written through the link");

    // `--force` claims the path, and claiming it means removing the link.
    // What the link aimed at is not ours to write.
    let forced = Installation::open(&dir, true).expect("reopen forced");
    let plan = forced.plan_install().expect("plan");
    forced.install(&plan, "4.6.0").expect("forced install");
    assert!(!victim.exists(), "not through the link, even forced");
    assert!(
        !std::fs::symlink_metadata(dir.join("fsl/SKILL.md"))
            .expect("read")
            .file_type()
            .is_symlink(),
        "the link is replaced by our file"
    );
}

/// A link at a *parent* component leads out of the directory just as a link
/// at the file does, and text alone cannot see it.
///
/// The manifest key stays contained (`fsl/SKILL.md`), so `is_contained`
/// accepts it. `create_dir_all`, `write` and `remove_file` then follow the
/// link. A repository can carry both halves, so a fresh clone plus one
/// `fslc skills install` wrote outside the directory it named, and
/// `uninstall` deleted outside it.
#[cfg(unix)]
#[test]
fn a_linked_parent_component_cannot_carry_a_write_or_a_removal_outside() {
    let base = temp_dir("linked-parent");
    let dir = base.join("skills");
    let victim = base.join("victim");
    std::fs::create_dir_all(&dir).expect("create");
    std::fs::create_dir_all(victim.join("secrets")).expect("create");
    std::fs::write(victim.join("secrets/key"), "private\n").expect("write");
    std::os::unix::fs::symlink(&victim, dir.join("fsl")).expect("link");

    // Writing, and `--force` does not reach it either.
    for force in [false, true] {
        let installation = Installation::open(&dir, force).expect("open");
        let refusal = installation
            .plan_install()
            .expect_err("planning must refuse");
        assert!(refusal.contains("is a symbolic link"), "{refusal}");
        assert!(refusal.contains("would leave"), "{refusal}");
    }
    assert_eq!(
        std::fs::read_dir(&victim).expect("read").count(),
        1,
        "nothing may be written through a linked parent"
    );

    // Removing, driven by a manifest that names a file inside the link.
    std::fs::remove_file(dir.join("fsl")).expect("unlink");
    std::os::unix::fs::symlink(victim.join("secrets"), dir.join("fsl")).expect("link");
    let digest = fsl_skills::installation::manifest::digest_for_test(b"private\n");
    std::fs::write(
        dir.join(".fslc-skills.json"),
        format!(
            r#"{{"schema":"fslc.skills.v1","fslc_version":"4.6.0","scope":"project","entries":{{"fsl/key":"{digest}"}}}}"#
        ),
    )
    .expect("write");

    let installation = Installation::open(&dir, true).expect("open");
    let refusal = installation
        .plan_uninstall()
        .expect_err("removal must refuse too");
    assert!(refusal.contains("is a symbolic link"), "{refusal}");
    assert!(
        victim.join("secrets/key").exists(),
        "a file outside the directory must survive"
    );
}

/// A write that fails halfway must not disown what a previous run recorded.
///
/// Recording only the prefix this run reached rewrote a complete manifest as
/// an empty one, with every file still on disk. `install` then called its own
/// writes foreign, and `uninstall` reported success while removing nothing.
#[cfg(unix)]
#[test]
fn a_failed_write_keeps_recording_what_the_run_before_placed() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = temp_dir("half-written").join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    installation
        .install(&installation.plan_install().expect("plan"), "4.6.0")
        .expect("install");
    let before = Manifest::read(&dir).expect("read").expect("a manifest");
    assert!(before.entries.len() > 1);

    // One file ours, recorded, and stale, so the next run has to rewrite it.
    let (relative, _) = EMBEDDED_SKILL_FILES[0];
    let path = dir.join(relative);
    std::fs::write(&path, "locally changed\n").expect("write");
    let mut manifest = before.clone();
    manifest.entries.insert(
        (*relative).to_owned(),
        fsl_skills::installation::manifest::digest_for_test(b"locally changed\n"),
    );
    manifest.write(&dir).expect("write");
    // The file exists, so only its own mode stops the rewrite. A read-only
    // parent would not: `write` opens an existing file without it.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).expect("chmod");

    let installation = Installation::open(&dir, false).expect("reopen");
    let plan = installation.plan_install().expect("plan");
    installation
        .install(&plan, "4.6.0")
        .expect_err("the write must fail");

    let after = Manifest::read(&dir).expect("read").expect("a manifest");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
    assert_eq!(
        after.entries.len(),
        before.entries.len(),
        "every file still on disk stays recorded"
    );
}

/// A path this binary no longer embeds is still ours, and goes.
#[test]
fn a_skill_the_binary_no_longer_carries_is_removed() {
    let base = temp_dir("retired");
    let dir = base.join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    installation
        .install(&installation.plan_install().expect("plan"), "4.6.0")
        .expect("install");

    let retired = dir.join("oldskill/SKILL.md");
    std::fs::create_dir_all(retired.parent().expect("a parent")).expect("create");
    std::fs::write(&retired, "retired\n").expect("write");
    let mut manifest = Manifest::read(&dir).expect("read").expect("a manifest");
    manifest.entries.insert(
        "oldskill/SKILL.md".to_owned(),
        fsl_skills::installation::manifest::digest_for_test(b"retired\n"),
    );
    manifest.write(&dir).expect("write");

    let reopened = Installation::open(&dir, false).expect("reopen");
    let plan = reopened.plan_install().expect("plan");
    assert!(
        !plan.is_settled(),
        "a retirement is work, not a settled set"
    );
    reopened.install(&plan, "4.6.0").expect("install");
    assert!(!retired.exists(), "the retired skill goes");
    assert!(
        !Manifest::read(&dir)
            .expect("read")
            .expect("a manifest")
            .entries
            .contains_key("oldskill/SKILL.md"),
        "and stops being recorded"
    );
}

/// Pruning stops at a directory somebody else put something in.
#[test]
fn a_directory_holding_someone_elses_file_survives_removal() {
    let base = temp_dir("keep-parent");
    let dir = base.join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    installation
        .install(&installation.plan_install().expect("plan"), "4.6.0")
        .expect("install");
    let mine = dir.join("fsl/references/mine.md");
    std::fs::write(&mine, "notes\n").expect("write");

    let reopened = Installation::open(&dir, false).expect("reopen");
    let plan = reopened.plan_uninstall().expect("plan");
    reopened.uninstall(&plan).expect("uninstall");

    assert!(mine.exists(), "a handwritten file keeps its parent");
    assert!(dir.join("fsl/references").exists());
    assert!(
        !dir.join("fsl-business").exists(),
        "a directory this install emptied is pruned"
    );
}

/// A retired file somebody edited is refused, like any other edited file.
#[test]
fn an_edited_retired_file_blocks_the_install_that_would_remove_it() {
    let dir = temp_dir("retired-edited").join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    installation
        .install(&installation.plan_install().expect("plan"), "4.6.0")
        .expect("install");

    // Record a path this binary does not embed, then edit it.
    let retired = dir.join("oldskill/SKILL.md");
    std::fs::create_dir_all(retired.parent().expect("a parent")).expect("create");
    std::fs::write(&retired, "as recorded\n").expect("write");
    let mut manifest = Manifest::read(&dir).expect("read").expect("a manifest");
    manifest.entries.insert(
        "oldskill/SKILL.md".to_owned(),
        fsl_skills::installation::manifest::digest_for_test(b"as recorded\n"),
    );
    manifest.write(&dir).expect("write");
    std::fs::write(&retired, "then edited\n").expect("edit");

    let plan = Installation::open(&dir, false)
        .expect("reopen")
        .plan_install()
        .expect("plan");
    assert_eq!(plan.blocked(), vec!["oldskill/SKILL.md"]);
    assert_eq!(
        std::fs::read_to_string(&retired).expect("read"),
        "then edited\n",
        "a refusal must not remove it"
    );

    let forced = Installation::open(&dir, true).expect("reopen");
    assert!(forced.plan_install().expect("plan").blocked().is_empty());
    forced
        .install(&forced.plan_install().expect("plan"), "4.6.0")
        .expect("install");
    assert!(!retired.exists(), "--force retires it");
}

/// A retired path already gone, or edited since, is not removed quietly.
///
/// Retirement deletes, so what it plans matters as much as how it classifies.
/// Asserting only the state left the action free to become `Remove`, which
/// `remove_files` carries out without complaint because it tolerates a path
/// that is not there.
#[test]
fn a_retired_path_that_is_not_ours_any_more_is_not_removed() {
    let dir = temp_dir("retired-gone").join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    installation
        .install(&installation.plan_install().expect("plan"), "4.6.0")
        .expect("install");

    let mut manifest = Manifest::read(&dir).expect("read").expect("a manifest");
    manifest
        .entries
        .insert("gone/SKILL.md".to_owned(), "0".repeat(64));
    manifest
        .entries
        .insert("theirs/SKILL.md".to_owned(), "0".repeat(64));
    manifest.write(&dir).expect("write");
    std::fs::create_dir_all(dir.join("theirs")).expect("create");
    std::fs::write(dir.join("theirs/SKILL.md"), "not mine\n").expect("write");

    let plan = Installation::open(&dir, false)
        .expect("reopen")
        .plan_install()
        .expect("plan");
    for (path, state, action) in [
        ("gone/SKILL.md", EntryState::Absent, PlanAction::Skip),
        ("theirs/SKILL.md", EntryState::Modified, PlanAction::Blocked),
    ] {
        let entry = plan
            .entries()
            .iter()
            .find(|entry| entry.path == path)
            .unwrap_or_else(|| panic!("{path} must be planned"));
        assert_eq!(entry.state, state, "{path}");
        assert_eq!(entry.action, action, "{path}");
    }
    assert_eq!(plan.blocked(), vec!["theirs/SKILL.md"]);

    // Carrying the plan out leaves the file alone. `install` skips a blocked
    // entry rather than failing: the refusal is the CLI's to report, from
    // `plan.blocked()`, and the exit code follows from that.
    Installation::open(&dir, false)
        .expect("reopen")
        .install(&plan, "4.6.0")
        .expect("install");
    assert_eq!(
        std::fs::read_to_string(dir.join("theirs/SKILL.md")).expect("read"),
        "not mine\n"
    );
}

/// Removing what no manifest records is nothing, not an error.
#[test]
fn a_removal_plan_without_a_manifest_is_empty() {
    let dir = temp_dir("no-manifest").join("skills");
    std::fs::create_dir_all(&dir).expect("create");
    let plan = Installation::open(&dir, false)
        .expect("open")
        .plan_uninstall()
        .expect("plan");
    assert_eq!(plan.entries().len(), 0);
    assert!(plan.is_settled(), "nothing planned is settled");
    assert_eq!(plan.removals(), 0);
}

/// A file the manifest records that somebody already deleted is not an error.
#[test]
fn removing_a_file_that_is_already_gone_is_not_a_failure() {
    let dir = temp_dir("already-gone").join("skills");
    let installation = Installation::open(&dir, false).expect("open");
    installation
        .install(&installation.plan_install().expect("plan"), "4.6.0")
        .expect("install");

    let reopened = Installation::open(&dir, false).expect("reopen");
    let plan = reopened.plan_uninstall().expect("plan");
    // Delete one behind the plan's back, the way a second process would.
    std::fs::remove_file(dir.join("fsl/SKILL.md")).expect("remove");

    let removed = reopened
        .uninstall(&plan)
        .expect("removal tolerates absence");
    assert_eq!(
        removed,
        EMBEDDED_SKILL_FILES.len() - 1,
        "it reports what it actually took"
    );
    assert!(Manifest::read(&dir).expect("read").is_none());
}

/// The message names the scope that is there and the flag that reaches it.
#[test]
fn the_scope_refusal_names_the_flag_for_each_direction() {
    let dir = temp_dir("scope-message");
    for (scope, expected) in [
        (Scope::User, "--user"),
        (Scope::Project, "--dir, or from inside the project"),
    ] {
        let here = dir.join(scope.as_str());
        std::fs::create_dir_all(&here).expect("create");
        Manifest::new("4.6.0", scope).write(&here).expect("write");

        let wanted = if scope == Scope::User {
            Scope::Project
        } else {
            Scope::User
        };
        let error = fsl_skills::installation::manifest::read_for_test(&here, wanted)
            .expect_err("the other scope is refused");
        assert!(error.contains(scope.as_str()), "{error}");
        assert!(error.contains(expected), "{error}");
    }
}

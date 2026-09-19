// SPDX-License-Identifier: Apache-2.0

//! The `fslc skills` contract: what it answers, and what it exits with.
//!
//! `--dir` names the directory outright, so none of this reads the home
//! directory or the data directory. The machine-wide scope is exercised by the
//! unit tests inside `skills::installation::user`, which can reach it.

use std::path::{Path, PathBuf};

use serde_json::Value;

use fsl_skills::EMBEDDED_SKILL_FILES;
use fsl_skills::cli::run;

const VERSION: &str = "4.6.0";

fn temp_dir(name: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("fslc-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("create the temporary directory");
    base
}

fn skills(args: &[&str]) -> Result<(Value, i32), String> {
    run(args.iter().map(|argument| (*argument).to_owned()), VERSION)
}

/// Run a subcommand against `dir`, and fail the test on an argument error.
fn at(dir: &Path, args: &[&str]) -> (Value, i32) {
    let mut full = vec![args[0], "--dir", dir.to_str().expect("UTF-8 path")];
    full.extend_from_slice(&args[1..]);
    skills(&full).expect("the command should run")
}

fn result_of(value: &Value) -> &str {
    value["result"].as_str().expect("a result")
}

fn count(value: &Value, field: &str) -> u64 {
    value[field]
        .as_u64()
        .unwrap_or_else(|| panic!("{field} is a count"))
}

#[test]
fn a_fresh_install_reports_what_it_wrote() {
    let dir = temp_dir("install").join("skills");
    let (body, status) = at(&dir, &["install"]);

    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "installed");
    assert_eq!(count(&body, "written"), EMBEDDED_SKILL_FILES.len() as u64);
    assert_eq!(body["scope"], "project");
    assert_eq!(body["command"], "skills install");
    assert_eq!(body["fslc_version"], VERSION);
    assert_eq!(body["dry_run"], false);
    assert_eq!(
        body["files"].as_array().expect("a file listing").len(),
        EMBEDDED_SKILL_FILES.len()
    );
}

#[test]
fn installing_twice_reports_up_to_date() {
    let dir = temp_dir("twice").join("skills");
    at(&dir, &["install"]);
    let (body, status) = at(&dir, &["install"]);

    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "up_to_date");
    assert_eq!(count(&body, "written"), 0);
}

#[test]
fn status_before_an_install_says_so_and_fails() {
    let dir = temp_dir("status-absent").join("skills");
    let (body, status) = at(&dir, &["status"]);

    assert_eq!(status, 1, "status works as a check, not only a report");
    assert_eq!(result_of(&body), "not_installed");
    assert!(body.get("files").is_none(), "there is nothing to list");
}

#[test]
fn status_after_an_install_succeeds() {
    let dir = temp_dir("status-present").join("skills");
    at(&dir, &["install"]);
    let (body, status) = at(&dir, &["status"]);

    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "up_to_date");
    assert_eq!(body["installed_version"], VERSION);
}

#[test]
fn a_hand_edited_file_blocks_an_install_and_survives_it() {
    let dir = temp_dir("blocked").join("skills");
    at(&dir, &["install"]);
    let edited = dir.join("fsl/SKILL.md");
    std::fs::write(&edited, "mine now\n").expect("edit");

    let (body, status) = at(&dir, &["install"]);
    assert_eq!(status, 1);
    assert_eq!(result_of(&body), "blocked");
    assert_eq!(body["blocked"], serde_json::json!(["fsl/SKILL.md"]));
    assert_eq!(
        std::fs::read_to_string(&edited).expect("read"),
        "mine now\n",
        "a refusal must not write"
    );

    let (body, status) = at(&dir, &["status"]);
    assert_eq!(status, 1);
    assert_eq!(result_of(&body), "drifted");

    let (body, status) = at(&dir, &["install", "--force"]);
    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "installed");
    assert_eq!(count(&body, "written"), 1);
}

#[test]
fn an_install_by_another_version_is_stale() {
    let dir = temp_dir("stale").join("skills");
    at(&dir, &["install"]);

    let (body, status) = run(
        ["status", "--dir", dir.to_str().expect("UTF-8 path")]
            .iter()
            .map(|argument| (*argument).to_owned()),
        "9.9.9",
    )
    .expect("the command should run");
    assert_eq!(status, 1);
    assert_eq!(result_of(&body), "stale");
    assert_eq!(body["installed_version"], VERSION);
}

#[test]
fn a_dry_run_plans_without_writing() {
    let dir = temp_dir("dry-run").join("skills");
    let (body, status) = at(&dir, &["install", "--dry-run"]);

    assert_eq!(status, 0);
    assert_eq!(body["dry_run"], true);
    assert_eq!(count(&body, "written"), EMBEDDED_SKILL_FILES.len() as u64);
    assert!(!dir.exists(), "a dry run must leave the directory alone");
}

#[test]
fn a_dry_run_removal_writes_nothing_either() {
    let dir = temp_dir("dry-run-removal").join("skills");
    at(&dir, &["install"]);
    let (body, status) = at(&dir, &["uninstall", "--dry-run"]);

    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "removed");
    assert_eq!(count(&body, "removed"), EMBEDDED_SKILL_FILES.len() as u64);
    assert!(
        dir.join("fsl/SKILL.md").exists(),
        "a dry run must leave the files alone"
    );
}

#[test]
fn uninstall_reports_what_it_took_back() {
    let dir = temp_dir("uninstall").join("skills");
    at(&dir, &["install"]);
    let (body, status) = at(&dir, &["uninstall"]);

    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "removed");
    assert_eq!(count(&body, "removed"), EMBEDDED_SKILL_FILES.len() as u64);

    let (body, status) = at(&dir, &["uninstall"]);
    assert_eq!(status, 1);
    assert_eq!(result_of(&body), "not_installed");
}

#[test]
fn every_response_carries_the_same_header() {
    let dir = temp_dir("header").join("skills");
    for arguments in [
        vec!["install"],
        vec!["status"],
        vec!["uninstall"],
        vec!["status"],
    ] {
        let (body, _) = at(&dir, &arguments);
        for field in ["command", "scope", "directory", "fslc_version", "dry_run"] {
            assert!(body.get(field).is_some(), "{} lacks {field}", arguments[0]);
        }
        assert_eq!(body["directory"], dir.display().to_string());
    }
}

#[test]
fn an_unknown_subcommand_is_an_argument_error() {
    let error = skills(&["frobnicate"]).expect_err("unknown subcommands are rejected");
    assert!(error.contains("frobnicate"), "{error}");
    assert!(error.contains("usage:"), "{error}");
}

#[test]
fn an_unknown_option_is_an_argument_error() {
    let error = skills(&["install", "--frobnicate"]).expect_err("unknown options are rejected");
    assert!(error.contains("--frobnicate"), "{error}");
}

#[test]
fn dir_without_a_path_is_an_argument_error() {
    let error = skills(&["install", "--dir"]).expect_err("--dir needs a value");
    assert!(error.contains("--dir"), "{error}");
}

#[test]
fn dir_and_user_together_are_an_argument_error() {
    let error =
        skills(&["install", "--dir", "/tmp/x", "--user"]).expect_err("the two cannot combine");
    assert!(error.contains("--dir"), "{error}");
    assert!(error.contains("--user"), "{error}");
}

#[test]
fn no_subcommand_prints_the_usage() {
    let error = skills(&[]).expect_err("a subcommand is required");
    assert!(error.contains("usage:"), "{error}");
}

/// A disk fault is not a usage error, and every other command says so.
#[test]
fn a_filesystem_failure_is_reported_as_io_not_usage() {
    let base = temp_dir("io");
    let not_a_directory = base.join("skills");
    std::fs::write(&not_a_directory, "in the way\n").expect("write");

    let (body, status) = skills(&[
        "install",
        "--dir",
        not_a_directory.to_str().expect("UTF-8 path"),
    ])
    .expect("a filesystem fault is an envelope, not an argument error");
    assert_eq!(status, 2);
    assert_eq!(body["result"], "error");
    assert_eq!(body["kind"], "io");
}

/// A manifest that cannot be parsed is a fault, not a usage mistake.
#[test]
fn an_unreadable_manifest_is_reported_as_io() {
    let dir = temp_dir("bad-manifest").join("skills");
    std::fs::create_dir_all(&dir).expect("create");
    std::fs::write(dir.join(".fslc-skills.json"), "{not json").expect("write");

    let (body, status) = at(&dir, &["status"]);
    assert_eq!(status, 2);
    assert_eq!(body["kind"], "io");
}

/// An argument mistake stays a usage error.
#[test]
fn argument_errors_stay_usage_errors() {
    for arguments in [
        vec!["frobnicate"],
        vec!["install", "--frobnicate"],
        vec!["install", "--dir"],
    ] {
        assert!(
            skills(&arguments).is_err(),
            "{arguments:?} must be an argument error"
        );
    }
}

/// Acting on the other scope's manifest replaces the record of an install
/// that is still on disk, and every file it named becomes unremovable.
#[test]
fn a_manifest_from_the_other_scope_is_refused_by_every_subcommand() {
    let dir = temp_dir("scope").join("skills");
    at(&dir, &["install"]);

    // Forge the scope the other half writes, leaving the entries in place.
    let manifest = dir.join(".fslc-skills.json");
    let body = std::fs::read_to_string(&manifest).expect("read");
    std::fs::write(
        &manifest,
        body.replace("\"scope\": \"project\"", "\"scope\": \"user\""),
    )
    .expect("write");

    for verb in ["install", "status", "uninstall"] {
        let (body, status) = at(&dir, &[verb]);
        assert_eq!(status, 2, "{verb} must refuse the other scope");
        assert_eq!(body["kind"], "io");
        assert!(
            body["message"]
                .as_str()
                .expect("a message")
                .contains("user scope"),
            "{verb} must say which scope it found"
        );
    }

    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest).expect("read")).expect("parse");
    assert_eq!(
        after["entries"].as_object().expect("entries").len(),
        EMBEDDED_SKILL_FILES.len(),
        "the record of the install still on disk must survive"
    );
}

/// The manifest is written last and by rename, so a run that dies partway
/// leaves the previous manifest intact rather than a partial one. That is what
/// makes a claim file unnecessary for work this small.
#[test]
fn a_manifest_is_written_last_and_in_one_step() {
    let dir = temp_dir("atomic").join("skills");
    at(&dir, &["install"]);
    assert!(dir.join(".fslc-skills.json").is_file());
    assert!(
        std::fs::read_dir(&dir)
            .expect("read")
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".fslc-skills.json.")),
        "no staging file may survive a write"
    );

    // A manifest that is already there survives a refusal: the run that would
    // replace it never gets far enough to.
    let before = std::fs::read_to_string(dir.join(".fslc-skills.json")).expect("read");
    std::fs::write(dir.join("fsl/SKILL.md"), "mine\n").expect("edit");
    let (_, status) = at(&dir, &["install"]);
    assert_eq!(status, 1);
    assert_eq!(
        std::fs::read_to_string(dir.join(".fslc-skills.json")).expect("read"),
        before,
        "a refusal leaves the previous manifest intact"
    );
}

/// The staging file a write goes through has to be one this run created.
///
/// It used to be `.fslc-skills.json.<pid>`, written with a call that follows
/// symbolic links. Anyone able to write in the skills directory could plant
/// that name aimed at a file of their choosing: the manifest landed there,
/// outside the directory, and the rename then moved the link into place as
/// the manifest. That is what decides which paths a removal deletes, so the
/// redirection outlived the run.
#[cfg(unix)]
#[test]
fn a_planted_staging_name_cannot_take_the_manifest_outside_the_directory() {
    let base = temp_dir("planted-staging");
    let dir = base.join("skills");
    let victim = base.join("victim.txt");
    std::fs::create_dir_all(&dir).expect("create");
    std::fs::write(&victim, "somebody else's bytes\n").expect("write");
    // The process id was the whole of the guessable part.
    std::os::unix::fs::symlink(
        &victim,
        dir.join(format!(".fslc-skills.json.{}", std::process::id())),
    )
    .expect("link");

    at(&dir, &["install"]);

    assert_eq!(
        std::fs::read_to_string(&victim).expect("read"),
        "somebody else's bytes\n",
        "nothing may be written outside the skills directory"
    );
    assert!(
        !std::fs::symlink_metadata(dir.join(".fslc-skills.json"))
            .expect("a manifest")
            .file_type()
            .is_symlink(),
        "the manifest must be a file this run wrote"
    );
}

/// `--dir` takes a path. Taking the next argument whatever it looked like
/// made `--dir --force` create a directory called `--force` in the working
/// directory and drop the flag the user passed.
#[test]
fn dir_refuses_an_option_where_a_path_belongs() {
    for option in ["--force", "--user", "--dry-run", "-x"] {
        let error = skills(&["install", "--dir", option]).expect_err("{option} must be refused");
        assert!(error.contains("requires a path"), "{option}: {error}");
        assert!(!Path::new(option).exists(), "{option} must not be created");
    }
}

/// A directory nobody may write is a fault, and it must not be mistaken for a
/// clean result. Running as root defeats the setup, so that case is skipped.
#[cfg(unix)]
#[test]
fn a_directory_that_cannot_be_written_is_reported_not_swallowed() {
    use std::os::unix::fs::PermissionsExt as _;

    let base = temp_dir("readonly");
    let dir = base.join("skills");
    std::fs::create_dir_all(&dir).expect("create");
    let mut mode = std::fs::metadata(&dir).expect("read").permissions();
    mode.set_mode(0o500);
    std::fs::set_permissions(&dir, mode).expect("set");

    let outcome = skills(&["install", "--dir", dir.to_str().expect("UTF-8 path")]);

    // Restore before asserting, so a failure does not leave the tree locked.
    let mut mode = std::fs::metadata(&dir).expect("read").permissions();
    mode.set_mode(0o700);
    std::fs::set_permissions(&dir, mode).expect("restore");

    let (body, status) = outcome.expect("a permission fault is an envelope");
    if body["result"] == "installed" {
        return; // running as root: the setup cannot deny anything
    }
    assert_eq!(status, 2);
    assert_eq!(body["kind"], "io");
    assert!(
        !dir.join("fsl/SKILL.md").exists(),
        "nothing may be reported as written that was not"
    );
}

/// Every other command leads with this. Building typed response structs made
/// it easy to drop, and it was dropped from the success responses only, while
/// the error envelope kept it.
#[test]
fn every_response_leads_with_the_envelope_version() {
    let dir = temp_dir("envelope").join("skills");
    for arguments in [
        vec!["install"],
        vec!["status"],
        vec!["install"],
        vec!["uninstall"],
        vec!["status"],
    ] {
        let (body, _) = at(&dir, &arguments);
        assert_eq!(
            body["fsl"], "1.0",
            "{} lacks the envelope version: {body}",
            arguments[0]
        );
    }

    // And the failure path, which takes a different route through the code.
    // Assert the status too: discarding it let the case pass even if the
    // install had succeeded, exercising no io failure at all.
    let (body, status) =
        skills(&["install", "--dir", "/proc/nonexistent/x"]).expect("the command should run");
    assert_eq!(status, 2, "this path must fail: {body}");
    assert_eq!(body["fsl"], "1.0", "an io failure lacks it: {body}");
}

/// The published contract is the parser surface. `status` reports; it never
/// writes, and its help and contract node declare neither flag.
#[test]
fn status_refuses_the_flags_it_does_not_declare() {
    let dir = temp_dir("status-flags").join("skills");
    at(&dir, &["install"]);
    for flag in ["--force", "--dry-run"] {
        let error = skills(&["status", "--dir", dir.to_str().expect("UTF-8"), flag])
            .expect_err("status must refuse a flag it does not declare");
        assert!(error.contains(flag), "{error}");
    }
}

/// A sibling names the subcommand before it reads options.
#[test]
fn an_unknown_subcommand_is_reported_before_its_options() {
    let error = skills(&["bogus", "--also-bogus"]).expect_err("rejected");
    assert!(
        error.contains("unknown skills subcommand 'bogus'"),
        "{error}"
    );
    assert!(
        !error.contains("--also-bogus"),
        "the subcommand is the error, not the flag: {error}"
    );
}

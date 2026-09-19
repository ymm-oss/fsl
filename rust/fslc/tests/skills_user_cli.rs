// SPDX-License-Identifier: Apache-2.0

//! `fslc skills --user`, and the search a project install runs without
//! `--dir`, driven through the real binary.
//!
//! `skills_cli.rs` reaches the project scope with `--dir`, which reads neither
//! the home directory nor the data directory. That left every line between the
//! arguments and the machine-wide installation unexecuted, and four defects
//! lived in exactly that gap.
//!
//! The environment goes to a subprocess rather than to this one. `set_var` is
//! `unsafe` in this edition and the workspace forbids `unsafe`, and a
//! process-wide variable would make these tests fight each other anyway.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// A home and a data directory this test owns.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("fslc-user-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).expect("create the sandbox");
        std::fs::create_dir_all(root.join("data")).expect("create the sandbox");
        // The command reports the directory it resolved, and a resolved path
        // has followed every link on the way. macOS puts the temporary
        // directory behind `/var -> private/var`, so compare against the same
        // form the command will produce.
        let root = std::fs::canonicalize(&root).expect("canonicalize the sandbox");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn skills_dir(&self) -> PathBuf {
        self.home().join(".claude/skills")
    }

    /// Run the command with this sandbox's environment, from `cwd`.
    fn run_in(&self, cwd: &std::path::Path, args: &[&str]) -> (Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .arg("skills")
            .args(args)
            .current_dir(cwd)
            .env("HOME", self.home())
            .env("FSL_DATA_DIR", self.data())
            .env_remove("XDG_DATA_HOME")
            .output()
            .expect("run fslc");
        let body = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid JSON for `fslc skills {}`: {error}; stderr={}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (body, output.status.code().expect("an exit code"))
    }

    fn run(&self, args: &[&str]) -> (Value, i32) {
        self.run_in(&self.root, args)
    }

    /// Run with one environment variable overridden.
    fn run_with(&self, name: &str, value: &str, args: &[&str]) -> (Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .arg("skills")
            .args(args)
            .current_dir(&self.root)
            .env("HOME", self.home())
            .env("FSL_DATA_DIR", self.data())
            .env_remove("XDG_DATA_HOME")
            .env(name, value)
            .output()
            .expect("run fslc");
        (
            serde_json::from_slice(&output.stdout).expect("JSON"),
            output.status.code().expect("an exit code"),
        )
    }
}

fn result_of(value: &Value) -> &str {
    value["result"].as_str().expect("a result")
}

#[test]
fn a_user_install_links_through_current_and_reports_what_it_did() {
    let sandbox = Sandbox::new("install");
    let (body, status) = sandbox.run(&["install", "--user"]);

    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "installed");
    assert_eq!(body["scope"], "user");
    assert_eq!(body["current_repointed"], true);
    assert!(body["payload_written"].as_u64().expect("a count") > 0);

    let link = sandbox.skills_dir().join("fsl");
    assert_eq!(
        std::fs::read_link(&link).expect("read"),
        sandbox.data().join("current/skills/fsl"),
        "a link must point through current, not at a release"
    );
    assert!(link.exists(), "and it must resolve");

    let (body, status) = sandbox.run(&["status", "--user"]);
    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "up_to_date");
}

/// The defect a `--dir`-only suite could not see: a dry run reported a verdict
/// the real run then refused, and the refusal left a payload behind.
#[test]
fn a_user_dry_run_agrees_with_the_run_it_predicts() {
    let sandbox = Sandbox::new("dry-run");
    std::fs::create_dir_all(sandbox.data().join("releases/foreign")).expect("create");
    std::os::unix::fs::symlink("releases/foreign", sandbox.data().join("current")).expect("link");

    let predicted = sandbox.run(&["install", "--user", "--dry-run"]);
    let actual = sandbox.run(&["install", "--user"]);

    assert_eq!(
        predicted.1, actual.1,
        "a dry run must not promise an exit code the run will not give"
    );
    assert_eq!(predicted.0["kind"], actual.0["kind"]);

    let releases: Vec<_> = std::fs::read_dir(sandbox.data().join("releases"))
        .expect("read")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(
        releases.len(),
        1,
        "a refusal writes no payload: {releases:?}"
    );
}

#[test]
fn a_user_dry_run_writes_nothing_on_the_path_that_would_succeed() {
    let sandbox = Sandbox::new("dry-clean");
    let (body, status) = sandbox.run(&["install", "--user", "--dry-run"]);

    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "installed");
    assert!(
        !sandbox.data().join("releases").exists(),
        "a dry run writes no payload"
    );
    assert!(
        !sandbox.skills_dir().join("fsl").exists(),
        "and places no link"
    );
}

#[test]
fn a_user_uninstall_takes_back_the_links_and_the_payload() {
    let sandbox = Sandbox::new("uninstall");
    sandbox.run(&["install", "--user"]);
    let (body, status) = sandbox.run(&["uninstall", "--user"]);

    assert_eq!(status, 0);
    assert_eq!(result_of(&body), "removed");
    assert_eq!(
        body["removed"].as_u64().expect("a count"),
        fsl_skills::embedded_skill_names().len() as u64
    );
    assert_eq!(
        std::fs::read_dir(sandbox.skills_dir())
            .expect("read")
            .count(),
        0
    );
    // The name says "and the payload", so check it. Asserting only the link
    // count and an empty directory passed while the payload stayed on disk,
    // unreferenced and, because removal is manifest-gated, unremovable.
    let releases = sandbox.data().join("releases");
    let left: Vec<PathBuf> = walkdir(&releases)
        .into_iter()
        .filter(|path| path.is_file())
        .collect();
    assert!(left.is_empty(), "the payload is still on disk: {left:?}");

    let (body, status) = sandbox.run(&["status", "--user"]);
    assert_eq!(status, 1);
    assert_eq!(result_of(&body), "not_installed");
}

/// `--force` in the user scope removes a real directory and everything under
/// it, which is a different act from replacing a link.
///
/// This pins the choice rather than endorsing it. The envelope reports
/// `occupied` rather than `foreign` so the two are told apart before the user
/// reaches for `--force`.
#[cfg(unix)]
#[test]
fn a_real_directory_is_reported_apart_from_a_foreign_link() {
    let sandbox = Sandbox::new("occupied");
    let mine = sandbox.skills_dir().join("fsl/notes");
    std::fs::create_dir_all(&mine).expect("create");
    std::fs::write(mine.join("mine.md"), "my own work\n").expect("write");

    let (body, status) = sandbox.run(&["install", "--user"]);
    assert_eq!(status, 1, "{body}");
    assert_eq!(result_of(&body), "blocked");
    let entry = body["links"]
        .as_array()
        .expect("links")
        .iter()
        .find(|entry| entry["skill"] == "fsl")
        .expect("the entry");
    assert_eq!(entry["state"], "occupied", "not `foreign`: {entry}");
    assert!(mine.join("mine.md").exists(), "a refusal writes nothing");

    // A link somebody else placed is a different state, and stays one.
    std::fs::remove_dir_all(sandbox.skills_dir().join("fsl")).expect("remove");
    let elsewhere = sandbox.root.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("create");
    std::os::unix::fs::symlink(&elsewhere, sandbox.skills_dir().join("fsl")).expect("link");
    let (body, _) = sandbox.run(&["install", "--user"]);
    let entry = body["links"]
        .as_array()
        .expect("links")
        .iter()
        .find(|entry| entry["skill"] == "fsl")
        .expect("the entry");
    assert_eq!(entry["state"], "foreign", "{entry}");
}

/// An empty `HOME` is `Some("")`, and joining onto it writes wherever the user
/// happened to be standing, over links that resolve to nothing.
#[test]
fn a_home_that_is_not_an_absolute_directory_is_refused() {
    let sandbox = Sandbox::new("bad-home");
    for value in ["", "relative/home"] {
        let (body, status) = sandbox.run_with("HOME", value, &["install", "--user"]);
        assert_eq!(status, 2, "HOME={value:?} must be refused");
        assert_eq!(body["kind"], "io");
    }
    assert!(
        !sandbox.root.join(".claude").exists(),
        "nothing may be written beside the working directory"
    );
}

/// An empty value means unset, which is what the XDG specification says and
/// what every path here needs: an empty one becomes the working directory.
#[test]
fn an_empty_xdg_data_home_falls_back_rather_than_naming_the_working_directory() {
    let sandbox = Sandbox::new("empty-xdg");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["skills", "install", "--user"])
        .current_dir(&sandbox.root)
        .env("HOME", sandbox.home())
        .env("XDG_DATA_HOME", "")
        .env_remove("FSL_DATA_DIR")
        .output()
        .expect("run fslc");
    let body: Value = serde_json::from_slice(&output.stdout).expect("JSON");

    assert_eq!(result_of(&body), "installed");
    assert!(
        !sandbox.root.join("fsl").exists(),
        "an empty XDG_DATA_HOME must not resolve to the working directory"
    );
    assert!(
        sandbox.home().join(".local/share/fsl").exists(),
        "it falls back to the home directory"
    );
}

/// The path a project install picks when it is not given one.
#[test]
fn a_project_install_without_dir_lands_under_the_repository_root() {
    let sandbox = Sandbox::new("project");
    let repository = sandbox.root.join("work/repo");
    std::fs::create_dir_all(repository.join(".git")).expect("create");
    let deep = repository.join("nested/deep");
    std::fs::create_dir_all(&deep).expect("create");

    let (body, status) = sandbox.run_in(&deep, &["install"]);

    assert_eq!(status, 0);
    assert_eq!(body["scope"], "project");
    assert_eq!(
        body["directory"],
        repository.join(".claude/skills").display().to_string(),
        "the search stops at the repository root"
    );
    assert!(
        !sandbox.skills_dir().exists(),
        "and never reaches the home directory"
    );
}

/// A symbolic link on the way to the home directory must not defeat the
/// refusal.
///
/// `current_dir` resolves symbolic links and `HOME` does not. Comparing the
/// two as written let a project install land on the user-wide directory while
/// reporting project scope, which is exactly what the bound exists to stop.
#[cfg(unix)]
#[test]
fn a_symlinked_home_does_not_slip_past_the_user_scope_refusal() {
    let sandbox = Sandbox::new("symlinked-home");
    let real = sandbox.home();
    std::fs::create_dir_all(real.join(".claude")).expect("create");
    let link = sandbox.root.join("home-link");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&real, &link).expect("link");

    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["skills", "install", "--dry-run"])
        .current_dir(&real)
        .env("HOME", &link)
        .env("FSL_DATA_DIR", sandbox.data())
        .env_remove("XDG_DATA_HOME")
        .output()
        .expect("run fslc");
    let body: Value = serde_json::from_slice(&output.stdout).expect("JSON");

    assert_eq!(output.status.code(), Some(2), "{body}");
    assert_eq!(result_of(&body), "error");
    let message = body["message"].as_str().expect("a message");
    assert!(message.contains("is the user-wide directory"), "{message}");
}

/// The bounded search compares the candidate as written, and a link defeats
/// that.
///
/// A repository carrying `.claude/skills -> ~/.claude/skills` passed the
/// bound, wrote the user-wide directory, and reported `"scope": "project"`.
/// The manifest it left then refused every later `--user` run.
#[cfg(unix)]
#[test]
fn a_repository_cannot_link_its_skills_directory_at_the_user_wide_one() {
    let sandbox = Sandbox::new("linked-skills-dir");
    let user_wide = sandbox.skills_dir();
    std::fs::create_dir_all(&user_wide).expect("create");
    let repo = sandbox.root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).expect("create");
    std::fs::create_dir_all(repo.join(".claude")).expect("create");
    std::os::unix::fs::symlink(&user_wide, repo.join(".claude/skills")).expect("link");

    let (body, status) = sandbox.run_in(&repo, &["install"]);

    assert_eq!(status, 2, "{body}");
    let message = body["message"].as_str().expect("a message");
    assert!(message.contains("is the user-wide directory"), "{message}");
    assert_eq!(
        std::fs::read_dir(&user_wide).expect("read").count(),
        0,
        "nothing may be written there"
    );
}

/// `--dir` is compared against the user-wide directory, and a `..` detour
/// must not get past that.
///
/// `canonicalize` cannot resolve a directory that is not there yet, which is
/// the ordinary case on a fresh machine, and a plain compare folds `.` but
/// not `..`.
#[test]
fn a_parent_detour_does_not_get_dir_into_the_user_wide_directory() {
    let sandbox = Sandbox::new("dir-detour");
    std::fs::create_dir_all(sandbox.home().join(".claude")).expect("create");
    let detour = sandbox.home().join(".claude/x/../skills");

    let (body, status) = sandbox.run(&["install", "--dir", detour.to_str().expect("UTF-8")]);

    assert_eq!(status, 2, "{body}");
    let message = body["message"].as_str().expect("a message");
    assert!(message.contains("is the user-wide directory"), "{message}");
    assert!(
        !sandbox.skills_dir().exists(),
        "the user-wide directory must not be created"
    );

    // Somewhere else is still what `--dir` is for.
    let elsewhere = sandbox.root.join("project/skills");
    let (body, status) = sandbox.run(&["install", "--dir", elsewhere.to_str().expect("UTF-8")]);
    assert_eq!(status, 0, "{body}");
    assert_eq!(result_of(&body), "installed");
}

/// Outside a repository the search is one step, so the refusal names one path.
#[test]
fn outside_a_repository_the_refusal_names_the_one_place_it_looked() {
    let sandbox = Sandbox::new("outside");
    let plain = sandbox.root.join("plain");
    std::fs::create_dir_all(&plain).expect("create");

    let (body, status) = sandbox.run_in(&plain, &["install"]);

    assert_eq!(status, 2);
    assert_eq!(body["kind"], "io");
    let message = body["message"].as_str().expect("a message");
    assert!(message.contains("no .claude directory at"), "{message}");
    assert!(
        !message.contains("looked at:"),
        "the message must not promise a search that never happened: {message}"
    );
}

/// `current` has two owners. `install.sh` links `~/.local/bin/fslc` at
/// `current/bin/fslc`, so moving the pointer to a release that holds no
/// `bin/` leaves the command itself unreachable. `--force` is permission to
/// change which skills every project sees, not to uninstall the binary.
#[test]
fn force_will_not_move_current_off_the_release_holding_the_commands() {
    let sandbox = Sandbox::new("strand");
    let installed = sandbox.data().join("releases/vOLD");
    std::fs::create_dir_all(installed.join("bin")).expect("create");
    std::fs::write(installed.join("bin/fslc"), "the installed command\n").expect("write");
    std::os::unix::fs::symlink("releases/vOLD", sandbox.data().join("current")).expect("link");

    let (body, status) = sandbox.run(&["install", "--user", "--force"]);

    assert_eq!(status, 2, "--force must not strand the commands");
    assert_eq!(body["kind"], "io");
    assert!(
        body["message"]
            .as_str()
            .expect("a message")
            .contains("holds the installed commands"),
        "{body}"
    );
    assert_eq!(
        std::fs::read_link(sandbox.data().join("current")).expect("read"),
        std::path::Path::new("releases/vOLD"),
        "current must not have moved"
    );
    assert!(installed.join("bin/fslc").exists());
}

/// `install.sh` uses this exit code to decide whether the install settled, so
/// a payload emptied behind the links has to read as work to do.
#[test]
fn status_looks_past_the_links_at_the_payload_they_reach() {
    let sandbox = Sandbox::new("hollow");
    sandbox.run(&["install", "--user"]);
    let (_, status) = sandbox.run(&["status", "--user"]);
    assert_eq!(status, 0, "a fresh install is settled");

    for entry in walkdir(&sandbox.data().join("releases")) {
        if entry.extension().is_some_and(|ext| ext == "md") {
            std::fs::remove_file(&entry).expect("remove");
        }
    }

    let (body, status) = sandbox.run(&["status", "--user"]);
    assert_eq!(status, 1, "an emptied payload is not up to date: {body}");
    assert_eq!(result_of(&body), "drifted");
}

/// `install.sh:14-17` refuses a relative data directory; accepting one here
/// writes links that are dead from every other directory.
#[test]
fn a_relative_data_directory_is_refused_like_the_installer_refuses_it() {
    let sandbox = Sandbox::new("relative-data");
    let (body, status) = sandbox.run_with("FSL_DATA_DIR", "reldata", &["install", "--user"]);

    assert_eq!(status, 2);
    assert_eq!(body["kind"], "io");
    assert!(
        body["message"]
            .as_str()
            .expect("a message")
            .contains("not an absolute path"),
        "{body}"
    );
    assert!(!sandbox.root.join("reldata").exists());
}

/// Every file under a directory, deepest first.
fn walkdir(root: &std::path::Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walkdir(&path));
        } else {
            found.push(path);
        }
    }
    found
}

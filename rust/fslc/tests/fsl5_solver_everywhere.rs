// SPDX-License-Identifier: Apache-2.0

//! decisions/048 J3 through the native CLI: a decision table the finite-domain
//! enumeration leaves to the solver is rejected by every command that needs a
//! valid spec, not by `check` alone, with the diagnostic `check` reports.
//!
//! The enumeration defers a table for two reasons, one fixture each under
//! `fixtures/fsl5_solver/`: the fields' domains multiply past
//! `fsl_core::DOMAIN_BOUND` (`large_domain.fsl`), or a condition reads
//! something other than the entity's fields, here a resource
//! (`resource_condition.fsl`). Both leave a value no row covers. The same
//! documents with that gap closed are accepted by the same commands, so the
//! rejection is the solver's answer and not the deferral itself.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fsl5_solver")
}

/// A scratch directory holding one document.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, source: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("fsl5_solver_everywhere")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        std::fs::write(directory.join("doc.fsl"), source).expect("write scratch source");
        Self(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `fslc <command> doc.fsl` in `directory` with the `fsl5` gate set: the
/// JSON envelope (`None` for a command whose success is not JSON) and exit.
fn fslc(directory: &Path, command: &[&str]) -> (Option<Value>, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(command)
        .arg("doc.fsl")
        .current_dir(directory)
        .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
        .output()
        .expect("run fslc");
    (
        serde_json::from_slice(&output.stdout).ok(),
        output.status.code().expect("exit code"),
    )
}

/// The commands that read the spec into a model; each needs a valid one.
const COMMANDS: &[&[&str]] = &[
    &["check"],
    &["verify", "--depth", "3"],
    &["kernel"],
    &["testgen"],
    &["document", "generate"],
    &["scenarios"],
    &["ledger"],
    &["explain"],
    &["typestate"],
];

/// The parts of an error envelope that name the diagnostic.
fn diagnostic(output: &Value) -> Value {
    serde_json::json!({
        "result": output["result"],
        "kind": output["kind"],
        "diagnostic_code": output["diagnostic_code"],
        "message": output["message"],
        "loc": output["loc"],
    })
}

fn source(file: &str) -> String {
    std::fs::read_to_string(fixture_dir().join(file)).expect("read fixture")
}

fn replace_once(source: &str, from: &str, to: &str) -> String {
    assert_eq!(source.matches(from).count(), 1, "{from:?} occurs once");
    source.replacen(from, to, 1)
}

/// Every command rejects `file` with `check`'s diagnostic, which is the
/// solver's `decide_not_exhaustive` at the table with `reason`.
fn assert_rejected_everywhere(file: &str, reason: &str, line: u64) {
    let scratch = Scratch::new(file, &source(file));
    let (checked, exit) = fslc(&scratch.0, &["check"]);
    let checked = checked.expect("check prints an envelope");
    assert_eq!(exit, 2, "check {file}: {checked:#}");
    let expected = diagnostic(&checked);
    assert_eq!(expected["diagnostic_code"], "decide_not_exhaustive");
    assert_eq!(expected["loc"]["line"], line, "{checked:#}");
    let message = expected["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(&format!("(checked by the solver, because {reason})"))
            && message.contains("; counterexample: "),
        "{message}"
    );
    for command in COMMANDS {
        let (output, exit) = fslc(&scratch.0, command);
        assert_eq!(
            (exit, output.as_ref().map(diagnostic)),
            (2, Some(expected.clone())),
            "{command:?} on {file}: {output:#?}"
        );
    }
}

#[test]
fn a_table_past_the_enumeration_bound_is_rejected_by_every_command() {
    assert_rejected_everywhere(
        "large_domain.fsl",
        "its fields take 1212201 values, more than the 1000000 enumerated",
        27,
    );
}

#[test]
fn a_table_reading_a_resource_is_rejected_by_every_command() {
    assert_rejected_everywhere(
        "resource_condition.fsl",
        "it reads 'budget', which is not a field of the entity",
        26,
    );
}

/// The same documents with the gap closed: still deferred to the solver, and
/// accepted. The resource document by every command; the large one by those
/// that do not list its 1212201 values themselves (`testgen`, `scenarios`,
/// `ledger` and `explain` take seconds to minutes on it, with or without
/// this check).
#[test]
fn the_tables_with_the_gap_closed_are_accepted() {
    for (label, closed, commands) in [
        (
            "large_domain_closed",
            replace_once(
                &source("large_domain.fsl"),
                "concept Large = Claim where amount > 10",
                "concept Large = Claim where amount >= 10",
            ),
            &[
                &["check"][..],
                &["verify", "--depth", "3"],
                &["kernel"],
                &["document", "generate"],
                &["typestate"],
            ][..],
        ),
        (
            "resource_condition_closed",
            replace_once(
                &source("resource_condition.fsl"),
                "concept Expensive  = Claim where amount > budget + 1",
                "concept Expensive  = Claim where amount > budget",
            ),
            COMMANDS,
        ),
    ] {
        let scratch = Scratch::new(label, &closed);
        for command in commands {
            let (output, exit) = fslc(&scratch.0, command);
            assert_eq!(exit, 0, "{command:?} on {label}: {output:#?}");
        }
    }
}

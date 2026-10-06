// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native contract for #1226: `--instances` / `--values` reject a name the
//! document does not declare as `entity` / `number` — including the name of a
//! raw `type X = lo..hi` range — in `requirements` and `business` documents,
//! not only in a kernel `spec`.
//!
//! `docs/manual/LANGUAGE.md` ("A `NAME` with no matching `entity`/`number`
//! declaration ... is a spec error (exit code 2)") and the frozen Python
//! reference (`verify values for undeclared number 'X'`) both reject it. Before
//! #1226 the native `parse_kernel_source_with_bounds` validated override names
//! for a kernel `spec` only; for `requirements` / `business` it rewrote the
//! bounds the `verify` block already carried and dropped every other name
//! without a word, so `--values Level=0..2` returned a verdict over the full
//! declared `0..4` domain while echoing the override under `bounds_overrides`.
//!
//! Detectors name the mutation they kill: removing the
//! `validate_requirements_scope_overrides` / `validate_business_scope_overrides`
//! call (`origin/main`). Preservation controls pin the names that must still be
//! accepted, including a `process` name with no `entity` line.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const FIXTURE_DIR: &str = "rust/fslc/tests/fixtures/issue_1226_raw_range_values";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run(arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn verify(name: &str, overrides: &[&str]) -> (Value, i32) {
    let path = format!("{FIXTURE_DIR}/{name}");
    let mut arguments = vec!["verify", path.as_str(), "--depth", "6", "--no-cache"];
    arguments.extend_from_slice(overrides);
    run(&arguments)
}

fn sweep(name: &str, overrides: &[&str]) -> (Value, i32) {
    let path = format!("{FIXTURE_DIR}/{name}");
    let mut arguments = vec!["sweep", path.as_str(), "--depth", "3..3"];
    arguments.extend_from_slice(overrides);
    run(&arguments)
}

fn assert_rejected(output: &Value, status: i32, message: &str) {
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "semantics", "{output:#}");
    assert_eq!(output["message"], message, "{output:#}");
    assert!(
        output.get("bounds_overrides").is_none(),
        "a rejected override must not be echoed as applied: {output:#}"
    );
}

/// Detector: the issue's reproduction. `LowLevel` fails in the declared
/// `Level = 0..4` domain; before the fix this returned `violated` / exit 1
/// over that full domain with `bounds_overrides.values.Level = [0, 2]`.
#[test]
fn requirements_rejects_values_for_a_raw_range_name() {
    let (output, status) = verify("requirements.fsl", &["--values", "Level=0..2"]);
    assert_rejected(
        &output,
        status,
        "verify values references undeclared number 'Level' at 1:1",
    );
}

/// Detector: the `--instances` axis of the same hole.
#[test]
fn requirements_rejects_instances_for_a_raw_range_name() {
    let (output, status) = verify("requirements.fsl", &["--instances", "Level=1"]);
    assert_rejected(
        &output,
        status,
        "verify instances references undeclared entity 'Level' at 1:1",
    );
}

/// Detector: a typo'd name was dropped the same way.
#[test]
fn requirements_rejects_an_undeclared_name() {
    let (output, status) = verify("requirements.fsl", &["--values", "Amonut=0..0"]);
    assert_rejected(
        &output,
        status,
        "verify values references undeclared number 'Amonut' at 1:1",
    );
}

/// Detector: an entity name on the `--values` axis is not a number.
#[test]
fn requirements_rejects_values_for_a_process_entity() {
    let (output, status) = verify("requirements.fsl", &["--values", "Ticket=0..0"]);
    assert_rejected(
        &output,
        status,
        "verify values references undeclared number 'Ticket' at 1:1",
    );
}

/// Detector: `sweep` runs every cell through the same seam; before the fix
/// the grid read `sweep_failed` / exit 1 over the undeclared axis.
#[test]
fn sweep_rejects_values_for_a_raw_range_name() {
    let (output, status) = sweep("requirements.fsl", &["--values", "Level=0..2"]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(
        output["message"], "verify values references undeclared number 'Level' at 1:1",
        "{output:#}"
    );
}

/// Detector: business documents had the same gap on `--instances`.
#[test]
fn business_rejects_an_undeclared_entity() {
    let (output, status) = verify("business.fsl", &["--instances", "Chnage=1"]);
    assert_rejected(
        &output,
        status,
        "verify instances references undeclared entity 'Chnage' at 1:1",
    );
}

/// Detector: the business dialect has no `number`, so any `--values` names
/// nothing it declares.
#[test]
fn business_rejects_any_values_override() {
    let (output, status) = verify("business.fsl", &["--values", "Change=0..0"]);
    assert_rejected(
        &output,
        status,
        "verify values references undeclared number 'Change' at 1:1",
    );
}

/// Preservation: a declared `number` override still applies and is echoed.
#[test]
fn requirements_accepts_a_declared_number() {
    let (output, status) = verify("requirements.fsl", &["--values", "Amount=0..0"]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(
        output["bounds_overrides"]["values"]["Amount"],
        serde_json::json!([0, 0])
    );
}

/// Preservation: a `process` with no `entity` line is still an entity name
/// (`lower_requirements` adds it), so `--instances Ticket=1` applies.
#[test]
fn requirements_accepts_a_process_entity() {
    let (output, status) = verify("requirements.fsl", &["--instances", "Ticket=1"]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(output["bounds_overrides"]["instances"]["Ticket"], 1);
}

/// Preservation: a declared business `entity` override still applies.
#[test]
fn business_accepts_a_declared_entity() {
    let (output, status) = verify("business.fsl", &["--instances", "Change=1"]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
    assert_eq!(output["bounds_overrides"]["instances"]["Change"], 1);
}

/// Preservation: `sweep` over a declared number is unchanged.
#[test]
fn sweep_accepts_a_declared_number() {
    let (output, status) = sweep("requirements.fsl", &["--values", "Amount=1..1"]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "sweep_failed", "{output:#}");
}

/// Preservation: a kernel `spec` rejected a raw range name before #1226, with
/// this exact message, and still does.
#[test]
fn kernel_spec_rejection_is_unchanged() {
    let (output, status) = verify("spec.fsl", &["--values", "Level=0..2"]);
    assert_rejected(
        &output,
        status,
        "verify values references undeclared number 'Level' at 1:1",
    );
    let (output, status) = verify("spec.fsl", &["--values", "Amount=0..0"]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
}

fn diff(old: &Path, new: &Path) -> (Value, i32) {
    let old = old.display().to_string();
    let new = new.display().to_string();
    run(&["diff", &old, &new, "--depth", "2"])
}

fn scratch(tag: &str) -> PathBuf {
    let scratch =
        std::env::temp_dir().join(format!("fslc-issue-1226-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    scratch
}

/// Preservation: `fslc diff` forwards the new document's own `verify` bounds
/// to the scoped loader when the scope changed. A requirements bound naming
/// no declared `entity`/`number` (`Ghost`; a separate, pre-existing gap lets
/// it through `check`) never shaped the model; the diff must neither start
/// failing on it nor drop it from `applied_to_old` — the output is the
/// pre-#1226 one. Detector for forwarding it unfiltered (exit 2 with
/// `verify values references undeclared number 'Ghost' at 1:1`).
#[test]
fn diff_keeps_an_undeclared_verify_bound_out_of_the_scoped_load() {
    let scratch = scratch("diff-undeclared");
    let source = |ghost_hi: i64| {
        format!(
            "requirements DiffGhost {{\n  number Amount\n  state {{ amount: Amount }}\n  init {{ amount = 0 }}\n}}\nverify {{ values Amount = 0..1; values Ghost = 1..{ghost_hi} }}\n"
        )
    };
    let old = scratch.join("old.fsl");
    let new = scratch.join("new.fsl");
    std::fs::write(&old, source(3)).expect("write old");
    std::fs::write(&new, source(4)).expect("write new");
    let (output, status) = diff(&old, &new);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "semantic_diff", "{output:#}");
    assert_eq!(output["summary"], serde_json::json!(["scope_changed"]));
    assert_eq!(
        output["scope"]["applied_to_old"]["values"],
        serde_json::json!({"Amount": [0, 1], "Ghost": [1, 4]}),
        "{output:#}"
    );
    std::fs::remove_dir_all(scratch).expect("remove scratch directory");
}

/// Preservation: a `process` name is still forwarded to the scoped load, so
/// the old model is re-scoped to the new `Ticket` count.
#[test]
fn diff_still_rescopes_a_process_entity() {
    let scratch = scratch("diff-process");
    let source =
        std::fs::read_to_string(repository_root().join(format!("{FIXTURE_DIR}/requirements.fsl")))
            .expect("read fixture");
    let old = scratch.join("old.fsl");
    let new = scratch.join("new.fsl");
    std::fs::write(&old, &source).expect("write old");
    std::fs::write(
        &new,
        source.replace("instances Ticket = 1", "instances Ticket = 2"),
    )
    .expect("write new");
    let (output, status) = diff(&old, &new);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["summary"], serde_json::json!(["scope_changed"]));
    assert_eq!(output["scope"]["applied_to_old"]["instances"]["Ticket"], 2);
    assert_eq!(output["directions"]["old_to_new"]["result"], "refines");
    assert_eq!(output["directions"]["new_to_old"]["result"], "refines");
    std::fs::remove_dir_all(scratch).expect("remove scratch directory");
}

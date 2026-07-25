// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #466: `fslc refine`, `fslc diff`, and the
//! `implements`-clause mutation oracle must never report `refines` (or fold
//! into `no_semantic_change`/`gate.passed:true`) when the implementation
//! spec violates its own type bounds within `--depth`. That is a property
//! of the refinement *input*, not a refinement fidelity verdict.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn run(args: &[String]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn write(path: &Path, source: &str) {
    std::fs::write(path, source).expect("write fixture");
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fslc-issue-466-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

const ABS: &str = "spec MinAbs { type AQty = 0..1 state { n: AQty } init { n = 1 } \
     action dec() { requires n > 0 n = n - 1 } }";
const IMPL_SELF_VIOLATING: &str =
    "spec MinImpl { type IQty = 0..1 state { n: IQty } init { n = 1 } action dec() { n = n - 1 } }";
const MAPPING_AUTO: &str = "refinement M { impl MinImpl abs MinAbs maps auto }";

/// `fslc refine` on an impl that breaks its own type bound within `--depth`
/// must report `result:"violated"`/exit 1 with a `note` explaining this is
/// a property of the refinement input — never `refines`/exit 0.
#[test]
fn refine_reports_violated_not_refines_when_impl_breaks_its_own_bound() {
    let dir = scratch("refine");
    let implementation = dir.join("impl.fsl");
    let abstraction = dir.join("abs.fsl");
    let mapping = dir.join("map.fsl");
    write(&implementation, IMPL_SELF_VIOLATING);
    write(&abstraction, ABS);
    write(&mapping, MAPPING_AUTO);

    let (output, status) = run(&[
        "refine".to_owned(),
        implementation.display().to_string(),
        abstraction.display().to_string(),
        mapping.display().to_string(),
        "--depth".to_owned(),
        "4".to_owned(),
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated");
    assert_eq!(output["violation_kind"], "type_bound");
    assert_ne!(output["result"], "refines");
    assert!(
        output["note"]
            .as_str()
            .is_some_and(|note| note.contains("property of the impl spec itself")),
        "{output:#}"
    );
}

/// `fslc diff` must not fold an introduced self-violation into
/// `no_semantic_change`/`gate.passed:true`: it must appear as an
/// `impl_violated` finding and fail the gate unconditionally (not gated by
/// `--forbid`).
#[test]
fn diff_reports_impl_violated_and_fails_the_gate_unconditionally() {
    let dir = scratch("diff");
    let clean = dir.join("clean.fsl");
    let broken = dir.join("broken.fsl");
    // Same spec name and shape on both sides (diff's automatic identity
    // mapping requires matching state/action names); only the guard on
    // `dec()` differs, so `broken` violates its own type bound `n >= 0`
    // where `clean` does not.
    write(
        &clean,
        "spec DiffTarget { type Qty = 0..1 state { n: Qty } init { n = 1 } \
         action dec() { requires n > 0 n = n - 1 } }",
    );
    write(
        &broken,
        "spec DiffTarget { type Qty = 0..1 state { n: Qty } init { n = 1 } \
         action dec() { n = n - 1 } }",
    );

    let (output, status) = run(&[
        "diff".to_owned(),
        clean.display().to_string(),
        broken.display().to_string(),
        "--depth".to_owned(),
        "4".to_owned(),
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["gate"]["passed"], false);
    assert!(
        output["gate"]["violations"]
            .as_array()
            .is_some_and(|violations| violations.iter().any(|v| v == "impl_violated")),
        "{output:#}"
    );
    assert!(
        output["summary"]
            .as_array()
            .is_some_and(|summary| summary.iter().any(|s| s == "impl_violated")),
        "{output:#}"
    );
    assert_ne!(output["summary"], serde_json::json!(["no_semantic_change"]));
    assert!(
        output["findings"]
            .as_array()
            .is_some_and(|findings| findings
                .iter()
                .any(|finding| finding["kind"] == "impl_violated")),
        "{output:#}"
    );
}

/// Regression control: an ordinary clean `fslc diff` (neither side
/// self-violating) must keep returning `gate.passed:true`/exit 0, so the
/// `impl_violated` gate addition does not over-trigger.
#[test]
fn diff_still_passes_when_neither_side_self_violates() {
    let dir = scratch("diff-clean");
    let left = dir.join("left.fsl");
    let right = dir.join("right.fsl");
    let source = "spec DiffCleanTarget { type Qty = 0..1 state { n: Qty } init { n = 1 } \
         action dec() { requires n > 0 n = n - 1 } }";
    write(&left, source);
    write(&right, source);

    let (output, status) = run(&[
        "diff".to_owned(),
        left.display().to_string(),
        right.display().to_string(),
        "--depth".to_owned(),
        "4".to_owned(),
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["gate"]["passed"], true);
    assert_eq!(output["summary"], serde_json::json!(["no_semantic_change"]));
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #1119: the explicit runtime's `exists` arm
//! copied the *whole* satisfied local scope -- witnessing binder included --
//! back into the caller's bindings. When the binder shadowed an action
//! parameter of the same name, the witness overwrote the parameter, so the
//! action's effects wrote the witness's index instead of the caller's.
//!
//! In the spec below `act(r)`'s guard mentions `exists r: R { ... }`, whose
//! only witness is `r = 1`. `act(0)` therefore used to assign `w[1]`/`a[1]`
//! instead of `w[0]`/`a[0]`, and the `explicit` and `auto` engines answered
//! `proved` (exit 0) for the violated `NoW0`, while `bmc` and `induction`
//! caught the divergence only as an internal
//! `trace state mismatch at step 1` (exit 3). The conformance vectors
//! carried the same wrong outcome.
//!
//! `typecheck.rs`'s `Quantified` arm types a quantifier body in a *cloned*
//! environment, so no expression can legitimately name the binder outside
//! the quantifier; the symbolic evaluator (`fsl-verifier`'s
//! `eval_quantified`) and the frozen Python reference scope it the same way.
//! The binder must therefore never escape.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// `act(r)`'s `exists r: R` binder shadows the parameter `r`, and its only
/// witness (`r = 1`) differs from the parameter value the violation needs
/// (`r = 0`). That gap is what made the leak observable.
const SPEC: &str = r"spec Sh {
  type R = 0..1
  state { a: Map<R, Bool>, w: Map<R, Bool> }
  init { forall r: R { a[r] = true  w[r] = false } }
  action act(r: R) {
    requires a[r] and (exists r: R { a[r] and not (r == 0) })
    w[r] = true
    a[r] = false
  }
  invariant NoW0 { not w[0] }
}
";

fn scratch(tag: &str) -> PathBuf {
    let scratch =
        std::env::temp_dir().join(format!("fslc-issue-1119-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    scratch
}

fn fixture(tag: &str) -> PathBuf {
    let path = scratch(tag).join("shadowed.fsl");
    std::fs::write(&path, SPEC).expect("write fixture");
    path
}

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

fn verify(spec: &Path, engine: &str) -> (Value, i32) {
    run(&[
        "verify".to_owned(),
        spec.display().to_string(),
        "--engine".to_owned(),
        engine.to_owned(),
    ])
}

/// Every engine must agree that `NoW0` is violated, and the counterexample
/// must be `act(0)` raising `w[0]` -- not `w[1]`, the shadowing witness's
/// index.
#[test]
fn every_engine_reports_the_shadowed_parameter_counterexample() {
    let spec = fixture("engines");
    for engine in ["explicit", "bmc", "induction", "auto"] {
        let (report, code) = verify(&spec, engine);
        assert_eq!(report["result"], "violated", "{engine} verdict: {report:#}");
        assert_eq!(code, 1, "{engine} exit status: {report:#}");
        assert_eq!(report["violation_kind"], "invariant", "{engine}");
        assert_eq!(report["invariant"], "NoW0", "{engine}");
        assert_eq!(report["violated_at_step"], 1, "{engine}");

        let action = &report["last_action"];
        assert_eq!(action["name"], "act", "{engine}: {report:#}");
        assert_eq!(
            action["params"]["r"], 0,
            "{engine} blamed the witness index instead of the parameter: {report:#}"
        );

        let step = &report["trace"][1];
        assert_eq!(step["action"]["params"]["r"], 0, "{engine}: {report:#}");
        assert_eq!(
            step["state"]["w"]["0"], true,
            "{engine}: act(0) must write w[0]: {report:#}"
        );
        assert_eq!(
            step["state"]["w"]["1"], false,
            "{engine}: act(0) must not write w[1]: {report:#}"
        );
        assert_eq!(
            step["state"]["a"]["0"], false,
            "{engine}: act(0) must clear a[0]: {report:#}"
        );
        assert_eq!(
            step["state"]["a"]["1"], true,
            "{engine}: act(0) must not clear a[1]: {report:#}"
        );
        assert!(
            step["changes"].get("w[0]").is_some() && step["changes"].get("w[1]").is_none(),
            "{engine} changed the wrong key: {report:#}"
        );
    }
}

/// The same leak reached the language-neutral conformance vectors, which an
/// implementation is expected to replay verbatim: `act(r=0)` was recorded as
/// a clean `ok` outcome flipping `a[1]`/`w[1]`. It must instead record the
/// `NoW0` invariant outcome whose attempted state flips `a[0]`/`w[0]`.
#[test]
fn conformance_vectors_apply_the_parameter_not_the_witness() {
    let spec = fixture("conformance");
    let (report, code) = run(&["conformance".to_owned(), spec.display().to_string()]);
    assert_eq!(code, 0, "conformance exit status: {report:#}");

    let vectors = report["vectors"]
        .as_array()
        .unwrap_or_else(|| panic!("conformance vectors: {report:#}"));
    let initial = report["states"]
        .as_array()
        .and_then(|states| states.iter().find(|state| state["depth"] == 0))
        .unwrap_or_else(|| panic!("initial conformance state: {report:#}"));
    let initial_id = &initial["id"];

    let vector = vectors
        .iter()
        .find(|vector| {
            vector["state"] == *initial_id
                && vector["action"]["name"] == "act"
                && vector["action"]["params"]["r"] == 0
        })
        .unwrap_or_else(|| panic!("act(r=0) vector from the initial state: {report:#}"));

    let outcome = &vector["outcome"];
    assert_eq!(
        outcome["kind"], "invariant",
        "act(r=0) must break NoW0, not succeed on the witness's index: {report:#}"
    );
    assert_eq!(outcome["name"], "NoW0", "{report:#}");
    let attempted = &outcome["attempted_state"];
    assert_eq!(
        attempted["w"]["0"], true,
        "act(r=0) must raise w[0]: {report:#}"
    );
    assert_eq!(
        attempted["w"]["1"], false,
        "act(r=0) must not raise w[1]: {report:#}"
    );
    assert_eq!(
        attempted["a"]["0"], false,
        "act(r=0) must clear a[0]: {report:#}"
    );
    assert_eq!(
        attempted["a"]["1"], true,
        "act(r=0) must not clear a[1]: {report:#}"
    );
}

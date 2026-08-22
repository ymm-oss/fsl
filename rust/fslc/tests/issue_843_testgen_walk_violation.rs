// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Controls for #843: `testgen`'s fixed-seed conformance walk must not bake a
//! Monitor rollback as a step's `expected` state.
//!
//! The walk is a concrete Monitor run capped at 100 steps and is independent
//! of `--depth`, so it can reach a violation the bounded verification
//! `testgen` runs first proved absent within `depth`. `StepResult` was
//! discarded there, and because the Monitor rolls a violating step back, the
//! pre-step state was recorded as that step's `expected` -- a no-op
//! expectation no FSL contract states. The observable consequences were a
//! conforming implementation failing the generated test, an implementation
//! that silently does nothing passing it, and `--target pytest` (which drives
//! the walk live) disagreeing with the five baked targets on the same input.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn fixture(name: &str) -> String {
    format!("rust/fslc/tests/fixtures/{name}")
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fslc-issue-843-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

const TARGETS: [(&str, &str); 6] = [
    ("pytest", "py"),
    ("vitest", "test.ts"),
    ("swift", "swift"),
    ("kotlin", "kt"),
    ("dart", "dart"),
    ("phpunit", "php"),
];

/// REJECTING CONTROL. The walk's violation must abort generation with the same
/// verdict, exit code, property name, step, and replayable trace `verify`
/// reports for the same violation -- not exit 0 with a harness whose walk
/// asserts that `inc()` leaves `c` at 29.
#[test]
fn a_walk_violation_past_the_verified_depth_aborts_generation() {
    let spec = fixture("testgen_walk_late_violation.fsl");
    let output_path = scratch("late_violation.test.ts");

    // Precondition: the bounded verification `testgen` runs first is clean at
    // the default depth, so this spec reaches the walk at all. Without this
    // the test would pass through the pre-existing #472 propagation path and
    // prove nothing about the walk.
    let (verified, verified_status) = run(&["verify", &spec, "--depth", "8"]);
    assert_eq!(verified_status, 0, "{verified:#}");
    assert_eq!(verified["result"], "verified", "{verified:#}");

    let (output, status) = run(&[
        "testgen",
        &spec,
        "--target",
        "vitest",
        "-o",
        output_path.to_str().expect("scratch path"),
    ]);

    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(output["violation_kind"], "invariant", "{output:#}");
    assert_eq!(output["invariant"], "AtMost29", "{output:#}");
    assert_eq!(output["violated_at_step"], 30, "{output:#}");
    assert_eq!(output["last_action"]["name"], "inc", "{output:#}");

    // The trace is the evidence contract: it must be the full replayable path
    // from the initial state through the violating step, and the violating
    // entry must carry the successor the spec states (c = 30), NOT the
    // rolled-back state (c = 29) the old code baked as `expected`.
    let trace = output["trace"].as_array().expect("trace array");
    assert_eq!(trace.len(), 31, "initial state plus 30 steps: {output:#}");
    assert_eq!(trace[0]["state"]["c"], 0, "{output:#}");
    assert_eq!(
        trace[30]["state"]["c"], 30,
        "the violating entry must record the attempted successor, not the rollback: {output:#}"
    );

    assert!(
        !output_path.exists(),
        "no harness may be written for a violating walk: {}",
        output_path.display()
    );

    // The verdict must agree with `verify` at a depth that reaches the same
    // violation. A disagreement here is the dual-evaluator failure AGENTS.md
    // ranks above everything else.
    //
    // Compare the WHOLE envelope, not a hand-picked field list. An earlier
    // revision of this fix rendered the walk violation through
    // `render_boundary_output` -- `verify`'s `partial_op`/`type_bound` path --
    // which produced an identical `result`/`invariant`/`violated_at_step` while
    // silently emitting `loc: null`, `violating_bindings: null`, and
    // `blame: null`. A field list chosen by the implementer passed that
    // revision. Only a full-envelope comparison rejects it, and AGENTS.md
    // forbids allowlisting a location difference.
    let (verify, verify_status) = run(&["verify", &spec, "--depth", "30"]);
    assert_eq!(verify_status, status, "{verify:#}");

    assert_envelope_parity(&output, &verify);

    // Named explicitly so a reader of #843 sees which fields the first
    // revision of this fix lost, independent of the parity helper.
    for key in ["loc", "violating_bindings", "blame"] {
        assert!(!output[key].is_null(), "{key} must not be null: {output:#}");
        assert_eq!(output[key], verify[key], "{key} must match verify");
    }
}

/// Fail if an exclusion names a field neither envelope carries.
///
/// An exclusion that matches nothing excludes nothing, and reads as a
/// considered decision while being decoration. An earlier revision of
/// `assert_envelope_parity` listed eight "run-dependent" fields of which seven
/// (`elapsed_s`, `depth`, `engine`, `closure`, `states_explored`,
/// `max_frontier_width`, `statistics`) appear in NEITHER envelope on this path
/// -- the list had been copied from `BmcOutputOptions`' field names rather than
/// read off the emitted JSON. Editing the list closes that instance; this check
/// closes the class.
///
/// Deliberately NOT applied to `VERIFY_ONLY`: `cache`'s absence is a legitimate
/// cold-cache state, so requiring its presence would reintroduce the ambient
/// dependence that made this assertion flaky in the first place.
fn assert_exclusions_are_live(
    excluded: &[&str],
    testgen: &Map<String, Value>,
    verify: &Map<String, Value>,
) {
    for key in excluded {
        assert!(
            testgen.contains_key(*key) && verify.contains_key(*key),
            "exclusion '{key}' names a field one of the envelopes does not \
             carry, so it excludes nothing: delete it from NOT_COMPARABLE \
             rather than leaving it to imply a considered decision"
        );
    }
}

/// Compare `testgen`'s violation envelope against `verify`'s for the same
/// violation. What is checked, exactly:
///
/// - **Compared for equality: every key both envelopes carry**, except the one
///   listed in `NOT_COMPARABLE` below.
/// - **Compared as a subset: keys only `verify` carries.** Each must be a known
///   `verify`-only key; a new one fails. It is a subset rather than an exact set
///   because `cache`'s PRESENCE is ambient -- `verify` memoizes verdicts and
///   emits that block only once an entry exists. Pinning the exact set made
///   this assertion fail on a cold cache and pass on a warm one, which is the
///   same flake inverted rather than removed; it took a repeat run in one
///   session to see that.
/// - **Rejected outright: keys only `testgen` carries.** It must not invent
///   fields.
/// - **Excluded from comparison: `cost` only**, for the reason stated on it.
///
/// This is NOT a claim that every field is compared. An earlier revision said
/// so while excluding eight "run-dependent" fields, of which SEVEN
/// (`elapsed_s`, `depth`, `engine`, `closure`, `states_explored`,
/// `max_frontier_width`, `statistics`) appear in NEITHER envelope on this
/// path -- the list was copied from `BmcOutputOptions`' field names, not read
/// off the JSON, so it excluded nothing and merely looked considered. The
/// `assert_exclusions_are_live` above exists so a dead exclusion cannot sit
/// there again.
///
/// Two revisions of the fix under test diverged from `verify` in OPPOSITE
/// directions and both passed a hand-picked field list, which is why the
/// default here is to compare rather than to skip.
fn assert_envelope_parity(testgen: &Value, verify: &Value) {
    /// Shared keys excluded from equality, each with the reason it *cannot* be
    /// compared -- not merely that it varies.
    ///
    /// `cost`: the walk runs no solver, so `testgen`'s block is structurally
    /// zero (`elapsed_s: 0.0`, zero solver checks) while `verify`'s reports
    /// real solving. These are different quantities, not the same quantity
    /// within a tolerance, so no comparison of them is meaningful.
    const NOT_COMPARABLE: [&str; 1] = ["cost"];
    /// Keys only `verify` carries. `versions` is its toolchain metadata block;
    /// `cache` is its memoization provenance, whose presence is ambient.
    const VERIFY_ONLY: [&str; 2] = ["cache", "versions"];

    let left = testgen.as_object().expect("testgen envelope");
    let right = verify.as_object().expect("verify envelope");

    assert_exclusions_are_live(&NOT_COMPARABLE, left, right);

    let unexpected = right
        .keys()
        .filter(|key| !left.contains_key(key.as_str()))
        .filter(|key| !VERIFY_ONLY.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "`verify` carries fields `testgen` does not: {unexpected:?}. Classify \
         each as verdict-bearing (a real divergence to fix) or as \
         `verify`-only metadata (add it to VERIFY_ONLY with a reason)"
    );

    let testgen_only = left
        .keys()
        .filter(|key| !right.contains_key(key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert!(
        testgen_only.is_empty(),
        "`testgen` invented fields `verify` does not emit: {testgen_only:?}"
    );

    let divergent = right
        .keys()
        .filter(|key| left.contains_key(key.as_str()))
        .filter(|key| !NOT_COMPARABLE.contains(&key.as_str()))
        .filter(|key| left.get(key.as_str()) != right.get(key.as_str()))
        .map(|key| {
            format!(
                "{key}: testgen={} verify={}",
                left.get(key.as_str())
                    .map_or_else(|| "<absent>".to_owned(), ToString::to_string),
                right
                    .get(key.as_str())
                    .map_or_else(|| "<absent>".to_owned(), ToString::to_string),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        divergent.is_empty(),
        "the walk violation must carry `verify`'s envelope for the same \
         violation; divergent fields: {divergent:#?}"
    );
}

/// REJECTING CONTROL for a second violation kind. `type_bound` takes a
/// different renderer from `invariant` inside `verify` itself, so kind
/// coverage is not a nicety here -- one control cannot stand for the other.
#[test]
fn a_type_bound_walk_violation_matches_verify_exactly() {
    let spec = fixture("testgen_walk_late_type_bound.fsl");
    let output_path = scratch("late_type_bound.test.ts");

    let (verified, verified_status) = run(&["verify", &spec, "--depth", "8"]);
    assert_eq!(verified_status, 0, "{verified:#}");
    assert_eq!(verified["result"], "verified", "{verified:#}");

    let (output, status) = run(&[
        "testgen",
        &spec,
        "--target",
        "vitest",
        "-o",
        output_path.to_str().expect("scratch path"),
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_eq!(output["violation_kind"], "type_bound", "{output:#}");
    assert_eq!(output["invariant"], "_bounds_c", "{output:#}");
    assert_eq!(output["violated_at_step"], 31, "{output:#}");
    assert!(!output_path.exists(), "a harness must not be written");

    let (verify, verify_status) = run(&["verify", &spec, "--depth", "31"]);
    assert_eq!(verify_status, status, "{verify:#}");
    assert_envelope_parity(&output, &verify);
    // The boundary renderer is the one that can resolve a generated
    // `_bounds_*` name; `render_violation` cannot, and returned null here.
    assert!(!output["violating_bindings"].is_null(), "{output:#}");
}

/// TARGET PARITY CONTROL. `--target pytest` drives the walk live against the
/// Monitor and would `pytest.fail` at step 30; the five baked targets embed
/// the walk. Before the fix those two groups reached opposite conclusions on
/// this one input. All six must now refuse identically.
#[test]
fn every_target_refuses_the_violating_walk_identically() {
    let spec = fixture("testgen_walk_late_violation.fsl");
    for (target, extension) in TARGETS {
        let output_path = scratch(&format!("parity_{target}.{extension}"));
        let (output, status) = run(&[
            "testgen",
            &spec,
            "--target",
            target,
            "-o",
            output_path.to_str().expect("scratch path"),
        ]);
        assert_eq!(status, 1, "target {target}: {output:#}");
        assert_eq!(output["result"], "violated", "target {target}: {output:#}");
        assert_eq!(
            output["violated_at_step"], 30,
            "target {target}: {output:#}"
        );
        assert!(
            !output_path.exists(),
            "target {target} wrote a harness for a violating walk"
        );
    }
}

/// ACCEPTANCE CONTROL. A spec whose walk never violates must still generate,
/// for every target, with the walk running to its natural end -- so the fix
/// refuses violating walks rather than shortening every walk.
#[test]
fn a_clean_walk_still_generates_for_every_target() {
    let spec = fixture("testgen_walk_clean.fsl");
    let mut baked = None;
    for (target, extension) in TARGETS {
        let output_path = scratch(&format!("clean_{target}.{extension}"));
        let (output, status) = run(&[
            "testgen",
            &spec,
            "--target",
            target,
            "-o",
            output_path.to_str().expect("scratch path"),
        ]);
        assert_eq!(status, 0, "target {target}: {output:#}");
        assert_eq!(output["result"], "generated", "target {target}: {output:#}");
        assert!(
            output_path.exists(),
            "target {target} generated no harness: {output:#}"
        );
        if target == "vitest" {
            baked = Some(output_path);
        }
    }

    // The walk stops when `inc` stops being enabled at c = 30, not at the
    // 100-step cap, and the baked expectations are the real successor states.
    // NOTE: `scratch` clears the path it returns, so this must reuse the path
    // the loop generated rather than deriving it again.
    let baked = baked.expect("vitest harness path");
    let content = std::fs::read_to_string(&baked).expect("read generated harness");
    assert_eq!(
        content.matches(r#""action": "inc""#).count(),
        30,
        "expected the full 30-step walk in {}",
        baked.display()
    );
    assert!(
        content.contains(r#""expected": {"c": 30}"#),
        "the last step's expectation must be the real successor"
    );
    assert_eq!(
        content.matches(r#""expected": {"c": 30}"#).count(),
        1,
        "a repeated terminal expectation would be the no-op pattern #843 reports"
    );
}

/// The walk cap is independent of `--depth`: the same spec must produce a
/// byte-identical baked walk at every depth. #843 states this as "100 fixed";
/// the cap is 100 but the walk here ends earlier because the action stops
/// being enabled, so pin the actual invariant -- `--depth` does not reach it.
#[test]
fn the_baked_walk_does_not_vary_with_depth() {
    let spec = fixture("testgen_walk_clean.fsl");
    let mut walks = Vec::new();
    for depth in ["2", "4", "12"] {
        let output_path = scratch(&format!("depth_{depth}.test.ts"));
        let (output, status) = run(&[
            "testgen",
            &spec,
            "--depth",
            depth,
            "--target",
            "vitest",
            "-o",
            output_path.to_str().expect("scratch path"),
        ]);
        assert_eq!(status, 0, "depth {depth}: {output:#}");
        let content = std::fs::read_to_string(&output_path).expect("read generated harness");
        let start = content.find("RANDOM_WALK").expect("baked walk");
        let end = content[start..].find("];").expect("baked walk end") + start;
        walks.push(content[start..end].to_owned());
    }
    assert_eq!(walks[0], walks[1], "depth must not change the baked walk");
    assert_eq!(walks[1], walks[2], "depth must not change the baked walk");
}

/// `domain testgen` wraps the generic path (`run_domain_testgen` calls
/// `run_testgen` and returns a non-zero status verbatim), so it inherits this
/// refusal. Pinned because the inheritance is a propagation contract, not a
/// property of the domain code: a future `domain testgen` that built its own
/// walk would silently reopen #843 on that surface with nothing failing.
#[test]
fn domain_testgen_inherits_the_walk_refusal() {
    // A `domain` document, not a `spec`: `parse_domain_document` runs before
    // the generic path, so a plain spec never reaches it.
    let spec = fixture("domain_testgen_walk_late_violation.fsl");
    let output_path = scratch("domain_walk.test.ts");
    let (output, status) = run(&[
        "domain",
        "testgen",
        &spec,
        "--target",
        "vitest",
        "-o",
        output_path.to_str().expect("scratch path"),
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "violated", "{output:#}");
    assert_ne!(output["kind"], "semantics", "{output:#}");
    assert_eq!(output["invariant"], "atMost29", "{output:#}");
    assert_eq!(output["violated_at_step"], 30, "{output:#}");
    assert!(
        !output_path.exists(),
        "domain testgen wrote a harness for a violating walk"
    );
}

/// PRESERVATION CONTROL for the `requirements` scenario walk. It had the same
/// discarded-`StepResult` shape, but `validate_requirement_trace_source` runs
/// before it on the only path that reaches it and already rejects a violating
/// acceptance trace with `kind:"acceptance"`/exit 2. The guard added at the
/// walk is therefore defence in depth: this test pins the ordering, so if a
/// future change lets a violating trace through validation the guard's
/// diagnostic -- not a no-op `expected_states` entry -- is what surfaces.
#[test]
fn a_violating_acceptance_trace_is_rejected_before_the_scenario_walk() {
    let spec = fixture("requirements_acceptance_walk_violation.fsl");
    let (output, status) = run(&["scenarios", &spec]);

    assert_eq!(status, 2, "{output:#}");
    assert_eq!(output["result"], "error", "{output:#}");
    assert_eq!(output["kind"], "acceptance", "{output:#}");
    assert_eq!(output["id"], "AC-1", "{output:#}");
    assert_eq!(
        output["failed_step"], 1,
        "the second step is the one that violates AtMost1: {output:#}"
    );
    assert!(
        output.get("scenarios").is_none(),
        "no scenario may be published for a rejected trace: {output:#}"
    );
    // The guard's own diagnostic must NOT be what fires: validation owns this
    // rejection today, and the message below would mean the two disagree.
    assert!(
        !output
            .to_string()
            .contains("after validation accepted the trace"),
        "validation must reject this first: {output:#}"
    );
}

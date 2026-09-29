// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #1153: an `x is some(v)` pattern binding
//! overwrote an action parameter of the same name, so the native
//! implementation and the frozen Python reference returned opposite verdicts
//! for the same spec (`act(r=0)` wrote `w[1]` natively and `w[0]` in Python).
//!
//! The frozen reference protects action parameters on purpose
//! (`bmc.py::_eval_requires`: `if k not in param_binds`); the port dropped the
//! condition in `fsl-runtime`, `fsl-verifier` and the type checker. The rule
//! restored here: a pattern binding never overwrites a name already in scope,
//! and a *new* name still reaches the action body (LANGUAGE.md section 9).

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn spec_text(requires: &str, invariant: &str, o_type: &str, o_init: &str) -> String {
    format!(
        r"spec Sh {{
  type R = 0..1
  state {{ w: Map<R, Bool>, o: Option<{o_type}> }}
  init {{ forall r: R {{ w[r] = false }}  o = {o_init} }}
  action act(r: R) {{ requires {requires}  w[r] = true }}
  invariant Inv {{ {invariant} }}
}}
"
    )
}

fn fixture(tag: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fslc-issue-1153-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let path = dir.join("spec.fsl");
    std::fs::write(&path, text).expect("write fixture");
    path
}

fn verify(path: &PathBuf, engine: &str) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["verify", &path.display().to_string(), "--engine", engine])
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

const ENGINES: [&str; 4] = ["explicit", "bmc", "induction", "auto"];

/// Every engine must report the same verdict; `violated` also pins the
/// counterexample's parameter and the key that changed.
fn assert_violated_at(tag: &str, text: &str, param: u64) {
    let path = fixture(tag, text);
    for engine in ENGINES {
        let (report, code) = verify(&path, engine);
        assert_eq!(report["result"], "violated", "{tag}/{engine}: {report:#}");
        assert_eq!(code, 1, "{tag}/{engine}: {report:#}");
        assert_eq!(
            report["last_action"]["params"]["r"], param,
            "{tag}/{engine}: {report:#}"
        );
        let step = &report["trace"][1];
        let key = format!("w[{param}]");
        assert!(
            step["changes"].get(&key).is_some(),
            "{tag}/{engine} changed the wrong key (want {key}): {report:#}"
        );
    }
}

fn assert_verified(tag: &str, text: &str) {
    let path = fixture(tag, text);
    for engine in ["explicit", "bmc", "induction"] {
        let (report, code) = verify(&path, engine);
        assert!(
            matches!(report["result"].as_str(), Some("verified" | "proved")),
            "{tag}/{engine}: {report:#}"
        );
        assert_eq!(code, 0, "{tag}/{engine}: {report:#}");
    }
}

/// The issue's calibration spec. `o = some(1)` but the guard's `r` is the
/// parameter, so `act(r=0)` writes `w[0]`: `not w[0]` is violated, and
/// `not w[1]` holds. Python answers exactly this; the port answered the
/// opposite.
#[test]
fn shadowing_pattern_binding_keeps_the_action_parameter() {
    let guard = "r == 0 and (o is some(r))";
    assert_violated_at("main-w0", &spec_text(guard, "not w[0]", "R", "some(1)"), 0);
    assert_verified("main-w1", &spec_text(guard, "not w[1]", "R", "some(1)"));
}

/// `and` is commutative for this shape: the pattern binding is invisible to
/// its sibling operand whichever side it is written on.
#[test]
fn and_is_commutative_over_a_shadowing_pattern_binding() {
    let flipped = "(o is some(r)) and r == 0";
    assert_violated_at(
        "flipped-w0",
        &spec_text(flipped, "not w[0]", "R", "some(1)"),
        0,
    );
    assert_verified(
        "flipped-w1",
        &spec_text(flipped, "not w[1]", "R", "some(1)"),
    );
}

/// Type-checker disagreement: matching an `Option<Bool>` into the parameter
/// `r: R` used to rewrite `r`'s type to `Bool`, so the body's `w[r]` was
/// rejected. Python accepts this spec.
#[test]
fn shadowing_pattern_binding_does_not_retype_the_parameter() {
    assert_violated_at(
        "retype",
        &spec_text(
            "r == 0 and (o is some(r))",
            "not w[0]",
            "Bool",
            "some(true)",
        ),
        0,
    );
}

/// Internal error on a failed match: `o = none`, so `o is some(r)` is false
/// and its negation holds for every `r`. The symbolic path used to insert the
/// binding anyway (rewriting `r` to the absent payload), so `act(r=1)` wrote
/// `w[0]` there; bmc/induction died with `trace state mismatch at step 1`
/// while explicit answered `violated`.
#[test]
fn failed_shadowing_match_agrees_across_engines() {
    assert_violated_at(
        "failed-match",
        &spec_text("not (o is some(r))", "not w[1]", "R", "none"),
        1,
    );
}

/// Over-detection control -- the one result that would overturn the decision.
/// `v` is a NEW name, so its binding must still reach the action body
/// (LANGUAGE.md section 9): `act` writes `w[v]` with `v = 1`, whatever `r`.
#[test]
fn a_new_pattern_name_still_reaches_the_action_body() {
    let path = fixture(
        "new-name",
        r"spec Sh {
  type R = 0..1
  state { w: Map<R, Bool>, o: Option<R> }
  init { forall r: R { w[r] = false }  o = some(1) }
  action act(r: R) { requires o is some(v)  w[v] = true }
  invariant NoW1 { not w[1] }
}
",
    );
    for engine in ENGINES {
        let (report, code) = verify(&path, engine);
        assert_eq!(report["result"], "violated", "{engine}: {report:#}");
        assert_eq!(code, 1, "{engine}: {report:#}");
        let step = &report["trace"][1];
        assert!(
            step["changes"].get("w[1]").is_some(),
            "{engine}: the new name `v` did not reach the body: {report:#}"
        );
    }
    let holds = fixture(
        "new-name-w0",
        &std::fs::read_to_string(&path)
            .expect("read fixture")
            .replace("NoW1 { not w[1] }", "NoW0 { not w[0] }"),
    );
    // `induction` is left out: `o` is unconstrained in its induction step, so
    // it answers `unknown_cti` for reasons unrelated to binding scope.
    for engine in ["explicit", "bmc"] {
        let (report, code) = verify(&holds, engine);
        assert_eq!(code, 0, "{engine}: w[0] must stay unwritten: {report:#}");
    }
}

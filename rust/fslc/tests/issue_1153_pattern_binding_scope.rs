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
//! Since #1259 the colliding shapes are rejected at `check` instead (#1157
//! item 1); the new-name control below is unchanged.

use std::path::{Path, PathBuf};
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

fn verify(path: &Path, engine: &str) -> (Value, i32) {
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

/// Since #1259 (item 1 of #1157) every shape below is a located `check`
/// error, so no engine sees it and the verdicts above can no longer split.
/// The evaluators keep their #1153 confinement for a model that bypasses
/// `check`; `fsl-verifier/tests/issue_1259_absent_pattern_binding.rs` pins the
/// engines' agreement on such a model for the state-variable collision.
fn assert_rejected(tag: &str, text: &str) {
    let path = fixture(tag, text);
    for command in ["check", "verify"] {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args([command, &path.display().to_string()])
            .output()
            .expect("run native CLI");
        let report: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
        assert_eq!(output.status.code(), Some(2), "{tag}/{command}: {report:#}");
        assert_eq!(report["result"], "error", "{tag}/{command}: {report:#}");
        let message = report["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("pattern binding 'r'") && message.contains("rename"),
            "{tag}/{command}: {report:#}"
        );
        assert_eq!(report["loc"]["line"], 5, "{tag}/{command}: {report:#}");
    }
}

/// The issue's calibration spec (`act(r=0)` wrote `w[1]` natively, `w[0]` in
/// Python), its flipped conjunct order (the ordering #1157 item 2 recorded as
/// diverging from Python), the `Option<Bool>` payload that retyped `r`, and the
/// failed match that crashed bmc/induction: each pattern `r` collides with
/// the action parameter `r`.
#[test]
fn a_pattern_binding_that_shadows_an_action_parameter_is_rejected() {
    assert_rejected(
        "main",
        &spec_text("r == 0 and (o is some(r))", "not w[0]", "R", "some(1)"),
    );
    assert_rejected(
        "flipped",
        &spec_text("(o is some(r)) and r == 0", "not w[0]", "R", "some(1)"),
    );
    assert_rejected(
        "retype",
        &spec_text(
            "r == 0 and (o is some(r))",
            "not w[0]",
            "Bool",
            "some(true)",
        ),
    );
    assert_rejected(
        "failed-match",
        &spec_text("not (o is some(r))", "not w[1]", "R", "none"),
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

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1569: alpha-renaming a user binder must rename its stage operand, not
//! silently redirect the read to the event/property's implicit Claim parameter.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

const R1: &str = r"requirements StageCaptureVerdict {
  number Amount
  entity Claim
  process Claim with amount: Amount = 0 {
    stages Draft, Approved, Paid
    initial Draft
    transition fast Draft -> Approved by Manager
    transition pay Draft -> Paid by Manager when exists $: Claim { stage($) == Approved and amount == 0 }
  }
  invariant NoPaid { forall c: Claim { stage(c) != Paid } }
}
verify { instances Claim = 2 values Amount = 0..1 }
";

const R2: &str = r"requirements SetCapture {
  number Amount
  entity Claim
  process Claim with amount: Amount = 0 {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager set amount = if exists $: Claim { stage($) == Approved } then 1 else 0
  }
  invariant AlwaysZero { forall c: Claim { claim_amount[c] == 0 } }
}
verify { instances Claim = 2 values Amount = 0..1 }
";

const R3: &str = r#"fsl 5
requirements AlwaysCapture {
  number Amount
  actor Manager
  entity Claim {
    amount: Amount = 0
    lifecycle Draft -> Approved -> Paid
  }
  event approve = Manager approves Claim Draft -> Approved
  event pay = Manager pays Claim Approved -> Paid
  rule REQ-1 "A paid claim is zero while some claim is still a draft" {
    always Claim in Paid has exists $: Claim { stage($) == Draft and amount == 0 }
  }
}
verify { instances Claim = 2 values Amount = 0..1 }
"#;

const RULE_GUARD: &str = r#"fsl 5
requirements RuleGuardCapture {
  number Amount
  actor Manager, System
  entity Claim {
    amount: Amount = 0
    lifecycle Draft -> Approved
  }
  event fast = System fasttracks Claim Draft -> Approved
  event approve = Manager approves Claim Draft -> Approved
  rule REQ-1 "A zero claim is approved only while another claim is approved" {
    allow Manager approves Claim only when exists $: Claim { stage($) == Approved and amount == 0 }
  }
}
verify { instances Claim = 2 values Amount = 0..1 }
"#;

const DECIDE: &str = r#"fsl 5
requirements DecideCapture {
  number Amount
  actor Manager, System
  entity Claim {
    amount: Amount = 0
    lifecycle Draft -> Approved
    lifecycle Draft -> Rejected
  }
  concept Backed = Claim where exists $: Claim { stage($) == Approved and amount == 0 }
  concept Unbacked = Claim where not (exists $: Claim { stage($) == Approved and amount == 0 })
  event approve = Manager approves Claim Draft -> Approved
  event reject = System rejects Claim Draft -> Rejected
  decide T "Who handles a draft claim" for Draft Claim {
    Backed => Manager approves
    Unbacked => System rejects
  }
}
verify { instances Claim = 2 values Amount = 0..1 }
"#;

const MEASURE: &str = r"fsl 5
requirements MeasureCapture {
  number Amount
  actor Manager
  entity Claim {
    amount: Amount = 0
    lifecycle Draft -> Approved
  }
  concept Backed = Claim where exists $: Claim { stage($) == Approved and amount == 0 }
  measure backed = count Backed
  event approve = Manager approves Claim Draft -> Approved
}
verify { instances Claim = 2 values Amount = 0..1 }
";

struct Observation {
    mode: &'static str,
    exit: i32,
    value: Value,
}

fn run(label: &str, source: &str, binder: &str, depth: &str) -> Vec<Observation> {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1569_substitute_stage_capture")
        .join(std::process::id().to_string())
        .join(std::thread::current().name().expect("named test thread"))
        .join(label)
        .join(binder);
    std::fs::create_dir_all(&directory).expect("create test-local directory");
    std::fs::write(directory.join("fixture.fsl"), source.replace('$', binder))
        .expect("write inline fixture");
    // Collect all commands before asserting, including wrong-verdict detectors.
    ["check", "kernel", "verify"]
        .map(|mode| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
            command.args([mode, "fixture.fsl"]);
            if mode == "verify" {
                command.args(["--depth", depth, "--no-cache"]);
            }
            let output = command
                .current_dir(&directory)
                .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
                .env("FSLC_CACHE", "off")
                .output()
                .expect("run fslc");
            let value: Value = serde_json::from_slice(&output.stdout)
                .unwrap_or_else(|error| panic!("{label}/{binder} {mode}: {error}: {output:?}"));
            let exit = output.status.code().expect("exit code");
            std::fs::write(
                directory.join(format!("{mode}.json")),
                serde_json::to_vec_pretty(&json!({
                    "exit": exit, "output": value,
                    "stderr": String::from_utf8_lossy(&output.stderr),
                }))
                .expect("serialize observation"),
            )
            .expect("retain complete observation");
            assert!(output.stderr.is_empty(), "{label} {mode}: {output:?}");
            Observation { mode, exit, value }
        })
        .into_iter()
        .collect()
}

fn assert_verdict(label: &str, source: &str, binder: &str, depth: &str, result: &str) {
    let observations = run(label, source, binder, depth);
    for observation in &observations {
        let (exit, expected) = match observation.mode {
            "check" => (0, "ok"),
            "kernel" => (0, "kernel"),
            "verify" => (i32::from(result == "violated"), result),
            _ => unreachable!(),
        };
        assert_eq!(
            (observation.exit, observation.value["result"].as_str()),
            (exit, Some(expected)),
            "{label}/{binder} {}: result/exit mismatch; complete observation retained",
            observation.mode
        );
    }
    let verify = &observations[2].value;
    if result == "violated" {
        let property = match label {
            "r1" => "NoPaid",
            "r2" => "AlwaysZero",
            "r3_full" => "REQ_1_always",
            _ => unreachable!(),
        };
        assert_eq!(verify["invariant"], property, "{label}/{binder}");
        let step = if label == "r3_full" { 3 } else { 2 };
        assert_eq!(verify["violated_at_step"], step, "{label}/{binder}");
    } else {
        assert_eq!(verify["checked_to_depth"], 2, "{verify:#}");
    }
}

// Canonicalize only bound names, with lexical scope. No Kernel field is dropped:
// source filenames/locations are identical because both inline fixtures use the
// same relative path and c/d have equal length. Free event c remains unchanged.
fn alpha_normalize(value: &mut Value, scope: &HashMap<String, String>) {
    match value {
        Value::Array(values) => {
            for value in values {
                alpha_normalize(value, scope);
            }
        }
        Value::Object(object) => {
            let mut nested = scope.clone();
            if let Some(binder) = object.get_mut("binder") {
                let name = binder["name"].as_str().expect("binder name").to_owned();
                let canonical = format!("bound_{}", scope.len());
                binder["name"] = Value::String(canonical.clone());
                nested.insert(name, canonical);
            }
            if object.get("kind").and_then(Value::as_str) == Some("var")
                && let Some(name) = object.get("name").and_then(Value::as_str)
                && let Some(canonical) = scope.get(name)
            {
                object.insert("name".into(), Value::String(canonical.clone()));
            }
            for value in object.values_mut() {
                alpha_normalize(value, &nested);
            }
        }
        _ => {}
    }
}

fn assert_kernel_pair(label: &str, source: &str, left: &str, right: &str) {
    let mut c = run(label, source, left, "2").remove(1);
    let mut d = run(label, source, right, "2").remove(1);
    assert_eq!((c.exit, d.exit), (0, 0));
    assert_eq!(c.value["result"], "kernel");
    assert_eq!(d.value["result"], "kernel");
    alpha_normalize(&mut c.value, &HashMap::new());
    alpha_normalize(&mut d.value, &HashMap::new());
    assert!(
        c.value == d.value,
        "{label}/{left}/{right}: complete kernels differ beyond alpha-renaming; raw JSON retained in test-local directories"
    );
}

macro_rules! verdict_pair {
    ($detector:ident, $control:ident, $kernel:ident, $label:literal, $source:ident, $depth:literal, $result:literal) => {
        #[test]
        fn $detector() {
            assert_verdict($label, $source, "c", $depth, $result);
        }
        #[test]
        fn $control() {
            assert_verdict($label, $source, "d", $depth, $result);
        }
        #[test]
        fn $kernel() {
            assert_kernel_pair($label, $source, "c", "d");
        }
    };
}

verdict_pair!(
    detector_r1_guard,
    control_r1_guard,
    detector_r1_kernel,
    "r1",
    R1,
    "8",
    "violated"
);

#[test]
fn control_full_kernel_alpha_comparison_accepts_noncapturing_binders() {
    for (label, source) in [("r1", R1), ("r2", R2), ("r3", R3)] {
        assert_kernel_pair(label, source, "d", "e");
    }
}
verdict_pair!(
    detector_r2_set,
    control_r2_set,
    detector_r2_kernel,
    "r2",
    R2,
    "8",
    "violated"
);
verdict_pair!(
    detector_r3_always,
    control_r3_always,
    detector_r3_kernel,
    "r3",
    R3,
    "2",
    "verified"
);

fn assert_fail_closed_pair(label: &str, source: &str, diagnostic: &str) {
    let c = run(label, source, "c", "2");
    let d = run(label, source, "d", "2");
    for (binder, observations) in [("c", &c), ("d", &d)] {
        for observation in observations {
            assert_eq!(
                observation.exit, 2,
                "{label}/{binder}: {}",
                observation.value
            );
            assert_eq!(
                observation.value["result"], "error",
                "{}",
                observation.value
            );
            let text = observation.value.to_string();
            assert!(text.contains(diagnostic), "{label}/{binder}: {text}");
            assert!(
                text.contains("unlowered stage access"),
                "{label}/{binder}: {text}"
            );
        }
    }
    for (c, d) in c.iter().zip(&d) {
        assert_eq!(c.value, d.value, "{label}: complete diagnostic envelopes");
    }
}

#[test]
fn detector_r3_full_depth_violation_is_not_spurious_step_two() {
    assert_verdict("r3_full", R3, "c", "8", "violated");
}

#[test]
fn control_r3_full_depth_violates_at_step_three() {
    assert_verdict("r3_full", R3, "d", "8", "violated");
}

#[test]
fn detector_r3_without_field_mention_still_reads_inner_binder() {
    let minimal = R3.replace(" and amount == 0", "");
    assert_verdict("r3_min", &minimal, "c", "2", "verified");
}

#[test]
fn control_r3_without_field_mention_is_verified_to_depth_two() {
    let minimal = R3.replace(" and amount == 0", "");
    assert_verdict("r3_min", &minimal, "d", "2", "verified");
}

#[test]
fn control_rule_guard_still_fails_closed_before_event_guard_substitution() {
    assert_fail_closed_pair(
        "rule_guard",
        RULE_GUARD,
        "rule_conflict_unsatisfiable_guard",
    );
}

#[test]
fn control_decision_table_still_fails_closed() {
    assert_fail_closed_pair("decide", DECIDE, "decide_not_exhaustive");
}

#[test]
fn control_measure_still_fails_closed_for_both_binders() {
    assert_fail_closed_pair("measure", MEASURE, "invalid KPI projection 'backed'");
}

#[test]
fn control_predicate_expansion_rejects_capture() {
    let source = r"spec KernelDefCapture {
  type Claim = 0..1
  state { m: Map<Claim, Bool> }
  init { forall c: Claim { m[c] = false } }
  def p(x: Claim) = exists c: Claim { m[c] or m[x] }
  action flip(c: Claim) { m[c] = true }
  invariant I { forall c: Claim { p(c) or true } }
}";
    for observation in run("def_capture", source, "c", "2") {
        assert_eq!(observation.exit, 2, "{}", observation.value);
        assert_eq!(observation.value["result"], "error");
        assert!(
            observation
                .value
                .to_string()
                .contains("would capture variable 'c'"),
            "{}",
            observation.value
        );
    }
}

#[test]
fn control_legacy_field_named_like_implicit_parameter_keeps_stage_operand() {
    // The field c is numeric, but stage(c) names the implicit Claim parameter.
    // Replacing field reads c -> claim_c[c] must not rewrite that operand.
    let source = r"requirements Collision {
  number Amount
  entity Claim
  process Claim with c: Amount = 0 {
    stages Draft, Approved
    initial Draft
    transition approve Draft -> Approved by Manager when stage(c) == Draft
  }
  invariant I { forall x: Claim { claim_c[x] == 0 } }
}
verify {
  instances Claim = 2
  values Amount = 0..1
}
";
    let observations = run("legacy_field_named_c", source, "c", "8");
    for observation in &observations {
        let expected = match observation.mode {
            "check" => "ok",
            "kernel" => "kernel",
            "verify" => "verified",
            _ => unreachable!(),
        };
        assert_eq!(
            (observation.exit, observation.value["result"].as_str()),
            (0, Some(expected)),
            "{}: {}",
            observation.mode,
            observation.value
        );
    }
    // Base 1844fa74 verifies the invariant; the transition is not vacuous.
    let verify = &observations[2].value;
    assert_eq!(verify["checked_to_depth"], 8);
    assert_eq!(verify["completeness"], "bounded");
    assert_eq!(verify["action_coverage"]["approve"], true);

    let kernel = &observations[1].value;
    let action = kernel["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .find(|action| action["name"] == "approve")
        .expect("approve action");
    assert_eq!(action["parameters"][0]["name"], "c");
    assert_eq!(action["parameters"][0]["type"]["name"], "Claim");
    let guards = action["guards"].as_array().expect("guards");
    assert_eq!(guards.len(), 2, "source-stage guard and user when guard");
    // Inspect the user guard, not merely the generated Draft precondition.
    assert_eq!(guards[1]["kind"], "requires");
    let expression = &guards[1]["expression"];
    assert_eq!(expression["kind"], "binary");
    assert_eq!(expression["operator"], "==");
    assert_eq!(expression["left"]["kind"], "index");
    assert_eq!(expression["left"]["collection"]["kind"], "var");
    assert_eq!(expression["left"]["collection"]["name"], "claim_stage");
    assert_eq!(expression["left"]["index"]["kind"], "var");
    assert_eq!(expression["left"]["index"]["name"], "c");
    assert_eq!(expression["right"]["kind"], "var");
    assert_eq!(expression["right"]["name"], "Draft");
}

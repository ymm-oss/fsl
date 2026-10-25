// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1442: a use-only compose preserves component lexical binding AND refusal.
//! Let/pattern tests are detectors; other binders and located errors are
//! preservation controls. Rejected patterns need only `check`: they must never
//! reach an engine. BMC, induction, BFS and Monitor replay exercise valid lets.
//! Output comparison retains every semantic field, including witnesses, public
//! assurance and locations. Only observed run-resource measurements are masked,
//! with live presence/type checks on BOTH outputs; see `mask_cost`.

#[path = "support/compose_scratch.rs"]
mod compose_scratch;

use std::path::PathBuf;

use compose_scratch::{check, run, scratch};
use serde_json::{Value, json};

const SHADOW: &str = "spec Shadow {
  state { on: Bool, seen: Bool }
  init { on = false seen = false }
  action observe() { let on = true seen = on }
  reachable Seen { seen }
}
";

const PATTERN: &str = "spec Pattern {
  state { value: Int, opt: Option<Int>, seen: Int }
  init { value = 0 opt = some(1) seen = 0 }
  action read() { if opt is some(value) { seen = value } }
  reachable Seen { seen == 1 }
}
";

const LET_SEQUENCE: &str = "spec Lets {
  state { on: Bool, seen: Bool }
  init { on = false seen = false }
  action read() { let on = not on let again = on seen = again }
  reachable Seen { seen }
}
";

const TEMPORAL: &str = "  leadsTo Trivial { forall k: K { k == 1 ~> k == 1 } }\n";
const BINDERS: &str = "spec Binders {
  type K = 0..1
  state { k: K, m: Map<K, Bool>, seen: Bool }
  init { k = 0 forall k: K { m[k] = false } seen = false }
  action read(k: K) {
    requires k == 1
    forall k: K where k == 1 { m[k] = true }
    seen = (forall k: K where k == 1 { k == 1 }) and sum(k: K of k) == 1
  }
  reachable Seen { seen and m[1] }
  leadsTo Trivial { forall k: K { k == 1 ~> k == 1 } }
}
";

fn pair(spec: &str, source: &str) -> PathBuf {
    let compose = format!("compose System {{\n  use {spec} as a from \"component.fsl\"\n}}\n");
    scratch(
        "issue-1442",
        &[("component.fsl", source), ("system.fsl", &compose)],
    )
}

// Exactly the state/action/property declarations in these fixtures, not a
// blanket replacement of `a.` or local parameter/pattern names. Map write
// paths and automatic bounds are derived from those same declarations.
const GLOBAL_NAMES: &[&str] = &[
    "on", "seen", "value", "opt", "k", "m", "observe", "read", "Seen", "Trivial",
];

fn public_name(name: &str) -> String {
    for global in GLOBAL_NAMES {
        for (composed, direct) in [
            (format!("a.{global}"), (*global).to_owned()),
            (format!("_bounds_a.{global}"), format!("_bounds_{global}")),
        ] {
            if let Some(suffix) = name.strip_prefix(&composed)
                && (suffix.is_empty() || suffix.starts_with('[') || suffix.starts_with('.'))
            {
                return format!("{direct}{suffix}");
            }
        }
    }
    name.to_owned()
}

fn normalize_names(value: Value) -> Value {
    match value {
        Value::Object(fields) => {
            let mut normalized = serde_json::Map::new();
            for (name, value) in fields {
                assert!(
                    normalized
                        .insert(public_name(&name), normalize_names(value))
                        .is_none(),
                    "name normalization must not merge fields"
                );
            }
            Value::Object(normalized)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(normalize_names).collect()),
        Value::String(text) => Value::String(public_name(&text)),
        other => other,
    }
}

fn mask_measurement(left: &mut Value, right: &mut Value, pointer: &str) {
    for output in [left, right] {
        let measured = output
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("excluded resource field must be live: {pointer}"));
        assert!(
            measured.is_null() || measured.as_f64().is_some_and(|n| n.is_finite() && n >= 0.0),
            "invalid resource measurement at {pointer}: {measured}"
        );
        *measured = Value::Null;
    }
}

fn mask_cost(left: &mut Value, right: &mut Value) {
    // Observed BMC/induction/explicit output: time, memory and solver operation
    // counts measure separate runs, not judgments. Keep the cost envelope,
    // property identities/order, and every unknown/new field in the comparison.
    for pointer in [
        "/cost/elapsed_s",
        "/cost/solver/checks",
        "/cost/solver/check_elapsed_s",
        "/cost/solver/conflicts",
        "/cost/solver/decisions",
        "/cost/solver/propagations",
        "/cost/solver/memory_mb",
    ] {
        mask_measurement(left, right, pointer);
    }
    let properties = left["cost"]["properties"]
        .as_array()
        .expect("live cost properties")
        .len();
    assert_eq!(
        right["cost"]["properties"].as_array().map(Vec::len),
        Some(properties),
        "cost property list must agree"
    );
    for index in 0..properties {
        for field in ["checks", "elapsed_s"] {
            mask_measurement(left, right, &format!("/cost/properties/{index}/{field}"));
        }
    }
}

fn assert_same(direct: (Value, i32), composed: (Value, i32), has_cost: bool) {
    assert_eq!(
        composed.1, direct.1,
        "compose exit differs; standalone={} composed={}",
        direct.0, composed.0
    );
    let mut left = direct.0;
    let mut right = composed.0;
    if let Some(spec) = left.get("spec") {
        assert_eq!(right["spec"], "System", "{right}");
        right["spec"] = spec.clone();
    } else {
        assert!(right.get("spec").is_none(), "{right}");
    }
    right = normalize_names(right);
    if has_cost {
        mask_cost(&mut left, &mut right);
    }
    // No loc exclusion: a component action error retains its original span,
    // as calibrated by preservation_type_error_keeps_component_location.
    assert_eq!(right, left, "complete normalized output differs");
}

fn verify_pair(spec: &str, source: &str, engine: &str) {
    let dir = pair(spec, source);
    let [direct, composed] = ["component.fsl", "system.fsl"].map(|file| {
        if engine == "bmc" {
            compose_scratch::verify(&dir, file)
        } else {
            run(
                &dir,
                &[
                    "verify",
                    file,
                    "--engine",
                    engine,
                    "--depth",
                    "2",
                    "--no-cache",
                ],
            )
        }
    });
    assert_eq!(direct.1, 0, "standalone calibration: {}", direct.0);
    assert_eq!(
        direct.0["result"],
        if engine == "bmc" {
            "verified"
        } else {
            "proved"
        },
        "standalone calibration: {}",
        direct.0
    );
    assert_eq!(direct.0["reachables"]["Seen"]["witnessed_at_step"], 1);
    assert_eq!(
        direct.0["reachables"]["Seen"]["witness"][1]["state"]["seen"],
        true
    );
    assert_same(direct, composed, true);
}

macro_rules! verify_test {
    ($name:ident, $spec:literal, $source:expr, $engine:literal) => {
        #[test]
        fn $name() {
            verify_pair($spec, &$source, $engine);
        }
    };
}

verify_test!(detector_issue_let_bmc, "Shadow", SHADOW, "bmc");
verify_test!(detector_issue_let_bfs, "Shadow", SHADOW, "explicit");
verify_test!(detector_issue_let_induction, "Shadow", SHADOW, "induction");
verify_test!(detector_sequential_let_bmc, "Lets", LET_SEQUENCE, "bmc");
verify_test!(
    detector_sequential_let_bfs,
    "Lets",
    LET_SEQUENCE,
    "explicit"
);
verify_test!(
    detector_sequential_let_induction,
    "Lets",
    LET_SEQUENCE,
    "induction"
);

fn reject_pattern(source: &str, column: u32) {
    let dir = pair("Pattern", source);
    let direct = check(&dir, "component.fsl");
    assert_eq!(direct.1, 2, "standalone calibration: {}", direct.0);
    assert_eq!(direct.0["kind"], "semantics");
    assert_eq!(direct.0["result"], "error");
    assert_eq!(direct.0["loc"], json!({"line": 4, "column": column}));
    assert_eq!(
        direct.0["message"],
        format!(
            "invalid model expression: pattern binding 'value' in 'is some(value)' shadows a state variable of the same name; rename the pattern variable at 4:{column}"
        )
    );
    assert_same(direct, check(&dir, "system.fsl"), false);
}

#[test]
fn detector_issue_pattern_if_remains_rejected() {
    reject_pattern(PATTERN, 19);
}

fn pattern_source(body: &str, boolean: bool) -> String {
    let (ty, zero, goal) = if boolean {
        ("Bool", "false", "seen")
    } else {
        ("Int", "0", "seen == 1")
    };
    format!(
        "spec Pattern {{\n  state {{ value: Int, opt: Option<Int>, seen: {ty} }}\n  init {{ value = 0 opt = some(1) seen = {zero} }}\n  action read() {{ {body} }}\n  reachable Seen {{ {goal} }}\n}}\n"
    )
}

#[test]
fn detector_pattern_conditional_remains_rejected() {
    reject_pattern(
        &pattern_source("seen = if opt is some(value) then value else 0", false),
        29,
    );
}

#[test]
fn detector_pattern_and_remains_rejected() {
    reject_pattern(
        &pattern_source("seen = (opt is some(value)) and value == 1", true),
        19,
    );
}

#[test]
fn detector_pattern_requires_remains_rejected() {
    reject_pattern(
        &pattern_source("requires opt is some(value) seen = value", false),
        3,
    );
}

verify_test!(
    preservation_binders_bmc,
    "Binders",
    BINDERS.replace(TEMPORAL, ""),
    "bmc"
);
verify_test!(
    preservation_binders_bfs,
    "Binders",
    BINDERS.replace(TEMPORAL, ""),
    "explicit"
);
verify_test!(
    preservation_binders_induction,
    "Binders",
    BINDERS.replace(TEMPORAL, ""),
    "induction"
);
verify_test!(preservation_leadsto_binders_bmc, "Binders", BINDERS, "bmc");
verify_test!(
    preservation_leadsto_binders_induction,
    "Binders",
    BINDERS,
    "induction"
);

fn replay_pair(
    spec: &str,
    source: &str,
    action: &str,
    params: &Value,
    initial: &Value,
    after: &Value,
) -> ((Value, i32), (Value, i32)) {
    let dir = pair(spec, source);
    let trace = |composed| {
        let qualify = |state: &Value| {
            Value::Object(
                state
                    .as_object()
                    .expect("state object")
                    .iter()
                    .map(|(name, value)| {
                        (
                            if composed {
                                format!("a.{name}")
                            } else {
                                name.clone()
                            },
                            value.clone(),
                        )
                    })
                    .collect(),
            )
        };
        json!({
            "$schema": "https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json",
            "schema_version": "1.2.0", "kernel_schema_version": "1.0.0",
            "spec": if composed { "System" } else { spec },
            "initial": qualify(initial),
            "events": [{"tick": 1, "action": if composed { format!("a.{action}") } else { action.to_owned() },
                "params": params, "state": qualify(after)}]
        })
    };
    for (file, composed) in [("direct.json", false), ("composed.json", true)] {
        std::fs::write(dir.join(file), trace(composed).to_string()).expect("write replay trace");
    }
    (
        run(&dir, &["replay", "component.fsl", "--trace", "direct.json"]),
        run(&dir, &["replay", "system.fsl", "--trace", "composed.json"]),
    )
}

fn replay_let(spec: &str, source: &str, action: &str) {
    let (direct, composed) = replay_pair(
        spec,
        source,
        action,
        &json!({}),
        &json!({"on": false, "seen": false}),
        &json!({"on": false, "seen": true}),
    );
    assert_eq!(direct.1, 0, "standalone calibration: {}", direct.0);
    assert_eq!(direct.0["result"], "conformant");
    assert_eq!(direct.0["final_state"], json!({"on": false, "seen": true}));
    assert_same(direct, composed, false);
}

#[test]
fn detector_issue_let_monitor() {
    replay_let("Shadow", SHADOW, "observe");
}

#[test]
fn detector_sequential_let_monitor() {
    replay_let("Lets", LET_SEQUENCE, "read");
}

fn replay_binders(correct: bool) {
    let (direct, composed) = replay_pair(
        "Binders",
        &BINDERS.replace(TEMPORAL, ""),
        "read",
        &json!({"k": 1}),
        &json!({"k": 0, "m": {"0": false, "1": false}, "seen": false}),
        &json!({"k": 0, "m": {"0": false, "1": true}, "seen": correct}),
    );
    assert_eq!(
        direct.1,
        i32::from(!correct),
        "standalone calibration: {}",
        direct.0
    );
    if correct {
        assert_eq!(direct.0["result"], "conformant");
        assert_eq!(direct.0["steps_checked"], 1);
    } else {
        assert_eq!(direct.0["result"], "nonconformant");
        assert_eq!(direct.0["violation"]["kind"], "state_mismatch");
        assert_eq!(direct.0["violation"]["mismatches"][0]["path"], "seen");
    }
    assert_same(direct, composed, false);
}

#[test]
fn preservation_binders_monitor() {
    replay_binders(true);
}

#[test]
fn preservation_monitor_rejects_wrong_observation() {
    replay_binders(false);
}

#[test]
fn preservation_type_error_keeps_component_location() {
    // Calibrated independently of #1442: original action span, not use span.
    let dir = pair(
        "TypeError",
        "spec TypeError {\n  state { on: Bool }\n  init { on = false }\n  action read() { on = 1 }\n}\n",
    );
    let direct = check(&dir, "component.fsl");
    assert_eq!(direct.1, 2);
    assert_eq!(direct.0["kind"], "semantics");
    assert_eq!(direct.0["loc"], json!({"line": 4, "column": 19}));
    assert_eq!(
        direct.0["message"],
        "invalid model expression: expression of type Int is not assignable to Bool at 4:19"
    );
    assert_same(direct, check(&dir, "system.fsl"), false);
}

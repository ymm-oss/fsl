// SPDX-License-Identifier: Apache-2.0

//! Named predicates must pass through kernel lowering in both counterfactual helpers.

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

const NAMED_GUARD: &str = r"spec C {
  type R = 0..1
  type N = 0..3
  state { x: Map<R, N> }
  init { forall r: R { x[r] = 0 } }
  def low(r: R) = x[r] < 2
  action inc(r: R) {
    requires low(r)
    x[r] = x[r] + 1
  }
  invariant AtMostTwo { forall r: R { x[r] <= 2 } }
}
";

fn run_explain(source: &str) -> Value {
    // Keep generated inputs inside the materialized worktree, not a shared temp directory.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = root.join(format!(
        ".issue-1350-explain-lowering-{}-{:?}.fsl",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, source).expect("write explain fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "explain",
            path.to_str().expect("UTF-8 fixture"),
            "--depth",
            "3",
        ])
        .current_dir(root)
        .output()
        .expect("run native explain");
    std::fs::remove_file(path).expect("remove explain fixture");
    assert!(
        output.status.success(),
        "explain failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("explain JSON");
    assert_eq!(value["fsl"], "1.0");
    assert_eq!(value["result"], "explained");
    assert_eq!(value["spec"], "C");
    assert_eq!(value["depth"], 3);
    value
}

/// Detector: lowering a named guard must retain the authored requires location/text.
#[test]
fn named_guard_invariant_counterfactual_keeps_authored_requires() {
    let value = run_explain(NAMED_GUARD);
    let items = value["counterfactuals"]
        .as_array()
        .expect("counterfactuals");
    assert_eq!(
        items.len(),
        1,
        "expected AtMostTwo candidate, got {items:?}"
    );
    let item = &items[0];
    assert_eq!(item["invariant"], "AtMostTwo");
    assert_eq!(
        item["weakening"],
        json!({
            "op": "requires-removal",
            "loc": {"line": 8, "column": 5},
            "target": "inc requires #1",
            "source_text": "requires low(r)",
        })
    );
    assert_eq!(item["violation"]["violation_kind"], "invariant");
    assert_eq!(item["violation"]["invariant"], "AtMostTwo");
    assert_eq!(item["violation"]["violated_at_step"], 3);
    assert_eq!(item["trace"].as_array().expect("trace").len(), 4);
    // The dedicated counterfactual_violating_step_records_successor_and_changes
    // test is the correctness oracle for violating trace evidence.
}

/// Detector: reachable candidate rebuilding must also expand a named action guard.
#[test]
fn named_guard_reachable_counterfactual_keeps_authored_assignment() {
    let source = NAMED_GUARD.replace(
        "invariant AtMostTwo { forall r: R { x[r] <= 2 } }",
        "reachable Two { exists r: R { x[r] == 2 } }",
    );
    let value = run_explain(&source);
    assert_eq!(
        value["reachable_counterfactuals"],
        json!([{
            "property": "Two",
            "weakening": {
                "op": "assignment-removal",
                "loc": {"line": 9, "column": 5},
                "target": "inc assignment",
                "source_text": "x[r] = x[r] + 1",
            },
            "result": "reachable_failed",
            "requirement": null,
            "faithfulness_class": "intent_unexercised",
            "recommended_action": "add a single-shot reachable for the action / raise --depth",
        }]),
        "expected reachable Two candidate, got {value}"
    );
    let witness = value["witnesses"]
        .as_array()
        .expect("witnesses")
        .iter()
        .find(|item| item["kind"] == "reachable" && item["target"] == "Two")
        .expect("Two is reachable in the original through inc");
    assert_eq!(
        witness["steps"],
        json!([
            {"action": "inc", "params": {"r": 0}},
            {"action": "inc", "params": {"r": 0}},
        ])
    );
}

fn without_live_elapsed(mut value: Value) -> Value {
    let items = value["counterfactuals"]
        .as_array_mut()
        .expect("counterfactuals");
    assert_eq!(items.len(), 1, "elapsed exclusion must not be vacuous");
    for item in items {
        let elapsed = item["violation"]["cost"]
            .as_object_mut()
            .expect("violation cost")
            .remove("elapsed_s")
            .expect("live elapsed_s on both expected and actual outputs");
        assert!(elapsed.as_f64().is_some_and(|seconds| seconds >= 0.0));
    }
    value
}

/// Preservation control: base capture with the four #1350 violating-evidence corrections.
#[test]
fn unused_def_and_empty_verify_preserve_complete_base_explanation() {
    let source = NAMED_GUARD.replace("requires low(r)", "requires x[r] < 2") + "verify {}\n";
    let actual = run_explain(&source);
    // Base output captured from fslc-5a539bbe (SHA-256 d4f8ada816cc6367d318cd35732d6a08a27b617f11ade6c803555ced0a333124).
    // Corrected for #1350: violating-step state, changes, violating_bindings,
    // and conjunct violating_bindings; all other expected values retain the base capture.
    // Only elapsed_s varied in repeated base runs; presence is checked on BOTH sides.
    // The dedicated counterfactual_violating_step_records_successor_and_changes
    // test is the correctness oracle for violating trace evidence.
    let expected: Value = serde_json::from_str(BASE_UNUSED_DEF_EXPLANATION).expect("base JSON");
    assert_eq!(without_live_elapsed(actual), without_live_elapsed(expected));
}

const BASE_UNUSED_DEF_EXPLANATION: &str = r#"{
  "fsl": "1.0", "result": "explained", "spec": "C", "depth": 3,
  "counterfactual_scope": {
    "properties": ["invariant", "reachable"], "liveness": "skipped",
    "reason": "counterfactual weakening is safety/reachability analysis; leadsTo remains verified by the normal verify command"
  },
  "skeleton": {
    "spec_kind": "kernel",
    "state": {"x": ["map", ["domain", 0, 1], ["domain", 0, 3]]},
    "actions": [{
      "name": "inc", "params": [{"name": "r", "type": "R", "lo": 0, "hi": 1}],
      "requires_text": ["requires x[r] < 2"], "ensures_text": [], "writes": ["x"], "requirement": null
    }],
    "properties": [{"name": "AtMostTwo", "kind": "invariant", "body_text": "forall r: R { x[r] <= 2 }", "requirement": null}],
    "auto_checks": [{"kind": "type_bound", "name": "_bounds_x", "target": "x", "requirement": null}],
    "domains": ["N: values 0..3 (4 values)", "R: 2 instances (0..1)"],
    "enums": {}, "stage_flows": [], "kpis": []
  },
  "witnesses": [{
    "name": "cover_inc", "kind": "action_coverage", "target": "inc", "requirement": null, "requirements": [],
    "steps": [{"action": "inc", "params": {"r": 0}}], "narration": ["1. inc(r=0)"],
    "initial_state": {"x": {"0": 0, "1": 0}}, "expected_states": [{"x": {"0": 1, "1": 0}}]
  }],
  "counterfactuals": [{
    "invariant": "AtMostTwo",
    "weakening": {"op": "requires-removal", "loc": {"line": 8, "column": 5}, "target": "inc requires #1", "source_text": "requires x[r] < 2"},
    "trace": [
      {"step": 0, "state": {"x": {"0": 0, "1": 0}}},
      {
        "step": 1, "state": {"x": {"0": 1, "1": 0}},
        "action": {"name": "inc", "params": {"r": 0}, "loc": {"line": 7, "column": 3}},
        "changes": {"x[0]": {"from": 0, "to": 1}},
        "blame": {"guards": [], "effects": [{"target": "x", "text": "x[r] = x[r] + 1", "loc": {"line": 9, "column": 5}}]}
      },
      {
        "step": 2, "state": {"x": {"0": 2, "1": 0}},
        "action": {"name": "inc", "params": {"r": 0}, "loc": {"line": 7, "column": 3}},
        "changes": {"x[0]": {"from": 1, "to": 2}},
        "blame": {"guards": [], "effects": [{"target": "x", "text": "x[r] = x[r] + 1", "loc": {"line": 9, "column": 5}}]}
      },
      {
        "step": 3, "state": {"x": {"0": 3, "1": 0}},
        "action": {"name": "inc", "params": {"r": 0}, "loc": {"line": 7, "column": 3}},
        "changes": {"x[0]": {"from": 2, "to": 3}},
        "blame": {"guards": [], "effects": [{"target": "x", "text": "x[r] = x[r] + 1", "loc": {"line": 9, "column": 5}}]}
      }
    ],
    "requirement": null,
    "violation": {
      "violation_kind": "invariant", "invariant": "AtMostTwo", "loc": {"line": 11, "column": 3},
      "violated_at_step": 3, "violating_bindings": [{"r": 0}],
      "blame": {"conjuncts": [{"index": 0, "text": "forall r: R { x[r] <= 2 }", "holds": false, "violating_bindings": [{"r": 0}]}]},
      "last_action": {"name": "inc", "params": {"r": 0}, "loc": {"line": 7, "column": 3}},
      "checked_to_depth": 3, "completeness": "bounded", "cost": {"elapsed_s": 0.007066083}
    }
  }]
}"#;

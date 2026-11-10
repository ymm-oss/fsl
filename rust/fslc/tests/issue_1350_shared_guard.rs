// SPDX-License-Identifier: Apache-2.0

//! One weakening must be credited to every affected invariant or reachable.

use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

const SINGLE_INVARIANT: &str = r"spec C {
  type N = 0..3
  state { x: N }
  init { x = 0 }
  action inc() {
    requires x < 2
    x = x + 1
  }
  invariant AtMostTwo { x <= 2 }
}
";

const SHARED_REACHABLE: &str = r"spec C {
  state { done: Bool }
  init { done = false }
  action finish() { done = true }
  reachable Done { done }
  reachable AlsoDone { done }
}
";

const TRACE_MAP: &str = r"spec C {
  type R = 0..1
  type N = 0..3
  state { x: Map<R, N> }
  init { forall r: R { x[r] = 0 } }
  action inc(r: R) {
    requires x[r] < 2
    x[r] = x[r] + 1
  }
  invariant AtMostTwo { forall r: R { x[r] <= 2 } }
}
";

fn run_cli(source: &str, fixture: &str, command: &str, expected_status: i32) -> Value {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Keep generated inputs inside this worktree and its ignored build tree.
    // Retain them for failure diagnosis without deleting any existing files.
    let directory = root.join("../target/issue-1350-shared-guard");
    std::fs::create_dir_all(&directory).expect("create local fixture directory");
    let path = directory.join(format!(
        "{fixture}-{}-{:?}.fsl",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, source).expect("write explain fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            command,
            path.to_str().expect("UTF-8 fixture path"),
            "--depth",
            "3",
        ])
        .current_dir(root)
        .output()
        .expect("run native CLI");
    assert_eq!(
        output.status.code(),
        Some(expected_status),
        "{command} failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("native CLI JSON")
}

fn run_explain(source: &str, fixture: &str) -> Value {
    let value = run_cli(source, fixture, "explain", 0);
    assert_eq!(value["fsl"], "1.0");
    assert_eq!(value["result"], "explained");
    assert_eq!(value["spec"], "C");
    assert_eq!(value["depth"], 3);
    assert_eq!(
        value["counterfactual_scope"],
        json!({
            "properties": ["invariant", "reachable"],
            "liveness": "skipped",
            "reason": "counterfactual weakening is safety/reachability analysis; leadsTo remains verified by the normal verify command",
        })
    );
    value
}

fn assert_guard_candidate(item: &Value, invariant: &str, property_line: usize) {
    assert_eq!(item["invariant"], invariant, "{item}");
    assert_eq!(
        item["weakening"],
        json!({
            "op": "requires-removal",
            "loc": {"line": 6, "column": 5},
            "target": "inc requires #1",
            "source_text": "requires x < 2",
        }),
        "{item}"
    );
    assert_eq!(item.get("requirement"), Some(&Value::Null), "{item}");
    assert!(item.get("note").is_none(), "{item}");
    assert_eq!(item["violation"]["violation_kind"], "invariant", "{item}");
    assert_eq!(item["violation"]["invariant"], invariant, "{item}");
    assert_eq!(
        item["violation"]["loc"],
        json!({"line": property_line, "column": 3}),
        "{item}"
    );
    assert_eq!(item["violation"]["violated_at_step"], 3, "{item}");
    let trace = item["trace"].as_array().expect("nonempty candidate trace");
    assert!(!trace.is_empty(), "{item}");
    assert_eq!(trace.len(), 4, "{item}");
    // Final-state correctness is checked separately by the trace detector.
}

/// Detector: the same removed guard protects both invariants, in either order.
#[test]
fn shared_guard_counterfactual_credits_each_violated_invariant() {
    let first = "  invariant AtMostTwo { x <= 2 }";
    let second = "  invariant NotThree { x != 3 }";
    let observations = [
        (
            "forward",
            SINGLE_INVARIANT.replace(first, &format!("{first}\n{second}")),
            ["AtMostTwo", "NotThree"],
        ),
        (
            "reversed",
            SINGLE_INVARIANT.replace(first, &format!("{second}\n{first}")),
            ["NotThree", "AtMostTwo"],
        ),
    ]
    .map(|(fixture, source, names)| (fixture, run_explain(&source, fixture), names));

    // Execute and inspect BOTH orders before asserting candidate completeness,
    // so red calibration also observes the reversed-order missing invariant.
    let mut missing = Vec::new();
    for (fixture, value, names) in &observations {
        let items = value["counterfactuals"]
            .as_array()
            .expect("counterfactuals");
        assert_eq!(items.len(), 2, "{fixture}: {items:?}");
        for (item, name) in items.iter().zip(names) {
            assert_eq!(
                item["invariant"], *name,
                "preserve {fixture} property order"
            );
            if item["weakening"].is_null() {
                missing.push(format!("{fixture}: {name}, note={}", item["note"]));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "shared guard must yield a candidate for each invariant; missing {missing:?}"
    );
    for (_, value, names) in &observations {
        for (index, name) in names.iter().enumerate() {
            assert_guard_candidate(&value["counterfactuals"][index], name, 9 + index);
        }
    }
}

/// Preservation control, not a detector of missing multi-invariant attribution.
/// Whole-output base-byte agreement is a separate local check, not a golden
/// that would pin the old trace defect or require a base binary in CI.
#[test]
fn single_invariant_explanation_keeps_its_candidate() {
    let value = run_explain(SINGLE_INVARIANT, "single");
    let items = value["counterfactuals"]
        .as_array()
        .expect("counterfactuals");
    assert_eq!(items.len(), 1, "{items:?}");
    assert_guard_candidate(&items[0], "AtMostTwo", 9);
}

/// Detector: losing one assignment makes both reachable goals fail.
#[test]
fn shared_assignment_counterfactual_credits_each_failed_reachable() {
    let value = run_explain(SHARED_REACHABLE, "shared-reachable");
    assert_eq!(value["counterfactuals"], json!([]));
    let expected = ["Done", "AlsoDone"].map(|property| {
        json!({
            "property": property,
            "weakening": {
                "op": "assignment-removal",
                "loc": {"line": 4, "column": 21},
                "target": "finish assignment",
                "source_text": "action finish() { done = true }",
            },
            "result": "reachable_failed",
            "requirement": null,
            "faithfulness_class": "intent_unexercised",
            "recommended_action": "add a single-shot reachable for the action / raise --depth",
        })
    });
    assert_eq!(
        value["reachable_counterfactuals"],
        json!(expected),
        "expected both Done and AlsoDone assignment-removal candidates"
    );
}

/// Detector: a counterfactual must show the violating successor, not rollback state.
#[test]
fn counterfactual_violating_step_records_successor_and_changes() {
    // Preserve the exact two-entity reproducer. Verify and explain may choose
    // different r values, so compare their violating values, not entity identity.
    // Remove ONLY the protecting guard, preserving all source locations.
    let unguarded = TRACE_MAP.replace("    requires x[r] < 2\n", "\n");
    let verified = run_cli(&unguarded, "trace-map-noguard", "verify", 1);
    assert_eq!(verified["result"], "violated");
    assert_eq!(verified["violation_kind"], "invariant");
    assert_eq!(verified["invariant"], "AtMostTwo");
    assert_eq!(verified["violated_at_step"], 3);
    let verify_trace = verified["trace"].as_array().expect("verify trace");
    assert_eq!(verify_trace.len(), 4);
    let expected = &verify_trace[3];
    assert_eq!(expected["step"], 3);
    let verify_r = expected["action"]["params"]["r"]
        .as_i64()
        .expect("verify entity binding")
        .to_string();
    assert_eq!(expected["state"]["x"][&verify_r], 3);
    assert!(
        !expected["changes"]
            .as_object()
            .expect("verify changes")
            .is_empty()
    );

    let explained = run_explain(TRACE_MAP, "trace-map");
    let items = explained["counterfactuals"]
        .as_array()
        .expect("counterfactuals");
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item["invariant"], "AtMostTwo");
    assert_eq!(item["weakening"]["op"], "requires-removal");
    assert_eq!(item["weakening"]["source_text"], "requires x[r] < 2");
    assert_eq!(item["violation"]["invariant"], "AtMostTwo");
    assert_eq!(item["violation"]["violated_at_step"], 3);
    let explain_trace = item["trace"].as_array().expect("counterfactual trace");
    assert_eq!(explain_trace.len(), 4);
    let actual = &explain_trace[3];
    assert_eq!(actual["step"], 3);
    let r = actual["action"]["params"]["r"]
        .as_i64()
        .expect("counterfactual entity binding")
        .to_string();
    let previous = &explain_trace[2];
    assert_eq!(previous["step"], 2);
    assert_eq!(previous["state"]["x"][&r], 2);
    let map = actual["state"]["x"].as_object().expect("step-3 Map state");
    let violating_entities = map
        .iter()
        .filter(|(_, value)| **value == json!(3))
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>();
    let changes = actual["changes"]
        .as_object()
        .expect("counterfactual changes");
    assert_eq!(
        violating_entities,
        vec![r.as_str()],
        "expected exactly one x[r]=3 at step 3 and nonempty changes; got state {} and changes {}",
        actual["state"],
        actual["changes"]
    );
    assert_eq!(actual["state"]["x"][&r], expected["state"]["x"][&verify_r]);
    assert!(!changes.is_empty());
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[&format!("x[{r}]")], json!({"from": 2, "to": 3}));
    let mut expected_state = previous["state"].clone();
    expected_state["x"][&r] = json!(3);
    assert_eq!(
        actual["state"], expected_state,
        "every other step-3 state key must equal its step-2 value"
    );
}

// SPDX-License-Identifier: Apache-2.0

//! Fallback search keeps valid siblings and never starts from a violated root.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

const GUARD_PARTIAL: &str = r"spec C {
  type N = 0..5
  type D = 0..2
  state { x: N, y: N }
  init { x = 0
  y = 0 }
  action bump(d: D) {
    requires d == 0
    x = x + d
  }
  action poke() {
    requires 6 / (2 - x) > 0
    y = 0
  }
  invariant Mid { x < 4 }
}
";

const BAD_INIT: &str = r"spec C {
  type N = 1..5
  type D = 0..2
  state { x: N }
  init { x = 0 }
  action bump(d: D) {
    requires d == 0
    x = x + d
  }
  invariant Mid { x < 3 }
}
";

const BOUNDS_CONTROL: &str = r"spec C {
  type N = 0..1
  type D = 0..2
  state { x: N }
  init { x = 0 }
  action bump(d: D) {
    requires d == 0
    x = x + d
  }
  invariant Zero { x == 0 }
}
";

const FREE_ORIGINAL_INIT: &str = r"spec C {
  type N = 0..6
  type D = 0..2
  state { x: N, w: Int }
  init { x = 0 }
  action bump(d: D) {
    requires d == 0
    x = x + d
  }
  action poke() {
    requires 6 / (x - 2) > 0
    w = 0
  }
  invariant Mid { x < 4 }
}
";

const FREE_INIT_MUTANT: &str = r"spec C {
  type Y = 0..5
  state { x: Int, y: Y, z: Int }
  init { x = 1
  y = 0
  z = 0 }
  action div() {
    z = 10 / x
  }
  action inc() {
    requires y < 3
    y = y + x / 7
  }
  invariant Low { y <= 1 }
}
";

const TRANS_CONTROL: &str = r"spec C {
  type N = 0..5
  type D = 0..2
  state { x: N }
  init { x = 0 }
  action bump(d: D) {
    requires d == 0
    x = x + d
  }
  trans NoBigStep { x <= old(x) + 1 }
  invariant Mid { x < 3 }
}
";

fn fixture_path(label: &str, extension: &str) -> PathBuf {
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/issue-1350-fallback-edges");
    std::fs::create_dir_all(&directory).expect("worktree-local fixture directory");
    directory.join(format!(
        "{label}-{}-{:?}.{extension}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn run_json(source: &str, label: &str, args: &[&str], status: i32) -> Value {
    let path = fixture_path(label, "fsl");
    std::fs::write(&path, source).expect("write inline fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg(args[0])
        .arg(&path)
        .args(&args[1..])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("native CLI");
    assert_eq!(
        output.status.code(),
        Some(status),
        "{args:?}: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("native JSON")
}

fn only_item(value: &Value, name: &str) -> Value {
    assert_eq!(value["result"], "explained", "{value}");
    assert_eq!(value["depth"], 3);
    assert_eq!(
        value["counterfactual_scope"]["properties"],
        json!(["invariant", "reachable"])
    );
    assert_eq!(value["counterfactual_scope"]["liveness"], "skipped");
    let entries = value["counterfactuals"]
        .as_array()
        .expect("counterfactuals");
    assert_eq!(entries.len(), 1, "{value}");
    assert_eq!(entries[0]["invariant"], name);
    assert_eq!(entries[0].get("requirement"), Some(&Value::Null));
    entries[0].clone()
}

fn replay_mid(source: &str, label: &str, deltas: &[i64], old_x: i64) {
    let trace = fixture_path(label, "json");
    let events = deltas
        .iter()
        .map(|d| json!({"action": "bump", "params": {"d": d}}))
        .collect::<Vec<_>>();
    std::fs::write(&trace, serde_json::to_vec(&events).expect("events JSON"))
        .expect("retain replay trace");
    let value = run_json(
        source,
        label,
        &["replay", "--trace", trace.to_str().expect("trace path")],
        1,
    );
    assert_eq!(value["result"], "nonconformant");
    assert_eq!(value["failed_at_event"], deltas.len() - 1, "{value}");
    assert_eq!(value["violation"]["kind"], "invariant", "{value}");
    assert_eq!(value["violation"]["name"], "Mid", "{value}");
    assert_eq!(value["violation"]["action"], "bump");
    assert_eq!(
        value["violation"]["params"],
        json!({"d": deltas.last().expect("last delta")})
    );
    assert_eq!(value["state_before"], json!({"x": old_x, "y": 0}));
}

/// Detector: an unrelated guard-partial instance cannot abort valid siblings.
#[test]
fn guard_partial_instance_keeps_shortest_clean_counterfactual() {
    let unguarded = GUARD_PARTIAL.replace("    requires d == 0\n", "\n");
    let verified = run_json(
        &unguarded,
        "guard-partial-verify",
        &["verify", "--depth", "3", "--no-cache"],
        1,
    );
    assert_eq!(verified["violation_kind"], "partial_op", "{verified}");
    assert_eq!(verified["invariant"], "_partial_poke", "{verified}");
    // Both witnesses reject only the target on their chosen edges. Ranking
    // selects d=2,2; d=1,2,1 remains the review probe's independent replay oracle.
    replay_mid(&unguarded, "shortest-replay", &[2, 2], 2);
    replay_mid(&unguarded, "review-replay", &[1, 2, 1], 3);
    let value = run_json(
        GUARD_PARTIAL,
        "guard-partial-explain",
        &["explain", "--depth", "3"],
        0,
    );
    let item = only_item(&value, "Mid");
    assert_eq!(
        item["weakening"],
        json!({
            "op": "requires-removal", "loc": {"line": 8, "column": 5},
            "target": "bump requires #1", "source_text": "requires d == 0",
        }),
        "expected a guard-removal witness, not a preempted partial_op note: {item}"
    );
    assert!(item.get("note").is_none(), "{item}");
    assert_eq!(item["violation"]["violation_kind"], "invariant");
    assert_eq!(item["violation"]["invariant"], "Mid");
    assert_eq!(item["violation"]["loc"], json!({"line": 15, "column": 3}));
    assert_eq!(item["violation"]["violated_at_step"], 2);
    let trace = item["trace"].as_array().expect("shortest trace");
    assert_eq!(trace.len(), 3);
    for (step, entry) in trace.iter().enumerate() {
        assert_eq!(entry["step"], step);
        assert_eq!(entry["state"], json!({"x": step * 2, "y": 0}));
        let x = entry["state"]["x"].as_i64().expect("x");
        assert!((0..=5).contains(&x), "out-of-bounds edge: {entry}");
        if step > 0 {
            assert_eq!(entry["action"]["name"], "bump");
            assert_eq!(entry["action"]["params"], json!({"d": 2}));
            assert_eq!(
                entry["changes"],
                json!({"x": {"from": (step - 1) * 2, "to": step * 2}})
            );
        }
    }
}

/// Detector: the concrete fallback cannot launder an already-invalid root.
#[test]
fn violated_initial_state_keeps_honest_note_instead_of_concrete_trace() {
    let verified = run_json(
        BAD_INIT,
        "bad-init-verify",
        &["verify", "--depth", "3", "--no-cache"],
        1,
    );
    assert_eq!(verified["violation_kind"], "type_bound");
    assert_eq!(verified["invariant"], "_bounds_x");
    // The existing verify pre-pass first reports bounds on its d=0 step;
    // the observed initial state is already invalid. Do not change verify.
    assert_eq!(verified["violated_at_step"], 1);
    assert_eq!(verified["trace"][0]["state"], json!({"x": 0}));
    let value = run_json(
        BAD_INIT,
        "bad-init-explain",
        &["explain", "--depth", "3"],
        0,
    );
    let item = only_item(&value, "Mid");
    assert!(
        item["weakening"].is_null() && item["trace"].is_null(),
        "out-of-bounds initial state must not supply a concrete witness: {item}"
    );
    assert!(item.get("violation").is_none(), "{item}");
    let note = item["note"].as_str().expect("honest preemption note");
    assert!(
        note.contains("preempted by type_bound _bounds_x at step 0"),
        "{note}"
    );
    assert!(!note.contains("no counterfactual within depth"), "{note}");
}

/// Preservation control: p6's existing valid d=1 candidate stays unchanged.
#[test]
fn existing_bounds_fallback_keeps_its_clean_guard_candidate() {
    let value = run_json(
        BOUNDS_CONTROL,
        "bounds-control",
        &["explain", "--depth", "3"],
        0,
    );
    let item = only_item(&value, "Zero");
    assert_eq!(
        item["weakening"],
        json!({
            "op": "requires-removal", "loc": {"line": 7, "column": 5},
            "target": "bump requires #1", "source_text": "requires d == 0",
        })
    );
    assert!(item.get("note").is_none());
    assert_eq!(item["violation"]["violation_kind"], "invariant");
    assert_eq!(item["violation"]["invariant"], "Zero");
    assert_eq!(item["violation"]["violated_at_step"], 1);
    assert_eq!(item["violation"]["loc"], json!({"line": 10, "column": 3}));
    let trace = item["trace"].as_array().expect("control trace");
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0]["state"], json!({"x": 0}));
    assert_eq!(trace[1]["state"], json!({"x": 1}));
    assert_eq!(trace[1]["action"]["name"], "bump");
    assert_eq!(trace[1]["action"]["params"], json!({"d": 1}));
    assert_eq!(trace[1]["changes"], json!({"x": {"from": 0, "to": 1}}));
}

fn assert_admissible_replay(
    source: &str,
    label: &str,
    events: &[Value],
    invariant: &str,
    state_before: &Value,
) {
    let trace = fixture_path(label, "json");
    std::fs::write(&trace, serde_json::to_vec(events).expect("replay JSON"))
        .expect("retain admissible valuation oracle");
    let value = run_json(
        source,
        label,
        &["replay", "--trace", trace.to_str().expect("trace path")],
        1,
    );
    assert_eq!(value["result"], "nonconformant");
    assert_eq!(value["failed_at_event"], events.len() - 1, "{value}");
    assert_eq!(value["violation"]["kind"], "invariant", "{value}");
    assert_eq!(value["violation"]["name"], invariant, "{value}");
    let last = events.last().expect("last event");
    assert_eq!(value["violation"]["action"], last["action"]);
    assert_eq!(value["violation"]["params"], last["params"]);
    assert_eq!(&value["state_before"], state_before);
}

fn assert_unavailable_note(item: &Value, root: &str, check: &str) {
    assert_eq!(item.get("weakening"), Some(&Value::Null), "{item}");
    assert_eq!(item.get("trace"), Some(&Value::Null), "{item}");
    assert!(item.get("violation").is_none(), "{item}");
    let note = item["note"].as_str().expect("startup diagnostic");
    assert!(note.starts_with(check), "{note}");
    assert!(
        note.contains(&format!(
            "; concrete search not run: nondeterministic initialization of state {root} is unsupported"
        )),
        "missing startup failure and free root {root}: {note}"
    );
    assert!(!note.contains("no counterfactual within depth"), "{note}");
}

/// R2 detector: the original's free w prevents concrete startup, not a witness.
#[test]
fn original_free_root_is_named_when_preempted_search_cannot_start() {
    // One supplied admissible valuation, not a search over nondeterministic init.
    let concrete = FREE_ORIGINAL_INIT
        .replace("init { x = 0 }", "init { x = 0\n  w = 5 }")
        .replace("    requires d == 0\n", "\n");
    assert_admissible_replay(
        &concrete,
        "free-w-oracle",
        &[
            json!({"action": "bump", "params": {"d": 2}}),
            json!({"action": "bump", "params": {"d": 2}}),
        ],
        "Mid",
        &json!({"x": 2, "w": 5}),
    );
    let value = run_json(
        FREE_ORIGINAL_INIT,
        "free-w",
        &["explain", "--depth", "3"],
        0,
    );
    assert_unavailable_note(
        &only_item(&value, "Mid"),
        "w",
        "preempted by partial_op _partial_poke at step 2",
    );
}

/// R2 detector: init removal frees x; a partial div must not hide that inability.
#[test]
fn mutant_free_root_is_named_when_preempted_search_cannot_start() {
    // The review's x=7 seed is admissible after removal of x's init assignment.
    let concrete = FREE_INIT_MUTANT.replace("init { x = 1", "init { x = 7");
    assert_admissible_replay(
        &concrete,
        "free-x-oracle",
        &[
            json!({"action": "inc", "params": {}}),
            json!({"action": "inc", "params": {}}),
        ],
        "Low",
        &json!({"x": 7, "y": 1, "z": 0}),
    );
    let value = run_json(FREE_INIT_MUTANT, "free-x", &["explain", "--depth", "3"], 0);
    assert_unavailable_note(
        &only_item(&value, "Low"),
        "x",
        "preempted by partial_op _partial_div at step 1",
    );
}

fn normalized_entry(mut item: Value) -> Value {
    let elapsed = item["violation"]["cost"]
        .get_mut("elapsed_s")
        .expect("live elapsed field on candidate");
    let value = elapsed.as_f64().expect("numeric elapsed field");
    assert!(value >= 0.0);
    *elapsed = json!(0);
    item
}

// Complete entry baselines captured twice before R2. Only the observed live
// elapsed_s varies; no trace, note, location, blame or assurance is excluded.
fn expected_bump_entry(with_y: bool) -> Value {
    let (guard_line, property_line, steps, delta, predicate) = if with_y {
        (8, 15, 2, 2, "x < 4")
    } else {
        (7, 11, 3, 1, "x < 3")
    };
    let trace = (0..=steps)
        .map(|step| {
            let state = if with_y {
                json!({"x": step * delta, "y": 0})
            } else {
                json!({"x": step * delta})
            };
            let mut entry = json!({"step": step, "state": state});
            if step > 0 {
                entry["action"] = json!({"name": "bump", "params": {"d": delta}, "loc": {"line": guard_line - 1, "column": 3}});
                entry["changes"] = json!({"x": {"from": (step - 1) * delta, "to": step * delta}});
                entry["blame"] = json!({"guards": [], "effects": [{"target": "x", "text": "x = x + d", "loc": {"line": guard_line + 1, "column": 5}}]});
            }
            entry
        })
        .collect::<Vec<_>>();
    json!({
        "invariant": "Mid",
        "weakening": {"op": "requires-removal", "loc": {"line": guard_line, "column": 5}, "target": "bump requires #1", "source_text": "requires d == 0"},
        "trace": trace,
        "requirement": null,
        "violation": {
            "violation_kind": "invariant", "invariant": "Mid", "loc": {"line": property_line, "column": 3},
            "violated_at_step": steps, "violating_bindings": [{}],
            "blame": {"conjuncts": [{"index": 0, "text": predicate, "holds": false, "violating_bindings": [{}]}]},
            "last_action": {"name": "bump", "params": {"d": delta}, "loc": {"line": guard_line - 1, "column": 3}},
            "checked_to_depth": steps, "completeness": "bounded", "cost": {"elapsed_s": 0},
        },
    })
}

/// R2 preservation: full pB entry, not just its candidate or trace projection.
#[test]
fn guard_partial_full_entry_is_preserved() {
    let value = run_json(GUARD_PARTIAL, "full-pb", &["explain", "--depth", "3"], 0);
    assert_eq!(
        normalized_entry(only_item(&value, "Mid")),
        expected_bump_entry(true)
    );
}

/// R2 preservation: full pH entry and every existing note byte remain unchanged.
#[test]
fn invalid_init_full_entry_is_preserved() {
    let value = run_json(BAD_INIT, "full-ph", &["explain", "--depth", "3"], 0);
    assert_eq!(
        only_item(&value, "Mid"),
        json!({
            "invariant": "Mid", "weakening": null, "trace": null, "requirement": null,
            "note": "preempted by type_bound _bounds_x at step 0; init-assignment removal of bounded state x was not analyzed (skipped: nondeterministic initialization is unsupported by concrete search)",
        })
    );
}

/// R2 preservation: full p8b trans-respecting candidate remains unchanged.
#[test]
fn trans_fallback_full_entry_is_preserved() {
    let value = run_json(TRANS_CONTROL, "full-p8b", &["explain", "--depth", "3"], 0);
    assert_eq!(
        normalized_entry(only_item(&value, "Mid")),
        expected_bump_entry(false)
    );
}

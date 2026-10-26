// SPDX-License-Identifier: Apache-2.0

//! A first non-user check failure must not hide a target counterfactual.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

const BOUNDS_PREEMPTION: &str = r"spec C {
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

const TRANS_PREEMPTION: &str = r"spec C {
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

const RANGE_INIT: &str = r"spec C {
  type N = 0..1
  state { x: N }
  init { x = 0 }
  action stay() { x = x }
  invariant Zero { x == 0 }
}
";

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

fn no_candidate_source() -> String {
    // This target deliberately cannot fail in bounds. It detects dishonest
    // absence notes, not spec strength, and does not weaken either reproducer.
    BOUNDS_PREEMPTION.replace("invariant Zero { x == 0 }", "invariant InBounds { x <= 1 }")
}

fn fixture_path(label: &str, extension: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/issue-1350-preemption");
    std::fs::create_dir_all(&directory).expect("create worktree-local fixture directory");
    // Retain generated evidence for diagnosis; do not delete inherited files or
    // write outside the materialized worktree.
    directory.join(format!(
        "{label}-{}-{:?}.{extension}",
        std::process::id(),
        std::thread::current().id()
    ))
}

fn run_cli(source: &str, label: &str, args: &[&str], expected_status: i32) -> String {
    let path = fixture_path(label, "fsl");
    std::fs::write(&path, source).expect("write inline fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg(args[0])
        .arg(&path)
        .args(&args[1..])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run native CLI");
    assert_eq!(
        output.status.code(),
        Some(expected_status),
        "{args:?}: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).expect("UTF-8 CLI output")
}

fn run_json(source: &str, label: &str, args: &[&str], expected_status: i32) -> Value {
    serde_json::from_str(&run_cli(source, label, args, expected_status)).expect("native CLI JSON")
}

fn explain(source: &str, label: &str, depth: &str) -> Value {
    let value = run_json(source, label, &["explain", "--depth", depth], 0);
    assert_eq!(value["fsl"], "1.0");
    assert_eq!(value["result"], "explained");
    assert_eq!(value["spec"], "C");
    assert_eq!(value["depth"], depth.parse::<usize>().expect("depth"));
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

fn only_item<'a>(value: &'a Value, invariant: &str) -> &'a Value {
    let items = value["counterfactuals"]
        .as_array()
        .expect("counterfactuals");
    assert_eq!(items.len(), 1, "{items:?}");
    let item = &items[0];
    assert_eq!(item["invariant"], invariant, "{item}");
    assert_eq!(item.get("requirement"), Some(&Value::Null), "{item}");
    item
}

fn assert_preempting_verify(source: &str, label: &str, kind: &str, name: &str, step: usize) {
    let value = run_json(source, label, &["verify", "--depth", "3"], 1);
    assert_eq!(value["result"], "violated", "{value}");
    assert_eq!(value["violation_kind"], kind, "{value}");
    assert_eq!(value["invariant"], name, "{value}");
    assert_eq!(value["violated_at_step"], step, "{value}");
    if step == 1 {
        assert_eq!(value["last_action"]["params"], json!({"d": 2}));
        assert_eq!(value["trace"][1]["state"], json!({"x": 2}));
    }
}

fn assert_d1_replay(source: &str, label: &str, invariant: &str, steps: usize) {
    let path = fixture_path(label, "json");
    let events = vec![json!({"action": "bump", "params": {"d": 1}}); steps];
    std::fs::write(&path, serde_json::to_vec(&events).expect("replay events"))
        .expect("write exact d=1 witness");
    let value = run_json(
        source,
        label,
        &[
            "replay",
            "--trace",
            path.to_str().expect("UTF-8 trace path"),
        ],
        1,
    );
    assert_eq!(value["result"], "nonconformant", "{value}");
    assert_eq!(value["failed_at_event"], steps - 1, "{value}");
    assert_eq!(value["violation"]["kind"], "invariant", "{value}");
    assert_eq!(value["violation"]["name"], invariant, "{value}");
    assert_eq!(value["violation"]["action"], "bump", "{value}");
    assert_eq!(value["violation"]["params"], json!({"d": 1}), "{value}");
    // Replay returns rollback prestate, not the violating successor.
    assert_eq!(value["state_before"], json!({"x": steps - 1}), "{value}");
}

fn assert_bump_candidate(item: &Value, invariant: &str, property_line: usize, steps: usize) {
    assert_eq!(
        item["weakening"],
        json!({
            "op": "requires-removal",
            "loc": {"line": 7, "column": 5},
            "target": "bump requires #1",
            "source_text": "requires d == 0",
        }),
        "expected {invariant} d=1 guard candidate, got {item}"
    );
    assert!(item.get("note").is_none(), "{item}");
    assert_eq!(item["violation"]["violation_kind"], "invariant", "{item}");
    assert_eq!(item["violation"]["invariant"], invariant, "{item}");
    assert_eq!(item["violation"]["violated_at_step"], steps, "{item}");
    assert_eq!(
        item["violation"]["loc"],
        json!({"line": property_line, "column": 3})
    );
    let trace = item["trace"].as_array().expect("nonempty target trace");
    assert_eq!(trace.len(), steps + 1, "{item}");
    let upper_bound = if invariant == "Zero" { 1 } else { 5 };
    for (index, entry) in trace.iter().enumerate() {
        assert_eq!(entry["step"], index, "{entry}");
        assert_eq!(entry["state"], json!({"x": index}), "{entry}");
        let x = entry["state"]["x"].as_i64().expect("integer x");
        assert!(
            (0..=upper_bound).contains(&x),
            "out-of-bounds witness: {entry}"
        );
        if index > 0 {
            assert_eq!(entry["action"]["name"], "bump", "{entry}");
            assert_eq!(entry["action"]["params"], json!({"d": 1}), "{entry}");
            assert_eq!(
                entry["changes"],
                json!({"x": {"from": index - 1, "to": index}}),
                "{entry}"
            );
            if invariant == "Mid" {
                let old_x = trace[index - 1]["state"]["x"].as_i64().expect("old x");
                assert!(x <= old_x + 1, "NoBigStep violated: {entry}");
            }
        }
    }
}

/// Detector: p6's d=2 bounds failure must not hide its valid d=1 target witness.
#[test]
fn bounds_preemption_finds_in_bounds_guard_counterfactual() {
    let unguarded = BOUNDS_PREEMPTION.replace("    requires d == 0\n", "\n");
    assert_preempting_verify(&unguarded, "bounds-noguard", "type_bound", "_bounds_x", 1);
    assert_d1_replay(&unguarded, "bounds-d1", "Zero", 1);
    let value = explain(BOUNDS_PREEMPTION, "bounds", "3");
    assert_bump_candidate(only_item(&value, "Zero"), "Zero", 10, 1);
}

/// Detector: p8b needs a longer trans-respecting path, not the first trans failure.
#[test]
fn trans_preemption_finds_respecting_guard_counterfactual() {
    let unguarded = TRANS_PREEMPTION.replace("    requires d == 0\n", "\n");
    assert_preempting_verify(&unguarded, "trans-noguard", "trans", "NoBigStep", 1);
    assert_d1_replay(&unguarded, "trans-d1", "Mid", 3);
    let value = explain(TRANS_PREEMPTION, "trans", "3");
    assert_bump_candidate(only_item(&value, "Mid"), "Mid", 11, 3);
}

fn assert_preemption_note(note: &str) {
    assert!(
        note.contains("preempted by type_bound _bounds_x at step 1"),
        "expected observed preemption kind/name/step, got {note:?}"
    );
    assert!(
        !note.contains("no counterfactual within depth"),
        "preempted analysis must not claim absence: {note}"
    );
}

/// Detector: absence of an in-bounds witness does not erase an observed preemption.
#[test]
fn preempted_search_reports_kind_name_step_without_absence_claim() {
    let source = no_candidate_source();
    let unguarded = source.replace("    requires d == 0\n", "\n");
    assert_preempting_verify(
        &unguarded,
        "no-candidate-noguard",
        "type_bound",
        "_bounds_x",
        1,
    );
    let value = explain(&source, "no-candidate", "3");
    let item = only_item(&value, "InBounds");
    assert_eq!(item.get("weakening"), Some(&Value::Null), "{item}");
    assert_eq!(item.get("trace"), Some(&Value::Null), "{item}");
    assert!(item.get("violation").is_none(), "{item}");
    assert_preemption_note(item["note"].as_str().expect("analysis note"));
}

/// Detector: the note must render in table evidence, not merely the raw JSON panel.
#[test]
fn preemption_note_is_rendered_in_html_evidence() {
    let report_path = fixture_path("preemption-note", "html");
    let value = run_json(
        &no_candidate_source(),
        "html-note",
        &[
            "html",
            "--depth",
            "3",
            "-o",
            report_path.to_str().expect("report path"),
        ],
        0,
    );
    assert_eq!(value["result"], "generated");
    assert_eq!(value["kind"], "html_report");
    let html = std::fs::read_to_string(report_path).expect("generated HTML");
    let section = html
        .split_once("id=\"counterfactuals\"")
        .expect("counterfactual section")
        .1
        .split_once("</section>")
        .expect("end counterfactual section")
        .0;
    assert!(section.contains("<code>InBounds</code>"), "{section}");
    assert!(section.contains("Trace / violation"), "{section}");
    assert!(section.contains("&quot;note&quot;"), "{section}");
    assert_preemption_note(section);
}

/// Detector: the authorized init policy permits analysis OR a specific skip reason.
#[test]
fn range_init_removal_is_analyzed_or_explicitly_explained() {
    let removed = RANGE_INIT.replace("init { x = 0 }", "init { }");
    assert_preempting_verify(&removed, "init-removed", "type_bound", "_bounds_x", 0);
    let value = explain(RANGE_INIT, "range-init", "3");
    let item = only_item(&value, "Zero");
    if item["weakening"].is_object() {
        assert_eq!(item["weakening"]["op"], "assignment-removal");
        assert_eq!(item["weakening"]["origin"], "init");
        assert_eq!(item["weakening"]["label"], "init weakening");
        assert_eq!(item["weakening"]["source_text"], "init { x = 0 }");
        assert_eq!(item["violation"]["violation_kind"], "invariant");
        assert_eq!(item["violation"]["invariant"], "Zero");
        assert_eq!(item["violation"]["violated_at_step"], 0);
        let trace = item["trace"].as_array().expect("initial violation trace");
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0]["step"], 0);
        assert_eq!(trace[0]["state"], json!({"x": 1}));
        assert!(item.get("note").is_none());
    } else {
        assert_eq!(item.get("weakening"), Some(&Value::Null));
        assert_eq!(item.get("trace"), Some(&Value::Null));
        let note = item["note"].as_str().expect("init analysis/skip reason");
        let analyzed = note.contains("preempted by type_bound _bounds_x at step 0");
        let reason = note.to_ascii_lowercase();
        let explained_skip = reason.contains("skip")
            && reason.contains("init")
            && reason.contains("assignment")
            && reason.contains('x')
            && (reason.contains("bounded") || reason.contains("range"));
        // Deliberate approved disjunction: no new runtime init enumeration is
        // required, but an unexplained generic depth note cannot pass.
        assert!(
            analyzed || explained_skip,
            "expected analyzed range init removal or explicit bounded-init skip reason, got {note:?}"
        );
        assert!(
            !note.contains("no counterfactual within depth")
                || (explained_skip
                    && note
                        .ends_with("; init-assignment removal of bounded state x was not analyzed (skipped: nondeterministic initialization is unsupported by concrete search)")),
            "{note}"
        );
    }
}

/// Preservation control, not a detector of non-user-check preemption.
#[test]
fn existing_guard_candidate_survives_preemption_fallback() {
    let value = explain(SINGLE_INVARIANT, "single", "3");
    let item = only_item(&value, "AtMostTwo");
    assert_eq!(
        item["weakening"],
        json!({
            "op": "requires-removal",
            "loc": {"line": 6, "column": 5},
            "target": "inc requires #1",
            "source_text": "requires x < 2",
        })
    );
    assert!(item.get("note").is_none());
    assert_eq!(item["violation"]["violation_kind"], "invariant");
    assert_eq!(item["violation"]["invariant"], "AtMostTwo");
    assert_eq!(item["violation"]["loc"], json!({"line": 9, "column": 3}));
    assert_eq!(item["violation"]["violated_at_step"], 3);
    let trace = item["trace"].as_array().expect("single candidate trace");
    assert_eq!(trace.len(), 4);
    for (step, entry) in trace.iter().enumerate() {
        assert_eq!(entry["step"], step);
        assert_eq!(entry["state"], json!({"x": step}));
        if step > 0 {
            assert_eq!(entry["action"]["name"], "inc");
            assert_eq!(entry["action"]["params"], json!({}));
            assert_eq!(
                entry["changes"],
                json!({"x": {"from": step - 1, "to": step}})
            );
        }
    }
}

/// Preservation control: a real shallow search retains the exact existing note.
#[test]
fn genuine_depth_note_is_preserved() {
    let value = explain(SINGLE_INVARIANT, "depth-control", "2");
    assert_eq!(
        value["counterfactuals"],
        json!([{
            "invariant": "AtMostTwo",
            "weakening": null,
            "trace": null,
            "requirement": null,
            "note": "no counterfactual within depth 2; init-assignment removal of bounded state x was not analyzed (skipped: nondeterministic initialization is unsupported by concrete search)",
        }])
    );
}

/// Preservation control: readable remains the deterministic scope/skeleton view.
#[test]
fn readable_output_keeps_scope_and_skeleton() {
    let text = run_cli(
        &no_candidate_source(),
        "readable",
        &["explain", "--depth", "3", "--readable"],
        0,
    );
    assert_eq!(text, READABLE_NO_CANDIDATE);
}

const READABLE_NO_CANDIDATE: &str = "Spec: C (depth 3)
Counterfactual scope: invariant/reachable (leadsTo liveness skipped; use verify)

Verification world:
  - D: values 0..2 (3 values)
  - N: values 0..1 (2 values)

State:
  - x: N

Actions:
  - bump(d: D)
    requires:
      - requires d == 0
    writes: x

Properties:
  - invariant InBounds
    body: x <= 1

Automatic checks:
  - type_bound: x (implicit bounded-domain check)
";

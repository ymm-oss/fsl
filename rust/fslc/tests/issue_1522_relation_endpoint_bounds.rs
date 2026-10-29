// SPDX-License-Identifier: Apache-2.0

//! #1522 / #1524 option B: relation compatibility compares endpoint carriers,
//! recursively, without tightening scalar ranges, Map keys, or alias identity.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const TEMPLATE: &str = r"spec R2 {
  type A = 0..1
  type B = 0..2
  type C = 0..3
  type D = 1..2
  type N = 0..2
  type M = 0..2
  state { r: $R, q: $Q, done: Bool }
  init { r = Set {} q = Set {} done = false }
  action go() { requires not done q = q.$UPDATE(2, 2) done = true }
  invariant Capture { $P }
}
";

const IF: &str = "not done or (if not done then r else q).contains(2, 2)";
const REVERSED: &str = "not done or (if not done then q else r).contains(2, 2)";
const SEQ: &str = "not done or Seq { r, q }.at(1).contains(2, 2)";
const SET: &str = "Set { r, q }.size() == 2";
const NESTED: &str = "Seq { Set { r }, Set { q } }.size() == 2";
const SCALAR: &str = r"spec Scalar {
  type A = 0..1
  type B = 0..2
  state { a: A, b: B, c: Bool }
  init { a = 0 b = 2 c = false }
  action go() {}
  invariant P { (if c then a else b) >= 0 }
}
";
const MAP: &str = r"spec Maps {
  type A = 0..1
  type B = 0..2
  state { a: Map<A, Bool>, b: Map<B, Bool>, c: Bool }
  init { forall k: A { a[k] = false } forall k: B { b[k] = false } c = false }
  action go() {}
  invariant P { (if c then a else b)[0] == false }
}
";
const NARROW_TEMPLATE: &str = r"spec NW {
  type N = 0..2
  type M = 0..4
  state { r: relation N -> N, q: relation N -> M, done: Bool }
  init { $INIT }
  action go() { requires not done $ACTION done = true }
  invariant P { $P }
}
";
const SET_WIDENING: &str = r"spec SetWidening {
  type N = 0..2
  type M = 0..4
  state { s: Set<N>, t: Set<M>, done: Bool }
  init { s = Set {} t = Set {} done = false }
  action go() { requires not done s = s.add(1) t = s.add(1) done = true }
  invariant P { not done or t.contains(1) }
}
";

fn source(r: &str, q: &str, predicate: &str, update: &str) -> String {
    TEMPLATE
        .replace("$R", r)
        .replace("$Q", q)
        .replace("$UPDATE", update)
        .replace("$P", predicate)
}

fn fixture(label: &str, source: &str) -> PathBuf {
    // Every invocation gets a new directory: preserve prior calibration evidence.
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1522_relation_endpoint_bounds")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).expect("create fixture directory");
    std::fs::write(directory.join("spec.fsl"), source).expect("write fixture");
    std::fs::write(
        directory.join("trace.json"),
        r#"[{"action":"go","params":{}}]"#,
    )
    .expect("write replay trace");
    directory
}

fn run(directory: &Path, consumer: &str) -> (Value, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    match consumer {
        "check" => command.args(["check", "spec.fsl"]),
        "replay" => command.args(["replay", "spec.fsl", "--trace", "trace.json"]),
        engine => command.args([
            "verify",
            "spec.fsl",
            "--depth",
            "4",
            "--no-cache",
            "--engine",
            engine,
        ]),
    };
    let output = command
        .current_dir(directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run native CLI");
    assert!(output.stderr.is_empty(), "{consumer}: {output:?}");
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{consumer}: {error}: {output:?}"));
    (value, output.status.code().expect("CLI exit code"))
}

fn assert_check_ok(label: &str, source: &str) {
    let directory = fixture(label, source);
    let (output, exit) = run(&directory, "check");
    assert_eq!(
        (exit, output["result"].as_str()),
        (0, Some("ok")),
        "{label}: {output:#}"
    );
    assert!(output.get("kind").is_none(), "{label}: {output:#}");
    assert!(output.get("message").is_none(), "{label}: {output:#}");
}

fn assert_rejected(
    label: &str,
    r: &str,
    q: &str,
    predicate: &str,
    column: usize,
    actual: &str,
    expected: &str,
) {
    assert_source_rejected(
        label,
        &source(r, q, predicate, "add"),
        &json!({"line": 11, "column": column}),
        actual,
        expected,
    );
}

fn assert_source_rejected(
    label: &str,
    source: &str,
    location: &Value,
    actual: &str,
    expected: &str,
) {
    let directory = fixture(label, source);
    // Collect all consumers before asserting: main's permissive check must not
    // hide the engine/replay observations behind the first expected RED.
    let outputs = ["check", "bmc", "induction", "explicit", "replay"]
        .map(|consumer| (consumer, run(&directory, consumer)));
    for (consumer, (output, exit)) in &outputs {
        assert_eq!(
            (*exit, output["result"].as_str(), output["kind"].as_str()),
            (2, Some("error"), Some("semantics")),
            "{label} {consumer}: all observations {outputs:#?}"
        );
        let mismatch = format!("expression of type {actual} is not assignable to {expected}");
        assert!(
            output["message"]
                .as_str()
                .expect("error message")
                .contains(&mismatch),
            "{label} {consumer}: {output:#}"
        );
        assert_eq!(&output["loc"], location, "{label} {consumer}: {output:#}");
        // Same frontend gate, not merely independently occurring exit-2 failures.
        let mut expected_output = outputs[0].1.0.clone();
        if *consumer == "replay" {
            // Observed CLI contract: check/verify attach versions to load errors;
            // replay returns the bare load error. Keep this exclusion live.
            assert!(
                expected_output
                    .as_object_mut()
                    .expect("error envelope")
                    .remove("versions")
                    .is_some()
            );
            assert!(output.get("versions").is_none(), "{output:#}");
        }
        assert_eq!(
            output, &expected_output,
            "{label} {consumer}: full error envelope"
        );
    }
}

#[test]
fn conditional_rejects_source_bound_mismatch() {
    assert_rejected(
        "if-source",
        "relation A -> B",
        "relation B -> B",
        IF,
        60,
        r#"Relation(Named("B"), Named("B"))"#,
        r#"Relation(Named("A"), Named("B"))"#,
    );
}

#[test]
fn reversed_conditional_rejects_source_bound_mismatch() {
    assert_rejected(
        "if-reversed",
        "relation A -> B",
        "relation B -> B",
        REVERSED,
        60,
        r#"Relation(Named("A"), Named("B"))"#,
        r#"Relation(Named("B"), Named("B"))"#,
    );
}

#[test]
fn conditional_rejects_codomain_only_bound_mismatch() {
    assert_rejected(
        "if-target",
        "relation A -> B",
        "relation A -> C",
        IF,
        60,
        r#"Relation(Named("A"), Named("C"))"#,
        r#"Relation(Named("A"), Named("B"))"#,
    );
}

#[test]
fn conditional_rejects_shifted_equal_size_carrier() {
    assert_rejected(
        "if-shifted",
        "relation A -> B",
        "relation D -> B",
        IF,
        60,
        r#"Relation(Named("D"), Named("B"))"#,
        r#"Relation(Named("A"), Named("B"))"#,
    );
}

#[test]
fn seq_at_rejects_relation_item_bound_mismatch() {
    // Literal items currently inherit the enclosing invariant's authored span
    // (11:3), not a per-item token span. No new span machinery is required.
    assert_rejected(
        "seq-at",
        "relation A -> B",
        "relation B -> B",
        SEQ,
        3,
        r#"Relation(Named("B"), Named("B"))"#,
        r#"Relation(Named("A"), Named("B"))"#,
    );
}

#[test]
fn set_rejects_relation_item_bound_mismatch() {
    assert_rejected(
        "set",
        "relation A -> B",
        "relation B -> B",
        SET,
        3,
        r#"Relation(Named("B"), Named("B"))"#,
        r#"Relation(Named("A"), Named("B"))"#,
    );
}

#[test]
fn nested_seq_set_rejects_relation_item_bound_mismatch() {
    assert_rejected(
        "nested",
        "relation A -> B",
        "relation B -> B",
        NESTED,
        3,
        r#"Relation(Named("B"), Named("B"))"#,
        r#"Relation(Named("A"), Named("B"))"#,
    );
}

#[test]
fn equal_bound_different_alias_literals_still_check() {
    // #1467 owns the remaining equal-carrier literal engine semantics. Do not
    // pin today's engine exit 2 as accepted behavior or claim it is supported.
    for (label, predicate) in [
        ("equal-seq", SEQ),
        ("equal-set", SET),
        ("equal-nested", NESTED),
    ] {
        assert_check_ok(
            label,
            &source("relation N -> N", "relation M -> M", predicate, "add"),
        );
    }
}

#[test]
fn same_alias_relations_still_check() {
    for (label, predicate) in [("same-if", IF), ("same-seq", SEQ), ("same-set", SET)] {
        assert_check_ok(
            label,
            &source("relation A -> B", "relation A -> B", predicate, "add"),
        );
    }
}

#[test]
fn scalar_range_aliases_remain_compatible() {
    assert_check_ok("scalar", SCALAR);
}

#[test]
fn map_key_range_aliases_remain_compatible() {
    assert_check_ok("map", MAP);
}

#[test]
fn self_relation_gate_still_distinguishes_declared_endpoint_aliases() {
    assert_check_ok(
        "self",
        &source("relation N -> N", "relation M -> M", "acyclic(r)", "add"),
    );
    let directory = fixture(
        "not-self",
        &source("relation N -> M", "relation M -> M", "acyclic(r)", "add"),
    );
    let (output, exit) = run(&directory, "check");
    assert_eq!(
        (exit, output["result"].as_str(), output["kind"].as_str()),
        (2, Some("error"), Some("semantics")),
        "{output:#}"
    );
    assert!(
        output["message"]
            .as_str()
            .expect("self-relation error")
            .contains("acyclic() requires a self-relation (relation T -> T)"),
        "{output:#}"
    );
    assert_eq!(
        output["loc"],
        json!({"line": 11, "column": 3}),
        "{output:#}"
    );
}

fn assert_equal_alias_verdicts(update: &str, violation: bool) {
    let directory = fixture(
        update,
        &source("relation N -> N", "relation M -> M", IF, update),
    );
    let outputs = ["check", "bmc", "induction", "explicit", "replay"]
        .map(|consumer| (consumer, run(&directory, consumer)));
    for (consumer, (output, exit)) in &outputs {
        let result = match (*consumer, violation) {
            ("check", _) => "ok",
            ("replay", true) => "nonconformant",
            ("replay", false) => "conformant",
            (_, true) => "violated",
            ("bmc", false) => "verified",
            (_, false) => "proved",
        };
        assert_eq!(
            (*exit, output["result"].as_str()),
            (i32::from(violation && *consumer != "check"), Some(result)),
            "{consumer}: {outputs:#?}"
        );
        assert!(output.get("kind").is_none(), "{consumer}: {output:#}");
        assert!(output.get("message").is_none(), "{consumer}: {output:#}");
        if *consumer == "check" {
            continue;
        }
        if violation {
            if *consumer == "replay" {
                assert_eq!(output["violation"]["kind"], "invariant", "{output:#}");
                assert_eq!(output["violation"]["name"], "Capture", "{output:#}");
                assert_eq!(output["failed_at_event"], 0, "{output:#}");
            } else {
                assert_eq!(output["violation_kind"], "invariant", "{output:#}");
                assert_eq!(output["invariant"], "Capture", "{output:#}");
                assert_eq!(output["violated_at_step"], 1, "{output:#}");
                assert_eq!(
                    output["trace"]
                        .as_array()
                        .expect("trace")
                        .last()
                        .expect("last state")["state"],
                    json!({"r": [], "q": [], "done": true}),
                    "{output:#}"
                );
            }
        } else if *consumer == "replay" {
            assert_eq!(output["steps_checked"], 1, "{output:#}");
            assert_eq!(
                output["final_state"],
                json!({"r": [], "q": [[2, 2]], "done": true}),
                "{output:#}"
            );
        } else {
            assert_eq!(
                output["completeness"],
                if *consumer == "bmc" {
                    "bounded"
                } else {
                    "unbounded"
                },
                "{output:#}"
            );
        }
    }
}

#[test]
fn equal_bound_alias_conditional_proves_on_all_engines_and_replay() {
    assert_equal_alias_verdicts("add", false);
}

#[test]
fn missing_pair_is_violated_on_all_engines_and_replay() {
    assert_equal_alias_verdicts("remove", true);
}

fn assert_narrow_rejected(
    label: &str,
    init: &str,
    action: &str,
    predicate: &str,
    location: &Value,
) {
    let source = NARROW_TEMPLATE
        .replace("$INIT", init)
        .replace("$ACTION", action)
        .replace("$P", predicate);
    assert_source_rejected(
        label,
        &source,
        location,
        r#"Relation(Named("N"), Named("N"))"#,
        r#"Relation(Named("N"), Named("M"))"#,
    );
}

#[test]
fn action_assignment_rejects_narrow_to_wide_relation() {
    assert_narrow_rejected(
        "act_pos",
        "r = Set {} q = Set {} done = false",
        "r = r.add(1, 2) q = r.add(1, 2)",
        "not done or q.contains(1, 2)",
        &json!({"line": 6, "column": 51}),
    );
}

#[test]
fn init_rejects_narrow_to_wide_relation() {
    assert_narrow_rejected(
        "init_pos",
        "r = Set {} q = r.add(1, 2) done = false",
        "r = r",
        "q.contains(1, 2)",
        &json!({"line": 5, "column": 21}),
    );
}

#[test]
fn let_assignment_rejects_narrow_to_wide_relation() {
    assert_narrow_rejected(
        "let_pos",
        "r = Set {} q = Set {} done = false",
        "let x = r.add(1, 2) q = x",
        "not done or q.contains(1, 2)",
        &json!({"line": 6, "column": 55}),
    );
}

#[test]
fn numeric_set_narrow_to_wide_assignment_still_checks() {
    assert_check_ok("set-widening", SET_WIDENING);
}

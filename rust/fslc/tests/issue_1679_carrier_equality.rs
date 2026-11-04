// SPDX-License-Identifier: Apache-2.0

//! #1679: a whole-container comparison, conditional or assignment whose carriers
//! the symbolic engines cannot coerce used to pass `check` and fail later with
//! `cannot coerce symbolic value`, while `fsl-bfs` answered. `check` now refuses
//! them. The controls below keep the forms that return a verdict on every engine.

#[path = "support/issue_1517_scratch.rs"]
mod scratch;

use serde_json::{Value, json};
use std::{fs, process::Command};

const TYPES: &str = "type K = 0..1 type A = 0..1 type A2 = 0..1 type B = 0..2";

fn map_spec(left: &str, right: &str, action: &str, property: &str) -> String {
    format!(
        "spec S {{ {TYPES}\n  state {{ l: Map<K, {left}>, r: Map<K, {right}>, c: Bool }}\n  init {{ forall k: K {{ l[k] = {zero} r[k] = {zero} }} c = false }}\n  action go() {{\n    {action}\n    c = not c\n  }}\n  invariant P {{ {property} }}\n}}\n",
        zero = if left.starts_with("Option") {
            "none"
        } else {
            "0"
        },
    )
}

/// The issue's first shape in every position. A guard reports the action at 4:3,
/// a statement itself, and a conditional the arm at fault.
const MAP_FORMS: [(&str, &str, &str, u32, u32); 8] = [
    ("eq", "requires l == r", "true", 4, 3),
    ("eq_reversed", "requires r == l", "true", 4, 3),
    ("ne", "requires l != r", "true", 4, 3),
    ("property", "", "l == r", 8, 3),
    ("arm", "", "l == (if c then r else l)", 8, 40),
    ("assign", "l = r", "true", 5, 5),
    ("assign_reversed", "r = l", "true", 5, 5),
    ("assign_arm", "l = if c then l else r", "true", 5, 26),
];

fn fslc(id: &str, source: &str, arguments: &[&str]) -> (Value, i32) {
    fslc_with(id, source, &[], arguments)
}

fn fslc_with(id: &str, source: &str, others: &[(&str, &str)], arguments: &[&str]) -> (Value, i32) {
    let scratch = scratch::Scratch::new(
        "FSLC_1679_INPUTS",
        id,
        &format!("1679-{id}-{}", arguments[0]),
    );
    fs::write(scratch.path().join("p.fsl"), source).expect("fixture");
    for (name, other) in others {
        fs::write(scratch.path().join(name), other).expect("fixture");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .arg("p.fsl")
        .current_dir(scratch.path())
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .expect("CLI");
    assert!(
        output.stderr.is_empty(),
        "{id}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        serde_json::from_slice(&output.stdout).expect("JSON packet"),
        output.status.code().expect("exit"),
    )
}

fn check(id: &str, source: &str) -> (Value, i32) {
    fslc(id, source, &["check"])
}

fn versions() -> Value {
    fsl_core::version_metadata(
        "fslc-rust",
        env!("CARGO_PKG_VERSION"),
        "native-z3",
        fsl_solver_z3::version(),
    )
}

fn assert_rejected(id: &str, source: &str, reason: &str, line: u32, column: u32) {
    assert_eq!(
        check(id, source),
        (
            json!({"fsl":"1.0","result":"error","kind":"semantics",
                "message":format!("invalid model expression: {reason} at {line}:{column}"),
                "loc":{"line":line,"column":column},"versions":versions()}),
            2
        ),
        "{id}\n{source}"
    );
}

fn not_assignable(actual: &str, expected: &str) -> String {
    format!("expression of type {actual} is not assignable to {expected}")
}

#[test]
fn map_int_value_against_a_range_value_is_refused_in_every_position() {
    for (id, action, property, line, column) in MAP_FORMS {
        let source = map_spec("Int", "A", action, property);
        let (packet, exit) = check(id, &source);
        assert_eq!(exit, 2, "{id}: {packet}");
        assert_eq!(packet["kind"], "semantics", "{id}: {packet}");
        assert_eq!(
            packet["loc"],
            json!({"line":line,"column":column}),
            "{id}: {packet}"
        );
        let message = packet["message"].as_str().expect("message");
        assert!(
            message.contains("Map(Named(\"K\"), Int)")
                && message.contains("Map(Named(\"K\"), Named(\"A\"))")
                && message.contains("is not assignable to"),
            "{id}: {message}"
        );
    }
}

#[test]
fn map_with_an_int_option_value_is_refused() {
    let source = map_spec("Option<Int>", "Option<A>", "", "l == r");
    assert_rejected(
        "map_option",
        &source,
        &not_assignable(
            "Map(Named(\"K\"), Option(Named(\"A\")))",
            "Map(Named(\"K\"), Option(Int))",
        ),
        8,
        3,
    );
}

fn arm_spec(state: &str, init: &str, action: &str) -> String {
    format!(
        "spec S {{ {TYPES}\n  state {{ {state}, c: Bool }}\n  init {{ {init} c = false }}\n  action go() {{\n{action}\n    c = not c\n  }}\n}}\n"
    )
}

const SETS: (&str, &str) = ("a: Set<A>, b: Set<B>", "a = Set {} b = Set {}");
const SEQS: (&str, &str) = (
    "q: Seq<Int, 2>, a: Seq<A, 2>, b: Seq<B, 2>, d: Seq<A, 3>",
    "q = Seq {} a = Seq {} b = Seq {} d = Seq {}",
);

fn set_b_to_a() -> String {
    not_assignable("Set(Named(\"B\"))", "Set(Named(\"A\"))")
}

#[test]
fn set_arm_is_refused_when_only_an_int_sibling_related_it_to_the_expectation() {
    for (id, guard, column) in [
        ("set_eq", "requires a == (if c then t else b)", 37),
        ("set_ne", "requires a != (if c then t else b)", 37),
        (
            "set_nested",
            "requires a == (if c then t else if c then t else b)",
            54,
        ),
        (
            "set_nested_then",
            "requires a == (if c then (if c then t else b) else a)",
            48,
        ),
    ] {
        let source = arm_spec(
            SETS.0,
            SETS.1,
            &format!("    let t = Set {{ 0 }}\n    {guard}"),
        );
        assert_rejected(id, &source, &set_b_to_a(), 6, column);
    }
}

#[test]
fn seq_arm_is_refused_when_only_an_int_sibling_related_it_to_the_expectation() {
    let b_to_a = not_assignable("Seq(Named(\"B\"), 2)", "Seq(Named(\"A\"), 2)");
    let d_to_a = not_assignable("Seq(Named(\"A\"), 3)", "Seq(Named(\"A\"), 2)");
    for (id, action, reason, line, column) in [
        (
            "seq_literal",
            "    let t = Seq { 0 }\n    requires a == (if c then t else b)",
            &b_to_a,
            6,
            37,
        ),
        (
            "seq_capacity",
            "    let t = Seq { 0 }\n    requires a == (if c then t else d)",
            &d_to_a,
            6,
            37,
        ),
        (
            "seq_declared_int",
            "    requires a == (if c then q else b)",
            &b_to_a,
            5,
            37,
        ),
        (
            "seq_declared_int_capacity",
            "    requires a == (if c then q else d)",
            &d_to_a,
            5,
            37,
        ),
    ] {
        let source = arm_spec(SEQS.0, SEQS.1, action);
        assert_rejected(id, &source, reason, line, column);
    }
}

/// Composite literals carry a Map or an arm into the same coercion.
#[test]
fn composite_literal_carrying_the_same_mismatch_is_refused() {
    for (id, guard) in [
        ("some_map", "some(m0) == some(m1)"),
        ("some_map_ne", "some(m0) != some(m1)"),
        ("some_map_reversed", "some(m1) == some(m0)"),
        ("seq_map", "Seq { m0 } == Seq { m1 }"),
        ("set_map", "Set { m0 } == Set { m1 }"),
        ("some_map_arm", "some(m0) == some(if c then m1 else m0)"),
        (
            "some_map_arm_second",
            "some(m0) == some(if c then m0 else m1)",
        ),
        ("some_arm", "some(m0) == (if c then some(m1) else some(m0))"),
        ("contains", "Seq { m0 }.contains(m1)"),
        ("head", "Seq { m0 }.head() == m1"),
        ("at", "Seq { m0 }.at(0) == m1"),
        ("some_set_arm", "some(a) == some(if c then t else b)"),
        ("seq_set_arm", "Seq { a } == Seq { if c then t else b }"),
        ("set_set_arm", "Set { a } == Set { if c then t else b }"),
        ("contains_set_arm", "Seq { a }.contains(if c then t else b)"),
        ("option_let_arm", "some(a) == (if c then u else v)"),
    ] {
        let source = format!(
            "spec S {{ {TYPES}\n  state {{ m0: Map<K, Int>, m1: Map<K, A>, a: Set<A>, b: Set<B>, c: Bool }}\n  init {{ forall k: K {{ m0[k] = 0 m1[k] = 0 }} a = Set {{}} b = Set {{}} c = false }}\n  action go() {{\n    let t = Set {{ 0 }}\n    let u = some(t)\n    let v = some(b)\n    requires {guard}\n    c = not c\n  }}\n}}\n"
        );
        let (packet, exit) = check(id, &source);
        assert_eq!(
            (exit, &packet["result"], &packet["kind"]),
            (2, &json!("error"), &json!("semantics")),
            "{id}: {packet}"
        );
        assert!(
            packet["message"]
                .as_str()
                .is_some_and(|message| message.contains("is not assignable to")),
            "{id}: {packet}"
        );
    }
}

#[test]
fn every_engine_reports_the_same_refusal() {
    let source = map_spec("Int", "A", "", "l == r");
    for engine in ["bmc", "explicit", "induction"] {
        let (packet, exit) = fslc(
            engine,
            &source,
            &["verify", "--engine", engine, "--depth", "2"],
        );
        assert_eq!(
            (exit, &packet["result"], &packet["kind"]),
            (2, &json!("error"), &json!("semantics")),
            "{engine}: {packet}"
        );
        let message = packet["message"].as_str().expect("message");
        assert!(!message.contains("cannot coerce"), "{engine}: {message}");
    }
}

/// `(violation expected, spec)`: each must check and return that verdict on bmc,
/// explicit and BFS, and a verdict rather than an error on induction.
fn preserved_forms() -> Vec<(&'static str, bool, String)> {
    let with = |state: (&str, &str), action: &str, property: &str| {
        format!(
            "spec S {{ {TYPES}\n  state {{ {}, c: Bool }}\n  init {{ {} c = false }}\n  action go() {{\n    {action}\n    c = not c\n  }}\n  invariant P {{ {property} }}\n}}\n",
            state.0, state.1
        )
    };
    let options = (
        "o: Option<Int>, p: Option<A>, r: Option<B>",
        "o = none p = none r = none",
    );
    let maps = (
        "m: Map<K, A>, n: Map<K, A2>, i: Map<K, Int>, j: Map<K, Int>",
        "forall k: K { m[k] = 0 n[k] = 0 i[k] = 0 j[k] = 0 }",
    );
    vec![
        (
            "set_literal",
            false,
            with(
                SETS,
                "requires a == Set { 0 } or true",
                "a == Set {} or a != Set {}",
            ),
        ),
        ("set_literal_reversed", false, with(SETS, "", "Set {} == a")),
        (
            "set_literal_refutes",
            true,
            with(SETS, "", "a == Set { 0 }"),
        ),
        (
            "set_let",
            false,
            with(
                SETS,
                "let t = Set { 0 }\n    requires a == t or true",
                "true",
            ),
        ),
        (
            "set_let_reversed",
            false,
            with(
                SETS,
                "let t = Set { 0 }\n    requires t == b or true",
                "true",
            ),
        ),
        (
            "set_let_arm_same_carrier",
            false,
            with(
                SETS,
                "let t = Set { 0 }\n    requires a == (if c then t else a) or true",
                "true",
            ),
        ),
        (
            "set_let_arm_same_carrier_swapped",
            false,
            with(
                SETS,
                "let t = Set { 0 }\n    requires a == (if c then a else t) or true",
                "true",
            ),
        ),
        ("seq_declared_int", false, with(SEQS, "", "q == a")),
        ("seq_declared_int_reversed", false, with(SEQS, "", "a == q")),
        ("seq_declared_int_capacity", false, with(SEQS, "", "q == d")),
        ("seq_declared_int_refutes", true, with(SEQS, "", "q != a")),
        (
            "seq_arm_same_carrier",
            false,
            with(SEQS, "", "a == (if c then q else a)"),
        ),
        (
            "seq_expectation_int_same_arms",
            false,
            with(SEQS, "", "q == (if c then a else a)"),
        ),
        ("option_int_range", false, with(options, "", "o == p")),
        ("option_range_range", false, with(options, "", "p == r")),
        ("option_refutes", true, with(options, "", "p != r")),
        ("map_equal_alias", false, with(maps, "", "m == n")),
        (
            "map_equal_alias_arm",
            false,
            with(maps, "", "m == (if c then n else m)"),
        ),
        ("map_equal_int", false, with(maps, "", "i == j")),
        ("map_equal_refutes", true, with(maps, "", "i != j")),
        (
            "some_set_let",
            false,
            with(
                SETS,
                "let t = Set { 0 }\n    requires some(t) == some(a) or true",
                "true",
            ),
        ),
        (
            "seq_set_let",
            false,
            with(
                SETS,
                "let t = Set { 0 }\n    requires Seq { t } == Seq { a } or true",
                "true",
            ),
        ),
        (
            "some_seq_declared_int",
            false,
            with(SEQS, "requires some(q) == some(a) or true", "true"),
        ),
        (
            "set_seq_declared_int",
            false,
            with(SEQS, "requires Set { q } == Set { a } or true", "true"),
        ),
    ]
}

#[test]
fn forms_with_a_verdict_on_every_engine_keep_checking_and_agree() {
    for (id, violates, source) in preserved_forms() {
        let (packet, exit) = check(id, &source);
        assert_eq!(exit, 0, "{id}: {packet}\n{source}");
        let want = i32::from(violates);
        for engine in ["bmc", "explicit"] {
            let (packet, exit) = fslc(id, &source, &["verify", "--engine", engine, "--depth", "2"]);
            assert_eq!(exit, want, "{id} {engine}: {packet}");
        }
        let (packet, exit) = fslc(id, &source, &["verify", "--engine", "induction"]);
        assert_ne!(exit, 2, "{id} induction: {packet}");
        let model = fsl_core::build_model(
            fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new("."))
                .expect("lowers"),
        )
        .expect("model");
        let bfs = fsl_runtime::bfs(model, 2).expect("bfs");
        assert_eq!(bfs.violation.is_some(), violates, "{id} bfs");
    }
}

/// A refinement map and a scenario expectation are evaluated concretely. These
/// forms return a verdict on every engine and must keep it: they still check
/// `Int` against a range through the assignable rule.
#[test]
fn concretely_evaluated_positions_keep_their_verdicts() {
    let whole_abs = "spec Ab { type K = 0..1 state { m: Map<K, Int> } init { forall k: K { m[k] = 0 } } action go() { m[0] = 0 } }\n";
    let same_abs =
        "spec Ab { state { same: Bool } init { same = true } action go() { same = true } }\n";
    let seq_abs = "spec Ab { type A = 0..1 state { s: Seq<A, 2> } init { s = Seq {} } action go() { s = s } }\n";
    let implementing = |state: &str, init: &str, map: &str| {
        format!(
            "requirements R {{\n  type K = 0..1\n  type A = 0..1\n  type B = 0..2\n  state {{ {state}, c: Bool }}\n  init {{ {init} c = false }}\n  implements Ab from \"abs.fsl\" {{\n    map {map}\n    action go() -> go()\n  }}\n  requirement REQ-1 \"go\" {{\n    action go() {{ c = c }}\n  }}\n}}\n"
        )
    };
    let maps = (
        "m: Map<K, A>, y: Map<K, Int>",
        "forall k: K { m[k] = 0 y[k] = 0 }",
    );
    let seqs = ("x: Seq<Int, 2>, y: Seq<B, 2>", "x = Seq {} y = Seq {}");
    let forms = [
        (
            "whole_map",
            implementing(maps.0, maps.1, "m = m"),
            whole_abs,
        ),
        (
            "map_compares",
            implementing(maps.0, maps.1, "same = m == y"),
            same_abs,
        ),
        (
            "map_arm",
            implementing(seqs.0, seqs.1, "s = if c then x else y"),
            seq_abs,
        ),
        (
            "expectation",
            format!(
                "requirements R {{\n  type K = 0..1\n  type A = 0..1\n  state {{ l: Map<K, Int>, r: Map<K, A>, c: Bool }}\n  init {{ forall k: K {{ l[k] = 0 r[k] = 0 }} c = false }}\n  requirement REQ-1 \"go\" {{\n    action go() {{ c = not c }}\n  }}\n  acceptance AC-1 \"same\" {{\n    go()\n    expect l == r\n  }}\n}}\n"
            ),
            same_abs,
        ),
    ];
    for (id, source, abs) in &forms {
        let others = [("abs.fsl", *abs)];
        let (packet, exit) = fslc_with(id, source, &others, &["check"]);
        assert_eq!(exit, 0, "{id}: {packet}\n{source}");
        for engine in ["bmc", "explicit", "induction"] {
            let (packet, exit) = fslc_with(
                id,
                source,
                &others,
                &["verify", "--engine", engine, "--depth", "2"],
            );
            assert_eq!(exit, 0, "{id} {engine}: {packet}");
        }
    }
}

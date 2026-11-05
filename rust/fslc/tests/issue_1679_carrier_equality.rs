// SPDX-License-Identifier: Apache-2.0

//! #1679: a whole-container comparison, conditional or assignment whose carriers
//! the symbolic engines cannot coerce used to pass `check` and fail later with
//! `cannot coerce symbolic value`, while `fsl-bfs` answered. `check` now refuses
//! them, and a collection holding `Int` below an element that no literal expectation
//! can type now coerces. The controls below keep the other forms, and pin the two
//! routes that still report it. A third, a literal under `old`, closed in #1683.

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
#[allow(clippy::too_many_lines)] // One literal row per preserved form.
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

/// A refinement map and a scenario expectation keep the assignable rule, so these
/// forms still check `Int` against a range and return a verdict on every engine.
/// The negations decide, so these positions are evaluated rather than skipped.
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
            "requirements R {\n  type K = 0..1\n  type A = 0..1\n  state { l: Map<K, Int>, r: Map<K, A>, c: Bool }\n  init { forall k: K { l[k] = 0 r[k] = 0 } c = false }\n  requirement REQ-1 \"go\" {\n    action go() { c = not c }\n  }\n  acceptance AC-1 \"same\" {\n    go()\n    expect l == r\n  }\n}\n"
                .to_owned(),
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
    let map_negation = implementing(maps.0, maps.1, "same = m != y");
    for engine in ["bmc", "explicit", "induction"] {
        let arguments = ["verify", "--engine", engine, "--depth", "2"];
        let others = [("abs.fsl", same_abs)];
        let (packet, exit) = fslc_with("map_negation", &map_negation, &others, &arguments);
        assert_eq!(
            (exit, &packet["result"]),
            (1, &json!("refinement_failed")),
            "{engine}: {packet}"
        );
    }
    let expectation_negation = forms[3].1.replace("expect l == r", "expect l != r");
    assert_ne!(expectation_negation, forms[3].1);
    let (packet, exit) = fslc("expectation_negation", &expectation_negation, &["check"]);
    assert_eq!(
        (exit, &packet["kind"]),
        (2, &json!("acceptance")),
        "{packet}"
    );
}

const REFINE_IMPL: &str = "spec Im { type K = 0..1 type A = 0..1\n  state { m: Map<K, A>, y: Map<K, Int>, c: Bool }\n  init { forall k: K { m[k] = 0 y[k] = 0 } c = false }\n  fair action go() { c = not c }\n}\n";
const REFINE_ABS: &str = "spec Ab { state { same: Bool } init { same = true }\n  fair action go() { same = same }\n  leadsTo L { same ~> same }\n}\n";

fn refine(id: &str, map: &str, progress: bool) -> (Value, i32) {
    refine_specs(id, REFINE_IMPL, REFINE_ABS, map, "", progress)
}

fn refine_specs(
    id: &str,
    im: &str,
    ab: &str,
    map: &str,
    actions: &str,
    progress: bool,
) -> (Value, i32) {
    let progress = if progress {
        "  preserve progress {\n    respond L by go\n  }\n"
    } else {
        ""
    };
    let mapping = format!(
        "refinement R {{\n  impl Im\n  abs Ab\n  map same = {map}\n  action go() -> go()\n{actions}{progress}}}\n"
    );
    let scratch = scratch::Scratch::new("FSLC_1679_INPUTS", id, &format!("1679-{id}-refine"));
    for (name, source) in [
        ("im.fsl", im),
        ("ab.fsl", ab),
        ("map.fsl", mapping.as_str()),
    ] {
        fs::write(scratch.path().join(name), source).expect("fixture");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["refine", "im.fsl", "ab.fsl", "map.fsl", "--depth", "2"])
        .current_dir(scratch.path())
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_UNSTABLE")
        .output()
        .expect("CLI");
    (
        serde_json::from_slice(&output.stdout).expect("JSON packet"),
        output.status.code().expect("exit"),
    )
}

/// The refinement step is concrete and decides either way. `preserve progress`
/// evaluates the same map symbolically, which is the recorded gap in this rule.
#[test]
fn refinement_map_comparison_decides_and_preserved_progress_is_the_recorded_gap() {
    let (packet, exit) = refine("refine_same", "m == y", false);
    assert_eq!(
        (exit, &packet["result"]),
        (0, &json!("refines")),
        "{packet}"
    );
    let (packet, exit) = refine("refine_differs", "m != y", false);
    assert_eq!(
        (exit, &packet["result"]),
        (1, &json!("refinement_failed")),
        "{packet}"
    );
    let (packet, exit) = refine("refine_progress", "m == y", true);
    assert_eq!(
        (exit, &packet["kind"]),
        (2, &json!("semantics")),
        "{packet}"
    );
    assert!(
        packet["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("cannot coerce symbolic value")),
        "{packet}"
    );
}

const WIDE_STATE: &str = "m0: Map<K, Int>, m1: Map<K, A>, a: Set<A>, q: Seq<Int, 2>, sa: Seq<A, 2>, sb: Seq<B, 2>, o: Option<Int>, p: Option<A>, r: Option<B>, n: Int, va: A, vb: B, c: Bool, done: Bool";

/// Line 1 declares types, 2 state, 3 init, 4 opens `go`, and `body` starts at 5.
fn wide_spec(init: &[(&str, &str)], body: &str, properties: &str) -> String {
    let defaults = [
        ("a", "Set {}"),
        ("q", "Seq {}"),
        ("sa", "Seq {}"),
        ("sb", "Seq {}"),
        ("o", "none"),
        ("p", "none"),
        ("r", "none"),
        ("n", "0"),
        ("va", "0"),
        ("vb", "0"),
        ("c", "false"),
        ("done", "false"),
    ];
    let init = defaults
        .iter()
        .map(|(name, value)| {
            let value = init
                .iter()
                .find(|(chosen, _)| chosen == name)
                .map_or(*value, |(_, chosen)| *chosen);
            format!("{name} = {value}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "spec S {{ {TYPES} type E = 1..0\n state {{ {WIDE_STATE} }}\n init {{ forall k: K {{ m0[k] = 0 m1[k] = 0 }} {init} }}\n action go() {{\n {body}\n }}\n {properties}\n}}\n"
    )
}

/// `(violation, bmc and explicit and induction verdicts, BFS)` on one spec.
fn violations(id: &str, source: &str) -> (bool, bool, bool, bool) {
    let verdict = |engine: &str| {
        let (packet, exit) = fslc(id, source, &["verify", "--engine", engine, "--depth", "2"]);
        assert!(exit < 2, "{id} {engine}: {packet}\n{source}");
        packet["result"] == "violated"
    };
    let model = fsl_core::build_model(
        fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new(".")).expect("lowers"),
    )
    .expect("model");
    (
        verdict("bmc"),
        verdict("explicit"),
        verdict("induction"),
        fsl_runtime::bfs(model, 2).expect("bfs").violation.is_some(),
    )
}

/// `(id, let, guard, state to match, value that matches)`: a let-bound collection
/// holding `Int` below an element, both directions, in Set and Seq and through
/// Option, Set and Seq elements. It used to pass `check` and report `cannot coerce`.
const LET_BOUND: [(&str, &str, &str, &str, &str); 8] = [
    (
        "set_of_some",
        "Set { some(0) }",
        "Set { p } == t",
        "p",
        "some(0)",
    ),
    (
        "seq_of_some",
        "Seq { some(0) }",
        "Seq { p } == t",
        "p",
        "some(0)",
    ),
    (
        "set_of_seq_capacity",
        "Set { Seq { 0 } }",
        "Set { q } == t",
        "q",
        "Seq { 0 }",
    ),
    (
        "seq_of_seq",
        "Seq { Seq { 0 } }",
        "Seq { sa } == t",
        "sa",
        "Seq { 0 }",
    ),
    (
        "set_of_set",
        "Set { Set { 0 } }",
        "Set { a } == t",
        "a",
        "Set { 0 }",
    ),
    (
        "seq_of_set",
        "Seq { Set { 0 } }",
        "Seq { a } == t",
        "a",
        "Set { 0 }",
    ),
    (
        "set_range_to_int",
        "Set { p }",
        "Set { o } == t",
        "o",
        "some(0)",
    ),
    (
        "seq_range_to_int",
        "Seq { p }",
        "Seq { o } == t",
        "o",
        "some(0)",
    ),
];

#[test]
fn let_bound_nested_int_collection_decides_on_every_engine() {
    for (id, bound, guard, state, matching) in LET_BOUND {
        let body = format!("let t = {bound}\n requires {guard}\n done = true");
        let property = "action idle() { c = c }\n invariant P { not done }";
        let init = if bound.contains('p') {
            vec![("p", "some(0)"), (state, matching)]
        } else {
            vec![(state, matching)]
        };
        let source = wide_spec(&init, &body, property);
        assert_eq!(
            violations(&format!("{id}_matches"), &source),
            (true, true, true, true),
            "{id}\n{source}"
        );
        let differing = if bound.contains('p') {
            vec![("p", "some(1)"), (state, matching)]
        } else {
            vec![]
        };
        let source = wide_spec(&differing, &body, property);
        assert_eq!(
            {
                let (bmc, explicit, _, bfs) = violations(&format!("{id}_differs"), &source);
                (bmc, explicit, bfs)
            },
            (false, false, false),
            "{id}\n{source}"
        );
    }
    // A literal outside the carrier never equals a value inside it.
    let source = wide_spec(
        &[("p", "some(0)")],
        "let t = Set { some(5) }\n requires Set { p } == t\n done = true",
        "action idle() { c = c }\n invariant P { not done }",
    );
    assert_eq!(
        violations("out_of_carrier", &source),
        (false, false, false, false)
    );
}

#[test]
fn old_operand_arm_answers_to_what_surrounds_it() {
    let arm = not_assignable("Seq(Named(\"B\"), 2)", "Seq(Named(\"A\"), 2)");
    let trans = wide_spec(
        &[],
        "c = not c",
        "trans T { sa == old(if c then q else sb) or true }",
    );
    assert_rejected("old_arm_trans", &trans, &arm, 7, 39);
    let ensures = wide_spec(
        &[],
        "ensures sa == old(if c then q else sb) or true\n c = not c",
        "",
    );
    assert_rejected("old_arm_ensures", &ensures, &arm, 5, 37);
    // Compatible, and the expectation does not type the operand's literals or scalars.
    for (id, property) in [
        (
            "old_int_arm",
            "trans T { sa == old(if c then q else sa) or true }",
        ),
        ("old_literal", "trans T { sa == old(Seq { 5 }) or true }"),
        (
            "old_option_arm",
            "trans T { p == old(if c then o else r) or true }",
        ),
        (
            "old_scalar_arm",
            "trans T { va == old(if c then n else vb) or true }",
        ),
        (
            "old_scalar_literal",
            "trans T { va == old(if c then 5 else 0) or true }",
        ),
    ] {
        let source = wide_spec(&[], "c = not c", property);
        assert_eq!(
            violations(id, &source),
            (false, false, false, false),
            "{id}\n{source}"
        );
    }
}

/// Decided by the user: a mismatch in a position that is never evaluated stays
/// refused, although each of these returned a verdict on every engine on base.
#[test]
#[allow(clippy::too_many_lines)] // One literal row per form.
fn never_evaluated_mismatch_stays_refused() {
    let map = not_assignable("Map(Named(\"K\"), Named(\"A\"))", "Map(Named(\"K\"), Int)");
    let good = "c = not c";
    for (id, body, property, flags, line, column) in [
        (
            "forall_empty",
            good,
            "invariant P { forall i: E { m0 == m1 } }",
            &[][..],
            7,
            2,
        ),
        (
            "exists_empty",
            good,
            "invariant P { not (exists i: E { m0 == m1 }) }",
            &[][..],
            7,
            2,
        ),
        (
            "forall_guard_empty",
            "requires forall i: E { m0 == m1 }\n c = not c",
            "",
            &[][..],
            4,
            2,
        ),
        (
            "param_empty",
            good,
            "action h(i: E) { requires m0 == m1\n c = not c }",
            &[][..],
            7,
            2,
        ),
        (
            "param_empty_assign",
            good,
            "action h(i: E) { m0 = m1 }",
            &[][..],
            7,
            19,
        ),
        (
            "exclude_invariant",
            good,
            "invariant Bad { m0 == m1 }\n invariant Q { true }",
            &["--exclude-property", "Bad"][..],
            7,
            2,
        ),
        (
            "exclude_reachable",
            good,
            "reachable Bad { m0 == m1 }\n invariant Q { true }",
            &["--exclude-property", "Bad"][..],
            7,
            2,
        ),
        (
            "exclude_trans",
            good,
            "trans Bad { old(m0) == m1 }\n invariant Q { true }",
            &["--exclude-property", "Bad"][..],
            7,
            2,
        ),
        (
            "select_other",
            good,
            "invariant Bad { m0 == m1 }\n invariant Q { true }",
            &["--property", "Q"][..],
            7,
            2,
        ),
        // bmc does not read a measure and this one's violation stops induction first.
        (
            "decreases_unread",
            good,
            "leadsTo L { n < 3 ~> n == 3 decreases (if m0 == m1 then 1 else 0) }",
            &[][..],
            7,
            44,
        ),
    ] {
        let source = wide_spec(&[], body, property);
        assert_rejected(id, &source, &map, line, column);
        let mut arguments = vec!["verify", "--engine", "bmc", "--depth", "2"];
        arguments.extend_from_slice(flags);
        let (packet, exit) = fslc(id, &source, &arguments);
        assert_eq!(
            (exit, &packet["kind"]),
            (2, &json!("semantics")),
            "{id}: {packet}"
        );
    }
    let arm = wide_spec(
        &[],
        good,
        "invariant P { forall i: E { sa == (if c then q else sb) } }",
    );
    let (packet, exit) = check("forall_empty_arm", &arm);
    assert_eq!(exit, 2, "{packet}");
}

const LEMMA_SPEC: &str = "spec D { type K = 0..1 type A = 0..1 type E = 1..0\n  state { x: 0..1, y: 0..1, m0: Map<K, Int>, m1: Map<K, A>, q: Seq<Int, 2>, sa: Seq<A, 2>, sb: Seq<A, 3> }\n  init { x = 0 y = 0 forall k: K { m0[k] = 0 m1[k] = 0 } q = Seq {} sa = Seq {} sb = Seq {} }\n  action go() { x = y y = x }\n  invariant P { x == 0 }\n}\n";
const NEEDED: &str = "x == 0 and y == 0";

/// A candidate keeps the assignable rule. The main property needs `NEEDED`, so these
/// show a mismatching candidate is rejected and unused, and a mismatch it never
/// evaluates leaves a candidate usable, which an invariant's check would have changed.
#[test]
fn lemma_candidate_keeps_the_assignable_rule_and_the_main_result() {
    let cases = [
        (
            "alone",
            "m0 == m1".to_owned(),
            "unknown_cti",
            "rejected",
            false,
        ),
        (
            "mismatch",
            format!("{NEEDED} and m0 == m1"),
            "unknown_cti",
            "rejected",
            false,
        ),
        ("needed", NEEDED.to_owned(), "proved", "proved", true),
        (
            "int_seq",
            format!("{NEEDED} and (q == sa or true)"),
            "proved",
            "proved",
            true,
        ),
        (
            "never_evaluated",
            format!("{NEEDED} and forall i: E {{ m0 == m1 }}"),
            "proved",
            "proved",
            true,
        ),
    ];
    for (id, lemma, main, status, used) in cases {
        let arguments = ["verify", "--engine", "induction", "--lemma", lemma.as_str()];
        let (packet, _) = fslc(&format!("lemma_{id}"), LEMMA_SPEC, &arguments);
        let entry = packet["lemmas"][0].clone();
        assert_eq!(
            (&packet["result"], &entry["status"], &entry["used"]),
            (&json!(main), &json!(status), &json!(used)),
            "{id}: {packet}"
        );
        if status == "rejected" {
            assert!(
                entry["proof"]["message"]
                    .as_str()
                    .is_some_and(|message| message.starts_with("cannot coerce symbolic value")),
                "{id}: {packet}"
            );
        }
    }
}

/// A value no rule converts stays an error: the rule does not extend to a sibling
/// that only an `Int` first arm related to the expectation.
#[test]
fn lemma_candidate_with_unrelated_carriers_is_not_coerced() {
    let spec = "spec L { type A = 0..1 type B = 0..2\n state { q: Seq<Int, 2>, sa: Seq<A, 2>, sb: Seq<B, 2>, c: Bool }\n init { q = Seq {} sa = Seq {} sb = Seq {} c = false }\n action go() { c = not c }\n invariant P { c or not c }\n}\n";
    let lemma = "sa == (if c then q else sb)";
    let (packet, _) = fslc(
        "lemma_arm",
        spec,
        &["verify", "--engine", "induction", "--lemma", lemma],
    );
    assert_eq!(
        packet["lemmas"][0]["proof"]["message"],
        "cannot coerce symbolic value Some(Seq(Named(\"B\"), 2)) to Seq(Named(\"A\"), 2)",
        "{packet}"
    );
}

/// `[bmc, explicit, induction, fsl-bfs]`, the last naming the violation it found.
fn results(id: &str, source: &str) -> [String; 4] {
    let verdict = |engine: &str| {
        let (packet, _) = fslc(id, source, &["verify", "--engine", engine, "--depth", "2"]);
        packet["result"].as_str().expect("result").to_owned()
    };
    let model = fsl_core::build_model(
        fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new(".")).expect("lowers"),
    )
    .expect("model");
    [
        verdict("bmc"),
        verdict("explicit"),
        verdict("induction"),
        fsl_runtime::bfs(model, 2)
            .expect("bfs")
            .violation
            .map_or_else(|| "none".to_owned(), |found| found.name),
    ]
}

const IDLE: &str = "action idle() { c = c }\n ";

/// `(id, let, guard, state where it holds, state where it does not)`: a value that
/// carries `Int` below an element with no `let` at the comparison, as a method's result
/// or through an `is` binding, and a bound one as a method's argument.
type RebuiltGuard = (
    &'static str,
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static [(&'static str, &'static str)],
);
const REBUILT_GUARDS: [RebuiltGuard; 11] = [
    (
        "result_add",
        "",
        "Set { p, none } == Set { o }.add(none)",
        &[("p", "some(1)"), ("o", "some(1)")],
        &[("p", "some(0)"), ("o", "some(1)")],
    ),
    (
        "result_remove",
        "",
        "Set { p }.remove(none) == Set { o }.remove(none)",
        &[("p", "none"), ("o", "none")],
        &[("p", "some(0)"), ("o", "some(1)")],
    ),
    (
        "result_push",
        "",
        "Seq { p, p } == Seq { o }.push(some(1))",
        &[("p", "some(1)"), ("o", "some(1)")],
        &[("p", "some(0)"), ("o", "some(1)")],
    ),
    (
        "result_head",
        "",
        "Seq { sa } == Seq { Seq { q } }.head()",
        &[("sa", "Seq { 1 }"), ("q", "Seq { 1 }")],
        &[("sa", "Seq { 0 }"), ("q", "Seq { 1 }")],
    ),
    (
        "is_binding",
        "",
        "some(Set { some(1) }) is some(x) and Set { p } == x",
        &[("p", "some(1)")],
        &[("p", "some(0)")],
    ),
    (
        "is_binding_argument",
        "",
        "some(Set { some(0) }) is some(t) and Set { Set { p } }.contains(t)",
        &[("p", "some(0)")],
        &[("p", "some(1)")],
    ),
    (
        "argument_set_contains",
        "let t = Set { some(0) }\n ",
        "Set { Set { p } }.contains(t)",
        &[("p", "some(0)")],
        &[("p", "some(1)")],
    ),
    (
        "argument_seq_contains",
        "let t = Seq { some(0) }\n ",
        "Seq { Seq { p } }.contains(t)",
        &[("p", "some(0)")],
        &[("p", "some(1)")],
    ),
    (
        "argument_add",
        "let t = Set { some(0) }\n ",
        "Set { Set { p } }.add(t).size() == 1",
        &[("p", "some(0)")],
        &[("p", "some(1)")],
    ),
    (
        "argument_remove",
        "let t = Set { some(0) }\n ",
        "Set { Set { p } }.remove(t).size() == 0",
        &[("p", "some(0)")],
        &[("p", "some(1)")],
    ),
    (
        "argument_push",
        "let t = Seq { some(1) }\n ",
        "Seq { Seq { p } }.push(t).at(1).at(0) == p",
        &[("p", "some(1)")],
        &[("p", "some(0)")],
    ),
];

#[test]
fn rebuilt_value_decides_in_a_guard_on_every_engine() {
    for (id, binding, guard, holds, differs) in REBUILT_GUARDS {
        let body = format!("{binding}requires {guard}\n done = true");
        let property = format!("{IDLE}invariant P {{ not done }}");
        let source = wide_spec(holds, &body, &property);
        assert_eq!(
            results(&format!("{id}_holds"), &source),
            ["violated", "violated", "violated", "P"],
            "{id}\n{source}"
        );
        let source = wide_spec(differs, &body, &property);
        assert_eq!(
            results(&format!("{id}_differs"), &source),
            ["verified", "proved", "unknown_cti", "none"],
            "{id}\n{source}"
        );
    }
}

const ADD: &str = "Set { p, none } == Set { o }.add(none)";

/// `(id, init, body, property, expected results)`.
type RebuiltRow = (
    &'static str,
    &'static [(&'static str, &'static str)],
    String,
    String,
    [&'static str; 4],
);

const DIFFERING: [(&str, &str); 2] = [("p", "some(0)"), ("o", "some(1)")];
const EQUAL: [(&str, &str); 2] = [("p", "some(1)"), ("o", "some(1)")];
const CHECKED_TRANS: &str =
    "trans X { old(done) or not done or Set { p, none } == Set { old(o) }.add(none) }";

#[allow(clippy::too_many_lines)] // One literal row per form.
fn rebuilt_rows() -> [RebuiltRow; 12] {
    let read = "p = (if c then Seq { p } else t).head()\n done = true";
    let copies = "p = o\n done = true";
    [
        (
            "read_holds",
            &[],
            format!("let t = Seq {{ some(1) }}\n {read}"),
            "invariant Q { not done or p == some(1) }".to_owned(),
            ["verified", "proved", "unknown_cti", "none"],
        ),
        (
            "read_fails",
            &[],
            format!("let t = Seq {{ some(1) }}\n {read}"),
            "invariant Q { not done or p == some(0) }".to_owned(),
            ["violated", "violated", "violated", "Q"],
        ),
        (
            "read_outside_carrier",
            &[],
            format!("let t = Seq {{ some(5) }}\n {read}"),
            "invariant T { true }".to_owned(),
            ["violated", "violated", "violated", "_bounds_p"],
        ),
        (
            "read_set_outside_carrier",
            &[],
            "let t = Seq { Set { 5 } }\n a = (if c then Seq { a } else t).at(0)\n done = true"
                .to_owned(),
            "invariant T { true }".to_owned(),
            ["violated", "violated", "violated", "_bounds_a"],
        ),
        (
            "read_over_capacity",
            &[],
            "let t = Seq { Seq { 0, 0, 0 } }\n sa = (if c then Seq { sa } else t).head()\n done = true"
                .to_owned(),
            "invariant T { true }".to_owned(),
            ["violated", "violated", "violated", "_bounds_sa"],
        ),
        (
            "invariant_holds",
            &DIFFERING,
            "done = true".to_owned(),
            format!("invariant Q {{ not ({ADD}) }}"),
            ["verified", "proved", "proved", "none"],
        ),
        (
            "invariant_fails",
            &EQUAL,
            "done = true".to_owned(),
            format!("invariant Q {{ not ({ADD}) }}"),
            ["violated", "violated", "violated", "Q"],
        ),
        (
            "reachable_holds",
            &DIFFERING,
            copies.to_owned(),
            format!("reachable R {{ {ADD} }}"),
            ["verified", "proved", "unknown_cti", "none"],
        ),
        (
            "reachable_fails",
            &DIFFERING,
            "done = true".to_owned(),
            format!("reachable R {{ {ADD} }}"),
            [
                "reachable_failed",
                "reachable_failed",
                "reachable_failed",
                "none",
            ],
        ),
        (
            "trans_holds",
            &DIFFERING,
            copies.to_owned(),
            CHECKED_TRANS.to_owned(),
            ["verified", "proved", "unknown_cti", "none"],
        ),
        (
            "leadsto_holds",
            &DIFFERING,
            copies.to_owned(),
            format!("leadsTo L {{ {ADD} ~> done }}"),
            ["verified", "error", "unknown_cti", "none"],
        ),
        (
            "leadsto_fails",
            &EQUAL,
            "c = not c".to_owned(),
            format!("leadsTo L {{ {ADD} ~> done }}"),
            ["violated", "error", "violated", "none"],
        ),
    ]
}

/// A read into state and each property position, passing and failing. Explicit refuses
/// `leadsTo` whatever it holds, and `fsl-bfs` reports no violation for either failing
/// `leadsTo` or `reachable`.
#[test]
fn rebuilt_value_decides_when_read_into_state_and_in_every_property() {
    let rows = rebuilt_rows();
    for (id, init, body, property, expected) in rows {
        let source = wide_spec(init, &body, &format!("{IDLE}{property}"));
        assert_eq!(results(id, &source), expected, "{id}\n{source}");
    }
    // `trans` failing, with the post-state compared to the pre-state's rebuilt value.
    let source = wide_spec(&DIFFERING, "done = true", &format!("{IDLE}{CHECKED_TRANS}"));
    assert_eq!(
        results("trans_fails", &source),
        ["violated", "violated", "violated", "X"],
        "{source}"
    );
    // The same step refused the same way as the direct read of an over-capacity literal
    // when a property selection leaves the bound out.
    for (id, body) in [
        (
            "rebuilt",
            "let t = Seq { Seq { 0, 0, 0 } }\n sa = (if c then Seq { sa } else t).head()\n done = true",
        ),
        ("direct", "let t = Seq { 0, 0, 0 }\n sa = t\n done = true"),
    ] {
        let source = wide_spec(&[], body, &format!("{IDLE}invariant P {{ not done }}"));
        for engine in ["bmc", "explicit"] {
            let (packet, exit) = fslc(
                &format!("selected_{id}"),
                &source,
                &["verify", "--engine", engine, "--property", "P"],
            );
            assert_eq!(
                (exit, &packet["message"]),
                (2, &json!("model sequence length exceeds capacity")),
                "{id} {engine}: {packet}"
            );
        }
    }
}

/// Evaluation types `old`'s operand from what surrounds it, as `check` always did (#1683).
/// A range-typed literal there used to pass `check` and report `cannot coerce` on bmc and
/// induction, while explicit answered. Each engine now agrees with explicit.
#[test]
fn range_typed_literal_under_old_is_typed_by_what_surrounds_it() {
    let trans = |property: &str| wide_spec(&[], "c = not c", &format!("trans X {{ {property} }}"));
    let rows = [
        ("set_range", trans("a == old(Set { vb }) or true"), "proved"),
        (
            "seq_range",
            trans("sa == old(Seq { vb }) or true"),
            "proved",
        ),
        (
            "seq_arm",
            trans("sa == old(if c then q else Seq { vb }) or true"),
            "proved",
        ),
        (
            "seq_over_capacity",
            trans("sa == old(Seq { vb, vb, vb }) or true"),
            "proved",
        ),
        (
            "seq_capacity_same_range",
            trans("sa == old(Seq { va }) or true"),
            "proved",
        ),
        (
            "explicit_finds_a_violation",
            trans("a == old(Set { vb })"),
            "violated",
        ),
        (
            "ensures",
            wide_spec(&[], "ensures a == old(Set { vb }) or true\n c = not c", ""),
            "proved",
        ),
    ];
    for (id, source, explicit) in rows {
        assert_eq!(check(id, &source).1, 0, "{id}\n{source}");
        for engine in ["bmc", "explicit", "induction"] {
            let (packet, exit) = fslc(id, &source, &["verify", "--engine", engine]);
            let expected = match (explicit, engine) {
                ("violated", _) => "violated",
                (_, "bmc") => "verified",
                _ => "proved",
            };
            assert_eq!(
                (exit, packet["result"].as_str()),
                (i32::from(explicit == "violated"), Some(expected)),
                "{id} {engine}: {packet}"
            );
        }
    }
    // A literal that keeps the expectation's carrier or holds `Int` answered already.
    for (id, property) in [
        ("same_range", "a == old(Set { va }) or true"),
        ("full_capacity", "sa == old(Seq { va, va }) or true"),
        ("int_scalar", "a == old(Set { n }) or true"),
        ("option", "p == old(some(vb)) or true"),
    ] {
        let source = trans(property);
        assert_eq!(
            violations(id, &source),
            (false, false, false, false),
            "{id}\n{source}"
        );
    }
}

/// A rebuilt value is not one of the mismatches a candidate or a preserved map keeps
/// reporting: it decides there, and a concrete run of the same spec agrees.
#[test]
fn rebuilt_value_decides_in_a_candidate_and_under_preserved_progress() {
    let spec = |p: &str, y: &str, extra: &str| {
        format!(
            "spec D {{ type A = 0..1\n state {{ x: 0..1, y: 0..1, p: Option<A>, o: Option<Int>, c: Bool }}\n init {{ x = 0 y = {y} p = {p} o = some(1) c = false }}\n action go() {{ x = y y = x }}\n invariant P {{ x == 0 }}\n{extra}}}\n"
        )
    };
    for (id, p, y, candidate, main, status, used, concrete) in [
        (
            "needed",
            "some(1)",
            "0",
            format!("x == 0 and y == 0 and {ADD}"),
            "proved",
            "proved",
            true,
            "proved",
        ),
        (
            "false_at_init",
            "some(0)",
            "0",
            format!("x == 0 and y == 0 and {ADD}"),
            "unknown_cti",
            "rejected",
            false,
            "violated",
        ),
        (
            "main_false",
            "some(1)",
            "1",
            ADD.to_owned(),
            "violated",
            "proved",
            false,
            "proved",
        ),
    ] {
        let source = spec(p, y, "");
        let (packet, _) = fslc(
            &format!("rebuilt_lemma_{id}"),
            &source,
            &["verify", "--engine", "induction", "--lemma", &candidate],
        );
        let entry = &packet["lemmas"][0];
        assert_eq!(
            (&packet["result"], &entry["status"], &entry["used"]),
            (&json!(main), &json!(status), &json!(used)),
            "{id}: {packet}"
        );
        assert!(
            !packet.to_string().contains("cannot coerce"),
            "{id}: {packet}"
        );
        // The candidate as an invariant, on explicit: a `proved` candidate holds there.
        let oracle = spec(p, y, &format!(" invariant C {{ {candidate} }}\n"));
        let (packet, _) = fslc(
            &format!("rebuilt_lemma_{id}_oracle"),
            &oracle,
            &["verify", "--engine", "explicit", "--property", "C"],
        );
        assert_eq!(packet["result"], concrete, "{id}: {packet}");
    }
    let (packet, _) = fslc(
        "rebuilt_lemma_main_oracle",
        &spec("some(1)", "1", ""),
        &["verify", "--engine", "explicit", "--property", "P"],
    );
    assert_eq!(packet["result"], "violated", "{packet}");

    let ab = "spec Ab { state { same: Bool } init { same = false }\n fair action go() { same = true }\n leadsTo L { not same ~> same }\n}\n";
    for (id, fairness, result, kind) in [
        ("fair", "fair ", "refines", None),
        ("unfair", "", "refinement_failed", Some("progress_lost")),
    ] {
        let im = format!(
            "spec Im {{ type A = 0..1\n state {{ p: Option<A>, o: Option<Int>, c: Bool }}\n init {{ p = some(0) o = some(1) c = false }}\n {fairness}action go() {{ p = some(1) }}\n {fairness}action idle() {{ c = c }}\n}}\n"
        );
        // The rebuilt map and the same map written by hand, which needs no rebuilding.
        for map in [ADD, "p == o"] {
            let (packet, _) = refine_specs(
                &format!("rebuilt_map_{id}"),
                &im,
                ab,
                map,
                "  action idle() -> stutter\n",
                true,
            );
            assert_eq!(packet["result"], result, "{id} {map}: {packet}");
            if let Some(kind) = kind {
                assert_eq!(packet["kind"], kind, "{id} {map}: {packet}");
            }
        }
    }
}

/// `check` also runs the range admission walk, and `build_model` alone is what
/// `fsl-bfs` and the Monitor have, so a rule held by only one walk fails here.
fn assert_refused_on_both_walks(id: &str, source: &str, expected: &str) {
    let (packet, exit) = check(id, source);
    assert_eq!(exit, 2, "{id}: {packet}\n{source}");
    assert!(
        packet["message"]
            .as_str()
            .is_some_and(|message| message.contains(expected)),
        "{id}: {packet}\n{expected}\n{source}"
    );
    let built = fsl_core::build_model(
        fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new(".")).expect("lowers"),
    );
    let Err(failure) = built else {
        panic!("{id}: build_model accepted\n{source}");
    };
    let failure = failure.to_string();
    assert!(
        failure.contains(expected),
        "{id}: {failure}\n{expected}\n{source}"
    );
}

/// #1480 gave `old` its own arm and each evaluation scope its own phase. The arm, Map
/// and `old` rules answer in every one of them, in ordinary validation.
#[test]
fn carrier_rules_run_in_every_evaluation_scope() {
    let arm = not_assignable("Seq(Named(\"B\"), 2)", "Seq(Named(\"A\"), 2)");
    let map = not_assignable("Map(Named(\"K\"), Named(\"A\"))", "Map(Named(\"K\"), Int)");
    let scopes = [
        ("trans", "c = not c", "trans T { {} or true }"),
        ("ensures", "ensures {} or true\n c = not c", ""),
        (
            "unless",
            "c = not c",
            "unless U { ({} or true) unless done }",
        ),
        ("until", "c = not c", "until U { ({} or true) until done }"),
        ("leadsto", "c = not c", "leadsTo L { {} or true ~> done }"),
        (
            "within",
            "c = not c",
            "leadsTo L { {} or true ~> within 2 done }",
        ),
        ("terminal", "c = not c", "terminal { {} or done }"),
        ("reachable", "c = not c", "reachable R { {} or true }"),
        (
            "binder",
            "c = not c",
            "invariant P { forall k: K { {} or true } }",
        ),
        (
            "filter",
            "c = not c",
            "invariant P { count(k: K where {} or true) >= 0 }",
        ),
        (
            "old_binder",
            "c = not c",
            "trans T { forall x in old(a) { {} or true } }",
        ),
        ("guard", "requires {} or true\n c = not c", ""),
        ("let", "let u = {}\n c = not c", ""),
        ("statement", "if {} { c = not c }", ""),
        (
            "statement_binder",
            "forall k: K { if {} { m0[k] = 1 } }",
            "",
        ),
        ("frameless", "c = not c", "invariant P { {} or true }"),
        ("second_old", "c = not c", "trans T { old({} or true) }"),
        (
            "endpoint",
            "c = not c",
            "invariant P { forall i in 0..(if {} then 1 else 1) { i >= 0 } }",
        ),
        (
            "aggregate_endpoint",
            "c = not c",
            "invariant P { count(i in 0..(if {} then 1 else 1)) >= 0 }",
        ),
    ];
    for (id, body, property) in scopes {
        for (form, written, expected) in [
            ("arm", "sa == (if c then q else sb)", &arm),
            ("map", "m0 == m1", &map),
            ("old_arm", "sa == old(if c then q else sb)", &arm),
            ("old_map", "m0 == old(m1)", &map),
        ] {
            let id = format!("scope_{id}_{form}");
            let source = wide_spec(
                &[],
                &body.replace("{}", written),
                &property.replace("{}", written),
            );
            assert_refused_on_both_walks(&id, &source, expected);
        }
    }
    let source = wide_spec(&[("done", "m0 == m1")], "c = not c", "");
    assert_refused_on_both_walks("scope_init", &source, &map);
    let source = wide_spec(&[], "c = not c", "").replace(
        " done = false }",
        " if m0 == m1 { done = true } else { done = false } }",
    );
    assert_refused_on_both_walks("scope_init_condition", &source, &map);
}

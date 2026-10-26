// SPDX-License-Identifier: Apache-2.0

//! #1459: operand kinds and collection signatures are checked before evaluation.
//! Detectors are separate from valid and already-rejecting preservation controls.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const OPTION_ERROR: &str = "'is none/some' requires an Option expression";
const RELATION_HINT: &str = "hint: use a relation A -> B operand";
const SELF_HINT: &str = "hint: use a relation with the same declared source and target type";
const COLOR_TO_INT: &str = "expression of type Named(\"Color\") is not assignable to Int";
const BOOL_TO_INT: &str = "expression of type Bool is not assignable to Int";
const INT_TO_BOOL: &str = "expression of type Int is not assignable to Bool";
const NON_RELATIONS: [(&str, &str); 6] = [
    ("acyclic", "acyclic(n)"),
    ("functional", "functional(n)"),
    ("injective", "injective(n)"),
    ("domain", "domain(n).size() == 0"),
    ("range", "range(n).size() == 0"),
    ("reachable", "reachable(n, 0, 1)"),
];

fn source(predicate: &str, surface: &str) -> String {
    let tail = match surface {
        "I" => format!("  invariant I {{ {predicate} }}\n"),
        "D" => format!("  invariant I {{ if true then true else ({predicate}) }}\n"),
        "A" => format!(
            "  invariant I {{ true }}\n  acceptance AC-1 \"operand\" {{\n    stay()\n    expect {predicate}\n  }}\n"
        ),
        "F" => format!(
            "  invariant I {{ true }}\n  forbidden FB-1 \"operand\" {{\n    flag({predicate})\n    expect rejected\n  }}\n"
        ),
        _ => panic!("unknown test surface {surface}"),
    };
    format!(
        r#"requirements Operand {{
  enum Color {{ Red, Blue }}
  type N = 0..2
  type M = 0..2
  state {{ c: Color, n: N, s: Set<N>, q: Seq<N, 3>, zero: Seq<N, 0>, o: Option<N>, r: relation N -> N, h: relation Color -> N, g: relation N -> Color, k: relation N -> M }}
  init {{ c = Red n = 0 s = Set {{}} q = Seq {{ 0 }} zero = Seq {{}} o = none r = Set {{}} h = Set {{}} g = Set {{}} k = Set {{}} }}
  requirement REQ-1 "operands" {{
    action stay() {{ n = n }}
    action flag(x: Bool) {{ requires false n = n }}
  }}
{tail}}}
"#
    )
}

fn expected_location(text: &str, surface: &str) -> Value {
    let (line, column) = text
        .lines()
        .enumerate()
        .find_map(|(index, line)| match surface {
            "I" if line.starts_with("  invariant I") => Some((index + 1, 3)),
            "A" if line.starts_with("    expect ") => Some((index + 1, 5)),
            "F" if line.starts_with("    flag(") => Some((index + 1, 5)),
            // The conditional's authored else span starts at the opening `(`,
            // not at the predicate token one column later.
            "D" => line.find("else (").map(|column| (index + 1, column + 6)),
            _ => None,
        })
        .expect("authored expression origin");
    json!({"line": line, "column": column})
}

fn run(label: &str, text: &str) -> [(&'static str, Value, i32); 3] {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let directory = root
        .join(".codex/tmp/issue_1459_static_operand_types")
        .join(label);
    std::fs::create_dir_all(&directory).expect("create owned test fixture directory");
    std::fs::write(directory.join("operand.fsl"), text).expect("write operand fixture");
    // Always collect check AND both verification results before asserting.
    let outputs = ["check", "bmc", "explicit"].map(|mode| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.args([
            if mode == "check" { "check" } else { "verify" },
            "operand.fsl",
        ]);
        if mode != "check" {
            command.args(["--depth", "1", "--engine", mode, "--no-cache"]);
        }
        let output = command
            .current_dir(&directory)
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run operand check");
        assert!(output.stderr.is_empty(), "{label} {mode}: {output:?}");
        let value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{label} {mode}: invalid JSON {error}: {output:?}"));
        (
            mode,
            value,
            output.status.code().expect("process exit code"),
        )
    });
    std::fs::write(
        directory.join("observations.json"),
        serde_json::to_vec_pretty(&outputs).expect("serialize complete observations"),
    )
    .expect("retain observations without deleting fixtures");
    outputs
}

fn assert_error(label: &str, text: &str, parts: &[&str], location: &Value) {
    let outputs = run(label, text);
    for (mode, value, status) in &outputs {
        assert_eq!(
            (*status, value["result"].as_str(), value["kind"].as_str()),
            (2, Some("error"), Some("semantics")),
            "{label} {mode}: all observations {outputs:#?}"
        );
        let message = value["message"]
            .as_str()
            .expect("semantic diagnostic message");
        for part in parts {
            assert!(
                message.contains(part),
                "{label} {mode}: missing {part:?}; all observations {outputs:#?}"
            );
        }
        assert_eq!(
            &value["loc"], location,
            "{label} {mode}: all observations {outputs:#?}"
        );
    }
}

fn reject(label: &str, predicate: &str, surfaces: &[&str], parts: &[&str]) {
    for surface in surfaces {
        let text = source(predicate, surface);
        let location = expected_location(&text, surface);
        assert_error(&format!("{label}-{surface}"), &text, parts, &location);
    }
}

fn accept(label: &str, predicate: &str) {
    for surface in ["I", "A", "F", "D"] {
        let outputs = run(&format!("{label}-{surface}"), &source(predicate, surface));
        for (mode, value, status) in &outputs {
            assert_eq!(*status, 0, "{label} {surface} {mode}: {outputs:#?}");
            if *mode == "check" {
                assert_eq!(value["result"], "ok", "{label}: {outputs:#?}");
            } else {
                assert!(
                    matches!(value["result"].as_str(), Some("verified" | "proved")),
                    "{label}: {outputs:#?}"
                );
                assert!(
                    value["checked_to_depth"].is_number(),
                    "{label}: {outputs:#?}"
                );
            }
            assert!(
                value.get("kind").is_none() && value.get("message").is_none(),
                "{label}: {outputs:#?}"
            );
        }
    }
}

fn option_rejections(surfaces: &[&str]) {
    for (index, predicate) in ["n is none", "n is some(z)"].iter().enumerate() {
        reject(
            &format!("option-{index}"),
            predicate,
            surfaces,
            &[OPTION_ERROR],
        );
    }
}

#[test]
fn option_patterns_reject_non_option_operands_in_invariants() {
    option_rejections(&["I"]);
}

#[test]
fn option_patterns_reject_non_option_operands_in_expectations() {
    option_rejections(&["A"]);
}

#[test]
fn option_patterns_reject_non_option_operands_in_forbidden_arguments() {
    option_rejections(&["F"]);
}

#[test]
fn option_patterns_reject_non_option_operands_in_dead_branches() {
    option_rejections(&["D"]);
}

#[test]
fn abs_requires_numeric_operands_in_all_expression_positions() {
    for (index, predicate, message) in [
        (0, "abs(c) == 0", COLOR_TO_INT),
        (1, "abs(true) == 0", BOOL_TO_INT),
    ] {
        reject(
            &format!("abs-kind-{index}"),
            predicate,
            &["I", "A", "F", "D"],
            &[message],
        );
    }
}

#[test]
fn abs_rejects_enum_domain_and_range_elements() {
    for (index, predicate) in [
        "forall x in domain(h.add(Red, 1)): abs(x) >= 0",
        "forall x in range(g.add(1, Red)): abs(x) >= 0",
    ]
    .iter()
    .enumerate()
    {
        reject(
            &format!("abs-projection-{index}"),
            predicate,
            &["I", "A", "F", "D"],
            &[COLOR_TO_INT],
        );
    }
    let text = "spec P {\n  enum Color { Red, Blue }\n  type N = 0..2\n  type M = 0..4\n  state { s: relation Color -> Color }\n  init { s = Set {} }\n  action links(a: Color, b: Color) { s = s.add(a, b) }\n  invariant P { forall x in domain(s) { abs(x) != 0 } }\n}\n";
    assert_error(
        "supplied-p15",
        text,
        &[COLOR_TO_INT],
        &json!({"line": 8, "column": 3}),
    );
}

fn relation_rejections(surfaces: &[&str]) {
    for (name, predicate) in NON_RELATIONS {
        reject(
            &format!("non-relation-{name}"),
            predicate,
            surfaces,
            &[&format!("{name}() requires a relation"), RELATION_HINT],
        );
    }
}

#[test]
fn relation_functions_reject_non_relation_operands_in_invariants() {
    relation_rejections(&["I"]);
}

#[test]
fn relation_functions_reject_non_relation_operands_in_traces() {
    relation_rejections(&["A", "F"]);
}

#[test]
fn relation_functions_reject_non_relation_operands_in_dead_branches() {
    relation_rejections(&["D"]);
}

#[test]
fn reachable_rejects_heterogeneous_relation_before_concrete_false_green() {
    let parts = [
        "reachable() requires a self-relation (relation T -> T)",
        SELF_HINT,
    ];
    // A/F evaluate this nonempty relation; neither endpoint is wrongly typed.
    reject(
        "heterogeneous-reachable",
        "not reachable(h.add(Red, 1).add(Blue, 2), Red, 2)",
        &["A", "F", "I", "D"],
        &parts,
    );
    let text = "spec P {\n  enum Color { Red, Blue }\n  type N = 0..2\n  type M = 0..9\n  state { r: relation Color -> N }\n  init { r = Set {} }\n  action l1() { r = r.add(Red, 1) }\n  action l2() { r = r.add(Blue, 2) }\n  invariant P { not reachable(r, Red, 2) }\n}\n";
    assert_error(
        "supplied-b03",
        text,
        &parts,
        &json!({"line": 9, "column": 3}),
    );
}

#[test]
fn acyclic_rejects_heterogeneous_relation_before_concrete_false_green() {
    let parts = [
        "acyclic() requires a self-relation (relation T -> T)",
        SELF_HINT,
    ];
    reject(
        "heterogeneous-acyclic",
        "acyclic(h.add(Red, 1).add(Blue, 0))",
        &["A", "F", "I", "D"],
        &parts,
    );
    let text = "spec P {\n  enum Color { Red, Blue }\n  type N = 0..2\n  state { r: relation Color -> N }\n  init { r = Set {} }\n  action l1() { r = r.add(Red, 1) }\n  action l2() { r = r.add(Blue, 0) }\n  invariant P { acyclic(r) }\n}\n";
    assert_error(
        "supplied-b13",
        text,
        &parts,
        &json!({"line": 8, "column": 3}),
    );
}

#[test]
fn numeric_cross_domain_self_relation_is_rejected() {
    // ANS-2120: equal numeric bounds do not make declared N -> M a self-relation.
    for (name, predicate) in [
        ("reachable", "not reachable(k, 0, 0)"),
        ("acyclic", "acyclic(k)"),
    ] {
        reject(
            &format!("numeric-cross-domain-{name}"),
            predicate,
            &["I", "A", "F", "D"],
            &[
                &format!("{name}() requires a self-relation (relation T -> T)"),
                SELF_HINT,
            ],
        );
    }
}

#[test]
fn collection_methods_reject_invalid_receivers() {
    for (index, predicate) in [
        "n.contains(0)",
        "n.size() == 0",
        "n.add(0) == n",
        "n.remove(0) == n",
        "n.push(0) == n",
        "n.pop() == n",
    ]
    .iter()
    .enumerate()
    {
        reject(
            &format!("numeric-receiver-{index}"),
            predicate,
            &["I"],
            &["method receiver has no supported collection methods"],
        );
    }
    for (index, predicate, message) in [
        (0, "s.push(0) == s", "invalid Set method 'push'/1"),
        (1, "q.add(0) == q", "invalid Seq method 'add'/1"),
        (2, "r.size() == 0", "invalid relation method 'size'/0"),
    ] {
        reject(
            &format!("unsupported-method-{index}"),
            predicate,
            &["I"],
            &[message],
        );
    }
}

#[test]
fn collection_methods_reject_wrong_arity() {
    for (index, predicate, message) in [
        (0, "s.contains()", "invalid Set method 'contains'/0"),
        (1, "s.contains(0, 1)", "invalid Set method 'contains'/2"),
        (2, "s.add() == s", "invalid Set method 'add'/0"),
        (3, "s.add(0, 1) == s", "invalid Set method 'add'/2"),
        (4, "s.remove() == s", "invalid Set method 'remove'/0"),
        (5, "s.remove(0, 1) == s", "invalid Set method 'remove'/2"),
        (6, "s.size(0) == 0", "invalid Set method 'size'/1"),
        (7, "q.contains()", "invalid Seq method 'contains'/0"),
        (8, "q.contains(0, 1)", "invalid Seq method 'contains'/2"),
        (9, "q.push() == q", "invalid Seq method 'push'/0"),
        (10, "q.push(0, 1) == q", "invalid Seq method 'push'/2"),
        (11, "q.pop(1) == q", "invalid Seq method 'pop'/1"),
        (12, "q.head(1) == 0", "invalid Seq method 'head'/1"),
        (13, "q.at() == 0", "invalid Seq method 'at'/0"),
        (14, "q.at(0, 1) == 0", "invalid Seq method 'at'/2"),
        (15, "q.size(0) == 0", "invalid Seq method 'size'/1"),
    ] {
        reject(&format!("arity-{index}"), predicate, &["I"], &[message]);
    }
    for name in ["contains", "add", "remove"] {
        for count in [0, 1, 3] {
            let args = vec!["0"; count].join(", ");
            let call = format!("r.{name}({args})");
            let predicate = if name == "contains" {
                call
            } else {
                format!("{call} == r")
            };
            reject(
                &format!("relation-arity-{name}-{count}"),
                &predicate,
                &["I"],
                &[
                    &format!("invalid relation method '{name}'/{count}"),
                    &format!("hint: use r.{name}(source, target)"),
                ],
            );
        }
    }
}

#[test]
fn collection_method_signatures_are_checked_in_dead_branches_and_traces() {
    reject(
        "method-positions-receiver",
        "n.contains(0)",
        &["A", "F", "D"],
        &["method receiver has no supported collection methods"],
    );
    reject(
        "method-positions-arity",
        "q.pop(1) == q",
        &["A", "F", "D"],
        &["invalid Seq method 'pop'/1"],
    );
    reject(
        "method-positions-relation",
        "r.contains(0)",
        &["A", "F", "D"],
        &[
            "invalid relation method 'contains'/1",
            "hint: use r.contains(source, target)",
        ],
    );
}

#[test]
fn empty_range_count_checks_its_original_filter() {
    for (index, predicate) in [
        "count(i in 1..0 where not 1) == 0",
        "count(i in 1..0 where 1) == 0",
    ]
    .iter()
    .enumerate()
    {
        reject(
            &format!("empty-count-{index}"),
            predicate,
            &["I", "A", "F", "D"],
            &[INT_TO_BOOL],
        );
    }
}

#[test]
fn empty_range_sum_checks_its_original_value() {
    for (index, predicate, message) in [
        (0, "sum(i in 1..0 of -Red) == 0", COLOR_TO_INT),
        (1, "sum(i in 1..0 of true) == 0", BOOL_TO_INT),
    ] {
        reject(
            &format!("empty-sum-{index}"),
            predicate,
            &["I", "A", "F", "D"],
            &[message],
        );
    }
}

#[test]
fn zero_capacity_sequence_aggregates_check_original_operands() {
    reject(
        "empty-seq-count",
        "count(i in zero where not 1) == 0",
        &["I", "A", "F", "D"],
        &[INT_TO_BOOL],
    );
    reject(
        "empty-seq-sum",
        "sum(i in zero of true) == 0",
        &["I", "A", "F", "D"],
        &[BOOL_TO_INT],
    );
}

#[test]
fn option_patterns_and_binding_continuations_still_pass() {
    accept(
        "valid-option",
        "o is none and not (o is some(v)) and ((some(n) is some(w)) => w == n)",
    );
}

#[test]
fn numeric_abs_still_passes() {
    accept(
        "valid-abs",
        "abs(n) == 0 and abs(-(-1)) == 1 and abs(abs(n)) == 0",
    );
}

#[test]
fn valid_collection_signatures_still_pass() {
    // One expression per runtime dispatch row (13 total), not one example per
    // receiver kind. All values stay within the declared item/index domains.
    for (index, predicate) in [
        "not s.contains(0)",
        "s.add(0).contains(0)",
        "s.add(0).remove(0) == s",
        "s.size() == 0",
        "q.contains(0)",
        "q.push(1).at(1) == 1",
        "q.pop().size() == 0",
        "q.head() == 0",
        "q.at(0) == 0",
        "q.size() == 1",
        "not r.contains(0, 1)",
        "r.add(0, 1).contains(0, 1)",
        "r.add(0, 1).remove(0, 1) == r",
    ]
    .iter()
    .enumerate()
    {
        accept(&format!("valid-dispatch-{index}"), predicate);
    }
}

#[test]
fn heterogeneous_non_path_relation_functions_still_pass() {
    accept(
        "valid-heterogeneous",
        "functional(h) and injective(h) and domain(h).size() == 0 and range(h).size() == 0 and functional(g) and injective(g) and domain(g).size() == 0 and range(g).size() == 0 and h.add(Red, 1).contains(Red, 1) and h.add(Red, 1).remove(Red, 1) == h and g.add(1, Blue).contains(1, Blue) and g.add(1, Blue).remove(1, Blue) == g",
    );
}

#[test]
fn well_typed_empty_aggregates_keep_identity_values() {
    accept(
        "valid-empty",
        "count(i in 1..0 where i == 1) == 0 and sum(i in 1..0 of i) == 0 and unique(i in 1..0) and not exactlyOne(i in 1..0) and count(i in zero) == 0 and sum(i in zero of i) == 0 and unique(i in zero) and not exactlyOne(i in zero)",
    );
}

fn assert_violation(label: &str, text: &str, step: u64, property: &str, line: u64) {
    let outputs = run(label, text);
    for (mode, value, status) in &outputs {
        if *mode == "check" {
            assert_eq!(
                (*status, value["result"].as_str()),
                (0, Some("ok")),
                "{label}: {outputs:#?}"
            );
        } else {
            assert_eq!(
                (*status, value["result"].as_str()),
                (1, Some("violated")),
                "{label}: {outputs:#?}"
            );
            assert_eq!(value["violated_at_step"], step, "{label}: {outputs:#?}");
            assert_eq!(
                value["violation_kind"], "invariant",
                "{label}: {outputs:#?}"
            );
            assert_eq!(value["invariant"], property, "{label}: {outputs:#?}");
            assert_eq!(
                value["loc"],
                json!({"line": line, "column": 3}),
                "{label}: {outputs:#?}"
            );
        }
        assert!(value.get("kind").is_none(), "{label}: {outputs:#?}");
    }
}

#[test]
fn well_typed_invariant_violation_is_not_hidden() {
    assert_violation(
        "valid-rejecting-invariant",
        &source("n != 0", "I"),
        0,
        "I",
        11,
    );
}

#[test]
fn self_relation_reachability_remains_non_reflexive() {
    accept(
        "valid-non-reflexive",
        "acyclic(r) and not reachable(r, 0, 0)",
    );
    let text = "spec Cycle {\n  type N = 0..2\n  state { r: relation N -> N }\n  init { r = Set {} }\n  action link() { r = r.add(0, 0) }\n  invariant P { not reachable(r, 0, 0) }\n}\n";
    assert_violation("valid-real-cycle", text, 1, "P", 6);
}

#[test]
fn existing_numeric_head_and_at_receiver_errors_stay_rejected() {
    for (name, predicate) in [("head", "n.head() == 0"), ("at", "n.at(0) == 0")] {
        reject(
            &format!("existing-receiver-{name}"),
            predicate,
            &["I", "A", "F", "D"],
            &[&format!("public Kernel cannot type method '{name}'")],
        );
    }
}

#[test]
fn cardinality_predicates_keep_checking_empty_range_filters() {
    reject(
        "existing-unique",
        "unique(i in 1..0 where not 1)",
        &["I", "A", "F", "D"],
        &[INT_TO_BOOL],
    );
    reject(
        "existing-exactly-one",
        "not exactlyOne(i in 1..0 where not 1)",
        &["I", "A", "F", "D"],
        &[INT_TO_BOOL],
    );
}

#[test]
fn existing_1458_endpoint_mismatch_stays_rejected() {
    reject(
        "existing-endpoint",
        "not reachable(r, Red, Blue)",
        &["I", "A", "F", "D"],
        &[
            "expression of type Named(\"Color\") is not assignable to Named(\"N\")",
            "hint: use endpoints matching the relation's declared source and target types",
        ],
    );
}

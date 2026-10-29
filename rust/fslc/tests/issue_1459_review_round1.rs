// SPDX-License-Identifier: Apache-2.0

//! #1459 review R1: retain every selectable relation's declared endpoint identity
//! and preserve authored origins when projection inference fails.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const SELF_HINT: &str = "hint: use a relation with the same declared source and target type";
const RELATION_HINT: &str = "hint: use a relation A -> B operand";
const ASTRA_CONDITIONAL: &str = r"spec ConditionalSelfRelation {
  type N = 0..2
  type M = 0..2
  state { r: relation N -> N, k: relation N -> M }
  init { r = Set {} k = Set {} }
  action stay() { }
  invariant P { acyclic(if false then r else k) }
}
";
const OPUS_REACHABLE: &str = r"spec P {
  type N = 0..2
  type M = 0..4
  state { r: relation N -> N, q: relation N -> M, flag: Bool }
  init { r = Set {}
         q = Set {}
         flag = true }
  action linkq(a: N, b: M) { q = q.add(a, b) }
  action flip() { flag = false }
  invariant P { not reachable(if flag then r else q, 0, 1) }
}
";
const OPUS_ACYCLIC: &str = r"spec P {
  type N = 0..2
  type M = 0..2
  state { r: relation N -> N, q: relation N -> M, flag: Bool }
  init { r = Set {}
         q = Set {}
         flag = true }
  action linkq(a: N, b: M) { q = q.add(a, b) }
  action flip() { flag = false }
  invariant P { acyclic(if flag then r else q) }
}
";
const TERMINAL: &str = r"spec P {
  type N = 0..2
  state { n: N }
  init { n = 0 }
  action bump() { requires n < 2
                  n = n + 1 }
  terminal { n is none or n == 2 }
}
";

fn model(property: &str) -> String {
    format!(
        r"spec ReviewSelf {{
  type N = 0..2
  type M = 0..2
  enum Color {{ Red, Blue }}
  state {{ r: relation N -> N, k: relation N -> M, h: relation Color -> N }}
  init {{ r = Set {{}} k = Set {{}} h = Set {{}} }}
  action stay() {{ }}
  {property}
}}
"
    )
}

fn projection(predicate: &str) -> String {
    format!(
        r"spec ProjectionOrigin {{
  type N = 0..2
  state {{ n: N }}
  init {{ n = 0 }}
  action stay() {{ }}
  invariant P {{ {predicate} }}
}}
"
    )
}

fn predicate(name: &str, operand: &str) -> String {
    match name {
        "acyclic" => format!("acyclic({operand})"),
        "reachable" => format!("not reachable({operand}, 0, 0)"),
        _ => panic!("unknown path function {name}"),
    }
}

fn run(label: &str, text: &str) -> [(&'static str, Value, i32); 3] {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let directory = root.join(".codex/tmp/issue_1459_review_round1").join(label);
    std::fs::create_dir_all(&directory).expect("create owned review fixture directory");
    std::fs::write(directory.join("review.fsl"), text).expect("write review fixture");
    let outputs = ["check", "bmc", "explicit"].map(|mode| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.args([
            if mode == "check" { "check" } else { "verify" },
            "review.fsl",
        ]);
        if mode != "check" {
            command.args(["--depth", "2", "--engine", mode, "--no-cache"]);
        }
        let output = command
            .current_dir(&directory)
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run review regression");
        assert!(output.stderr.is_empty(), "{label} {mode}: {output:?}");
        let value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{label} {mode}: invalid JSON {error}: {output:?}"));
        (mode, value, output.status.code().expect("process exit"))
    });
    // Retain every mode before an expected red assertion can stop the test.
    std::fs::write(
        directory.join("observations.json"),
        serde_json::to_vec_pretty(&outputs).expect("serialize review observations"),
    )
    .expect("retain review observations without deleting fixtures");
    outputs
}

fn reject(label: &str, text: &str, parts: &[&str], location: &Value) {
    let outputs = run(label, text);
    for (mode, value, status) in &outputs {
        assert_eq!(
            (*status, value["result"].as_str(), value["kind"].as_str()),
            (2, Some("error"), Some("semantics")),
            "{label} {mode}: {outputs:#?}"
        );
        let message = value["message"].as_str().expect("semantic diagnostic");
        for part in parts {
            assert!(
                message.contains(part),
                "{label} {mode}: missing {part:?}: {outputs:#?}"
            );
        }
        assert_eq!(&value["loc"], location, "{label} {mode}: {outputs:#?}");
    }
}

fn self_rejection(label: &str, name: &str, operand: &str, property: &str) {
    let text = model(&format!("{property} P {{ {} }}", predicate(name, operand)));
    reject(
        label,
        &text,
        &[
            &format!("{name}() requires a self-relation (relation T -> T)"),
            SELF_HINT,
        ],
        &json!({"line": 8, "column": 3}),
    );
}

fn projection_rejection(label: &str, name: &str, body: &str, column: u64) {
    reject(
        label,
        &projection(body),
        &[&format!("{name}() requires a relation"), RELATION_HINT],
        &json!({"line": 6, "column": column}),
    );
}

fn verdict(label: &str, text: &str, violated: bool, line: u64) {
    let outputs = run(label, text);
    for (mode, value, status) in &outputs {
        if *mode == "check" {
            assert_eq!(
                (*status, value["result"].as_str()),
                (0, Some("ok")),
                "{label}: {outputs:#?}"
            );
        } else if violated {
            assert_eq!(
                (*status, value["result"].as_str()),
                (1, Some("violated")),
                "{label}: {outputs:#?}"
            );
            assert_eq!(
                value["violation_kind"], "invariant",
                "{label}: {outputs:#?}"
            );
            assert_eq!(value["invariant"], "P", "{label}: {outputs:#?}");
            assert_eq!(value["violated_at_step"], 0, "{label}: {outputs:#?}");
            assert_eq!(
                value["loc"],
                json!({"line": line, "column": 3}),
                "{label}: {outputs:#?}"
            );
        } else {
            assert_eq!(*status, 0, "{label}: {outputs:#?}");
            assert_eq!(
                value["result"],
                if *mode == "bmc" { "verified" } else { "proved" },
                "{label}: {outputs:#?}"
            );
        }
        assert!(
            value.get("kind").is_none() && value.get("message").is_none(),
            "{label}: {outputs:#?}"
        );
    }
}

#[test]
fn acyclic_rejects_selected_nonself_else_branch() {
    reject(
        "astra-conditional",
        ASTRA_CONDITIONAL,
        &[
            "acyclic() requires a self-relation (relation T -> T)",
            SELF_HINT,
        ],
        &json!({"line": 7, "column": 3}),
    );
}

#[test]
fn reachable_rejects_selected_nonself_else_branch() {
    self_rejection(
        "reachable-selected",
        "reachable",
        "if false then r else k",
        "invariant",
    );
}

#[test]
fn acyclic_rejects_unselected_nonself_else_branch() {
    self_rejection(
        "acyclic-unselected",
        "acyclic",
        "if true then r else k",
        "invariant",
    );
}

#[test]
fn reachable_rejects_unselected_nonself_else_branch() {
    self_rejection(
        "reachable-unselected",
        "reachable",
        "if true then r else k",
        "invariant",
    );
}

#[test]
fn acyclic_rejects_nested_nonself_then_branch() {
    self_rejection(
        "acyclic-nested-then",
        "acyclic",
        "if false then r else (if true then k else r)",
        "invariant",
    );
}

#[test]
fn reachable_rejects_nested_nonself_then_branch() {
    self_rejection(
        "reachable-nested-then",
        "reachable",
        "if false then r else (if true then k else r)",
        "invariant",
    );
}

#[test]
fn acyclic_rejects_nested_nonself_else_branch() {
    self_rejection(
        "acyclic-nested-else",
        "acyclic",
        "if true then (if false then r else k) else r",
        "invariant",
    );
}

#[test]
fn reachable_rejects_nested_nonself_else_branch() {
    self_rejection(
        "reachable-nested-else",
        "reachable",
        "if true then (if false then r else k) else r",
        "invariant",
    );
}

#[test]
fn acyclic_rejects_conditional_nonself_through_old() {
    self_rejection(
        "acyclic-old",
        "acyclic",
        "old(if false then r else k)",
        "trans",
    );
}

#[test]
fn reachable_rejects_conditional_nonself_through_old() {
    self_rejection(
        "reachable-old",
        "reachable",
        "old(if false then r else k)",
        "trans",
    );
}

fn let_source(name: &str, valid: bool) -> String {
    let binding = if valid {
        "if false then r else r.add(0, 1)"
    } else {
        "if false then r else k"
    };
    let action = format!(
        "action stay() {{\n    let selected = {binding}\n    let chosen = selected\n    ensures {}\n  }}",
        predicate(name, "chosen")
    );
    model("invariant P { true }").replace("action stay() { }", &action)
}

#[test]
fn acyclic_rejects_conditional_nonself_through_let_alias() {
    reject(
        "acyclic-let",
        &let_source("acyclic", false),
        &[
            "acyclic() requires a self-relation (relation T -> T)",
            SELF_HINT,
        ],
        &json!({"line": 7, "column": 3}),
    );
}

#[test]
fn reachable_rejects_conditional_nonself_through_let_alias() {
    reject(
        "reachable-let",
        &let_source("reachable", false),
        &[
            "reachable() requires a self-relation (relation T -> T)",
            SELF_HINT,
        ],
        &json!({"line": 7, "column": 3}),
    );
}

#[test]
fn opus_reachable_rejects_conditional_endpoint_bound_mismatch_at_else_arm() {
    reject(
        "opus-s01",
        OPUS_REACHABLE,
        &[
            r#"expression of type Relation(Named("N"), Named("M")) is not assignable to Relation(Named("N"), Named("N"))"#,
        ],
        &json!({"line": 10, "column": 51}),
    );
}

#[test]
fn opus_acyclic_rejects_selected_nonself_relation_with_identical_bounds() {
    reject(
        "opus-s02",
        OPUS_ACYCLIC,
        &[
            "acyclic() requires a self-relation (relation T -> T)",
            SELF_HINT,
        ],
        &json!({"line": 10, "column": 3}),
    );
}

#[test]
fn domain_equality_invariant_has_authored_origin() {
    projection_rejection("domain-equality", "domain", "domain(n) == domain(n)", 3);
}

#[test]
fn range_equality_invariant_has_authored_origin() {
    projection_rejection("range-equality", "range", "range(n) == range(n)", 3);
}

#[test]
fn domain_equality_conditional_branch_has_authored_origin() {
    projection_rejection(
        "domain-branch",
        "domain",
        "if true then true else (domain(n) == domain(n))",
        40,
    );
}

#[test]
fn range_equality_conditional_branch_has_authored_origin() {
    projection_rejection(
        "range-branch",
        "range",
        "if true then true else (range(n) == range(n))",
        40,
    );
}

#[test]
fn domain_equality_empty_literal_fallback_keeps_relation_diagnostic() {
    projection_rejection("domain-empty-right", "domain", "domain(n) == Set {}", 3);
}

#[test]
fn domain_equality_empty_literal_on_left_keeps_relation_diagnostic() {
    projection_rejection("domain-empty-left", "domain", "Set {} == domain(n)", 3);
}

#[test]
fn range_equality_empty_literal_fallback_keeps_relation_diagnostic() {
    projection_rejection("range-empty-right", "range", "range(n) == Set {}", 3);
}

#[test]
fn range_equality_empty_literal_on_left_keeps_relation_diagnostic() {
    projection_rejection("range-empty-left", "range", "Set {} == range(n)", 3);
}

#[test]
fn terminal_option_misuse_has_terminal_declaration_origin() {
    reject(
        "terminal-option",
        TERMINAL,
        &["'is none/some' requires an Option expression"],
        &json!({"line": 7, "column": 3}),
    );
}

#[test]
fn direct_nonself_then_branches_stay_rejected() {
    for name in ["acyclic", "reachable"] {
        for condition in ["true", "false"] {
            self_rejection(
                &format!("existing-then-{name}-{condition}"),
                name,
                &format!("if {condition} then k else r"),
                "invariant",
            );
        }
    }
}

#[test]
fn color_to_numeric_conditional_branches_stay_rejected() {
    for name in ["acyclic", "reachable"] {
        for operand in ["if false then r else h", "if true then h else r"] {
            let text = model(&format!("invariant P {{ {} }}", predicate(name, operand)));
            let line = text.lines().nth(7).expect("property line");
            let column = line.rfind("else ").expect("else branch") + 6;
            reject(
                &format!("existing-color-{name}-{}", operand.contains("then h")),
                &text,
                &["not assignable", "Color"],
                &json!({"line": 8, "column": column}),
            );
        }
    }
}

#[test]
fn genuine_self_relation_conditionals_keep_real_acyclic_verdicts() {
    for (index, operand, violated) in [
        (0, "if false then r else r.add(0, 1)", false),
        (1, "if true then r.add(0, 0) else r", true),
    ] {
        verdict(
            &format!("valid-acyclic-{index}"),
            &model(&format!("invariant P {{ acyclic({operand}) }}")),
            violated,
            8,
        );
    }
}

#[test]
fn genuine_self_relation_conditionals_keep_real_reachable_verdicts() {
    for (index, operand, violated) in [
        (0, "if true then r.add(0, 1) else r", false),
        (1, "if false then r.add(0, 1) else r", true),
    ] {
        verdict(
            &format!("valid-reachable-{index}"),
            &model(&format!("invariant P {{ reachable({operand}, 0, 1) }}")),
            violated,
            8,
        );
    }
}

#[test]
fn genuine_relation_projection_equalities_keep_real_verdicts() {
    for (index, expression, violated) in [
        (0, "domain(r.add(0, 1)) == Set { 0 }", false),
        (1, "range(r.add(0, 1)) == Set { 1 }", false),
        (2, "domain(r.add(0, 1)) == Set { 1 }", true),
        (3, "range(r.add(0, 1)) == Set { 0 }", true),
        (4, "domain(h.add(Red, 1)) == Set { Red }", false),
        (5, "range(h.add(Red, 1)) == Set { 1 }", false),
    ] {
        verdict(
            &format!("valid-projection-{index}"),
            &model(&format!("invariant P {{ {expression} }}")),
            violated,
            8,
        );
    }
}

#[test]
fn genuine_self_relation_conditionals_through_old_and_let_still_pass() {
    for name in ["acyclic", "reachable"] {
        let text = model(&format!(
            "trans P {{ {} }}",
            predicate(name, "old(if false then r else r.add(0, 1))")
        ));
        verdict(&format!("valid-old-{name}"), &text, false, 8);
        verdict(
            &format!("valid-let-{name}"),
            &let_source(name, true),
            false,
            8,
        );
    }
}

#[test]
fn precise_inner_conditional_origin_is_not_overwritten() {
    let text = projection("if true then true else (if 1 then true else true)");
    let column = text
        .lines()
        .nth(5)
        .expect("property line")
        .find("1 then")
        .expect("inner condition")
        + 1;
    reject(
        "precise-inner",
        &text,
        &["expression of type Int is not assignable to Bool"],
        &json!({"line": 6, "column": column}),
    );
}

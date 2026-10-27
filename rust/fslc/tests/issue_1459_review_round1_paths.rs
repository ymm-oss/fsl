// SPDX-License-Identifier: Apache-2.0

//! #1459 R1 supplement: retain selectable-relation identity through update
//! receivers, old in ensures, and let aliases in requires; range-bound errors
//! retain the enclosing property's authored origin rather than generated 1:1.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const SELF_HINT: &str = "hint: use a relation with the same declared source and target type";

fn source(position: &str, expression: &str, valid: bool) -> String {
    let target = if valid { "N" } else { "M" };
    let (action, property) = match position {
        "invariant" => (
            "action stay() { }".to_owned(),
            format!("invariant P {{ {expression} }}"),
        ),
        "ensures" => (
            format!("action stay() {{\n    ensures {expression}\n  }}"),
            "invariant P { true }".to_owned(),
        ),
        "requires-alias" => (
            format!(
                "action stay() {{\n    let selected = if flag then r else q\n    let chosen = selected\n    requires {expression}\n  }}"
            ),
            "invariant P { true }".to_owned(),
        ),
        _ => panic!("unknown path position {position}"),
    };
    format!(
        r"spec ReviewPaths {{
  type N = 0..2
  type M = 0..2
  state {{ r: relation N -> N, q: relation N -> {target}, flag: Bool }}
  init {{ r = Set {{}} q = Set {{}} flag = false }}
  {action}
  {property}
}}
"
    )
}

fn check(label: &str, text: &str) -> (Value, i32) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let directory = root
        .join(".codex/tmp/issue_1459_review_round1_paths")
        .join(label);
    std::fs::create_dir_all(&directory).expect("create owned path fixture directory");
    std::fs::write(directory.join("paths.fsl"), text).expect("write path fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "paths.fsl"])
        .current_dir(directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run path checker");
    assert!(output.stderr.is_empty(), "{label}: {output:?}");
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{label}: invalid JSON {error}: {output:?}"));
    (value, output.status.code().expect("native exit status"))
}

fn reject(label: &str, position: &str, expression: &str, name: &str) {
    let (value, status) = check(label, &source(position, expression, false));
    assert_eq!(
        (status, value["result"].as_str(), value["kind"].as_str()),
        (2, Some("error"), Some("semantics")),
        "{label}: {value:#}"
    );
    let message = value["message"].as_str().expect("self-relation diagnostic");
    assert!(
        message.contains(&format!(
            "{name}() requires a self-relation (relation T -> T)"
        )) && message.contains(SELF_HINT),
        "{label}: {value:#}"
    );
    let line = if position == "invariant" { 7 } else { 6 };
    assert_eq!(
        value["loc"],
        json!({"line": line, "column": 3}),
        "{label}: {value:#}"
    );
}

fn accept(label: &str, position: &str, expression: &str) {
    let (value, status) = check(label, &source(position, expression, true));
    assert_eq!(
        (status, value["result"].as_str()),
        (0, Some("ok")),
        "{label}: {value:#}"
    );
    assert!(
        value.get("kind").is_none() && value.get("message").is_none(),
        "{label}: {value:#}"
    );
}

#[test]
fn acyclic_rejects_conditional_add_receiver_selecting_nonself_relation() {
    reject(
        "acyclic-add",
        "invariant",
        "acyclic((if flag then r else q).add(0, 1))",
        "acyclic",
    );
}

#[test]
fn acyclic_rejects_conditional_remove_receiver_selecting_nonself_relation() {
    reject(
        "acyclic-remove",
        "invariant",
        "acyclic((if flag then r else q).remove(0, 1))",
        "acyclic",
    );
}

#[test]
fn reachable_rejects_conditional_add_receiver_selecting_nonself_relation() {
    reject(
        "reachable-add",
        "invariant",
        "reachable((if flag then r else q).add(0, 1), 0, 1)",
        "reachable",
    );
}

#[test]
fn reachable_rejects_conditional_remove_receiver_selecting_nonself_relation() {
    reject(
        "reachable-remove",
        "invariant",
        "not reachable((if flag then r else q).remove(0, 1), 0, 1)",
        "reachable",
    );
}

#[test]
fn acyclic_rejects_old_conditional_nonself_relation_in_ensures() {
    reject(
        "acyclic-old-ensures",
        "ensures",
        "acyclic(old(if flag then r else q))",
        "acyclic",
    );
}

#[test]
fn reachable_rejects_old_conditional_nonself_relation_in_ensures() {
    reject(
        "reachable-old-ensures",
        "ensures",
        "not reachable(old(if flag then r else q), 0, 1)",
        "reachable",
    );
}

#[test]
fn reachable_rejects_conditional_nonself_let_alias_in_requires() {
    reject(
        "reachable-let-requires",
        "requires-alias",
        "not reachable(chosen, 0, 1)",
        "reachable",
    );
}

#[test]
fn genuine_self_relation_conditional_update_receivers_still_check() {
    for (label, expression) in [
        (
            "valid-acyclic-add",
            "acyclic((if flag then r else q).add(0, 1))",
        ),
        (
            "valid-acyclic-remove",
            "acyclic((if flag then r else q).remove(0, 1))",
        ),
        (
            "valid-reachable-add",
            "reachable((if flag then r else q).add(0, 1), 0, 1)",
        ),
        (
            "valid-reachable-remove",
            "not reachable((if flag then r else q).remove(0, 1), 0, 1)",
        ),
    ] {
        accept(label, "invariant", expression);
    }
}

#[test]
fn genuine_self_relation_old_conditionals_in_ensures_still_check() {
    for (label, expression) in [
        (
            "valid-acyclic-old-ensures",
            "acyclic(old(if flag then r else q))",
        ),
        (
            "valid-reachable-old-ensures",
            "not reachable(old(if flag then r else q), 0, 1)",
        ),
    ] {
        accept(label, "ensures", expression);
    }
}

#[test]
fn genuine_self_relation_let_alias_in_requires_still_checks() {
    accept(
        "valid-reachable-let-requires",
        "requires-alias",
        "not reachable(chosen, 0, 1)",
    );
}

fn range_source(predicate: &str) -> String {
    format!(
        r"spec RangeOrigin {{
  enum Color {{ Red, Blue }}
  type N = 0..2
  state {{ n: N }}
  init {{ n = 0 }}
  action stay() {{ }}
  invariant P {{ {predicate} }}
}}
"
    )
}

fn reject_range_bound(label: &str, predicate: &str, parts: &[&str]) {
    let (value, status) = check(label, &range_source(predicate));
    assert_eq!(
        (status, value["result"].as_str(), value["kind"].as_str()),
        (2, Some("error"), Some("semantics")),
        "{label}: {value:#}"
    );
    let message = value["message"].as_str().expect("range-bound diagnostic");
    for part in parts {
        assert!(
            message.contains(part),
            "{label}: missing {part:?}: {value:#}"
        );
    }
    assert_eq!(
        value["loc"],
        json!({"line": 7, "column": 3}),
        "{label}: {value:#}"
    );
}

#[test]
fn aggregate_projection_range_bound_has_authored_property_origin() {
    reject_range_bound(
        "range-projection",
        "count(i in 0..domain(n).size() where true) == 0",
        &[
            "domain() requires a relation",
            "hint: use a relation A -> B operand",
        ],
    );
}

#[test]
fn aggregate_boolean_range_bound_has_authored_property_origin() {
    reject_range_bound(
        "range-bool",
        "count(i in 0..true where true) == 0",
        &["expression of type Bool is not assignable to Int"],
    );
}

#[test]
fn quantified_enum_range_bound_has_authored_property_origin() {
    reject_range_bound(
        "range-enum",
        "forall i in 0..Red { true }",
        &["expression of type Named(\"Color\") is not assignable to Int"],
    );
}

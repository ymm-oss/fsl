// SPDX-License-Identifier: Apache-2.0

//! #1459: operand/signature validation also covers action statements and contracts.
//! The frozen #1458 binary supplies red calibration; never exercise Kernel export.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const EMPTY_AT: &str = "spec EmptyAt { state { q: Seq<Int, 2>, n: Int } init { q = Seq {} n = 0 } action Touch() { n = q.at() } invariant Safe { true } }";
const PREFIX: &str = r"spec StatementOperand {
  enum Color { Red, Blue }
  type N = 0..2
  state { q: Seq<N, 2>, n: N, flag: Bool }
  init { q = Seq { 0 } n = 0 flag = false }
  action Touch() {
";
const SUFFIX: &str = "  }\n  invariant Safe { true }\n}\n";
const VALID_BODY: &str = r"spec ValidStatementSignatures {
  type N = 0..2
  state { q: Seq<N, 2>, n: N, s: Set<N>, r: relation N -> N, flag: Bool }
  init { q = Seq { 0 } n = 0 s = Set {} r = Set {} flag = false }
  action Touch() {
    requires q.size() == 1
    let item = q.at(0)
    let first = q.head()
    let has_item = q.contains(0)
    let pushed = q.push(1)
    let popped = q.pop()
    let set_has = s.contains(0)
    let set_added = s.add(0)
    let set_removed = s.remove(0)
    let set_size = s.size()
    let rel_has = r.contains(0, 1)
    let rel_added = r.add(0, 1)
    let rel_removed = r.remove(0, 1)
    n = item
    q = pushed.pop()
    s = set_added.remove(0)
    r = rel_added.remove(0, 1)
    flag = first == item and has_item == (item == 0) and popped.size() == 0 and not set_has and set_removed.size() == set_size and not rel_has and rel_removed == r
    ensures q.size() == 1 and s.size() == 0 and not r.contains(0, 1)
  }
  invariant Safe { true }
}
";

fn source(position: &str, expression: &str) -> String {
    let body = match position {
        "assignment" => format!("    {expression}\n"),
        "requires" => format!("    requires {expression}\n    n = n\n"),
        "ensures" => format!("    n = n\n    ensures {expression}\n"),
        _ => panic!("unknown statement position {position}"),
    };
    format!("{PREFIX}{body}{SUFFIX}")
}

fn run(label: &str, text: &str, mode: &str) -> (Value, i32) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let directory = root
        .join(".codex/tmp/issue_1459_statement_signatures")
        .join(label);
    std::fs::create_dir_all(&directory).expect("create owned statement fixture directory");
    std::fs::write(directory.join("statement.fsl"), text).expect("write statement fixture");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command.args([
        if mode == "check" { "check" } else { "verify" },
        "statement.fsl",
    ]);
    if mode != "check" {
        command.args(["--depth", "1", "--engine", mode, "--no-cache"]);
    }
    let output = command
        .current_dir(directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run statement check");
    assert!(output.stderr.is_empty(), "{label} {mode}: {output:?}");
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{label} {mode}: invalid JSON {error}: {output:?}"));
    (value, output.status.code().expect("native process exit"))
}

fn assert_rejected(label: &str, text: &str, parts: &[&str], location: &Value) {
    let (value, status) = run(label, text, "check");
    assert_eq!(
        (status, value["result"].as_str(), value["kind"].as_str()),
        (2, Some("error"), Some("semantics")),
        "{label}: {value:#}"
    );
    let message = value["message"]
        .as_str()
        .expect("static semantic diagnostic");
    for part in parts {
        assert!(
            message.contains(part),
            "{label}: missing {part:?}: {value:#}"
        );
    }
    assert_eq!(&value["loc"], location, "{label}: {value:#}");
}

fn reject(label: &str, position: &str, expression: &str, parts: &[&str]) {
    let text = source(position, expression);
    let location = match position {
        "assignment" => json!({"line": 7, "column": 5}),
        "requires" | "ensures" => json!({"line": 6, "column": 3}),
        _ => panic!("unknown statement position {position}"),
    };
    assert_rejected(label, &text, parts, &location);
}

#[test]
fn empty_at_action_assignment_is_a_located_static_arity_error() {
    // Assignment diagnostics use the parser's authored target span. There is
    // no independent method-call span in the shared Kernel expression AST.
    assert_rejected(
        "exact-empty-at",
        EMPTY_AT,
        &["invalid Seq method 'at'/0"],
        &json!({"line": 1, "column": 92}),
    );
}

#[test]
fn action_assignment_rejects_non_option_pattern() {
    reject(
        "assign-option",
        "assignment",
        "flag = n is none",
        &["'is none/some' requires an Option expression"],
    );
}

#[test]
fn requires_rejects_non_option_pattern() {
    reject(
        "requires-option",
        "requires",
        "n is none",
        &["'is none/some' requires an Option expression"],
    );
}

#[test]
fn ensures_rejects_non_option_pattern() {
    reject(
        "ensures-option",
        "ensures",
        "n is none",
        &["'is none/some' requires an Option expression"],
    );
}

#[test]
fn action_assignment_rejects_enum_abs() {
    reject(
        "assign-abs",
        "assignment",
        "n = abs(Red)",
        &["expression of type Named(\"Color\") is not assignable to Int"],
    );
}

#[test]
fn requires_rejects_enum_abs() {
    reject(
        "requires-abs",
        "requires",
        "abs(Red) == 0",
        &["expression of type Named(\"Color\") is not assignable to Int"],
    );
}

#[test]
fn ensures_rejects_enum_abs() {
    reject(
        "ensures-abs",
        "ensures",
        "abs(Red) == 0",
        &["expression of type Named(\"Color\") is not assignable to Int"],
    );
}

#[test]
fn action_assignment_rejects_non_relation_function() {
    reject(
        "assign-relation",
        "assignment",
        "flag = acyclic(n)",
        &[
            "acyclic() requires a relation",
            "hint: use a relation A -> B operand",
        ],
    );
}

#[test]
fn requires_rejects_non_relation_function() {
    reject(
        "requires-relation",
        "requires",
        "acyclic(n)",
        &[
            "acyclic() requires a relation",
            "hint: use a relation A -> B operand",
        ],
    );
}

#[test]
fn ensures_rejects_non_relation_function() {
    reject(
        "ensures-relation",
        "ensures",
        "acyclic(n)",
        &[
            "acyclic() requires a relation",
            "hint: use a relation A -> B operand",
        ],
    );
}

#[test]
fn action_assignment_rejects_invalid_method_receiver() {
    reject(
        "assign-receiver",
        "assignment",
        "flag = n.contains(0)",
        &["method receiver has no supported collection methods"],
    );
}

#[test]
fn requires_rejects_invalid_method_receiver() {
    reject(
        "requires-receiver",
        "requires",
        "n.contains(0)",
        &["method receiver has no supported collection methods"],
    );
}

#[test]
fn ensures_rejects_invalid_method_receiver() {
    reject(
        "ensures-receiver",
        "ensures",
        "n.contains(0)",
        &["method receiver has no supported collection methods"],
    );
}

#[test]
fn action_assignment_rejects_wrong_method_arity() {
    reject(
        "assign-arity",
        "assignment",
        "q = q.pop(1)",
        &["invalid Seq method 'pop'/1"],
    );
}

#[test]
fn requires_rejects_wrong_method_arity() {
    reject(
        "requires-arity",
        "requires",
        "q.pop(1).size() == 0",
        &["invalid Seq method 'pop'/1"],
    );
}

#[test]
fn ensures_rejects_wrong_method_arity() {
    reject(
        "ensures-arity",
        "ensures",
        "q.pop(1).size() == 0",
        &["invalid Seq method 'pop'/1"],
    );
}

#[test]
fn well_typed_action_body_collection_signatures_still_pass() {
    for mode in ["check", "bmc", "explicit"] {
        let (value, status) = run("valid-body", VALID_BODY, mode);
        assert_eq!(status, 0, "valid body {mode}: {value:#}");
        if mode == "check" {
            assert_eq!(value["result"], "ok", "{value:#}");
        } else {
            assert!(
                matches!(value["result"].as_str(), Some("verified" | "proved")),
                "{value:#}"
            );
        }
        assert!(
            value.get("kind").is_none() && value.get("message").is_none(),
            "{value:#}"
        );
    }
}

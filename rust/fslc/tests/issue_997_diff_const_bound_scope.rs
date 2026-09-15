// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

fn write(path: &Path, source: &str) {
    std::fs::write(path, source).expect("write fixture");
}

fn run(args: &[String]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn diff_forbid_scope_changed(old: &Path, new: &Path) -> (Value, i32) {
    run(&[
        "diff".to_owned(),
        old.display().to_string(),
        new.display().to_string(),
        "--depth".to_owned(),
        "1".to_owned(),
        "--forbid".to_owned(),
        "scope_changed".to_owned(),
    ])
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let scratch = std::env::temp_dir().join(format!("fslc-issue-997-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("create scratch directory");
    scratch
}

// #997: `fslc diff` recorded a `verify { values X = lo..hi }` bound in
// `scope.old`/`scope.new` only when `lo`/`hi` were bare integer literals,
// silently dropping any other expression -- including a reference to a
// declared `const`. That made a const-only bound change invisible to
// `scope_changed` and to `--forbid scope_changed`, and it was never applied
// to OLD. The fix resolves `values` bounds against the same evaluated
// `TypeDef::Domain` `check` uses, so the recorded bound no longer depends on
// the bound expression's surface shape.
#[test]
fn resolvable_const_bound_scope_change_is_detected_and_forbidden() {
    let scratch = scratch("const-bound");
    let old = scratch.join("old.fsl");
    let new = scratch.join("new.fsl");
    write(
        &old,
        r"requirements ConstBound {
  number Amount
  const LO = 0
  const HI = 2
  state { amount: Amount }
  init { amount = 0 }
}
verify { values Amount = LO..HI }
",
    );
    write(
        &new,
        r"requirements ConstBound {
  number Amount
  const LO = 0
  const HI = 5
  state { amount: Amount }
  init { amount = 0 }
}
verify { values Amount = LO..HI }
",
    );

    let (result, status) = diff_forbid_scope_changed(&old, &new);
    assert_eq!(status, 1, "{result}");
    assert_eq!(result["scope"]["old"]["values"], json!({"Amount": [0, 2]}));
    assert_eq!(result["scope"]["new"]["values"], json!({"Amount": [0, 5]}));
    assert_eq!(
        result["scope"]["applied_to_old"]["values"],
        json!({"Amount": [0, 5]})
    );
    assert_eq!(result["summary"], json!(["scope_changed"]));
    assert!(result["findings"].as_array().is_some_and(|findings| {
        findings
            .iter()
            .any(|finding| finding["kind"] == "scope_changed")
    }));
    assert_eq!(result["gate"]["passed"], false);
    assert_eq!(result["gate"]["violations"], json!(["scope_changed"]));

    std::fs::remove_dir_all(scratch).expect("remove scratch directory");
}

// A partial fix that only special-cases a named `const` reference (e.g.
// matching `Expr::Var`) still misses `-1..HI`: the lower bound is
// `Expr::Neg`, a different AST shape carrying the same "not a bare integer
// literal" problem. Only resolving through the model's evaluated domain --
// which does not inspect the expression's shape at all -- catches both.
#[test]
fn mixed_negative_literal_and_named_const_bound_is_detected() {
    let scratch = scratch("neg-bound");
    let old = scratch.join("old.fsl");
    let new = scratch.join("new.fsl");
    write(
        &old,
        r"requirements NegBound {
  number Amount
  const HI = 2
  state { amount: Amount }
  init { amount = 0 }
}
verify { values Amount = -1..HI }
",
    );
    write(
        &new,
        r"requirements NegBound {
  number Amount
  const HI = 5
  state { amount: Amount }
  init { amount = 0 }
}
verify { values Amount = -1..HI }
",
    );

    let (result, status) = diff_forbid_scope_changed(&old, &new);
    assert_eq!(status, 1, "{result}");
    assert_eq!(result["scope"]["old"]["values"], json!({"Amount": [-1, 2]}));
    assert_eq!(result["scope"]["new"]["values"], json!({"Amount": [-1, 5]}));
    assert_eq!(
        result["scope"]["applied_to_old"]["values"],
        json!({"Amount": [-1, 5]})
    );
    assert_eq!(result["gate"]["passed"], false);

    std::fs::remove_dir_all(scratch).expect("remove scratch directory");
}

// Preservation: an unchanged const bound must not be reported as changed
// merely because it is now evaluated instead of pattern-matched.
#[test]
fn unchanged_const_bound_self_diff_is_not_scope_changed() {
    let scratch = scratch("const-bound-self");
    let source = scratch.join("spec.fsl");
    write(
        &source,
        r"requirements ConstBound {
  number Amount
  const LO = 0
  const HI = 2
  state { amount: Amount }
  init { amount = 0 }
}
verify { values Amount = LO..HI }
",
    );

    let (result, status) = diff_forbid_scope_changed(&source, &source);
    assert_eq!(status, 0, "{result}");
    assert_eq!(result["summary"], json!(["no_semantic_change"]));
    assert_eq!(result["scope"]["old"]["values"], json!({"Amount": [0, 2]}));
    assert_eq!(result["gate"]["passed"], true);

    std::fs::remove_dir_all(scratch).expect("remove scratch directory");
}

// Preservation (issue #1058, intentionally untouched by #997): an
// undeclared `values` name keeps its established literal fallback /
// const-skip split. This is not the single-owner evaluator path -- the name
// has no `TypeDef::Domain` in the model at all -- and #997 must not change
// it either way.
#[test]
fn undeclared_values_name_keeps_established_literal_fallback_and_const_skip() {
    let scratch = scratch("undeclared-name");
    let literal_old = scratch.join("literal-old.fsl");
    let literal_new = scratch.join("literal-new.fsl");
    write(
        &literal_old,
        r"requirements GhostLiteral {
  number Amount
  const LO = 0
  const HI = 2
  state { amount: Amount }
  init { amount = 0 }
}
verify { values Amount = LO..HI; values Ghost = 1..3 }
",
    );
    write(
        &literal_new,
        r"requirements GhostLiteral {
  number Amount
  const LO = 0
  const HI = 2
  state { amount: Amount }
  init { amount = 0 }
}
verify { values Amount = LO..HI; values Ghost = 1..4 }
",
    );
    let (literal_result, status) = diff_forbid_scope_changed(&literal_old, &literal_new);
    assert_eq!(status, 1, "{literal_result}");
    assert_eq!(
        literal_result["scope"]["old"]["values"],
        json!({"Amount": [0, 2], "Ghost": [1, 3]})
    );
    assert_eq!(
        literal_result["scope"]["new"]["values"],
        json!({"Amount": [0, 2], "Ghost": [1, 4]})
    );

    let const_bound = scratch.join("const-bound.fsl");
    write(
        &const_bound,
        r"requirements GhostConst {
  number Amount
  const LO = 0
  const HI = 2
  state { amount: Amount }
  init { amount = 0 }
}
verify { values Amount = LO..HI; values Ghost = LO..HI }
",
    );
    let (const_result, status) = diff_forbid_scope_changed(&const_bound, &const_bound);
    assert_eq!(status, 0, "{const_result}");
    assert_eq!(
        const_result["scope"]["old"]["values"],
        json!({"Amount": [0, 2]}),
        "undeclared 'Ghost' with a non-literal bound stays absent from scope: {const_result}"
    );

    std::fs::remove_dir_all(scratch).expect("remove scratch directory");
}

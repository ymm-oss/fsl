// SPDX-License-Identifier: Apache-2.0

//! #1458: relation projections preserve their declared endpoint types.
//! Nine detectors restore valid enum uses or reject enum/Int confusion. Seven
//! controls preserve numeric projections, including a real invariant violation.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const DOMAIN_CONTAINS: &str = r"spec P {
  enum Color { Red, Blue }
  state { r: relation Color -> Color }
  init { r = Set {} }
  action link(a: Color, b: Color) { r = r.add(a, b) }
  invariant P { forall x in domain(r): not r.contains(x, Blue) }
}
";

const RANGE_CONTAINS: &str = r"spec P {
  enum Color { Red, Blue }
  type N = 0..2
  state { r: relation N -> Color }
  init { r = Set {} }
  action link(a: N, b: Color) { r = r.add(a, b) }
  invariant P { forall y in range(r): not r.contains(1, y) }
}
";

const DOMAIN_CYCLE: &str = r"spec P {
  enum Color { Red, Blue }
  state { r: relation Color -> Color }
  init { r = Set {} }
  action link(a: Color, b: Color) { r = r.add(a, b) }
  invariant P { not (exists x in domain(r): reachable(r, x, x)) }
}
";

const ENDPOINT_BYPASS: &str = r"spec P {
  enum Color { Red, Blue }
  type N = 0..2
  type M = 0..9
  state { s: relation Color -> Color, r: relation N -> N }
  init { s = Set {}
         r = Set {} }
  action links(a: Color, b: Color) { s = s.add(a, b) }
  action linkr(a: N, b: N) { r = r.add(a, b) }
  invariant P { forall x in domain(s): not r.contains(x, x) }
}
";

const INTEGER_MEMBERSHIP: &str = r"spec P {
  enum Color { Red, Blue }
  state { s: relation Color -> Color }
  init { s = Set {} }
  action link(a: Color, b: Color) { s = s.add(a, b) }
  invariant P { not domain(s).contains(0) }
}
";

const INTEGER_COMPARISON: &str = r"spec P {
  enum Color { Red, Blue }
  state { s: relation Color -> Color }
  init { s = Set {} }
  action link(a: Color, b: Color) { s = s.add(a, b) }
  invariant P { forall x in domain(s): x != 0 }
}
";

fn numeric_source(relation: &str, predicate: &str) -> String {
    format!(
        r"spec NumericProjection {{
  enum Color {{ Red, Blue }}
  type N = 0..2
  state {{ r: relation {relation} }}
  init {{ r = Set {{}} }}
  action stay() {{ }}
  invariant P {{ {predicate} }}
}}
"
    )
}

/// Collect every response before asserting: an erroneous check must not hide
/// verification behavior. Depth two also exposes the two-action endpoint bypass.
fn run(label: &str, source: &str) -> [(&'static str, Value, i32); 3] {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1458_domain_range_types")
        .join(label);
    std::fs::create_dir_all(&directory).expect("create per-test projection fixture directory");
    std::fs::write(directory.join("projection.fsl"), source).expect("write projection fixture");

    ["check", "bmc", "explicit"].map(|mode| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.args([
            if mode == "check" { "check" } else { "verify" },
            "projection.fsl",
        ]);
        if mode != "check" {
            command.args(["--engine", mode, "--depth", "2", "--no-cache"]);
        }
        let output = command
            .current_dir(&directory)
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run native projection check");
        assert!(
            output.stderr.is_empty(),
            "{label} {mode}: unexpected stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{label} {mode}: invalid JSON ({error}): stdout={}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        (
            mode,
            envelope,
            output.status.code().expect("native exit status"),
        )
    })
}

fn assert_contract(
    label: &str,
    mode: &str,
    output: &Value,
    status: i32,
    expected: (i32, &str, Option<&str>),
    outcomes: &Value,
) {
    assert_eq!(
        (status, output["result"].as_str(), output["kind"].as_str()),
        (expected.0, Some(expected.1), expected.2),
        "{label} {mode}: observed all commands: {outcomes}"
    );
}

fn outcomes(outputs: &[(&str, Value, i32); 3]) -> Value {
    json!(outputs.each_ref().map(|(mode, output, status)| {
        json!({"command": mode, "exit": status, "result": output["result"], "kind": output.get("kind")})
    }))
}

fn assert_violation(label: &str, source: &str, step: u64, line: u64) {
    let outputs = run(label, source);
    let observed = outcomes(&outputs);
    for (mode, output, status) in outputs {
        let expected = if mode == "check" {
            (0, "ok", None)
        } else {
            (1, "violated", None)
        };
        assert_contract(label, mode, &output, status, expected, &observed);
        assert!(
            output.get("message").is_none(),
            "{label} {mode}: {output:#}"
        );
        if mode != "check" {
            assert_eq!(output["violation_kind"], "invariant", "{label}: {output:#}");
            assert_eq!(output["invariant"], "P", "{label}: {output:#}");
            assert_eq!(output["violated_at_step"], step, "{label}: {output:#}");
            assert_eq!(
                output["loc"],
                json!({"line": line, "column": 3}),
                "{label}: {output:#}"
            );
        }
    }
}

fn assert_rejected(label: &str, source: &str, endpoint_bypass: bool) {
    let outputs = run(label, source);
    let observed = outcomes(&outputs);
    let (mismatch, line) = if endpoint_bypass {
        (
            "expression of type Named(\"Color\") is not assignable to Named(\"N\")",
            10,
        )
    } else {
        (
            "expression of type Int is not assignable to Named(\"Color\")",
            6,
        )
    };
    for (mode, output, status) in outputs {
        assert_contract(
            label,
            mode,
            &output,
            status,
            (2, "error", Some("semantics")),
            &observed,
        );
        let message = output["message"].as_str().expect("semantic error message");
        assert!(message.contains(mismatch), "{label} {mode}: {output:#}");
        if endpoint_bypass {
            assert!(
                message.contains(
                    "hint: use endpoints matching the relation's declared source and target types"
                ),
                "{label} {mode}: {output:#}"
            );
        }
        assert_eq!(
            output["loc"],
            json!({"line": line, "column": 3}),
            "{label} {mode}: {output:#}"
        );
    }
}

fn assert_numeric_projection(label: &str, relation: &str, predicate: &str) {
    let outputs = run(label, &numeric_source(relation, predicate));
    let observed = outcomes(&outputs);
    for (mode, output, status) in outputs {
        let expected = match mode {
            "check" => "ok",
            "bmc" => "verified",
            "explicit" => "proved",
            _ => unreachable!("fixed command inventory"),
        };
        assert_contract(label, mode, &output, status, (0, expected, None), &observed);
        assert!(
            output.get("message").is_none(),
            "{label} {mode}: {output:#}"
        );
        if mode == "bmc" {
            assert_eq!(output["completeness"], "bounded", "{label}: {output:#}");
            assert_eq!(output["checked_to_depth"], 2, "{label}: {output:#}");
        } else if mode == "explicit" {
            assert_eq!(output["closure"], true, "{label}: {output:#}");
        }
    }
}

#[test]
fn enum_domain_binder_preserves_contains_endpoint_type() {
    assert_violation("enum-domain-contains", DOMAIN_CONTAINS, 1, 6);
}

#[test]
fn enum_range_binder_preserves_heterogeneous_target_type() {
    assert_violation("enum-range-contains", RANGE_CONTAINS, 1, 7);
}

#[test]
fn enum_domain_cycle_idiom_preserves_reachable_endpoint_type() {
    assert_violation("enum-domain-cycle", DOMAIN_CYCLE, 1, 6);
}

#[test]
fn enum_domain_binder_cannot_bypass_numeric_relation_endpoint_check() {
    assert_rejected("enum-domain-bypass", ENDPOINT_BYPASS, true);
}

#[test]
fn enum_range_binder_cannot_bypass_numeric_relation_endpoint_check() {
    assert_rejected(
        "enum-range-bypass",
        &ENDPOINT_BYPASS.replace("domain(s)", "range(s)"),
        true,
    );
}

#[test]
fn enum_domain_rejects_integer_membership() {
    assert_rejected("enum-domain-integer-member", INTEGER_MEMBERSHIP, false);
}

#[test]
fn enum_range_rejects_integer_membership() {
    assert_rejected(
        "enum-range-integer-member",
        &INTEGER_MEMBERSHIP.replace("domain(s)", "range(s)"),
        false,
    );
}

#[test]
fn enum_domain_binder_rejects_integer_comparison() {
    assert_rejected("enum-domain-integer-comparison", INTEGER_COMPARISON, false);
}

#[test]
fn enum_range_binder_rejects_integer_comparison() {
    assert_rejected(
        "enum-range-integer-comparison",
        &INTEGER_COMPARISON.replace("domain(s)", "range(s)"),
        false,
    );
}

#[test]
fn numeric_relation_domain_still_accepts_integer_membership() {
    assert_numeric_projection(
        "numeric-domain-member",
        "N -> N",
        "domain(r.add(0, 1)).contains(0)",
    );
}

#[test]
fn numeric_relation_range_still_accepts_integer_membership() {
    assert_numeric_projection(
        "numeric-range-member",
        "N -> N",
        "range(r.add(0, 1)).contains(1)",
    );
}

#[test]
fn numeric_relation_domain_binder_still_compares_with_integer() {
    assert_numeric_projection(
        "numeric-domain-integer-comparison",
        "N -> N",
        "forall x in domain(r.add(0, 1)): x == 0",
    );
}

#[test]
fn numeric_relation_range_binder_still_compares_with_integer() {
    assert_numeric_projection(
        "numeric-range-integer-comparison",
        "N -> N",
        "forall y in range(r.add(0, 1)): y == 1",
    );
}

#[test]
fn heterogeneous_relation_domain_uses_numeric_source() {
    assert_numeric_projection(
        "numeric-heterogeneous-domain",
        "N -> Color",
        "domain(r.add(1, Red)).contains(1)",
    );
}

#[test]
fn heterogeneous_relation_range_uses_numeric_target() {
    assert_numeric_projection(
        "numeric-heterogeneous-range",
        "Color -> N",
        "range(r.add(Red, 1)).contains(1)",
    );
}

#[test]
fn numeric_projection_still_detects_negated_present_member() {
    assert_violation(
        "numeric-negated-projection",
        &numeric_source("N -> N", "not domain(r.add(0, 1)).contains(0)"),
        0,
        7,
    );
}

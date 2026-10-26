// SPDX-License-Identifier: Apache-2.0

//! #1458: relation endpoints must match their independently declared types.
//! Fifteen detectors exercise evaluated expressions, not dead branches. Fourteen
//! preservation controls protect valid relation operations and real violations.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

const COLOR_TO_N: &str = "expression of type Named(\"Color\") is not assignable to Named(\"N\")";
const INT_TO_COLOR: &str = "expression of type Int is not assignable to Named(\"Color\")";
const ENDPOINT_HINT: &str =
    "hint: use endpoints matching the relation's declared source and target types";

/// Keep the invariant's authored origin at 7:3 for all endpoint diagnostics.
fn source(relation: &str, predicate: &str) -> String {
    format!(
        r"spec RelEndpoint {{
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

/// Collect both responses before asserting either: a false-green check must not
/// prevent the detector from also exercising the evaluated verify expression.
fn run(label: &str, relation: &str, predicate: &str) -> [(&'static str, Value, i32); 2] {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1458_relation_endpoint_types")
        .join(label);
    std::fs::create_dir_all(&directory).expect("create per-test relation fixture directory");
    std::fs::write(directory.join("relation.fsl"), source(relation, predicate))
        .expect("write inline relation fixture");

    ["check", "verify"].map(|verb| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
        command.args([verb, "relation.fsl"]);
        if verb == "verify" {
            command.args(["--depth", "1"]);
        }
        let output = command
            .current_dir(&directory)
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run native relation endpoint check");
        assert!(
            output.stderr.is_empty(),
            "{label} {verb}: unexpected stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{label} {verb}: invalid JSON ({error}): stdout={}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
        (
            verb,
            envelope,
            output.status.code().expect("native exit status"),
        )
    })
}

fn assert_rejected(label: &str, relation: &str, predicate: &str, mismatch: &str) {
    let outputs = run(label, relation, predicate);
    let outcomes = outputs.each_ref().map(|(verb, output, status)| {
        json!({"command": verb, "exit": status, "result": output["result"], "kind": output.get("kind")})
    });
    for (verb, output, status) in &outputs {
        assert_eq!(
            (*status, output["result"].as_str(), output["kind"].as_str()),
            (2, Some("error"), Some("semantics")),
            "{label} {verb}: observed both commands: {outcomes:?}"
        );
        let message = output["message"].as_str().expect("semantic error message");
        assert!(message.contains(mismatch), "{label} {verb}: {output:#}");
        assert!(
            message.contains(ENDPOINT_HINT),
            "{label} {verb}: {output:#}"
        );
        assert_eq!(
            output["loc"],
            json!({"line": 7, "column": 3}),
            "{label} {verb}: {output:#}"
        );
    }
}

fn assert_verified(label: &str, relation: &str, predicate: &str) {
    for (verb, output, status) in run(label, relation, predicate) {
        let expected_result = if verb == "check" { "ok" } else { "verified" };
        assert_eq!(
            (status, output["result"].as_str()),
            (0, Some(expected_result)),
            "{label} {verb}: {output:#}"
        );
        assert!(output.get("kind").is_none(), "{label} {verb}: {output:#}");
        assert!(
            output.get("message").is_none(),
            "{label} {verb}: {output:#}"
        );
        if verb == "verify" {
            assert_eq!(output["completeness"], "bounded", "{label}: {output:#}");
            assert_eq!(output["checked_to_depth"], 1, "{label}: {output:#}");
        }
    }
}

#[test]
fn reachable_rejects_evaluated_enum_endpoints_on_numeric_relation() {
    assert_rejected(
        "reachable-evaluated-enums",
        "N -> N",
        "not reachable(r, Red, Blue)",
        COLOR_TO_N,
    );
}

#[test]
fn contains_rejects_evaluated_enum_endpoints_on_numeric_relation() {
    assert_rejected(
        "contains-evaluated-enums",
        "N -> N",
        "not r.contains(Red, Blue)",
        COLOR_TO_N,
    );
}

#[test]
fn add_contains_rejects_evaluated_enum_pair_on_numeric_relation() {
    assert_rejected(
        "add-contains-evaluated-enums",
        "N -> N",
        "r.add(Red, Blue).contains(Red, Blue)",
        COLOR_TO_N,
    );
}

#[test]
fn remove_rejects_evaluated_enum_endpoints_on_numeric_relation() {
    assert_rejected(
        "remove-evaluated-enums",
        "N -> N",
        "r.remove(Red, Blue) == r",
        COLOR_TO_N,
    );
}

#[test]
fn reachable_rejects_wrong_source_endpoint() {
    assert_rejected(
        "reachable-source",
        "N -> N",
        "not reachable(r, Red, 1)",
        COLOR_TO_N,
    );
}

#[test]
fn reachable_rejects_wrong_target_endpoint() {
    assert_rejected(
        "reachable-target",
        "N -> N",
        "not reachable(r, 1, Blue)",
        COLOR_TO_N,
    );
}

#[test]
fn contains_rejects_wrong_source_endpoint() {
    assert_rejected(
        "contains-source",
        "N -> N",
        "not r.contains(Red, 1)",
        COLOR_TO_N,
    );
}

#[test]
fn contains_rejects_wrong_target_endpoint() {
    assert_rejected(
        "contains-target",
        "N -> N",
        "not r.contains(1, Blue)",
        COLOR_TO_N,
    );
}

#[test]
fn add_rejects_wrong_source_endpoint_without_contains() {
    assert_rejected("add-source", "N -> N", "r.add(Red, 1) != r", COLOR_TO_N);
}

#[test]
fn add_rejects_wrong_target_endpoint_without_contains() {
    assert_rejected("add-target", "N -> N", "r.add(1, Blue) != r", COLOR_TO_N);
}

#[test]
fn remove_rejects_wrong_source_endpoint() {
    assert_rejected(
        "remove-source",
        "N -> N",
        "r.remove(Red, 1) == r",
        COLOR_TO_N,
    );
}

#[test]
fn remove_rejects_wrong_target_endpoint() {
    assert_rejected(
        "remove-target",
        "N -> N",
        "r.remove(1, Blue) == r",
        COLOR_TO_N,
    );
}

#[test]
fn heterogeneous_contains_rejects_numeric_enum_target() {
    assert_rejected(
        "heterogeneous-contains-target",
        "N -> Color",
        "not r.contains(1, 2)",
        INT_TO_COLOR,
    );
}

#[test]
fn heterogeneous_add_rejects_numeric_enum_target_without_contains() {
    assert_rejected(
        "heterogeneous-add-target",
        "N -> Color",
        "r.add(1, 2) != r",
        INT_TO_COLOR,
    );
}

#[test]
fn heterogeneous_remove_rejects_numeric_enum_target() {
    assert_rejected(
        "heterogeneous-remove-target",
        "N -> Color",
        "r.remove(1, 2) == r",
        INT_TO_COLOR,
    );
}

#[test]
fn well_typed_relation_contains_still_passes() {
    assert_verified("contains-valid", "N -> N", "not r.contains(1, 2)");
}

#[test]
fn well_typed_relation_add_still_passes() {
    assert_verified("add-valid", "N -> N", "r.add(1, 2).contains(1, 2)");
}

#[test]
fn well_typed_relation_remove_still_passes() {
    assert_verified("remove-valid", "N -> N", "r.add(1, 2).remove(1, 2) == r");
}

#[test]
fn well_typed_relation_reachable_still_passes() {
    assert_verified(
        "reachable-numeric-valid",
        "N -> N",
        "reachable(r.add(0, 1).add(1, 2), 0, 2)",
    );
    assert_verified(
        "reachable-enum-valid",
        "Color -> Color",
        "reachable(r.add(Red, Blue), Red, Blue)",
    );
}

#[test]
fn well_typed_relation_acyclic_still_passes() {
    assert_verified("acyclic-valid", "N -> N", "acyclic(r.add(0, 1))");
}

#[test]
fn well_typed_relation_functional_still_passes() {
    assert_verified("functional-valid", "N -> N", "functional(r.add(0, 1))");
}

#[test]
fn well_typed_relation_injective_still_passes() {
    assert_verified("injective-valid", "N -> N", "injective(r.add(0, 1))");
}

#[test]
fn well_typed_relation_domain_still_passes() {
    assert_verified(
        "domain-numeric-valid",
        "N -> N",
        "domain(r.add(0, 1)).contains(0)",
    );
    assert_verified(
        "domain-enum-valid",
        "Color -> Color",
        "domain(r.add(Red, Blue)).size() == 1 and range(r.add(Red, Blue)).size() == 1",
    );
}

#[test]
fn well_typed_relation_range_still_passes() {
    assert_verified(
        "range-numeric-valid",
        "N -> N",
        "range(r.add(0, 1)).contains(1)",
    );
    assert_verified(
        "range-enum-valid",
        "Color -> Color",
        "domain(r.add(Red, Blue)).size() == 1 and range(r.add(Red, Blue)).size() == 1",
    );
}

#[test]
fn empty_relation_literal_still_passes() {
    assert_verified("empty-literal-valid", "N -> N", "r == Set {}");
}

#[test]
fn relation_set_updates_remain_idempotent() {
    assert_verified(
        "set-updates-valid",
        "N -> N",
        "r.add(0, 1).add(0, 1).remove(2, 2) == r.add(0, 1)",
    );
}

#[test]
fn heterogeneous_relation_endpoints_preserve_their_independent_types() {
    assert_verified(
        "numeric-to-enum-valid",
        "N -> Color",
        "r.add(1, Red).contains(1, Red) and r.add(1, Red).remove(1, Red) == r",
    );
    assert_verified(
        "enum-to-numeric-valid",
        "Color -> N",
        "r.add(Red, 1).contains(Red, 1) and r.add(Red, 1).remove(Red, 1) == r",
    );
    assert_verified(
        "boolean-endpoints-valid",
        "Bool -> Bool",
        "r.add(false, true).contains(false, true)",
    );
}

#[test]
fn nested_relation_receivers_and_endpoints_still_pass() {
    assert_verified(
        "nested-receiver-valid",
        "N -> N",
        "(if true then r else r).add(1, 2).contains(1, 2)",
    );
    assert_verified(
        "nested-endpoint-valid",
        "N -> N",
        "r.add(if true then 1 else 2, 0 + 1).contains(1, 1)",
    );
}

#[test]
fn well_typed_stored_pair_still_violates_negated_membership() {
    for (verb, output, status) in run(
        "stored-pair-violation-valid",
        "N -> N",
        "not r.add(1, 2).contains(1, 2)",
    ) {
        let expected = if verb == "check" {
            (0, "ok")
        } else {
            (1, "violated")
        };
        assert_eq!(
            (status, output["result"].as_str()),
            (expected.0, Some(expected.1)),
            "{verb}: {output:#}"
        );
        assert!(output.get("kind").is_none(), "{verb}: {output:#}");
        assert!(output.get("message").is_none(), "{verb}: {output:#}");
        if verb == "verify" {
            assert_eq!(output["violation_kind"], "invariant", "{output:#}");
            assert_eq!(output["invariant"], "P", "{output:#}");
            assert_eq!(output["violated_at_step"], 0, "{output:#}");
        }
    }
}

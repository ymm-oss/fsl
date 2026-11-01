// SPDX-License-Identifier: Apache-2.0

//! Phase 2 only: install after the reviewed predicate API exists. Do not link
//! this file into the old-source semantic RED test build.

use fslc_rust::outcome::exit_status_matches;
use serde_json::{Value, json};

// Frozen from e8163bc6 outcome.rs's explicit registry, independent of the
// candidate's new predicate. `generated` is ordinary success at0 and has
// additional command+kind exceptions tested separately below.
const SUCCESSES: &[&str] = &[
    "ok",
    "verified",
    "proved",
    "refines",
    "conformant",
    "sweep_passed",
    "generated",
    "created",
    "imported",
    "imported_with_warnings",
    "analyzed",
    "expanded",
    "explained",
    "kernel",
    "typestate",
    "scenarios",
    "migrated",
    "compared",
    "compat_profile_generated",
    "conformance",
    "conformance_coverage",
    "testgen_trace",
    "testplan",
    "conformance_checked",
    "document_conformant",
    "observed_conformant",
    "replay_conformant",
    "observed_supported",
    "verified_under_assumptions",
    "agent_analyzed",
    "ai_project_analyzed",
    "causal_analyzed",
    "causal_model_checked",
    "causal_diffed",
    "causal_ledger",
    "causal_expectations_checked",
    "causal_expectations_observed",
    "statistically_supported",
    "skipped",
];
const CANONICAL_FAILURES: &[&str] = &[
    "violated",
    "reachable_failed",
    "unknown_cti",
    "unknown_budget",
    "refinement_failed",
    "impl_violated",
    "indeterminate",
];
const OTHER_FAILURES: &[&str] = &[
    "nonconformant",
    "sweep_failed",
    "sweep_inconclusive",
    "observed_mismatch",
    "replay_nonconformant",
    "document_drifted",
    "migration_refused",
    "statistically_unsupported",
    "dataset_invalid",
    "evaluator_untrusted",
    "slice_missing",
    "insufficient_samples",
    "inconclusive",
    "unknown",
];

fn statuses(command: &str, subcommand: Option<&str>, output: &Value, expected: [bool; 4]) {
    for (status, expected) in expected.into_iter().enumerate() {
        let status = i32::try_from(status).unwrap();
        assert_eq!(
            exit_status_matches(command, subcommand, output, status),
            expected,
            "command={command} subcommand={subcommand:?} status={status} output={output}"
        );
    }
}

#[test]
fn entire_registered_ordinary_catalog_obeys_its_exit_class() {
    assert_eq!(SUCCESSES.len(), 39);
    assert_eq!(CANONICAL_FAILURES.len(), 7);
    assert_eq!(OTHER_FAILURES.len(), 14);
    for result in SUCCESSES {
        statuses(
            "check",
            None,
            &json!({"result":result}),
            [true, false, false, false],
        );
    }
    for result in CANONICAL_FAILURES {
        statuses(
            "verify",
            None,
            &json!({"result":result}),
            [false, true, false, false],
        );
    }
    for result in OTHER_FAILURES {
        // Their producers own a nonzero code; this predicate does not coerce it.
        statuses(
            "check",
            None,
            &json!({"result":result}),
            [false, true, true, true],
        );
    }
    statuses(
        "migrate",
        None,
        &json!({"result":"migrate_manual_required"}),
        [false, true, false, false],
    );
    for output in [
        json!({}),
        json!({"result":null}),
        json!({"result":"unregistered-1418"}),
    ] {
        statuses("check", None, &output, [false; 4]);
    }
}

#[test]
fn ordinary_errors_two_and_internal_errors_three_are_exact() {
    for kind in ["usage", "parse", "type", "semantics", "io"] {
        statuses(
            "check",
            None,
            &json!({"result":"error","kind":kind}),
            [false, false, true, false],
        );
    }
    statuses(
        "check",
        None,
        &json!({"result":"error","kind":"internal"}),
        [false, false, false, true],
    );
}

#[test]
fn approval_statuses_are_closed_and_preserve_existing_gate_meanings() {
    for status in ["approved", "drifted"] {
        statuses(
            "approval",
            Some("check"),
            &json!({"result":"approval_check","status":status}),
            [true, false, false, false],
        );
    }
    statuses(
        "approval",
        Some("check"),
        &json!({"result":"approval_check","status":"signature-invalid"}),
        [false, true, false, false],
    );
    for output in [
        json!({"result":"approval_check"}),
        json!({"result":"approval_check","status":"unknown"}),
    ] {
        statuses("approval", Some("check"), &output, [false; 4]);
    }
    statuses(
        "approval",
        Some("diff"),
        &json!({"result":"approval_diff","status":"signature-invalid"}),
        [false, true, false, false],
    );
    for output in [
        json!({"result":"approval_diff"}),
        json!({"result":"approval_diff","status":"approved"}),
    ] {
        statuses("approval", Some("diff"), &output, [false; 4]);
    }
}

#[test]
fn sibling_verdict_fields_preserve_format_lint_diff_and_optional_mutation_gates() {
    for (output, success) in [
        (json!({"result":"format_check","changed":false}), true),
        (json!({"result":"format_check","changed":true}), false),
        (json!({"result":"lint","finding_count":0}), true),
        (json!({"result":"lint","finding_count":1}), false),
        (
            json!({"result":"semantic_diff","gate":{"passed":true}}),
            true,
        ),
        (
            json!({"result":"semantic_diff","gate":{"passed":false}}),
            false,
        ),
        (
            json!({
                "result":"semantic_diff_batch",
                "comparisons":[
                    {"result":"semantic_diff","gate":{"passed":true,"violations":[]}},
                    {"result":"semantic_diff","gate":{"passed":true,"violations":[]}}
                ],
                "gate":{"passed":true,"violations":[]}
            }),
            true,
        ),
        (
            json!({
                "result":"semantic_diff_batch",
                "comparisons":[
                    {"result":"semantic_diff","gate":{"passed":false,"violations":["impl_violated"]}},
                    {"result":"semantic_diff","gate":{"passed":true,"violations":[]}}
                ],
                "gate":{"passed":false,"violations":["impl_violated"]}
            }),
            false,
        ),
        (json!({"result":"mutated"}), true),
        (json!({"result":"mutated","gate":{"passed":true}}), true),
        (json!({"result":"mutated","gate":{"passed":false}}), false),
        (json!({"result":"mutated","gate":{}}), false),
        (json!({"result":"semantic_diff"}), false),
    ] {
        statuses(
            "check",
            None,
            &output,
            if success {
                [true, false, false, false]
            } else {
                [false, true, false, false]
            },
        );
    }
}

#[test]
fn batch_operational_errors_require_exactly_two_even_for_nested_internal_errors() {
    for kind in ["io", "parse", "type", "semantics", "internal"] {
        statuses(
            "diff",
            None,
            &json!({
                "result":"semantic_diff_batch",
                "comparisons":[
                    {"result":"error","kind":kind},
                    {"result":"error","kind":kind}
                ],
                "gate":{"passed":true,"violations":[]}
            }),
            [false, false, true, false],
        );
        // A failed semantic child remains reflected in the parent gate, but
        // the operational error wins the maximum exit, in either order.
        let semantic = json!({
            "result":"semantic_diff",
            "gate":{"passed":false,"violations":["impl_violated"]}
        });
        let error = json!({"result":"error","kind":kind});
        for comparisons in [json!([semantic, error]), json!([error, semantic])] {
            statuses(
                "diff",
                None,
                &json!({
                    "result":"semantic_diff_batch",
                    "comparisons":comparisons,
                    "gate":{"passed":false,"violations":["impl_violated"]}
                }),
                [false, false, true, false],
            );
        }
    }
}

#[test]
fn batch_rejects_missing_empty_malformed_or_unregistered_children() {
    for comparisons in [
        Value::Null,
        json!({}),
        json!("comparisons"),
        json!([]),
        json!([null]),
        json!([{}]),
        json!([{"result":null}]),
        json!([{"result":"unknown-1418"}]),
        json!([{"result":"semantic_diff"}]),
        json!([{"result":"semantic_diff","gate":{}}]),
        json!([{"result":"semantic_diff","gate":{"passed":"true"}}]),
        json!([{
            "result":"semantic_diff_batch",
            "comparisons":[{"result":"error","kind":"io"}],
            "gate":{"passed":true}
        }]),
    ] {
        statuses(
            "diff",
            None,
            &json!({"result":"semantic_diff_batch","comparisons":comparisons,"gate":{"passed":true}}),
            [false; 4],
        );
    }
    statuses(
        "diff",
        None,
        &json!({"result":"semantic_diff_batch","gate":{"passed":true}}),
        [false; 4],
    );
}

#[test]
fn batch_requires_a_boolean_parent_gate_consistent_with_semantic_children_only() {
    for gate in [Value::Null, json!({}), json!({"passed":"true"})] {
        statuses(
            "diff",
            None,
            &json!({
                "result":"semantic_diff_batch",
                "comparisons":[{"result":"semantic_diff","gate":{"passed":true,"violations":[]}}],
                "gate":gate
            }),
            [false; 4],
        );
    }
    for output in [
        json!({
            "result":"semantic_diff_batch",
            "comparisons":[{"result":"semantic_diff","gate":{"passed":true,"violations":[]}}]
        }),
        json!({
            "result":"semantic_diff_batch",
            "comparisons":[{"result":"semantic_diff","gate":{"passed":true,"violations":[]}}],
            "gate":{"passed":false}
        }),
        json!({
            "result":"semantic_diff_batch",
            "comparisons":[{"result":"semantic_diff","gate":{"passed":false,"violations":["impl_violated"]}}],
            "gate":{"passed":true}
        }),
        json!({
            "result":"semantic_diff_batch",
            "comparisons":[{"result":"error","kind":"io"}],
            "gate":{"passed":false}
        }),
    ] {
        statuses("diff", None, &output, [false; 4]);
    }
}

#[test]
fn generated_nonzero_exception_requires_both_exact_context_and_artifact_kind() {
    for (command, subcommand, kind) in [
        ("html", None, "html_report"),
        ("ledger", None, "audit_ledger"),
        ("approval", Some("create"), "html_report"),
        ("approval", Some("check"), "html_report"),
        ("ledger", None, "html_report"), // failed HTML approval overlay
    ] {
        statuses(
            command,
            subcommand,
            &json!({"result":"generated","kind":kind}),
            [true, true, false, false],
        );
    }
    for (command, subcommand, kind) in [
        ("html", None, "audit_ledger"),
        ("approval", Some("diff"), "html_report"),
        ("approval", Some("create"), "audit_ledger"),
        ("approval", Some("check"), "scenarios"),
        ("testgen", None, "html_report"),
        ("document", Some("generate"), "html_report"),
        ("domain", Some("generate"), "html_report"),
        ("check", None, "audit_ledger"),
        ("ledger", None, "testgen"),
    ] {
        statuses(
            command,
            subcommand,
            &json!({"result":"generated","kind":kind}),
            [true, false, false, false],
        );
    }
    statuses(
        "html",
        None,
        &json!({"result":"generated"}),
        [true, false, false, false],
    );
}

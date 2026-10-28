// SPDX-License-Identifier: Apache-2.0

//! Empty relation equality must agree across symbolic verification, BFS, and Monitor replay.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

const EMPTY: &str = "invariant Empty { r == Set {} }";
const NOT_EMPTY: &str = "invariant NotEmpty { not (r == Set {}) }";
const BOTH: &str = "invariant Empty { r == Set {} }\n  invariant NotEmpty { not (r == Set {}) }";
const REVERSED: &str = "invariant Empty { Set {} == r }";
const UNEQUAL: &str = "invariant NotEmpty { r != Set {} }";

#[derive(Clone, Copy)]
enum Lifecycle {
    Stay,
    RoundTrip,
    Nonempty,
}

#[allow(clippy::too_many_lines)]
fn assert_case(
    label: &str,
    field: &str,
    lifecycle: Lifecycle,
    properties: &str,
    violation: Option<(&str, usize)>,
) {
    let (state, init, actions, trace, final_state) = match lifecycle {
        Lifecycle::Stay => (
            "",
            "",
            "action stay() {}".to_owned(),
            json!([{"action": "stay", "params": {}}]),
            json!({"r": []}),
        ),
        Lifecycle::RoundTrip | Lifecycle::Nonempty => {
            let (add, remove, member) = if field == "Set<K>" {
                ("r.add(0)", "r.remove(0)", json!([0]))
            } else {
                ("r.add(0, 0)", "r.remove(0, 0)", json!([[0, 0]]))
            };
            let round_trip = matches!(lifecycle, Lifecycle::RoundTrip);
            (
                ", phase: 0..2",
                "phase = 0",
                format!(
                    "action insert() {{ requires phase == 0 r = {add} phase = 1 }}\n  \
                     action erase() {{ requires phase == 1 r = {remove} phase = 2 }}"
                ),
                if round_trip {
                    json!([{"action": "insert", "params": {}}, {"action": "erase", "params": {}}])
                } else {
                    json!([{"action": "insert", "params": {}}])
                },
                if round_trip {
                    json!({"r": [], "phase": 2})
                } else {
                    json!({"r": member, "phase": 1})
                },
            )
        }
    };
    // A singleton domain makes removal inductive without auxiliary invariants:
    // no unreachable induction state can contain an unrelated surviving pair.
    let upper_bound = usize::from(matches!(lifecycle, Lifecycle::Stay));
    let source = format!(
        "spec EmptyRelation {{\n  type K = 0..{upper_bound}\n  state {{ r: {field}{state} }}\n  \
         init {{ r = Set {{}} {init} }}\n  {actions}\n  {properties}\n}}\n"
    );
    // Keep each invocation isolated without deleting another run's evidence.
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1568_empty_relation_equality")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).expect("create acceptance directory");
    std::fs::write(directory.join("spec.fsl"), source).expect("write spec");
    std::fs::write(directory.join("trace.json"), trace.to_string()).expect("write trace");

    // Execute every consumer before asserting, so a false red does not hide a false green.
    let outputs = [
        ("check", vec!["check", "spec.fsl"]),
        (
            "bmc",
            vec![
                "verify",
                "spec.fsl",
                "--engine",
                "bmc",
                "--depth",
                "4",
                "--no-cache",
            ],
        ),
        (
            "induction",
            vec![
                "verify",
                "spec.fsl",
                "--engine",
                "induction",
                "--depth",
                "4",
                "--no-cache",
            ],
        ),
        (
            "explicit",
            vec![
                "verify",
                "spec.fsl",
                "--engine",
                "explicit",
                "--depth",
                "4",
                "--no-cache",
            ],
        ),
        (
            "replay",
            vec!["replay", "spec.fsl", "--trace", "trace.json"],
        ),
    ]
    .map(|(consumer, args)| {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&directory)
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
            .env("FSLC_CACHE", "off")
            .output()
            .expect("run native CLI");
        assert!(output.stderr.is_empty(), "{label} {consumer}: {output:?}");
        let value: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{label} {consumer}: {error}: {output:?}"));
        (
            consumer,
            value,
            output.status.code().expect("CLI exit code"),
        )
    });

    for (consumer, output, exit) in &outputs {
        let expected_result = match (*consumer, violation) {
            ("check", _) => "ok",
            ("replay", Some(_)) => "nonconformant",
            ("replay", None) => "conformant",
            (_, Some(_)) => "violated",
            ("bmc", None) => "verified",
            (_, None) => "proved",
        };
        let expected_exit = i32::from(*consumer != "check" && violation.is_some());
        assert_eq!(
            (output["result"].as_str(), *exit),
            (Some(expected_result), expected_exit),
            "{label} {consumer}: all observations {outputs:#?}"
        );
        assert_eq!(
            output["spec"], "EmptyRelation",
            "{label} {consumer}: {output:#}"
        );
        if *consumer == "check" {
            continue;
        }
        if let Some((name, step)) = violation {
            if *consumer == "replay" {
                assert_eq!(
                    output["violation"]["kind"], "invariant",
                    "{label}: {output:#}"
                );
                assert_eq!(output["violation"]["name"], name, "{label}: {output:#}");
                assert_eq!(
                    output["failed_at_event"],
                    step.saturating_sub(1),
                    "{label}: {output:#}"
                );
            } else {
                assert_eq!(output["violation_kind"], "invariant", "{label}: {output:#}");
                assert_eq!(output["invariant"], name, "{label}: {output:#}");
                assert_eq!(output["violated_at_step"], step, "{label}: {output:#}");
                let trace = output["trace"].as_array().expect("counterexample trace");
                assert_eq!(
                    trace.last().unwrap()["state"],
                    final_state,
                    "{label}: {output:#}"
                );
            }
        } else if *consumer == "replay" {
            assert_eq!(
                output["steps_checked"],
                trace.as_array().unwrap().len(),
                "{label}: {output:#}"
            );
            assert_eq!(output["final_state"], final_state, "{label}: {output:#}");
        } else {
            let completeness = if *consumer == "bmc" {
                "bounded"
            } else {
                "unbounded"
            };
            assert_eq!(output["completeness"], completeness, "{label}: {output:#}");
        }
    }
}

#[test]
fn empty_relation_equals_empty_set() {
    assert_case("empty", "relation K -> K", Lifecycle::Stay, EMPTY, None);
}

#[test]
fn negated_empty_equality_is_not_a_false_green() {
    assert_case(
        "not-empty",
        "relation K -> K",
        Lifecycle::Stay,
        NOT_EMPTY,
        Some(("NotEmpty", 0)),
    );
}

#[test]
fn both_invariants_report_only_not_empty() {
    assert_case(
        "both",
        "relation K -> K",
        Lifecycle::Stay,
        BOTH,
        Some(("NotEmpty", 0)),
    );
}

#[test]
fn empty_literal_on_the_left_equals_empty_relation() {
    assert_case(
        "reversed",
        "relation K -> K",
        Lifecycle::Stay,
        REVERSED,
        None,
    );
}

#[test]
fn empty_relation_inequality_is_not_a_false_green() {
    assert_case(
        "unequal",
        "relation K -> K",
        Lifecycle::Stay,
        UNEQUAL,
        Some(("NotEmpty", 0)),
    );
}

#[test]
fn removing_the_inserted_pair_restores_empty_equality() {
    assert_case(
        "round-trip-empty",
        "relation K -> K",
        Lifecycle::RoundTrip,
        "invariant Empty { phase != 2 or r == Set {} }",
        None,
    );
}

#[test]
fn removing_the_inserted_pair_rejects_not_empty() {
    assert_case(
        "round-trip-not-empty",
        "relation K -> K",
        Lifecycle::RoundTrip,
        "invariant NotEmpty { phase != 2 or not (r == Set {}) }",
        Some(("NotEmpty", 2)),
    );
}

#[test]
fn ordinary_sets_preserve_the_same_equality_contract() {
    for (label, properties, violation) in [
        ("set-empty", EMPTY, None),
        ("set-not-empty", NOT_EMPTY, Some(("NotEmpty", 0))),
        ("set-both", BOTH, Some(("NotEmpty", 0))),
        ("set-reversed", REVERSED, None),
        ("set-unequal", UNEQUAL, Some(("NotEmpty", 0))),
    ] {
        assert_case(label, "Set<K>", Lifecycle::Stay, properties, violation);
    }
    assert_case(
        "set-round-trip-empty",
        "Set<K>",
        Lifecycle::RoundTrip,
        "invariant Empty { phase != 2 or r == Set {} }",
        None,
    );
    assert_case(
        "set-round-trip-not-empty",
        "Set<K>",
        Lifecycle::RoundTrip,
        "invariant NotEmpty { phase != 2 or not (r == Set {}) }",
        Some(("NotEmpty", 2)),
    );
}

#[test]
fn a_nonempty_relation_is_not_equal_to_empty_set() {
    assert_case(
        "nonempty-unequal",
        "relation K -> K",
        Lifecycle::Nonempty,
        "invariant NotEmpty { phase != 1 or r != Set {} }",
        None,
    );
    assert_case(
        "nonempty-equal",
        "relation K -> K",
        Lifecycle::Nonempty,
        "invariant Empty { phase != 1 or r == Set {} }",
        Some(("Empty", 1)),
    );
}

#[test]
fn option_wrapped_empty_relation_equals_empty_set() {
    assert_case(
        "option-empty",
        "relation K -> K",
        Lifecycle::Stay,
        "invariant Empty { some(r) == some(Set {}) }",
        None,
    );
}

#[test]
fn option_wrapped_negated_empty_equality_is_not_a_false_green() {
    assert_case(
        "option-not-empty",
        "relation K -> K",
        Lifecycle::Stay,
        "invariant NotEmpty { not (some(r) == some(Set {})) }",
        Some(("NotEmpty", 0)),
    );
}

#[test]
fn option_wrapped_empty_relation_inequality_is_not_a_false_green() {
    assert_case(
        "option-unequal",
        "relation K -> K",
        Lifecycle::Stay,
        "invariant NotEmpty { some(r) != some(Set {}) }",
        Some(("NotEmpty", 0)),
    );
}

#[test]
fn option_wrapped_last_pair_removal_restores_empty_equality() {
    assert_case(
        "option-round-trip-empty",
        "relation K -> K",
        Lifecycle::RoundTrip,
        "invariant Empty { phase != 2 or some(r) == some(Set {}) }",
        None,
    );
}

#[test]
fn option_wrapped_last_pair_removal_rejects_negated_empty_equality() {
    assert_case(
        "option-round-trip-not-empty",
        "relation K -> K",
        Lifecycle::RoundTrip,
        "invariant NotEmpty { phase != 2 or not (some(r) == some(Set {})) }",
        Some(("NotEmpty", 2)),
    );
}

#[test]
fn option_wrapped_last_pair_removal_rejects_empty_inequality() {
    assert_case(
        "option-round-trip-unequal",
        "relation K -> K",
        Lifecycle::RoundTrip,
        "invariant NotEmpty { phase != 2 or some(r) != some(Set {}) }",
        Some(("NotEmpty", 2)),
    );
}

#[test]
fn option_wrapped_nonempty_relation_is_not_equal_to_empty_set() {
    assert_case(
        "option-nonempty-equal",
        "relation K -> K",
        Lifecycle::Nonempty,
        "invariant Empty { phase != 1 or some(r) == some(Set {}) }",
        Some(("Empty", 1)),
    );
}

fn assert_empty_refinement_map(label: &str, field: &str, expression: &str) {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1568_empty_relation_equality")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&directory).expect("create refinement acceptance directory");
    // The same empty relation supplies the identity-map preservation control;
    // literal maps deliberately ignore it.
    let implementation = "spec Impl { type K = 0..1 \
        state { q: relation K -> K, n: 0..1 } init { q = Set {} n = 0 } \
        action tick() { requires n == 0 n = 1 } }";
    let abstraction = format!(
        "spec Abs {{ type K = 0..1 state {{ r: {field} }} \
         init {{ r = Set {{}} }} action stay() {{}} \
         invariant Empty {{ r == Set {{}} }} }}"
    );
    let mapping = format!(
        "refinement ImplRefinesAbs {{ impl Impl abs Abs \
         map r = {expression} action tick() -> stay() }}"
    );
    for (name, source) in [
        ("impl.fsl", implementation),
        ("abs.fsl", abstraction.as_str()),
        ("map.fsl", mapping.as_str()),
    ] {
        std::fs::write(directory.join(name), source).expect("write refinement fixture");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "4"])
        .current_dir(&directory)
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run native refine CLI");
    assert!(output.stderr.is_empty(), "{label}: {output:?}");
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{label}: {error}: {output:?}"));
    assert_eq!(
        (value["result"].as_str(), output.status.code()),
        (Some("refines"), Some(0)),
        "{label}: {value:#}"
    );
    assert_eq!(value["fsl"], "1.0", "{label}: {value:#}");
    assert_eq!(value["impl"], "Impl", "{label}: {value:#}");
    assert_eq!(value["abs"], "Abs", "{label}: {value:#}");
    assert_eq!(
        value["action_map"],
        json!({"tick": "stay"}),
        "{label}: {value:#}"
    );
}

#[test]
fn refinement_maps_empty_set_literal_to_abstract_relation() {
    assert_empty_refinement_map("refine-literal-relation", "relation K -> K", "Set {}");
}

#[test]
fn refinement_empty_set_literal_preserves_ordinary_sets() {
    assert_empty_refinement_map("refine-literal-set", "Set<K>", "Set {}");
}

#[test]
fn refinement_empty_relation_identity_map_still_refines() {
    assert_empty_refinement_map("refine-relation-identity", "relation K -> K", "q");
}

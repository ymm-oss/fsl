// SPDX-License-Identifier: Apache-2.0

//! BMC's boundary pre-pass decides the vacuity probe on its own walk
//! (`fsl_runtime::find_boundary_violation_reaching`). On a model whose
//! reachable states exceed the budget, both of what it reports must be what
//! the two separate walks report. On a model where a `partial_op` finding stops
//! the walk first, it decides nothing, and the vacuity probe walks on its own.
//!
//! The explicit engine decides it on its walk too
//! (`fsl_runtime::verify_explicit_reaching`), only when that walk visited
//! every state the probe's own walk can visit: all bounds checked, no
//! violation, no state budget stop, and fewer states than the probe's budget.

#[path = "support/fslc_cli.rs"]
mod fslc_cli;

use std::collections::BTreeSet;
use std::path::Path;

use fsl_runtime::Reachability::{Exhausted, Reachable, Unreachable};

fn fixture() -> fsl_core::KernelModel {
    load("issue_1848_shared_concrete_walk.fsl")
}

fn load(name: &str) -> fsl_core::KernelModel {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let source = std::fs::read_to_string(&path).expect("read fixture");
    let document = fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new("."))
        .expect("parse fixture");
    fsl_core::build_model(document).expect("build fixture")
}

#[test]
fn the_shared_walk_reports_what_the_separate_walks_report_at_the_budget() {
    let model = fixture();
    let expressions = fsl_runtime::vacuity_probe_expressions(&model);
    assert!(
        !expressions.is_empty(),
        "the fixture has vacuity candidates"
    );
    for budget in [200, 2_000] {
        let (probe, reachability) =
            fsl_runtime::find_boundary_violation_reaching(&model, &expressions, 20, budget)
                .expect("shared walk");
        let boundary =
            fsl_runtime::find_boundary_violation(&model, 20, budget).expect("boundary walk");
        let separate = fsl_runtime::expression_reachability(&model, &expressions, 20, budget)
            .expect("vacuity walk");
        // The separate boundary walk stops after exactly `budget` states with
        // no finding; the shared walk must stop at the same point.
        let expected = fsl_runtime::BoundaryProbe {
            finding: None,
            exhausted: true,
            states_explored: budget,
        };
        assert_eq!(probe, expected, "budget {budget}");
        assert_eq!(boundary, expected, "budget {budget}");
        assert_eq!(reachability, Some(separate), "budget {budget}");
    }
}

/// `divide` divides by zero from `x == 1`, so the walk stops at step 2, before
/// it reaches `x == 4`. The vacuity probe walks past that successor and
/// reaches the antecedent, which a verdict decided on the stopped walk would
/// not have reached.
#[test]
fn a_partial_op_finding_leaves_the_vacuity_probe_to_its_own_walk() {
    let model = load("issue_1848_partial_op_stop.fsl");
    let expressions = fsl_runtime::vacuity_probe_expressions(&model);
    assert_eq!(expressions.len(), 1, "the implication's antecedent");
    let budget = fsl_runtime::CONCRETE_PROBE_BUDGET;
    let (probe, reachability) =
        fsl_runtime::find_boundary_violation_reaching(&model, &expressions, 10, budget)
            .expect("shared walk");
    let (violation, trace) = probe.finding.expect("the walk stops at the division");
    assert_eq!(
        violation,
        fsl_runtime::Violation {
            kind: "partial_op".to_owned(),
            name: "_partial_divide".to_owned(),
            step: 2,
        }
    );
    let actions: Vec<Option<&str>> = trace
        .iter()
        .map(|step| step.action.as_ref().map(|action| action.name.as_str()))
        .collect();
    assert_eq!(actions, [None, Some("advance"), Some("divide")]);
    assert!(!probe.exhausted);
    assert_eq!(probe.states_explored, 3);
    assert_eq!(reachability, None, "the stopped walk decides nothing");
    assert_eq!(
        fsl_runtime::expression_reachability(&model, &expressions, 10, budget)
            .expect("vacuity walk"),
        [fsl_runtime::Reachability::Reachable]
    );
}

fn parse(source: &str) -> fsl_core::KernelModel {
    let document = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
        .expect("parse model");
    fsl_core::build_model(document).expect("build model")
}

/// The explicit walk with every bound checked and the probe's own budget.
fn explicit_reaching(
    model: &fsl_core::KernelModel,
    depth: usize,
    max_states: usize,
) -> (
    fsl_runtime::ExplicitResult,
    Option<Vec<fsl_runtime::Reachability>>,
) {
    fsl_runtime::verify_explicit_reaching(
        model.clone(),
        depth,
        max_states,
        None,
        &fsl_runtime::vacuity_probe_expressions(model),
        fsl_runtime::CONCRETE_PROBE_BUDGET,
    )
    .expect("explicit walk")
}

/// `x` counts from 0 to 5. Each antecedent holds in one state only: the
/// root, the last level a depth-3 walk reaches, and the state past it.
const COUNTER: &str = "spec Counter {\n  state { x: 0..5 }\n  init { x = 0 }\n  \
                       action inc() {\n    requires x < 5\n    x = x + 1\n  }\n  \
                       invariant AtRoot { x == 0 => x < 5 }\n  \
                       invariant AtThree { x == 3 => x < 5 }\n  \
                       invariant AtFive { x == 5 => x > 0 }\n}\n";

/// The explicit walk visits the root, and the level it stops at without
/// expanding it, as the probe's own walk does.
#[test]
fn the_explicit_walk_decides_the_probe_at_its_root_and_its_last_level() {
    let model = parse(COUNTER);
    let expressions = fsl_runtime::vacuity_probe_expressions(&model);
    let budget = fsl_runtime::CONCRETE_PROBE_BUDGET;
    let (result, reachability) = explicit_reaching(&model, 3, 100);
    assert_eq!(result.states_explored, 4);
    assert!(!result.closure);
    assert_eq!(reachability, Some(vec![Reachable, Reachable, Unreachable]));
    assert_eq!(
        fsl_runtime::expression_reachability(&model, &expressions, 3, budget)
            .expect("vacuity walk"),
        [Reachable, Reachable, Unreachable]
    );
    // At depth 10 the walk closes after the level that holds x == 5.
    let (result, reachability) = explicit_reaching(&model, 10, 100);
    assert_eq!((result.states_explored, result.depth_reached), (6, 5));
    assert!(result.closure);
    assert_eq!(reachability, Some(vec![Reachable, Reachable, Reachable]));
}

/// Around `CONCRETE_PROBE_BUDGET`: one state under it, the walk decides the
/// probe; at it and one over, the probe's own walk stops at its budget, so
/// the explicit walk decides nothing and rendering walks the probe as before.
#[test]
fn the_explicit_walk_decides_the_probe_only_under_its_budget() {
    assert_eq!(fsl_runtime::CONCRETE_PROBE_BUDGET, 50_000);
    for (name, states, shared, separate) in [
        ("under", 49_999, Some(vec![Unreachable]), Unreachable),
        ("full", 50_000, None, Exhausted),
        ("over", 50_001, None, Exhausted),
    ] {
        let model = load(&format!("issue_1851_probe_budget_{name}.fsl"));
        let (result, reachability) = explicit_reaching(&model, 500, 1_000_000);
        assert_eq!(result.states_explored, states, "{name}");
        assert!(result.closure, "{name}");
        assert_eq!(result.violation, None, "{name}");
        assert_eq!(reachability, shared, "{name}");
        assert_eq!(
            fsl_runtime::expression_reachability(
                &model,
                &fsl_runtime::vacuity_probe_expressions(&model),
                result.depth_reached,
                fsl_runtime::CONCRETE_PROBE_BUDGET,
            )
            .expect("vacuity walk"),
            [separate],
            "{name}"
        );
    }
}

/// The warnings `verify --engine explicit` renders for a fixture.
fn explicit_warnings(fixture: &str, extra: &[&str]) -> (Vec<(String, String)>, serde_json::Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let output = fslc_cli::command()
        .arg("verify")
        .arg(&path)
        .args(["--engine", "explicit"])
        .args(extra)
        .output()
        .expect("run fslc");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{fixture}: no JSON envelope: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(output.status.code(), Some(0), "{fixture}: {json}");
    let warnings = json["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .map(|warning| {
            (
                warning["kind"].as_str().expect("kind").to_owned(),
                warning["name"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    (warnings, json)
}

#[test]
fn explicit_renders_the_probe_verdict_on_either_side_of_its_budget() {
    for (name, states, kind) in [
        ("under", 49_999, "vacuous_implication"),
        ("full", 50_000, "vacuity_probe_truncated"),
        ("over", 50_001, "vacuity_probe_truncated"),
    ] {
        let (warnings, json) = explicit_warnings(
            &format!("issue_1851_probe_budget_{name}.fsl"),
            &["--depth", "500"],
        );
        assert_eq!(json["result"], "proved", "{name}");
        assert_eq!(json["states_explored"], states, "{name}");
        assert_eq!(
            warnings,
            [
                (kind.to_owned(), "NeverPastTheCorner".to_owned()),
                ("deadlock".to_owned(), String::new()),
            ],
            "{name}"
        );
    }
}

/// Selecting one property leaves `x`'s bound unchecked, so the explicit walk
/// reaches `x == 3`, a successor the probe's own walk skips as out of range.
/// The explicit walk then decides nothing, and the antecedent `x > 2` is
/// reported unreachable as the probe's own walk finds it.
#[test]
fn an_unchecked_bound_leaves_the_probe_to_its_own_walk() {
    let model = load("issue_1851_excluded_bound.fsl");
    let expressions = fsl_runtime::vacuity_probe_expressions(&model);
    let (result, reachability) = fsl_runtime::verify_explicit_reaching(
        model.clone(),
        4,
        100,
        Some(&BTreeSet::new()),
        &expressions,
        fsl_runtime::CONCRETE_PROBE_BUDGET,
    )
    .expect("explicit walk");
    assert_eq!(result.states_explored, 5, "x reaches 4");
    assert_eq!(result.violation, None);
    assert_eq!(reachability, None);
    assert_eq!(
        fsl_runtime::expression_reachability(
            &model,
            &expressions,
            4,
            fsl_runtime::CONCRETE_PROBE_BUDGET
        )
        .expect("vacuity walk"),
        [Unreachable]
    );
    let (warnings, json) = explicit_warnings(
        "issue_1851_excluded_bound.fsl",
        &["--depth", "4", "--property", "PastTheRange"],
    );
    assert_eq!(json["result"], "verified");
    assert_eq!(json["states_explored"], 5);
    assert_eq!(
        warnings,
        [("vacuous_implication".to_owned(), "PastTheRange".to_owned())]
    );
}

/// A violation stops the explicit walk before it reaches every state, and a
/// state budget stops it the same way; either way it decides nothing.
#[test]
fn a_stopped_explicit_walk_leaves_the_probe_to_its_own_walk() {
    let model = load("issue_1848_partial_op_stop.fsl");
    let (result, reachability) = explicit_reaching(&model, 10, 1_000);
    let violation = result.violation.expect("the walk stops at the division");
    assert_eq!(violation.violation.kind, "partial_op");
    assert_eq!(violation.violation.step, 2);
    assert_eq!(reachability, None);

    let model = fixture();
    let (result, reachability) = explicit_reaching(&model, 20, 200);
    assert!(result.budget_exceeded);
    assert_eq!(result.violation, None);
    assert_eq!(reachability, None);
}

// SPDX-License-Identifier: Apache-2.0

//! BMC's boundary pre-pass decides the vacuity probe on its own walk
//! (`fsl_runtime::find_boundary_violation_reaching`). On a model whose
//! reachable states exceed the budget, both of what it reports must be what
//! the two separate walks report. On a model where a `partial_op` finding stops
//! the walk first, it decides nothing, and the vacuity probe walks on its own.

use std::path::Path;

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

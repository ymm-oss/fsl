// SPDX-License-Identifier: Apache-2.0

//! BMC's boundary pre-pass decides the vacuity probe on its own walk
//! (`fsl_runtime::find_boundary_violation_reaching`). On a model whose
//! reachable states exceed the budget, both of what it reports must be what
//! the two separate walks report.

use std::path::Path;

fn fixture() -> fsl_core::KernelModel {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/issue_1848_shared_concrete_walk.fsl");
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

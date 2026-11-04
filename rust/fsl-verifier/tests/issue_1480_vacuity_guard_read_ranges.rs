// SPDX-License-Identifier: Apache-2.0
//! Actual guard-read diagnostics require both bound permission and ACTION definedness.
use fsl_core::{FsResolver, KernelModel, Span, build_model, parse_kernel_source};
use fsl_solver::{SatResult, SmtSolver, Sort};
use fsl_syntax::SourcePos;
use fsl_verifier::{
    BmcResult, VacuityFinding, VerifyError, model_vacuity_findings, verify_bounded,
};
use std::collections::BTreeMap;
const GUARD: &str = "x!=5 and (forall i in 0..x {i<=5})";
const SOURCE: &str = r"spec NeedsBound {
  state { x: 0..5 }
  init { x=0 }
  action inc() {
    requires x!=5 and (forall i in 0..x {i<=5})
    x=x+1
  }
  leadsTo Reach { x!=5 ~> x==5 decreases 5-x }
}";
const PARTIAL: &str = r"spec Partial {
  state {n:0..2,q:Seq<Int,1>}
  init {n=0 q=Seq {0}}
  action go() {
    requires forall i in 0..n {q.head()==q.head()}
    n=n q=q
  }
}";
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}
fn model(source: &str) -> KernelModel {
    let parsed = parse_kernel_source(source, &FsResolver::new("."));
    eprintln!("VREAD_SOURCE source={source:?} parsed={parsed:#?}");
    let actual = build_model(parsed.unwrap());
    eprintln!("VREAD_MODEL actual={actual:#?}");
    actual.unwrap()
}
fn coverage(action: &str, covered: bool) -> BTreeMap<String, bool> {
    BTreeMap::from([(action.into(), covered)])
}
fn finding() -> VacuityFinding {
    VacuityFinding::AlwaysTrueRequires {
        action: "inc".into(),
        clause_index: 0,
        span: Span {
            start: SourcePos {
                offset: 74,
                line: 5,
                column: 5,
            },
            end: SourcePos {
                offset: 82,
                line: 5,
                column: 13,
            },
        },
    }
}
fn base(spec: &str, action: &str, vacuity: Vec<VacuityFinding>) -> BmcResult {
    BmcResult {
        spec: spec.into(),
        depth: 1,
        violation: None,
        leadsto_violation: None,
        reachables: BTreeMap::new(),
        reachable_diagnostics: BTreeMap::new(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: coverage(action, true),
        frontier_progress: false,
        vacuity,
    }
}
fn pair(
    source: &str,
    action: &str,
) -> (
    Result<Vec<VacuityFinding>, VerifyError>,
    Result<BmcResult, VerifyError>,
) {
    let model = model(source);
    let mut direct_solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let direct = block_on(model_vacuity_findings(
        &model,
        &mut direct_solver,
        &coverage(action, true),
    ));
    let mut base_solver = fsl_solver_z3::Z3Solver::new().unwrap();
    let actual_base = block_on(verify_bounded(&model, &mut base_solver, 1));
    eprintln!("VREAD_PAIR direct={direct:#?} actual_base={actual_base:#?}");
    (direct, actual_base)
}
#[test]
fn same_ranked_dynamic_guard_has_no_finding_and_a_real_bounded_base() {
    let actual = pair(SOURCE, "inc");
    assert_eq!(actual, (Ok(vec![]), Ok(base("NeedsBound", "inc", vec![]))));
}
#[test]
fn total_dynamic_range_guard_has_the_complete_genuine_finding() {
    let source = SOURCE.replace(GUARD, "forall i in 0..x {i<=5}");
    let actual = pair(&source, "inc");
    assert_eq!(
        actual,
        (
            Ok(vec![finding()]),
            Ok(base("NeedsBound", "inc", vec![finding()]))
        )
    );
    assert_ne!(actual.0, Ok(vec![]));
}
#[test]
fn no_range_finding_and_genuine_counterexample_are_preserved() {
    let outputs = [("x>=0", vec![finding()]), ("x<5", vec![])].map(|(guard, expected)| {
        let source = SOURCE.replace(GUARD, guard);
        (pair(&source, "inc"), expected)
    });
    for (actual, expected) in outputs {
        assert_eq!(
            actual,
            (
                Ok(expected.clone()),
                Ok(base("NeedsBound", "inc", expected))
            )
        );
    }
}
#[test]
fn range_guard_with_abstract_empty_head_abstains_but_reachable_base_is_safe() {
    // Init q=[0] is safe; abstract q=[] is also bound-valid, so ACTION D
    // cannot be established merely from head() == itself under the range.
    let actual = pair(PARTIAL, "go");
    assert_eq!(actual, (Ok(vec![]), Ok(base("Partial", "go", vec![]))));
}
#[test]
fn actual_diagnostic_bound_assertions_do_not_escape_their_scope() {
    let rows = [
        (
            SOURCE.replace(GUARD, "forall i in 0..x {i<=5}"),
            "inc",
            "x@vac_requires",
            6,
            vec![finding()],
        ),
        (PARTIAL.to_owned(), "go", "n@vac_requires", 3, vec![]),
    ];
    for (source, action, probe_name, outside_value, expected) in rows {
        let model = model(&source);
        let mut solver = fsl_solver_z3::Z3Solver::new().unwrap();
        let actual = block_on(model_vacuity_findings(
            &model,
            &mut solver,
            &coverage(action, true),
        ));
        // Existing native spelling after this completed public diagnostic call.
        let previous_probe = solver.constant(probe_name, &Sort::Int).unwrap();
        solver.push();
        let outside = solver
            .equal(&previous_probe, &solver.int_value(outside_value))
            .unwrap();
        solver.assert(&outside).unwrap();
        let after = block_on(solver.check());
        let popped = solver.pop(1);
        eprintln!("VREAD_POP actual={actual:#?} after={after:?} popped={popped:?}");
        assert_eq!(actual, Ok(expected));
        assert_eq!(after, Ok(SatResult::Sat));
        assert_eq!(popped, Ok(()));
    }
}

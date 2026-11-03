// SPDX-License-Identifier: Apache-2.0

//! Independently authored #1480 fixed-outer temporal controls.
//! Run only `current_r3_characterization` on CURRENT-R3. That test records
//! observations; its process success is not a future-semantics verdict.
//! The unignored `future_` tests are literal acceptance tests for the proposed
//! coherent core-roster/verifier-conversion change. See the adjacent manifest.

use std::collections::BTreeMap;
use std::future::Future;

use fsl_core::{
    FsResolver, FslValue as V, KernelBinder as Binder, KernelExpr as Expr, KernelModel, ModelError,
    QualifiedName, TraceAction, TraceStep, TypeDef, TypeRef, build_model, parse_kernel_source,
    static_leadsto_bindings,
};
use fsl_runtime::{
    BoundedLivenessMonitor, BoundedLivenessPending, BoundedLivenessStatus,
    BoundedLivenessViolation, Monitor, RuntimeError, StepResult,
};
use fsl_verifier::{BmcResult, BmcViolation, LeadsToViolation, VerifyError, verify_bounded};

type Row = BTreeMap<String, V>;
type RosterResult = Result<Vec<Row>, ModelError>;

const DECLARATIONS: &str =
    "const N = 1 type K = 0..1 enum Tag { Red, Blue } struct Cell { flag: Bool }";
const STATE: &str = "state { n: 0..2, ready: Bool } init { n = 0 ready = false } action idle() {}";

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("native solver unexpectedly yielded Pending"),
    }
}

fn expr(text: &str) -> Expr {
    fsl_syntax::parse_expr(text).expect("authored raw expression syntax")
}

fn range(name: &str, lo: &str, hi: &str) -> Binder {
    Binder::Range {
        name: name.into(),
        lo: Box::new(expr(lo)),
        hi: Box::new(expr(hi)),
        where_expr: None,
    }
}

fn typed(name: &str, type_name: &str) -> Binder {
    Binder::Typed {
        name: name.into(),
        type_name: QualifiedName {
            namespace: None,
            name: type_name.into(),
        },
        where_expr: None,
    }
}

fn qualified() -> Binder {
    Binder::Typed {
        name: "q".into(),
        type_name: QualifiedName {
            namespace: Some("foreign".into()),
            name: "Tag".into(),
        },
        where_expr: None,
    }
}

fn filtered() -> Binder {
    Binder::Range {
        name: "j".into(),
        lo: Box::new(Expr::Num(0)),
        hi: Box::new(Expr::Num(0)),
        where_expr: Some(Box::new(Expr::Bool(true))),
    }
}

fn collection() -> Binder {
    Binder::Collection {
        name: "j".into(),
        collection: Box::new(Expr::Set(vec![Expr::Num(0)])),
        where_expr: None,
    }
}

struct Fixture {
    id: &'static str,
    // None means API-only invalid AST. No fake FSL spelling is manufactured.
    wrappers: Option<Vec<&'static str>>,
    binders: Vec<Binder>,
    constant_n: bool,
}

fn fixture(id: &'static str, wrappers: &[&'static str], binders: Vec<Binder>) -> Fixture {
    Fixture {
        id,
        wrappers: Some(wrappers.to_vec()),
        binders,
        constant_n: false,
    }
}

fn positive_fixtures() -> Vec<Fixture> {
    vec![
        fixture(
            "T01-triangle",
            &["i in 0..1", "j in 0..i"],
            vec![range("i", "0", "1"), range("j", "0", "i")],
        ),
        fixture(
            "T02-bool-conditional",
            &["b: Bool", "i in 0..(if b then 1 else 0)"],
            vec![typed("b", "Bool"), range("i", "0", "if b then 1 else 0")],
        ),
        fixture(
            "T03-euclidean",
            &["i in 1..2", "j in (-3/i)..-1"],
            vec![range("i", "1", "2"), range("j", "-3/i", "-1")],
        ),
        fixture(
            "T04-enum-bool",
            &["t: Tag", "b: Bool"],
            vec![typed("t", "Tag"), typed("b", "Bool")],
        ),
        fixture(
            "T05-coincident",
            &["i in 0..1", "i in 0..0"],
            vec![range("i", "0", "1"), range("i", "0", "0")],
        ),
        fixture(
            "T06-own-name",
            &["i in 0..1", "i in i..i", "k in i..i"],
            vec![
                range("i", "0", "1"),
                range("i", "i", "i"),
                range("k", "i", "i"),
            ],
        ),
        fixture(
            "T07-latest-name",
            &["i in 1..2", "i in 0..0", "k in i..i"],
            vec![
                range("i", "1", "2"),
                range("i", "0", "0"),
                range("k", "i", "i"),
            ],
        ),
    ]
}

fn preservation_fixtures() -> Vec<Fixture> {
    let mut cases = vec![fixture(
        "T10-named-domain",
        &["k: K", "j in 0..k"],
        vec![typed("k", "K"), range("j", "0", "k")],
    )];
    for (id, value) in [
        ("T08-min-singleton", i64::MIN),
        ("T09-max-singleton", i64::MAX),
    ] {
        cases.push(Fixture {
            id,
            wrappers: None,
            binders: vec![Binder::Range {
                name: "i".into(),
                lo: Box::new(Expr::Num(value)),
                hi: Box::new(Expr::Num(value)),
                where_expr: None,
            }],
            constant_n: false,
        });
    }
    cases
}

fn empty_prefix_fixtures() -> Vec<Fixture> {
    vec![
        fixture(
            "E01-empty-dependent",
            &["i in 1..0", "j in 0..(i/(i-i))"],
            vec![range("i", "1", "0"), range("j", "0", "i/(i-i)")],
        ),
        fixture(
            "E02-empty-static-divzero",
            &["i in 1..0", "j in 0..(1/0)"],
            vec![range("i", "1", "0"), range("j", "0", "1/0")],
        ),
        fixture(
            "E03-empty-static-overflow",
            &["i in 1..0", "j in 0..(9223372036854775807+1)"],
            vec![
                range("i", "1", "0"),
                range("j", "0", "9223372036854775807+1"),
            ],
        ),
        fixture(
            "E04-empty-filter",
            &["i in 1..0", "j in 0..0 where true"],
            vec![range("i", "1", "0"), filtered()],
        ),
        fixture(
            "E05-empty-collection",
            &["i in 1..0", "j in Set { 0 }"],
            vec![range("i", "1", "0"), collection()],
        ),
        fixture(
            "E06-empty-struct",
            &["i in 1..0", "j: Cell"],
            vec![range("i", "1", "0"), typed("j", "Cell")],
        ),
        fixture(
            "E07-empty-unbounded",
            &["i in 1..0", "j: Int"],
            vec![range("i", "1", "0"), typed("j", "Int")],
        ),
        fixture(
            "E08-empty-bool-bound",
            &["i in 1..0", "j in false..true"],
            vec![range("i", "1", "0"), range("j", "false", "true")],
        ),
        fixture(
            "E09-empty-inactive-type-error",
            &["i in 1..0", "j in 0..(if true then 0 else false)"],
            vec![
                range("i", "1", "0"),
                range("j", "0", "if true then 0 else false"),
            ],
        ),
    ]
}

fn refusal_fixtures() -> Vec<Fixture> {
    vec![
        fixture("F01-free-state", &["i in 0..n"], vec![range("i", "0", "n")]),
        fixture(
            "F02-state-const-collision",
            &["i in 0..n"],
            vec![range("i", "0", "n")],
        ),
        fixture("F03-filter", &["j in 0..0 where true"], vec![filtered()]),
        fixture("F04-collection", &["j in Set { 0 }"], vec![collection()]),
        fixture("F05-struct", &["j: Cell"], vec![typed("j", "Cell")]),
        fixture("F06-unbounded", &["j: Int"], vec![typed("j", "Int")]),
        fixture(
            "F07-missing-name",
            &["i in 0..missing"],
            vec![range("i", "0", "missing")],
        ),
        fixture(
            "F08-negation-overflow",
            &["i in 0..(-(-9223372036854775807-1))"],
            vec![range("i", "0", "-(-9223372036854775807-1)")],
        ),
        fixture(
            "F09-bool-arm-type",
            &["b: Bool", "i in 0..(if b then 1 else false)"],
            vec![
                typed("b", "Bool"),
                range("i", "0", "if b then 1 else false"),
            ],
        ),
    ]
}

fn priority_fixtures() -> Vec<Fixture> {
    vec![
        fixture(
            "P01-failed-fold-then-missing",
            &["i in 0..(1/0)", "j in 0..missing"],
            vec![range("i", "0", "1/0"), range("j", "0", "missing")],
        ),
        fixture(
            "P02-missing-then-failed-fold",
            &["j in 0..missing", "i in 0..(1/0)"],
            vec![range("j", "0", "missing"), range("i", "0", "1/0")],
        ),
        fixture(
            "P03-lo-before-hi",
            &["i in missing..(1/0)"],
            vec![range("i", "missing", "1/0")],
        ),
        fixture(
            "P04-lo-failed-before-hi",
            &["i in (1/0)..missing"],
            vec![range("i", "1/0", "missing")],
        ),
        fixture(
            "S01-own-name-no-outer",
            &["i in 0..i"],
            vec![range("i", "0", "i")],
        ),
        fixture(
            "S02-constant-shadow",
            &["N in 0..0"],
            vec![range("N", "0", "0")],
        ),
    ]
}

fn fixtures() -> Vec<Fixture> {
    let mut cases = positive_fixtures();
    cases.extend(preservation_fixtures());
    cases.extend(empty_prefix_fixtures());
    cases.extend(refusal_fixtures());
    cases.extend(priority_fixtures());
    cases
        .iter_mut()
        .find(|case| case.id == "F02-state-const-collision")
        .unwrap()
        .constant_n = true;
    for (id, binders) in [
        ("Q01-qualified", vec![qualified()]),
        (
            "Q02-qualified-before-failed",
            vec![qualified(), range("i", "0", "1/0")],
        ),
        (
            "Q03-failed-before-qualified",
            vec![range("i", "0", "1/0"), qualified()],
        ),
        (
            "Q04-empty-before-qualified",
            vec![range("i", "1", "0"), qualified()],
        ),
    ] {
        cases.push(Fixture {
            id,
            wrappers: None,
            binders,
            constant_n: false,
        });
    }
    cases
}

fn source(wrappers: &[&str], before: &str, after: &str, collision: bool) -> String {
    let mut body = format!("{before} ~> within 1 {after}");
    for wrapper in wrappers.iter().rev() {
        body = format!("forall {wrapper} {{ {body} }}");
    }
    let constant = if collision { "const n = 1" } else { "" };
    format!("spec Temporal {{ {DECLARATIONS} {constant} {STATE} leadsTo L {{ {body} }} }}")
}

fn admitted(id: &str, text: &str) -> Result<KernelModel, ModelError> {
    let parsed = parse_kernel_source(text, &FsResolver::new("."));
    eprintln!("TEMPORAL_PARSE id={id} source={text:?} whole={parsed:#?}");
    // All source cases have ordinary syntax. Invalid public Kernel namespace
    // cases are constructed directly and never arrive here.
    let result = build_model(parsed.expect("authored source syntax"));
    eprintln!("TEMPORAL_ADMISSION id={id} whole={result:#?}");
    result
}

fn raw_model(fixture: &Fixture, before: &str, after: &str) -> KernelModel {
    let mut model = admitted("raw-seed", &source(&[], "true", "false", false)).unwrap();
    model.leadstos[0].binders.clone_from(&fixture.binders);
    model.leadstos[0].before = expr(before);
    model.leadstos[0].after = expr(after);
    if fixture.constant_n {
        model.consts.insert("n".into(), V::Int(1));
    }
    model
}

#[derive(Debug)]
struct PublicObservation {
    roster: RosterResult,
    initial: Result<Row, RuntimeError>,
    monitor: Result<(), RuntimeError>,
    first: Option<Result<Option<BoundedLivenessViolation>, RuntimeError>>,
    first_status: Option<BoundedLivenessStatus>,
    idle: Option<Result<StepResult, RuntimeError>>,
    second: Option<Result<Option<BoundedLivenessViolation>, RuntimeError>>,
    second_status: Option<BoundedLivenessStatus>,
    bmc: Result<BmcResult, VerifyError>,
}

fn observe(id: &str, model: &KernelModel) -> PublicObservation {
    eprintln!("TEMPORAL_INPUT id={id} whole_model={model:#?}");
    let roster = static_leadsto_bindings(model, &model.leadstos[0]);
    let mut state_monitor = Monitor::new(model.clone());
    eprintln!("TEMPORAL_STATE_NEW id={id} whole={state_monitor:#?}");
    let initial = state_monitor
        .as_ref()
        .map(|monitor| monitor.state.clone())
        .map_err(Clone::clone);
    let mut live = BoundedLivenessMonitor::new(model.clone());
    eprintln!("TEMPORAL_LIVENESS_NEW id={id} whole={live:#?}");
    let monitor = live.as_ref().map(|_| ()).map_err(Clone::clone);
    let mut first = None;
    let mut first_status = None;
    let mut idle = None;
    let mut second = None;
    let mut second_status = None;
    if let (Ok(state), Ok(liveness)) = (&mut state_monitor, &mut live) {
        first = Some(liveness.observe(&state.state, 0));
        first_status = Some(liveness.status());
        idle = Some(state.attempt("idle", &BTreeMap::new()));
        second = Some(liveness.observe(&state.state, 1));
        second_status = Some(liveness.status());
    }
    let mut solver = fsl_solver_z3::Z3Solver::new().expect("native Z3 solver");
    let bmc = block_on(verify_bounded(model, &mut solver, 1));
    eprintln!(
        "TEMPORAL_VERIFY_CLASS id={id} is_solver_failure={:?}",
        bmc.as_ref().err().map(VerifyError::is_solver_failure)
    );
    let observation = PublicObservation {
        roster,
        initial,
        monitor,
        first,
        first_status,
        idle,
        second,
        second_status,
        bmc,
    };
    // Complete ModelError, all four RuntimeError fields, full BmcResult/trace
    // and both pending snapshots are retained before any semantic assertion.
    eprintln!("TEMPORAL_OBSERVATION id={id} whole={observation:#?}");
    observation
}

fn ints(i: i64, j: i64) -> Row {
    BTreeMap::from([("i".into(), V::Int(i)), ("j".into(), V::Int(j))])
}

fn bool_int(b: bool, i: i64) -> Row {
    BTreeMap::from([("b".into(), V::Bool(b)), ("i".into(), V::Int(i))])
}

fn tag_bool(tag: &str, b: bool) -> Row {
    BTreeMap::from([
        (
            "t".into(),
            V::Enum {
                type_name: "Tag".into(),
                member: tag.into(),
            },
        ),
        ("b".into(), V::Bool(b)),
    ])
}

fn literal_rosters() -> Vec<(&'static str, Vec<Row>, Vec<&'static str>)> {
    vec![
        (
            "T01-triangle",
            vec![ints(0, 0), ints(1, 0), ints(1, 1)],
            vec![
                "i == 0 and j == 0",
                "i == 1 and j == 0",
                "i == 1 and j == 1",
            ],
        ),
        (
            "T02-bool-conditional",
            vec![bool_int(false, 0), bool_int(true, 0), bool_int(true, 1)],
            vec!["not b and i == 0", "b and i == 0", "b and i == 1"],
        ),
        (
            "T03-euclidean",
            vec![
                ints(1, -3),
                ints(1, -2),
                ints(1, -1),
                ints(2, -2),
                ints(2, -1),
            ],
            vec![
                "i == 1 and j == -3",
                "i == 1 and j == -2",
                "i == 1 and j == -1",
                "i == 2 and j == -2",
                "i == 2 and j == -1",
            ],
        ),
        (
            "T04-enum-bool",
            vec![
                tag_bool("Red", false),
                tag_bool("Red", true),
                tag_bool("Blue", false),
                tag_bool("Blue", true),
            ],
            vec![
                "t == Red and not b",
                "t == Red and b",
                "t == Blue and not b",
                "t == Blue and b",
            ],
        ),
        (
            "T10-named-domain",
            vec![
                BTreeMap::from([("k".into(), V::Int(0)), ("j".into(), V::Int(0))]),
                BTreeMap::from([("k".into(), V::Int(1)), ("j".into(), V::Int(0))]),
                BTreeMap::from([("k".into(), V::Int(1)), ("j".into(), V::Int(1))]),
            ],
            vec![
                "k == 0 and j == 0",
                "k == 1 and j == 0",
                "k == 1 and j == 1",
            ],
        ),
    ]
}

fn extreme_literal_rows() -> Vec<(&'static str, Row, Expr)> {
    vec![
        (
            "T08-min-singleton",
            BTreeMap::from([("i".into(), V::Int(-9_223_372_036_854_775_808))]),
            Expr::Binary {
                op: "==".into(),
                left: Box::new(Expr::Var("i".into())),
                right: Box::new(Expr::Num(-9_223_372_036_854_775_808)),
            },
        ),
        (
            "T09-max-singleton",
            BTreeMap::from([("i".into(), V::Int(9_223_372_036_854_775_807))]),
            Expr::Binary {
                op: "==".into(),
                left: Box::new(Expr::Var("i".into())),
                right: Box::new(Expr::Num(9_223_372_036_854_775_807)),
            },
        ),
    ]
}

fn extreme_probe(case: &Fixture, selector: Expr, response: bool) -> KernelModel {
    let mut model = raw_model(case, "true", "true");
    model.leadstos[0].after = if response {
        Expr::Bool(true)
    } else {
        Expr::Not(Box::new(selector))
    };
    model
}

fn named_outer_identity(
    model: &KernelModel,
) -> (
    Result<Option<TypeRef>, ModelError>,
    Result<Vec<V>, ModelError>,
) {
    let declared = TypeRef::Named("K".into());
    let inferred = model.expression_type(
        &Expr::Var("k".into()),
        &[("k".into(), declared.clone())],
        None,
    );
    let values = model.domain_values(&declared);
    eprintln!(
        "TEMPORAL_NAMED_OUTER binder={:#?} declared={declared:#?} definition={:#?} inferred={inferred:#?} values={values:#?}",
        model.leadstos[0].binders[0],
        model.types.get("K")
    );
    (inferred, values)
}

#[test]
fn current_r3_characterization() {
    let cases = fixtures();
    eprintln!(
        "TEMPORAL_COHORT reference_base=339f8f3235c61c76bff9c6e69f9da7089252b7e4 reference_r3_source_gross=736 execution_identity=parent_receipt fixture_count={}",
        cases.len()
    );
    for case in &cases {
        if let Some(wrappers) = &case.wrappers {
            let text = source(wrappers, "true", "false", case.constant_n);
            if let Ok(model) = admitted(case.id, &text) {
                let _observation = observe(&format!("{}/checked", case.id), &model);
            }
        } else {
            eprintln!(
                "TEMPORAL_API_ONLY id={} checked_source=not_represented",
                case.id
            );
        }
        let model = raw_model(case, "true", "false");
        let _observation = observe(&format!("{}/raw", case.id), &model);
    }
    // Every literal pair gets its own public witness probe and response twin,
    // including (1,1). A current unsupported error is an observation, not RED.
    for (id, rows, selectors) in literal_rosters() {
        let case = cases.iter().find(|case| case.id == id).unwrap();
        for (row, selector) in rows.iter().zip(selectors) {
            for response in [false, true] {
                let after = if response {
                    "true".into()
                } else {
                    format!("not ({selector})")
                };
                eprintln!(
                    "TEMPORAL_LITERAL_INPUT id={id} row={row:#?} selector={selector:?} response={response}"
                );
                let text = source(case.wrappers.as_ref().unwrap(), "true", &after, false);
                if let Ok(model) = admitted(id, &text) {
                    let _observation =
                        observe(&format!("{id}/checked/{selector}/{response}"), &model);
                }
                let model = raw_model(case, "true", &after);
                let _observation = observe(&format!("{id}/raw/{selector}/{response}"), &model);
            }
        }
    }
    for (id, row, selector) in extreme_literal_rows() {
        let case = cases.iter().find(|case| case.id == id).unwrap();
        for response in [false, true] {
            let model = extreme_probe(case, selector.clone(), response);
            eprintln!(
                "TEMPORAL_EXTREME_LITERAL_INPUT id={id} row={row:#?} selector={selector:#?} response={response}"
            );
            let _observation = observe(&format!("{id}/raw/{response}"), &model);
        }
    }
    let named = cases
        .iter()
        .find(|case| case.id == "T10-named-domain")
        .unwrap();
    let _identity = named_outer_identity(&raw_model(named, "true", "true"));
    let triangle = cases.iter().find(|case| case.id == "T01-triangle").unwrap();
    let text = source(
        triangle.wrappers.as_ref().unwrap(),
        "true",
        "not (i == 0 and j == 1)",
        false,
    );
    if let Ok(model) = admitted("T01-impossible-0-1", &text) {
        let _observation = observe("T01-impossible-0-1/checked", &model);
    }
    let model = raw_model(triangle, "true", "not (i == 0 and j == 1)");
    let _observation = observe("T01-impossible-0-1/raw", &model);
    let _restoration = record_property_restoration();
    eprintln!("TEMPORAL_RECORDING_COMPLETE desired_future_semantics=not_asserted");
}

fn literal_state() -> Row {
    BTreeMap::from([("n".into(), V::Int(0)), ("ready".into(), V::Bool(false))])
}

fn status(pending: Vec<BoundedLivenessPending>) -> BoundedLivenessStatus {
    BoundedLivenessStatus {
        checked_properties: vec!["L".into()],
        unbounded_properties: vec![],
        pending,
    }
}

fn expected_bmc(binding: Option<&Row>) -> BmcResult {
    let leadsto_violation = binding.map(|row| BmcViolation {
        kind: "leadsTo".into(),
        name: "L".into(),
        step: 1,
        last_action: Some("idle".into()),
        trace: vec![
            TraceStep {
                step: 0,
                state: literal_state(),
                action: None,
                changes: BTreeMap::new(),
            },
            TraceStep {
                step: 1,
                state: literal_state(),
                action: Some(TraceAction {
                    name: "idle".into(),
                    params: BTreeMap::new(),
                }),
                changes: BTreeMap::new(),
            },
        ],
        leads_to: Some(LeadsToViolation {
            bindings: row.clone(),
            pending_since: 0,
            loop_start: None,
            deadline: Some(1),
            within: Some(1),
            stutter: false,
            hint:
                "leadsTo deadline missed: P holds at step 0, but Q does not hold within 1 step(s)"
                    .into(),
        }),
    });
    BmcResult {
        spec: "Temporal".into(),
        depth: 1,
        violation: None,
        leadsto_violation,
        reachables: BTreeMap::new(),
        reachable_diagnostics: BTreeMap::new(),
        deadlock_step: None,
        deadlock_trace: None,
        action_coverage: BTreeMap::from([("idle".into(), true)]),
        frontier_progress: false,
        vacuity: vec![],
    }
}

fn assert_public(observed: &PublicObservation, roster: &[Row], deciding: Option<&Row>) {
    assert_eq!(observed.roster, Ok(roster.to_vec()));
    assert_eq!(observed.initial, Ok(literal_state()));
    assert_eq!(observed.monitor, Ok(()));
    assert_eq!(observed.first, Some(Ok(None)));
    let pending = deciding
        .map(|row| BoundedLivenessPending {
            property: "L".into(),
            bindings: row.clone(),
            pending_since: 0,
            deadline: 1,
            within: 1,
        })
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(observed.first_status, Some(status(pending.clone())));
    assert_eq!(
        observed.idle,
        Some(Ok(StepResult {
            action: "idle".into(),
            params: BTreeMap::new(),
            state: literal_state(),
            attempted_state: None,
            violation: None,
        }))
    );
    let violation = deciding.map(|row| BoundedLivenessViolation {
        property: "L".into(),
        bindings: row.clone(),
        pending_since: 0,
        deadline: 1,
        within: 1,
        step: 1,
    });
    assert_eq!(observed.second, Some(Ok(violation)));
    // A missed-deadline return leaves its pending record available to status.
    assert_eq!(observed.second_status, Some(status(pending)));
    let expected = expected_bmc(deciding);
    eprintln!("TEMPORAL_EXPECTED_BMC whole={expected:#?}");
    assert_eq!(observed.bmc, Ok(expected));
}

#[test]
fn future_literal_rosters_and_each_public_witness() {
    let cases = fixtures();
    for (id, rows, selectors) in literal_rosters() {
        let case = cases.iter().find(|case| case.id == id).unwrap();
        assert_eq!(rows.len(), selectors.len());
        for (row, selector) in rows.iter().zip(selectors) {
            for response in [false, true] {
                let after = if response {
                    "true".into()
                } else {
                    format!("not ({selector})")
                };
                let text = source(case.wrappers.as_ref().unwrap(), "true", &after, false);
                let checked = admitted(id, &text).expect("future checked admission");
                let raw = raw_model(case, "true", &after);
                for (route, model) in [("checked", checked), ("raw", raw)] {
                    let observed = observe(&format!("{id}/{route}/{selector}/{response}"), &model);
                    assert_public(&observed, &rows, if response { None } else { Some(row) });
                }
            }
        }
    }
    let named = cases
        .iter()
        .find(|case| case.id == "T10-named-domain")
        .unwrap();
    let checked = admitted(
        "T10-named-domain/identity",
        &source(named.wrappers.as_ref().unwrap(), "true", "true", false),
    )
    .unwrap();
    let raw = raw_model(named, "true", "true");
    for model in [checked, raw] {
        let (inferred, values) = named_outer_identity(&model);
        assert_eq!(model.leadstos[0].binders[0], typed("k", "K"));
        assert_eq!(
            model.types.get("K"),
            Some(&TypeDef::Domain {
                lo: 0,
                hi: 1,
                symmetric: false
            })
        );
        assert_eq!(inferred, Ok(Some(TypeRef::Named("K".into()))));
        assert_eq!(values, Ok(vec![V::Int(0), V::Int(1)]));
    }
    // Reject a rectangular fourth row through actual public temporal behavior.
    let triangle = cases.iter().find(|case| case.id == "T01-triangle").unwrap();
    let text = source(
        triangle.wrappers.as_ref().unwrap(),
        "true",
        "not (i == 0 and j == 1)",
        false,
    );
    let model = admitted("T01-impossible-0-1", &text).unwrap();
    let raw = raw_model(triangle, "true", "not (i == 0 and j == 1)");
    for (route, model) in [("checked", model), ("raw", raw)] {
        let observed = observe(&format!("T01-impossible-0-1/{route}"), &model);
        assert_public(&observed, &[ints(0, 0), ints(1, 0), ints(1, 1)], None);
    }
}

#[test]
fn future_extreme_singleton_public_witnesses() {
    let cases = fixtures();
    for (id, row, selector) in extreme_literal_rows() {
        let case = cases.iter().find(|case| case.id == id).unwrap();
        for response in [false, true] {
            let model = extreme_probe(case, selector.clone(), response);
            let observed = observe(&format!("{id}/raw/{response}"), &model);
            assert_public(
                &observed,
                std::slice::from_ref(&row),
                if response { None } else { Some(&row) },
            );
        }
    }
}

#[test]
fn future_raw_shadow_duplicate_and_empty_dependent_rosters() {
    let cases = fixtures();
    let coincident = BTreeMap::from([("i".into(), V::Int(0))]);
    let i0k0 = BTreeMap::from([("i".into(), V::Int(0)), ("k".into(), V::Int(0))]);
    let i1k1 = BTreeMap::from([("i".into(), V::Int(1)), ("k".into(), V::Int(1))]);
    for (id, expected) in [
        ("T05-coincident", vec![coincident.clone(), coincident]),
        ("T06-own-name", vec![i0k0.clone(), i1k1]),
        ("T07-latest-name", vec![i0k0.clone(), i0k0]),
        ("E01-empty-dependent", vec![]),
    ] {
        let case = cases.iter().find(|case| case.id == id).unwrap();
        let model = raw_model(case, "true", "true");
        let observed = observe(&format!("{id}/raw/future"), &model);
        assert_public(&observed, &expected, None);
    }
    let (model, actual_first, actual_second) = record_property_restoration();
    let expected_first = vec![
        BTreeMap::from([("i".into(), V::Int(0)), ("k".into(), V::Int(0))]),
        BTreeMap::from([("i".into(), V::Int(0)), ("k".into(), V::Int(0))]),
    ];
    assert_eq!(actual_first, Ok(expected_first));
    assert_eq!(
        actual_second,
        Ok(vec![BTreeMap::from([("j".into(), V::Int(5))])])
    );
    assert_eq!(model.consts.get("i"), Some(&V::Int(5)));
}

fn record_property_restoration() -> (KernelModel, RosterResult, RosterResult) {
    let case = fixture(
        "S03-property-restore",
        &[],
        vec![
            range("i", "1", "2"),
            range("i", "0", "0"),
            range("k", "i", "i"),
        ],
    );
    let mut model = raw_model(&case, "true", "true");
    model.consts.insert("i".into(), V::Int(5));
    let mut second = model.leadstos[0].clone();
    second.name = "Second".into();
    second.binders = vec![range("j", "i", "i")];
    model.leadstos.push(second);
    let first_rows = static_leadsto_bindings(&model, &model.leadstos[0]);
    let second_rows = static_leadsto_bindings(&model, &model.leadstos[1]);
    eprintln!(
        "TEMPORAL_PROPERTY_RESTORE whole_model={model:#?} first={first_rows:#?} second={second_rows:#?}"
    );
    let _observed = observe("S03-property-restore/raw", &model);
    (model, first_rows, second_rows)
}

// Frozen from both immutable CURRENT-R3 recordings, before the new production
// proposal. Messages preserve separate Core/Runtime and verifier ownership.
const LEGACY_STATIC_BOUNDS: &str = "leadsTo range binder must use static integer bounds";
const LEGACY_INVALID_STATIC: &str = "invalid static integer expression";
const LEGACY_PLANNER: &str =
    "unsupported integer range execution [planner]: unsupported-expression";
const LEGACY_ELIGIBILITY: &str = "unsupported integer range execution [eligibility]: prerequisites are not established for this evaluation scope";
const LEGACY_FILTER: &str = "leadsTo wrapper binders do not support where filters";
const LEGACY_COLLECTION: &str = "leadsTo wrapper binders do not support collection domains";
const LEGACY_VERIFIER_COLLECTION: &str =
    "collection binders are not implemented in the current verifier slice";
const LEGACY_QUALIFIED: &str = "qualified types must be lowered before public Kernel export";
const LEGACY_VERIFIER_QUALIFIED: &str = "qualified type remained after kernel lowering";
const LEGACY_MISSING: &str = "'missing' is not an integer constant";
const LEGACY_FREE_STATE: &str = "'n' is not an integer constant";

fn literal_model_refusal(message: &str) -> ModelError {
    ModelError {
        message: message.into(),
        origin: None,
        span: None,
        name_resolution: false,
        diagnostic_code: None,
        hint: None,
        quick_fix: None,
    }
}

const CHECKED_BINDER_SPAN: fsl_core::Span = fsl_core::Span {
    start: fsl_syntax::SourcePos {
        offset: 170,
        line: 1,
        column: 171,
    },
    end: fsl_syntax::SourcePos {
        offset: 177,
        line: 1,
        column: 178,
    },
};

const CHECKED_INACTIVE_ARM_SPAN: fsl_core::Span = fsl_core::Span {
    start: fsl_syntax::SourcePos {
        offset: 237,
        line: 1,
        column: 238,
    },
    end: fsl_syntax::SourcePos {
        offset: 242,
        line: 1,
        column: 243,
    },
};

fn literal_type_refusal(message: &str, id: &str, span: fsl_core::Span) -> ModelError {
    ModelError {
        origin: Some(Box::new(fsl_core::OriginChain {
            id: fsl_core::OriginId(id.into()),
            dialect: "kernel".into(),
            primary: Some(fsl_core::OriginSite {
                source_file: None,
                span: Some(span),
                dialect: "kernel".into(),
                declaration_path: vec![],
            }),
            secondary: vec![],
            lowering_steps: vec![],
            generated: false,
        })),
        ..literal_model_refusal(message)
    }
}

fn literal_checked_refusal(id: &str) -> Option<ModelError> {
    let wrong_type = "invalid model expression: expression of type Bool is not assignable to Int";
    let missing = "invalid model expression: public Kernel cannot type identifier 'missing'";
    match id {
        "E04-empty-filter" | "F03-filter" => Some(ModelError {
            span: Some(CHECKED_BINDER_SPAN),
            ..literal_model_refusal(LEGACY_FILTER)
        }),
        "E05-empty-collection" | "F04-collection" => Some(ModelError {
            span: Some(CHECKED_BINDER_SPAN),
            ..literal_model_refusal(LEGACY_COLLECTION)
        }),
        "E06-empty-struct" | "F05-struct" => Some(literal_type_refusal(
            "invalid model expression: type 'Cell' is not a finite scalar",
            "kernel:type-diagnostic:170:177",
            CHECKED_BINDER_SPAN,
        )),
        "E08-empty-bool-bound" => Some(literal_type_refusal(
            wrong_type,
            "kernel:type-diagnostic:170:177",
            CHECKED_BINDER_SPAN,
        )),
        "E09-empty-inactive-type-error" => Some(literal_type_refusal(
            wrong_type,
            "kernel:type-diagnostic:237:242",
            CHECKED_INACTIVE_ARM_SPAN,
        )),
        "F07-missing-name"
        | "P01-failed-fold-then-missing"
        | "P02-missing-then-failed-fold"
        | "P03-lo-before-hi"
        | "P04-lo-failed-before-hi" => Some(literal_type_refusal(
            missing,
            "kernel:type-diagnostic:170:177",
            CHECKED_BINDER_SPAN,
        )),
        "E02-empty-static-divzero"
        | "E03-empty-static-overflow"
        | "E07-empty-unbounded"
        | "F01-free-state"
        | "F06-unbounded"
        | "F08-negation-overflow" => None,
        _ => panic!("no frozen checked-admission contract for {id}"),
    }
}

fn assert_literal_refusal(observed: &PublicObservation, core: &str, verifier: &str) {
    assert_eq!(observed.roster, Err(literal_model_refusal(core)));
    assert_eq!(observed.initial, Ok(literal_state()));
    assert_eq!(
        observed.monitor,
        Err(RuntimeError {
            message: core.into(),
            span: None,
            partial_operation: None,
            map_key_outside_domain: false,
        })
    );
    assert_eq!(observed.first, None);
    assert_eq!(observed.first_status, None);
    assert_eq!(observed.idle, None);
    assert_eq!(observed.second, None);
    assert_eq!(observed.second_status, None);
    assert_eq!(
        observed
            .bmc
            .as_ref()
            .err()
            .map(VerifyError::is_solver_failure),
        Some(false)
    );
    assert_eq!(observed.bmc, Err(VerifyError::new(verifier)));
}

#[test]
// Keep the frozen literal refusal table beside its checked/raw assertions.
#[allow(clippy::too_many_lines)]
fn current_legacy_static_error_contracts() {
    let cases = fixtures();
    for (id, core, verifier) in [
        ("Q01-qualified", LEGACY_QUALIFIED, LEGACY_VERIFIER_QUALIFIED),
        (
            "Q02-qualified-before-failed",
            LEGACY_QUALIFIED,
            LEGACY_VERIFIER_QUALIFIED,
        ),
        (
            "Q03-failed-before-qualified",
            LEGACY_STATIC_BOUNDS,
            LEGACY_INVALID_STATIC,
        ),
        (
            "Q04-empty-before-qualified",
            LEGACY_QUALIFIED,
            LEGACY_VERIFIER_QUALIFIED,
        ),
        (
            "P01-failed-fold-then-missing",
            LEGACY_STATIC_BOUNDS,
            LEGACY_INVALID_STATIC,
        ),
        (
            "P02-missing-then-failed-fold",
            LEGACY_MISSING,
            LEGACY_PLANNER,
        ),
        ("P03-lo-before-hi", LEGACY_MISSING, LEGACY_PLANNER),
        (
            "P04-lo-failed-before-hi",
            LEGACY_STATIC_BOUNDS,
            LEGACY_INVALID_STATIC,
        ),
        (
            "E02-empty-static-divzero",
            LEGACY_STATIC_BOUNDS,
            LEGACY_INVALID_STATIC,
        ),
        (
            "E03-empty-static-overflow",
            LEGACY_STATIC_BOUNDS,
            LEGACY_INVALID_STATIC,
        ),
        ("E04-empty-filter", LEGACY_FILTER, LEGACY_FILTER),
        (
            "E05-empty-collection",
            LEGACY_COLLECTION,
            LEGACY_VERIFIER_COLLECTION,
        ),
        (
            "E06-empty-struct",
            "type 'Cell' is not a finite scalar",
            "type 'Cell' is not a finite scalar",
        ),
        (
            "E07-empty-unbounded",
            "type 'Int' is not a finite scalar",
            "type 'Int' is not a finite scalar",
        ),
        ("E08-empty-bool-bound", LEGACY_STATIC_BOUNDS, LEGACY_PLANNER),
        (
            "E09-empty-inactive-type-error",
            LEGACY_STATIC_BOUNDS,
            LEGACY_PLANNER,
        ),
        ("F01-free-state", LEGACY_FREE_STATE, LEGACY_ELIGIBILITY),
        ("F03-filter", LEGACY_FILTER, LEGACY_FILTER),
        (
            "F04-collection",
            LEGACY_COLLECTION,
            LEGACY_VERIFIER_COLLECTION,
        ),
        (
            "F05-struct",
            "type 'Cell' is not a finite scalar",
            "type 'Cell' is not a finite scalar",
        ),
        (
            "F06-unbounded",
            "type 'Int' is not a finite scalar",
            "type 'Int' is not a finite scalar",
        ),
        ("F07-missing-name", LEGACY_MISSING, LEGACY_PLANNER),
        (
            "F08-negation-overflow",
            LEGACY_STATIC_BOUNDS,
            "integer overflow in static negation",
        ),
    ] {
        let case = cases.iter().find(|case| case.id == id).unwrap();
        let raw = raw_model(case, "true", "false");
        let observed = observe(&format!("{id}/raw/frozen"), &raw);
        assert_literal_refusal(&observed, core, verifier);
        match &case.wrappers {
            Some(wrappers) => {
                let checked = admitted(id, &source(wrappers, "true", "false", case.constant_n));
                if let Some(expected) = literal_checked_refusal(id) {
                    let actual = checked.expect_err("frozen rejection");
                    if matches!(id, "E06-empty-struct" | "F05-struct") {
                        assert_eq!(
                            actual.to_string(),
                            "invalid model expression: type 'Cell' is not a finite scalar at 1:171"
                        );
                    }
                    assert_eq!(actual, expected);
                } else {
                    let model = checked.expect("frozen checked admission");
                    let observed = observe(&format!("{id}/checked/frozen"), &model);
                    assert_literal_refusal(&observed, core, verifier);
                }
            }
            None => assert!(matches!(
                id,
                "Q01-qualified"
                    | "Q02-qualified-before-failed"
                    | "Q03-failed-before-qualified"
                    | "Q04-empty-before-qualified"
            )),
        }
    }
}

#[test]
fn future_free_state_const_collision_refuses_without_admission_change() {
    let cases = fixtures();
    let collision = cases
        .iter()
        .find(|case| case.id == "F02-state-const-collision")
        .unwrap();
    let checked = admitted(
        collision.id,
        &source(collision.wrappers.as_ref().unwrap(), "true", "false", true),
    )
    .expect("existing checked admission remains accepted");
    let raw = raw_model(collision, "true", "false");
    for (route, model) in [("checked", checked), ("raw", raw)] {
        let observed = observe(&format!("F02/{route}/future-refusal"), &model);
        assert_literal_refusal(&observed, LEGACY_FREE_STATE, LEGACY_ELIGIBILITY);
    }
    // The reference to n is an actual enclosing binder here, despite the state
    // field and optional global namesake. This raw control adopts no source
    // admission change for shadowing.
    for constant_n in [false, true] {
        let local = Fixture {
            id: "F02-enclosing-local-n",
            wrappers: None,
            binders: vec![range("n", "0", "0"), range("i", "0", "n")],
            constant_n,
        };
        let model = raw_model(&local, "true", "true");
        let expected = BTreeMap::from([("i".into(), V::Int(0)), ("n".into(), V::Int(0))]);
        let observed = observe(&format!("F02/local-n/const-{constant_n}"), &model);
        assert_public(&observed, &[expected], None);
    }
}

#[test]
// Keep written binder/endpoint priority cases in one ordered literal table.
#[allow(clippy::too_many_lines)]
fn future_outer_dependent_error_priority() {
    // Parent-adopted new-feature envelope, grounded in the existing scalar
    // folder and From<ModelError> conversion. It is not a recorded legacy
    // dependent-range result: that capability was previously unsupported.
    let division = "division by zero";
    let scalar = "type 'Cell' is not a finite scalar";
    for (id, binders, core, verifier) in [
        (
            "D01-reachable-dependent-before-qualified",
            vec![
                range("i", "0", "0"),
                range("j", "0", "i/(i-i)"),
                qualified(),
            ],
            division,
            division,
        ),
        (
            "D02-empty-dependent-before-qualified",
            vec![
                range("i", "1", "0"),
                range("j", "0", "i/(i-i)"),
                qualified(),
            ],
            LEGACY_QUALIFIED,
            LEGACY_VERIFIER_QUALIFIED,
        ),
        (
            "D03-reachable-dependent-before-missing",
            vec![
                range("i", "0", "0"),
                range("j", "0", "i/(i-i)"),
                range("k", "0", "missing"),
            ],
            division,
            division,
        ),
        (
            "D04-empty-dependent-before-missing",
            vec![
                range("i", "1", "0"),
                range("j", "0", "i/(i-i)"),
                range("k", "0", "missing"),
            ],
            LEGACY_MISSING,
            LEGACY_PLANNER,
        ),
        (
            "D05-reachable-dependent-lo-before-static-hi",
            vec![range("i", "0", "0"), range("j", "i/(i-i)", "missing")],
            division,
            division,
        ),
        (
            "D06-empty-dependent-lo-with-static-hi",
            vec![range("i", "1", "0"), range("j", "i/(i-i)", "missing")],
            LEGACY_MISSING,
            LEGACY_PLANNER,
        ),
        (
            "D07-reachable-static-lo-before-dependent-hi",
            vec![range("i", "0", "0"), range("j", "missing", "i/(i-i)")],
            LEGACY_MISSING,
            LEGACY_PLANNER,
        ),
        (
            "D08-empty-static-lo-before-dependent-hi",
            vec![range("i", "1", "0"), range("j", "missing", "i/(i-i)")],
            LEGACY_MISSING,
            LEGACY_PLANNER,
        ),
        (
            "D09-reachable-missing-before-dependent",
            vec![
                range("i", "0", "0"),
                range("k", "0", "missing"),
                range("j", "0", "i/(i-i)"),
            ],
            LEGACY_MISSING,
            LEGACY_PLANNER,
        ),
        (
            "D10-reachable-qualified-before-dependent",
            vec![
                range("i", "0", "0"),
                qualified(),
                range("j", "0", "i/(i-i)"),
            ],
            LEGACY_QUALIFIED,
            LEGACY_VERIFIER_QUALIFIED,
        ),
        (
            "D11-reachable-dependent-before-static-divzero",
            vec![
                range("i", "0", "0"),
                range("j", "0", "i/(i-i)"),
                range("k", "0", "1/0"),
            ],
            division,
            division,
        ),
        (
            "D12-empty-dependent-before-static-divzero",
            vec![
                range("i", "1", "0"),
                range("j", "0", "i/(i-i)"),
                range("k", "0", "1/0"),
            ],
            LEGACY_STATIC_BOUNDS,
            LEGACY_INVALID_STATIC,
        ),
        (
            "D13-reachable-dependent-before-nonscalar",
            vec![
                range("i", "0", "0"),
                range("j", "0", "i/(i-i)"),
                typed("c", "Cell"),
            ],
            division,
            division,
        ),
        (
            "D14-empty-dependent-before-nonscalar",
            vec![
                range("i", "1", "0"),
                range("j", "0", "i/(i-i)"),
                typed("c", "Cell"),
            ],
            scalar,
            scalar,
        ),
    ] {
        let case = Fixture {
            id,
            wrappers: None,
            binders,
            constant_n: false,
        };
        let model = raw_model(&case, "true", "false");
        let observed = observe(id, &model);
        assert_literal_refusal(&observed, core, verifier);
    }
    // With no independent failure after the empty prefix, the dependent
    // division is never evaluated and the complete public result is successful.
    let empty = Fixture {
        id: "D15-empty-dependent-without-later-error",
        wrappers: None,
        binders: vec![range("i", "1", "0"), range("j", "0", "i/(i-i)")],
        constant_n: false,
    };
    let observed = observe(empty.id, &raw_model(&empty, "true", "true"));
    assert_public(&observed, &[], None);
}

#[test]
fn future_dependent_endpoint_full_authored_typing() {
    // These are the existing full checker's literal messages, adopted for the
    // new dependent endpoint path. No Model-context prefix/location is added.
    let arm = "expression of type Bool is not assignable to Int";
    let condition = "expression of type Int is not assignable to Bool";
    let bad_else = "if i == 0 then 0 else false";
    let bad_condition = "if i then 0 else 0";
    for (id, binders, core, verifier) in [
        (
            "Y01-empty-dependent-wrong-else",
            vec![range("i", "1", "0"), range("j", "0", bad_else)],
            arm,
            arm,
        ),
        (
            "Y02-reached-dependent-inactive-wrong-else",
            vec![range("i", "0", "0"), range("j", "0", bad_else)],
            arm,
            arm,
        ),
        (
            "Y03-reached-dependent-inactive-wrong-then",
            vec![
                range("i", "0", "0"),
                range("j", "0", "if i != 0 then false else 0"),
            ],
            arm,
            arm,
        ),
        (
            "Y04-empty-dependent-wrong-condition",
            vec![range("i", "1", "0"), range("j", "0", bad_condition)],
            condition,
            condition,
        ),
        (
            "Y05-empty-condition-lo-before-arm-hi",
            vec![range("i", "1", "0"), range("j", bad_condition, bad_else)],
            condition,
            condition,
        ),
        (
            "Y06-empty-arm-lo-before-condition-hi",
            vec![range("i", "1", "0"), range("j", bad_else, bad_condition)],
            arm,
            arm,
        ),
        (
            "Y07-empty-dependent-type-before-qualified",
            vec![range("i", "1", "0"), range("j", "0", bad_else), qualified()],
            arm,
            arm,
        ),
        (
            "Y08-empty-qualified-before-dependent-type",
            vec![range("i", "1", "0"), qualified(), range("j", "0", bad_else)],
            LEGACY_QUALIFIED,
            LEGACY_VERIFIER_QUALIFIED,
        ),
        (
            "Y09-empty-valid-divzero-lo-before-wrong-arm-hi",
            vec![range("i", "1", "0"), range("j", "i/(i-i)", bad_else)],
            arm,
            arm,
        ),
    ] {
        // Every prior i is the actual Int local declared by its range binder.
        // Empty prefixes supply no invented value for typing either endpoint.
        let case = Fixture {
            id,
            wrappers: None,
            binders,
            constant_n: false,
        };
        let observed = observe(id, &raw_model(&case, "true", "false"));
        assert_literal_refusal(&observed, core, verifier);
    }
    // Full authored typing must not evaluate well-typed dependent arithmetic
    // under an empty prefix, including when both endpoints would divide by zero.
    let empty = Fixture {
        id: "Y10-empty-well-typed-dependent-divisions",
        wrappers: None,
        binders: vec![range("i", "1", "0"), range("j", "i/(i-i)", "i/(i-i)")],
        constant_n: false,
    };
    let observed = observe(empty.id, &raw_model(&empty, "true", "true"));
    assert_public(&observed, &[], None);
}

#[test]
fn current_checked_dependent_bad_arm_refusal() {
    // Frozen E09's checked error identifies its else false at offsets 237..242.
    // The sole authored condition change true -> i == 0 adds exactly two bytes.
    let span = fsl_core::Span {
        start: fsl_syntax::SourcePos {
            offset: 239,
            line: 1,
            column: 240,
        },
        end: fsl_syntax::SourcePos {
            offset: 244,
            line: 1,
            column: 245,
        },
    };
    let expected = literal_type_refusal(
        "invalid model expression: expression of type Bool is not assignable to Int",
        "kernel:type-diagnostic:239:244",
        span,
    );
    let text = source(
        &["i in 1..0", "j in 0..(if i == 0 then 0 else false)"],
        "true",
        "false",
        false,
    );
    assert_eq!(
        admitted("Y11-checked-dependent-wrong-else", &text).expect_err("existing checked refusal"),
        expected
    );
}

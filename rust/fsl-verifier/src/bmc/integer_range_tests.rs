// SPDX-License-Identifier: Apache-2.0
//! Future U4 BMC child controls; registration is an UNAPPLIED integration patch.
use super::*;
use crate::eval::{
    binder_values, eval_binding, eval_in, integer_range_draft, integer_range_plan,
    prepared_quantified_status as prepared_status, prepared_quantified_value as prepared_value,
    property_range_source_status,
};
use crate::value::{TypedBinding, TypedBindings, int_term};
use fsl_core::binder_range::{RangePlan, RangeUnknown};
use fsl_core::{FsResolver, KernelBinder, KernelExpr, TypeRef, build_model, parse_kernel_source};
type Z = fsl_solver_z3::Z3Solver;
type T = <Z as SmtSolver>::Term;
type Scope = TypedBindings<T>;
const ELIGIBILITY: &str = "unsupported integer range execution [eligibility]: prerequisites are not established for this evaluation scope";
const BASE: &str = "spec U4 {const c=7 type K=0..2 enum Tag {Red,Blue} state {n:K,u:Int} init {n=1 u=0} action go(lim:K){} action flag(b:Bool){} action tag(e:Tag){} }";
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
    {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("native solver yielded Pending"),
    }
}
fn model() -> KernelModel {
    build_model(parse_kernel_source(BASE, &FsResolver::new(".")).unwrap()).unwrap()
}
fn expr(text: &str) -> KernelExpr {
    fsl_syntax::parse_expr(text).unwrap()
}
fn binder(text: &str) -> KernelBinder {
    let KernelExpr::Quantified { binder, .. } = expr(&format!("forall {text} {{true}}")) else {
        panic!("quantifier fixture")
    };
    binder
}
fn truth(s: &mut Z, term: &T, expected: bool) {
    eprintln!("U4_TRUTH term={term:?} expected={expected}");
    let context = block_on(s.check());
    eprintln!("U4_CONTEXT actual={context:?}");
    assert_eq!(context.unwrap(), SatResult::Sat);
    s.push();
    let opposite = if expected {
        s.not(term).unwrap()
    } else {
        term.clone()
    };
    s.assert(&opposite).unwrap();
    let opposite = block_on(s.check());
    eprintln!("U4_OPPOSITE_QUERY actual={opposite:?}");
    assert_eq!(opposite.unwrap(), SatResult::Unsat);
    s.pop(1).unwrap();
}
fn frame(s: &mut Z, m: &KernelModel, index: usize, n: i64) -> SymbolicState<T> {
    let state = symbolic_state(s, m, index).unwrap();
    for (name, value) in [("n", n), ("u", 0)] {
        let equal = s
            .equal(int_term(&state[name]).unwrap(), &s.int_value(value))
            .unwrap();
        s.assert(&equal).unwrap();
    }
    assert_eq!(block_on(s.check()).unwrap(), SatResult::Sat);
    state
}
fn checked<'f>(
    s: &mut Z,
    m: &KernelModel,
    read: &'f SymbolicState<T>,
) -> CheckedStateCarriers<'f, T> {
    let mut names = BTreeSet::new();
    for (name, _) in &m.state {
        let valid = bounds(s, m, &read[name]).unwrap();
        let violated = block_on(probe_not(s, &valid)).unwrap();
        eprintln!("U4_REAL_BOUND {name} violated={violated}");
        assert!(!violated);
        s.assert(&valid).unwrap();
        names.insert(name.clone()); // Only AFTER actual check AND successful assert.
    }
    CheckedStateCarriers {
        read_state: read,
        old_frame: None,
        names,
    }
}
fn capture(
    s: &Z,
    m: &KernelModel,
    read: &SymbolicState<T>,
    scope: &mut Scope,
    name: &str,
    rhs: &str,
) {
    let value = eval_binding(s, m, &expr(rhs), read, scope, None).unwrap();
    eprintln!("U4_REAL_CAPTURE {name}={rhs} actual={value:?}");
    assert_eq!(value.binding.1, Some(TypeRef::Int));
    assert!(value.range_fact.as_ref().unwrap().valid_when().is_some());
    scope.insert(name.into(), value);
}
fn prepare<'f>(
    s: &mut Z,
    m: &KernelModel,
    b: &'f KernelBinder,
    read: &'f SymbolicState<T>,
    scope: &'f Scope,
    ledger: &CheckedStateCarriers<'f, T>,
) -> Result<DischargedIntegerRange<'f, T>, VerifyError> {
    let plan = integer_range_plan(m, b, scope);
    let status = property_range_source_status(s, m, b, read, scope)?;
    eprintln!("U4_DRAFT plan={plan:?} status={status:?}");
    assert!(!block_on(probe_not(s, &status.fully_defined))?);
    s.assert(&status.fully_defined)?;
    let draft = integer_range_draft(s, m, b, read, scope, (plan, status))?;
    block_on(discharge_integer_range(s, draft, ledger))
}
fn refusal<R: std::fmt::Debug>(result: Result<R, VerifyError>, message: &str) {
    eprintln!("U4_REFUSAL_PAYLOAD actual={result:?}");
    match result {
        Err(error) => {
            eprintln!("U4_REFUSAL {error:?}");
            assert_eq!(error, VerifyError::new(message));
        }
        Ok(_) => panic!("unexpected permission/candidate list; expected {message}"),
    }
}
fn refuse_prepared(
    result: Result<DischargedIntegerRange<'_, T>, VerifyError>,
    message: &str,
    s: &mut Z,
) {
    match result {
        Err(error) => {
            eprintln!("U4_PREPARED_REFUSAL {error:?}");
            assert_eq!(error, VerifyError::new(message));
        }
        Ok(prepared) => {
            eprintln!("U4_WRONG_PREPARED candidates={:?}", prepared.candidates());
            let context = block_on(s.check());
            eprintln!("U4_WRONG_PREPARED context={context:?}");
            if matches!(context, Ok(SatResult::Sat)) {
                for (name, value, membership) in prepared.candidates() {
                    eprintln!(
                        "U4_WRONG_CANDIDATE name={name} binding={value:?} value={:?} membership={membership:?} truth={:?}",
                        int_term(&value.0).map(|term| s.model_eval(term)),
                        s.model_eval(membership)
                    );
                }
            } else {
                eprintln!(
                    "U4_WRONG_PREPARED concrete values/M truth unavailable in non-SAT context={context:?}; complete terms are retained above"
                );
            }
            panic!("unexpected prepared payload; expected {message}");
        }
    }
}
fn roster(s: &mut Z, prepared: &DischargedIntegerRange<'_, T>, expected: &[(i64, bool)]) {
    let candidates = prepared.candidates();
    eprintln!("U4_CANDIDATES {candidates:?}");
    assert_eq!(candidates.len(), expected.len());
    for ((name, value, membership), (integer, present)) in candidates.iter().zip(expected) {
        assert_eq!(name, "i");
        assert_eq!(value.1, Some(TypeRef::Int));
        let same = s
            .equal(int_term(&value.0).unwrap(), &s.int_value(*integer))
            .unwrap();
        truth(s, &same, true);
        truth(s, membership, *present);
    }
}
fn reducers(
    s: &mut Z,
    m: &KernelModel,
    prepared: &DischargedIntegerRange<'_, T>,
    body: &str,
    expected: bool,
) {
    let pointer = prepared.candidates().as_ptr();
    let status = prepared_status(s, m, "forall", &expr(body), prepared).unwrap();
    eprintln!("U4_PREPARED_STATUS {status:?}");
    truth(s, &status.fully_defined, true);
    truth(s, &status.first_partial, false);
    assert!(!status.has_partial_operation);
    let value = prepared_value(s, m, "forall", &expr(body), prepared).unwrap();
    eprintln!("U4_PREPARED_VALUE {value:?}");
    truth(s, bool_term(&value).unwrap(), expected);
    assert_eq!(pointer, prepared.candidates().as_ptr()); // Interface identity, not hidden-work proof.
}
#[test]
fn captured_singleton_membership_is_not_permission_and_both_reducers_borrow() {
    for repeat in 0..2 {
        let mut solver = Z::new().unwrap();
        let m = model();
        let read = frame(&mut solver, &m, 0, 1);
        let mut scope = Scope::new();
        capture(&solver, &m, &read, &mut scope, "top", "n+1");
        capture(&solver, &m, &read, &mut scope, "next", "top+1");
        let ledger = checked(&mut solver, &m, &read);
        for (text, expected, body) in [
            (
                "i in top..top",
                vec![(1, false), (2, true), (3, false)],
                "i==2",
            ),
            (
                "i in next..next",
                vec![(2, false), (3, true), (4, false)],
                "i==3",
            ),
        ] {
            let b = binder(text);
            let unbridged = binder_values(&solver, &m, &b, &read, &scope, None);
            eprintln!("U4_CAPTURED_UNBRIDGED actual={unbridged:?}");
            refusal(unbridged, ELIGIBILITY);
            let prepared = prepare(&mut solver, &m, &b, &read, &scope, &ledger).unwrap();
            eprintln!("U4_CAPTURED repeat={repeat} source={text}");
            roster(&mut solver, &prepared, &expected);
            reducers(&mut solver, &m, &prepared, body, true);
            reducers(&mut solver, &m, &prepared, "i<2", false);
        }
    }
}
#[test]
fn captures_use_original_frame_and_original_lexical_input_index() {
    for repeat in 0..2 {
        for (captured_n, current_n, allowed) in [(3, 1, false), (1, 1, true)] {
            let mut solver = Z::new().unwrap();
            let m = model();
            let old = frame(&mut solver, &m, 0, captured_n);
            let read = frame(&mut solver, &m, 1, current_n);
            let bad = frame(&mut solver, &m, 2, 3);
            let mut scope = Scope::new();
            capture(&solver, &m, &bad, &mut scope, "aaa", "n+1");
            capture(&solver, &m, &old, &mut scope, "top", "n+1");
            let ledger = checked(&mut solver, &m, &read);
            let b = binder("i in top..top");
            let result = prepare(&mut solver, &m, &b, &read, &scope, &ledger);
            eprintln!(
                "U4_CAPTURE_FRAME repeat={repeat} captured_n={captured_n} current_n={current_n}"
            );
            if allowed {
                let prepared = result.unwrap();
                roster(&mut solver, &prepared, &[(1, false), (2, true), (3, false)]);
                reducers(&mut solver, &m, &prepared, "i==2", true);
            } else {
                refuse_prepared(result, ELIGIBILITY, &mut solver);
            }
            assert_eq!(block_on(solver.check()).unwrap(), SatResult::Sat); // P was PROBED, never asserted.
            let unrelated = binder("i in aaa..aaa");
            refuse_prepared(
                prepare(&mut solver, &m, &unrelated, &read, &scope, &ledger),
                ELIGIBILITY,
                &mut solver,
            );
            assert_eq!(block_on(solver.check()).unwrap(), SatResult::Sat);
        }
    }
}
#[test]
fn current_bound_failure_precedes_a_true_captured_predicate() {
    let mut solver = Z::new().unwrap();
    let m = model();
    let old = frame(&mut solver, &m, 0, 1);
    let read = frame(&mut solver, &m, 1, 3);
    let mut scope = Scope::new();
    capture(&solver, &m, &old, &mut scope, "top", "n+1");
    let fact = scope
        .records()
        .find(|(name, _)| name.as_str() == "top")
        .unwrap()
        .1
        .range_fact
        .as_ref()
        .unwrap();
    truth(&mut solver, fact.valid_when().unwrap(), true);
    let instances = action_instances(&solver, &m).unwrap();
    let states = vec![read];
    let pending = BTreeSet::new();
    let actual = block_on(check_state_properties(
        &mut solver,
        &m,
        &states,
        &[],
        &instances,
        0,
        StatePropertyChecks {
            checked_bounds: None,
            pending_reachables: &pending,
        },
    ));
    eprintln!("U4_CAPTURED_CURRENT_BOUND actual={actual:?}");
    let expected = BmcViolation {
        kind: "type_bound".into(),
        name: "_bounds_n".into(),
        step: 0,
        last_action: None,
        trace: vec![TraceStep {
            step: 0,
            state: BTreeMap::from([
                ("n".into(), FslValue::Int(3)),
                ("u".into(), FslValue::Int(0)),
            ]),
            action: None,
            changes: BTreeMap::new(),
        }],
        leads_to: None,
    };
    assert_eq!(actual, Ok(Some(expected)));
    // No checked ledger or discharged record is constructed after this failed check.
}
#[test]
fn empty_keeps_capture_permission_and_unreached_body_definedness() {
    for valid in [true, false] {
        let mut solver = Z::new().unwrap();
        let m = model();
        let old = frame(&mut solver, &m, 0, if valid { 1 } else { 3 });
        let read = frame(&mut solver, &m, 1, 1);
        let mut scope = Scope::new();
        capture(&solver, &m, &old, &mut scope, "top", "n+1");
        let ledger = checked(&mut solver, &m, &read);
        let b = binder("i in top..0");
        assert!(matches!(
            integer_range_plan(&m, &b, &scope),
            RangePlan::Empty { .. }
        ));
        let result = prepare(&mut solver, &m, &b, &read, &scope, &ledger);
        if valid {
            let prepared = result.unwrap();
            roster(&mut solver, &prepared, &[]);
            reducers(
                &mut solver,
                &m,
                &prepared,
                "(9223372036854775807+1)==0",
                true,
            );
            let nonempty = binder("i in top..top");
            let reached = prepare(&mut solver, &m, &nonempty, &read, &scope, &ledger).unwrap();
            let overflow_body = expr("(9223372036854775807+1)==0");
            let status = prepared_status(&solver, &m, "forall", &overflow_body, &reached).unwrap();
            eprintln!("U4_REACHED_BODY {status:?}");
            truth(&mut solver, &status.fully_defined, false);
            truth(&mut solver, &status.first_partial, false);
            assert!(!status.has_partial_operation);
        } else {
            refuse_prepared(result, ELIGIBILITY, &mut solver);
        }
        assert_eq!(block_on(solver.check()).unwrap(), SatResult::Sat);
    }
}
#[test]
fn unknown_shadowing_overwrites_and_real_parameters_do_not_invent_facts() {
    const MISSING: &str = "unsupported integer range execution [planner]: missing-finite-fact";
    let mut solver = Z::new().unwrap();
    let m = model();
    let read = frame(&mut solver, &m, 0, 1);
    let mut scope = Scope::new();
    capture(&solver, &m, &read, &mut scope, "top", "n+1");
    let copied = scope.get("top").unwrap().clone();
    scope.insert("top".into(), copied); // Existing .into() removes captured metadata.
    scope.insert(
        "c".into(),
        TypedBinding(
            SymbolicValue::Scalar {
                ty: TypeRef::Int,
                term: solver.int_value(2),
            },
            Some(TypeRef::Int),
        ),
    );
    let ledger = checked(&mut solver, &m, &read);
    for text in ["i in top..top", "i in c..c"] {
        let b = binder(text);
        assert_eq!(
            integer_range_plan(&m, &b, &scope),
            RangePlan::Unknown(RangeUnknown::MissingFiniteFact)
        );
        refuse_prepared(
            prepare(&mut solver, &m, &b, &read, &scope, &ledger),
            MISSING,
            &mut solver,
        );
        let actual = binder_values(&solver, &m, &b, &read, &scope, None);
        eprintln!("U4_UNKNOWN_SHARED actual={actual:?}");
        refusal(actual, MISSING);
    }
    let wide_source = "spec Wide {type K=0..2 type Wide=(-9223372036854775807-1)..9223372036854775807 state {n:K,u:Wide} init {n=1 u=0} action idle(){}}";
    let wide =
        build_model(parse_kernel_source(wide_source, &FsResolver::new(".")).unwrap()).unwrap();
    let wide_read = frame(&mut solver, &wide, 1, 1);
    eprintln!("U4_WIDE_INPUT source={wide_source:?} read={wide_read:?}");
    let wide_checked = checked(&mut solver, &wide, &wide_read);
    let b = binder("i in (u-u)..0");
    assert_eq!(
        integer_range_plan(&wide, &b, &Scope::new()),
        RangePlan::Unknown(RangeUnknown::EnvelopeUnrepresentable)
    );
    refuse_prepared(
        prepare(
            &mut solver,
            &wide,
            &b,
            &wide_read,
            &Scope::new(),
            &wide_checked,
        ),
        "unsupported integer range execution [planner]: envelope-unrepresentable",
        &mut solver,
    );
    let instances = action_instances(&solver, &m).unwrap();
    for (action, ty) in [
        ("flag", TypeRef::Bool),
        ("tag", TypeRef::Named("Tag".into())),
    ] {
        let instance = instances
            .iter()
            .find(|instance| instance.action == action)
            .unwrap();
        for (_, record) in instance.params.records() {
            assert_eq!(record.binding.1.as_ref(), Some(&ty));
            assert!(record.range_fact.is_none());
        }
    }
    let params = &instances
        .iter()
        .filter(|instance| instance.action == "go")
        .nth(1)
        .unwrap()
        .params;
    let b = binder("i in 0..(lim+1)");
    let actual = binder_values(&solver, &m, &b, &read, params, None).unwrap();
    eprintln!("U4_PARAMETER_ROSTER {actual:?}");
    assert_eq!(actual.len(), 3);
    for ((name, value, present), expected) in actual.iter().zip([0, 1, 2]) {
        assert_eq!(name, "i");
        let same = solver
            .equal(int_term(&value.0).unwrap(), &solver.int_value(expected))
            .unwrap();
        truth(&mut solver, &same, true);
        if let Some(present) = present {
            truth(&mut solver, present, true);
        }
    }
}
#[test]
fn popped_outer_scope_needs_fresh_checks_and_cannot_reuse_permission() {
    let mut solver = Z::new().unwrap();
    let m = model();
    solver.push();
    {
        let read = frame(&mut solver, &m, 0, 1);
        let scope = Scope::new();
        let ledger = checked(&mut solver, &m, &read);
        let b = binder("i in 0..n");
        let prepared = prepare(&mut solver, &m, &b, &read, &scope, &ledger).unwrap();
        roster(&mut solver, &prepared, &[(0, true), (1, true), (2, false)]);
        reducers(&mut solver, &m, &prepared, "i<=1", true);
    } // Actual record, borrowed read frame and ledger end before the outer pop.
    solver.pop(1).unwrap();
    let read = frame(&mut solver, &m, 1, 1);
    let scope = Scope::new();
    let ledger = CheckedStateCarriers {
        read_state: &read,
        old_frame: None,
        names: BTreeSet::new(),
    };
    let b = binder("i in 0..n");
    refuse_prepared(
        prepare(&mut solver, &m, &b, &read, &scope, &ledger),
        ELIGIBILITY,
        &mut solver,
    );
    for ctx in [None, Some(&ledger)] {
        let actual = eval_in(
            &solver,
            &m,
            &expr("true and (forall i in 0..n {i<=1})"),
            &read,
            &mut Scope::new(),
            None,
            ctx,
        );
        eprintln!(
            "BRIDGE_FRESH_SCOPE context_present={} actual={actual:?}",
            ctx.is_some()
        );
        refusal(actual, ELIGIBILITY);
    }
}

#[test]
fn old_typed_and_collection_candidates_are_preserved() {
    let mut solver = Z::new().unwrap();
    let m = model();
    let read = frame(&mut solver, &m, 0, 1);
    let scope = Scope::new();
    let typed = binder("i:K");
    let actual = binder_values(&solver, &m, &typed, &read, &scope, None).unwrap();
    eprintln!("U4_TYPED {actual:?}");
    assert_eq!(actual.len(), 3);
    for ((name, value, membership), expected) in actual.iter().zip([0, 1, 2]) {
        assert_eq!(name, "i");
        assert!(membership.is_none());
        let same = solver
            .equal(int_term(&value.0).unwrap(), &solver.int_value(expected))
            .unwrap();
        truth(&mut solver, &same, true);
    }
    let collection = binder("i in Set {Red,Blue}");
    let actual = binder_values(&solver, &m, &collection, &read, &scope, None).unwrap();
    eprintln!("U4_COLLECTION {actual:?}");
    assert_eq!(actual.len(), 2);
    for ((name, value, membership), member) in actual.iter().zip(["Blue", "Red"]) {
        assert_eq!(name, "i");
        let expected = concrete_value(
            &solver,
            &m,
            &TypeRef::Named("Tag".into()),
            &FslValue::Enum {
                type_name: "Tag".into(),
                member: member.into(),
            },
        )
        .unwrap();
        let same = logical_equal(&solver, &m, &value.0, &expected).unwrap();
        truth(&mut solver, &same, true);
        truth(&mut solver, membership.as_ref().unwrap(), true);
    }
}

#[test]
fn genuine_property_partial_operations_keep_where_membership_and_source_gating() {
    let source = "spec Partial {type K=0..2 state {n:K,u:Int,q:Seq<K,1>} init {n=1 u=0 q=Seq {}} action idle(){}}";
    let m = build_model(parse_kernel_source(source, &FsResolver::new(".")).unwrap()).unwrap();
    let mut solver = Z::new().unwrap();
    let read = frame(&mut solver, &m, 0, 1);
    let empty =
        concrete_value(&solver, &m, read["q"].ty().unwrap(), &FslValue::Seq(vec![])).unwrap();
    solver
        .assert(&logical_equal(&solver, &m, &read["q"], &empty).unwrap())
        .unwrap();
    eprintln!("U4_PARTIAL_INPUT source={source:?} read={read:?} q_expected={empty:?}");
    let mut scope = Scope::new();
    capture(&solver, &m, &read, &mut scope, "top", "n+1");
    let ledger = checked(&mut solver, &m, &read);
    let singleton = "i in top..top";
    let empty = "i in top..0";
    let head = "q.head()==0";
    // Status literals: [fully_defined, first_partial, has_partial_operation].
    for (range, body, status, value, count) in [
        (empty, head, [true, false, false], Some(true), 0),
        (singleton, head, [false, true, true], None, 3),
        (singleton, "q.at(0)==0", [false, true, true], None, 3),
        (
            singleton,
            "true or q.head()==0",
            [true, false, true],
            Some(true),
            3,
        ),
        (
            singleton,
            "i<2 and q.at(0)==0",
            [true, false, true],
            Some(false),
            3,
        ),
        (
            "i in top..top where false",
            head,
            [true, false, true],
            Some(true),
            3,
        ),
        (
            "i in top..top where q.head()==0",
            "true",
            [false, true, true],
            None,
            3,
        ),
        (
            "i in top..top where i==2 or q.head()==0",
            "true",
            [true, false, true],
            Some(true),
            3,
        ),
        (
            "i in top..0 where q.head()==0",
            "true",
            [true, false, false],
            Some(true),
            0,
        ),
    ] {
        let b = binder(range);
        let prepared = prepare(&mut solver, &m, &b, &read, &scope, &ledger).unwrap();
        roster(
            &mut solver,
            &prepared,
            if count == 0 {
                &[]
            } else {
                &[(1, false), (2, true), (3, false)]
            },
        );
        let actual = prepared_status(&solver, &m, "forall", &expr(body), &prepared).unwrap();
        eprintln!(
            "U4_PARTIAL source={source:?} range={range} body={body} expected={status:?} actual={actual:?}"
        );
        truth(&mut solver, &actual.fully_defined, status[0]);
        truth(&mut solver, &actual.first_partial, status[1]);
        assert_eq!(actual.has_partial_operation, status[2]);
        if let Some(expected) = value {
            let actual = prepared_value(&solver, &m, "forall", &expr(body), &prepared).unwrap();
            eprintln!("U4_PARTIAL_VALUE actual={actual:?}");
            truth(&mut solver, bool_term(&actual).unwrap(), expected);
        }
    }
    let endpoint = binder("i in (n+q.head()+1)..0");
    let actual = property_range_source_status(&solver, &m, &endpoint, &read, &scope).unwrap();
    eprintln!("U4_PARTIAL_ENDPOINT binder={endpoint:?} read={read:?} actual={actual:?}");
    truth(&mut solver, &actual.fully_defined, false);
    truth(&mut solver, &actual.first_partial, true);
    assert!(actual.has_partial_operation); // Reversed-looking loop cannot erase reached source failure.
}

fn bridge_model(predicate: &str) -> KernelModel {
    let source = format!(
        "spec A {{ type K=0..2 state {{ n:K }} init {{ n=0 }} action go() {{}} invariant P {{ {predicate} }} }}"
    );
    let actual = build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap());
    eprintln!("BRIDGE_SOURCE source={source:?} actual={actual:?}");
    actual.unwrap()
}
fn bridge_violation(state: BTreeMap<String, FslValue>, kind: &str, name: &str) -> BmcViolation {
    BmcViolation {
        kind: kind.into(),
        name: name.into(),
        step: 0,
        last_action: None,
        trace: vec![TraceStep {
            step: 0,
            state,
            action: None,
            changes: BTreeMap::new(),
        }],
        leads_to: None,
    }
}
#[test]
fn bridge_wrapped_forall_nested_count_sum_and_conditional_literals() {
    let rows = [
        (0, "n!=0 or (forall i in 0..n { i==0 })", false),
        (0, "n!=0 or (forall i in 0..n { i!=0 })", true),
        (
            0,
            "true and (forall i in 0..n {forall j in 0..n {i==0 and j==0}})",
            false,
        ),
        (
            0,
            "true and (forall i in 0..n {forall j in 0..n {i==0 and j!=0}})",
            true,
        ),
        (0, "count(i in 0..n)==1 and sum(i in 0..n of i)==0", false),
        (0, "count(i in 0..n)==0 and sum(i in 0..n of i)==0", true),
        (1, "count(i in 0..n)==2 and sum(i in 0..n of i)==1", false),
        (1, "count(i in 0..n)==2 and sum(i in 0..n of i)==0", true),
        (2, "count(i in 0..n)==3 and sum(i in 0..n of i)==3", false),
        (2, "count(i in 0..n)==0 and sum(i in 0..n of i)==3", true),
        (
            0,
            "if n==0 then count(i in 0..n)==1 else sum(i in 0..n of i)==3",
            false,
        ),
        (
            0,
            "if n==0 then count(i in 0..n)==0 else sum(i in 0..n of i)==3",
            true,
        ),
        (
            2,
            "if n==0 then count(i in 0..n)==1 else sum(i in 0..n of i)==3",
            false,
        ),
        (
            2,
            "if n==0 then count(i in 0..n)==1 else sum(i in 0..n of i)==0",
            true,
        ),
    ];
    let outputs = rows.map(|(n, predicate, bad)| {
        let m = bridge_model(predicate);
        let state = BTreeMap::from([("n".into(), FslValue::Int(n))]);
        let mut solver = Z::new().unwrap();
        let actual = block_on(verify_bounded_from_state(&m, &mut solver, 0, None, &state));
        eprintln!("BRIDGE_LITERAL n={n} predicate={predicate:?} bad={bad} actual={actual:?}");
        let expected = bad.then(|| bridge_violation(state, "invariant", "P"));
        (actual, expected)
    });
    for (actual, expected) in outputs {
        assert_eq!(actual.map(|result| result.violation), Ok(expected));
    }
}
#[test]
fn bridge_selected_public_and_full_bound_priority_are_preserved() {
    let m = bridge_model("n!=0 or (forall i in 0..n { i==0 })");
    for n in [0, 3] {
        let state = BTreeMap::from([("n".into(), FslValue::Int(n))]);
        let mut solver = Z::new().unwrap();
        let selected = BTreeSet::new();
        let actual = block_on(verify_bounded_from_state(
            &m,
            &mut solver,
            0,
            Some(&selected),
            &state,
        ));
        eprintln!("BRIDGE_SELECTED state={state:?} actual={actual:?}");
        assert_eq!(actual, Err(VerifyError::new(ELIGIBILITY)));
        for expected in [FslValue::Bool(false), FslValue::Bool(true)] {
            let mut solver = Z::new().unwrap();
            let actual = block_on(crate::expression_matches_value(
                &m,
                &mut solver,
                &m.invariants[0].expr,
                &state,
                &expected,
            ));
            eprintln!("BRIDGE_PUBLIC state={state:?} expected={expected:?} actual={actual:?}");
            assert_eq!(actual, Err(VerifyError::new(ELIGIBILITY)));
        }
    }
    let state = BTreeMap::from([("n".into(), FslValue::Int(3))]);
    let mut solver = Z::new().unwrap();
    let actual = block_on(verify_bounded_from_state(&m, &mut solver, 0, None, &state));
    eprintln!("BRIDGE_BOUND actual={actual:?}");
    assert_eq!(
        actual.unwrap().violation,
        Some(bridge_violation(state, "type_bound", "_bounds_n"))
    );
}
#[test]
fn bridge_wrong_frame_pointer_refuses_checked_context() {
    let mut solver = Z::new().unwrap();
    let m = model();
    let read = frame(&mut solver, &m, 0, 1);
    let ledger = checked(&mut solver, &m, &read);
    let other = read.clone();
    let query = expr("true and (forall i in 0..n {i<=1})");
    let wrong = eval_in(
        &solver,
        &m,
        &query,
        &other,
        &mut Scope::new(),
        None,
        Some(&ledger),
    );
    eprintln!("BRIDGE_WRONG_POINTER actual={wrong:?}");
    refusal(wrong, ELIGIBILITY);
}
#[test]
fn bridge_checked_frame_value_uses_its_actual_ledger() {
    let mut solver = Z::new().unwrap();
    let m = model();
    let read = frame(&mut solver, &m, 0, 1);
    let ledger = checked(&mut solver, &m, &read);
    let query = expr("true and (forall i in 0..n {i<=1})");
    let actual = eval_in(
        &solver,
        &m,
        &query,
        &read,
        &mut Scope::new(),
        None,
        Some(&ledger),
    );
    eprintln!("BRIDGE_CHECKED_VALUE actual={actual:?}");
    truth(&mut solver, bool_term(&actual.unwrap()).unwrap(), true);
}
#[test]
fn bridge_wrapped_empty_where_and_reached_partial_keep_priority() {
    let rows = [
        ("true and (forall i in (n+1)..n {q.head()==0})", false),
        (
            "true and (forall i in 0..n where false {q.head()==0})",
            false,
        ),
        ("true and (forall i in 0..n where true {q.head()==0})", true),
        ("true and (forall i in 0..n where q.head()==0 {true})", true),
    ];
    let outputs = rows.map(|(predicate, partial)| {
        let source = format!(
            "spec A {{type K=0..2 state {{n:K,q:Seq<K,1>}} init {{n=0 q=Seq {{}}}} action go(){{}} invariant P {{{predicate}}}}}"
        );
        let m = build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap()).unwrap();
        let state = BTreeMap::from([
            ("n".into(), FslValue::Int(0)),
            ("q".into(), FslValue::Seq(vec![])),
        ]);
        let mut solver = Z::new().unwrap();
        let actual = block_on(verify_bounded_from_state(&m, &mut solver, 0, None, &state));
        eprintln!(
            "BRIDGE_WHERE source={source:?} state={state:?} partial={partial} actual={actual:?}"
        );
        (
            actual,
            partial.then(|| bridge_violation(state, "partial_op", "_partial_property_P")),
        )
    });
    for (actual, expected) in outputs {
        assert_eq!(actual.map(|result| result.violation), Ok(expected));
    }
}

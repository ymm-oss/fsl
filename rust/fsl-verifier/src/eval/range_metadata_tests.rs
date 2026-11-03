// SPDX-License-Identifier: Apache-2.0
//! Independent U3a+b interface freeze; unexecuted until BOTH source units exist.
use super::*;
use crate::value::{BindingRecord, RangeFact};
use fsl_core::binder_range::{
    InclusiveI64, RangeBinding, RangePlan, RangePrerequisite, RangeUnknown,
};
use fsl_core::{FsResolver, build_model, parse_kernel_source};
use fsl_solver::{SatResult, SmtSolver};

type Z = fsl_solver_z3::Z3Solver;
type T = <Z as SmtSolver>::Term;
type Scope = crate::value::TypedBindings<T>;
type Record = BindingRecord<T>;
const BASE: &str = r"spec Facts {
  const c = 7
  type K = 0..2
  enum Tag { Red, Blue }
  state { n: K, u: Int }
  init { n = 1 u = 0 }
  action fill(lim: K) { let top = lim + 1 requires top >= 0 }
  action bool_arg(b: Bool) { }
  action enum_arg(e: Tag) { }
  action capture() { let top = n + 1 requires top >= 0 n = 2 }
  action divide(lim: K) { let q = lim / 1 requires q >= 0 }
  action prior(x: K) { let next = x + 1 requires next >= 0 }
}";

// a: common helpers are charged here, once; b-only helpers start below.
fn model(source: &str) -> KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).unwrap()).unwrap()
}
fn expr(source: &str) -> Expr {
    fsl_syntax::parse_expr(source).unwrap()
}
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
fn truth(s: &mut Z, term: &T, expected: bool) {
    eprintln!("U3_TRUTH expected={expected} term={term:?}");
    assert_eq!(
        block_on(s.check()).unwrap(),
        SatResult::Sat,
        "nonvacuous context"
    );
    s.push();
    let wrong = if expected {
        s.not(term).unwrap()
    } else {
        term.clone()
    };
    s.assert(&wrong).unwrap();
    assert_eq!(block_on(s.check()).unwrap(), SatResult::Unsat);
    s.pop(1).unwrap();
}
fn pin(s: &mut Z, value: &SymbolicValue<T>, expected: i64) {
    let equal = s
        .equal(int_term(value).unwrap(), &s.int_value(expected))
        .unwrap();
    s.assert(&equal).unwrap();
}
fn binding(s: &mut Z, r: &Record, ty: &TypeRef, expected: i64) {
    eprintln!("U3_RECORD {r:?}");
    assert_eq!(r.binding.1.as_ref(), Some(ty));
    let equal = s
        .equal(int_term(&r.binding.0).unwrap(), &s.int_value(expected))
        .unwrap();
    truth(s, &equal, true);
}
fn fact(s: &mut Z, r: &Record, envelope: (i64, i64), captured: bool, valid: bool) {
    eprintln!("U3_FACT {r:?}");
    let f = r.range_fact.as_ref().expect("literal expected fact");
    let interval = f.envelope().envelope();
    assert_eq!((interval.lo(), interval.hi()), envelope);
    assert_eq!(matches!(f, RangeFact::CapturedLet { .. }), captured);
    match (captured, f.valid_when()) {
        (true, Some(predicate)) => truth(s, predicate, valid),
        (false, None) => assert!(valid),
        _ => panic!("wrong provenance/validity channel"),
    }
}
fn record<'a>(scope: &'a Scope, name: &str) -> &'a Record {
    scope
        .records()
        .find(|(key, _)| key.as_str() == name)
        .unwrap()
        .1
}
fn parameter(s: &Z, value: i64, ty: TypeRef) -> Record {
    BindingRecord::with_range_fact(
        TypedBinding(int_value(s, s.int_value(value)), Some(ty)),
        RangeFact::parameter(value),
    )
}
fn params(s: &Z, m: &KernelModel, action: &str, index: usize) -> Scope {
    crate::transition::action_instances(s, m)
        .unwrap()
        .into_iter()
        .filter(|instance| instance.action == action)
        .nth(index)
        .unwrap()
        .params
}

#[test]
fn a_atomic_overwrite_clone_and_public_conversions() {
    let mut s = Z::new().unwrap();
    let mut scope = Scope::new();
    scope.insert("x".into(), parameter(&s, 2, TypeRef::Int));
    let outer = scope.clone();
    scope.insert("x".into(), int_value(&s, s.int_value(7)));
    binding(&mut s, record(&scope, "x"), &TypeRef::Int, 7);
    assert!(record(&scope, "x").range_fact.is_none());
    scope.insert(
        "x".into(),
        SymbolicValue::Scalar {
            ty: TypeRef::Bool,
            term: s.bool_value(false),
        },
    );
    assert_eq!(record(&scope, "x").binding.1, Some(TypeRef::Bool));
    truth(&mut s, bool_term(scope.get("x").unwrap()).unwrap(), false);
    assert!(record(&scope, "x").range_fact.is_none());
    scope.clone_from(&outer);
    binding(&mut s, record(&scope, "x"), &TypeRef::Int, 2);
    fact(&mut s, record(&scope, "x"), (2, 2), false, true);
    assert_eq!(binding_types(&scope), [("x".into(), TypeRef::Int)]);
    scope.insert(
        "x".into(),
        TypedBinding(int_value(&s, s.int_value(2)), Some(TypeRef::Range(0, 5))),
    );
    binding(&mut s, record(&scope, "x"), &TypeRef::Range(0, 5), 2);
    assert!(record(&scope, "x").range_fact.is_none()); // Frozen two-field constructor.
    let public: crate::Bindings<T> = BTreeMap::from([("x".into(), int_value(&s, s.int_value(1)))]);
    let state: crate::SymbolicState<T> = public.clone();
    let converted: Scope = public.into_iter().collect();
    binding(&mut s, record(&converted, "x"), &TypeRef::Int, 1);
    assert!(record(&converted, "x").range_fact.is_none());
    assert_eq!(state.keys().cloned().collect::<Vec<_>>(), ["x"]);
    let collection = SymbolicValue::Collection {
        ty: TypeRef::Set(Box::new(TypeRef::Int)),
        entries: vec![(vec![int_value(&s, s.int_value(1))], s.bool_value(true))],
    };
    scope.insert("items".into(), collection);
    assert!(record(&scope, "items").range_fact.is_none());
    let SymbolicValue::Collection { entries, ty } = &record(&scope, "items").binding.0 else {
        panic!("collection shape")
    };
    assert_eq!(ty, &TypeRef::Set(Box::new(TypeRef::Int)));
    assert_eq!((entries.len(), entries[0].0.len()), (1, 1));
    let equal = s
        .equal(int_term(&entries[0].0[0]).unwrap(), &s.int_value(1))
        .unwrap();
    truth(&mut s, &equal, true);
    truth(&mut s, &entries[0].1, true);
    let captured = RangeFact::captured(InclusiveI64::new(2, 2).unwrap(), s.bool_value(false));
    let r = BindingRecord::with_range_fact(
        TypedBinding(int_value(&s, s.int_value(2)), Some(TypeRef::Int)),
        captured,
    );
    fact(&mut s, &r, (2, 2), true, false); // Private constructor shape, not inferred support.
}

#[test]
fn a_actual_parameter_rosters_keep_values_types_and_only_integer_facts() {
    let mut s = Z::new().unwrap();
    let m = model(
        "spec Params { type K=0..2 enum Tag {Red,Blue} state {n:K} init {n=0} action int_arg(x:K){} action bool_arg(b:Bool){} action enum_arg(e:Tag){} }",
    );
    let actual = crate::transition::action_instances(&s, &m).unwrap();
    let enum_value = |member: &str| FslValue::Enum {
        type_name: "Tag".into(),
        member: member.into(),
    };
    let expected = [
        (
            "int_arg",
            "x",
            TypeRef::Named("K".into()),
            vec![FslValue::Int(0), FslValue::Int(1), FslValue::Int(2)],
        ),
        (
            "bool_arg",
            "b",
            TypeRef::Bool,
            vec![FslValue::Bool(false), FslValue::Bool(true)],
        ),
        (
            "enum_arg",
            "e",
            TypeRef::Named("Tag".into()),
            vec![enum_value("Red"), enum_value("Blue")],
        ),
    ];
    assert_eq!(actual.len(), 7);
    let mut offset = 0;
    for (action_index, (action, name, ty, values)) in expected.into_iter().enumerate() {
        for value in values {
            let instance = &actual[offset];
            eprintln!("U3_PARAMETER {instance:?}");
            assert_eq!(
                (instance.action.as_str(), instance.action_index),
                (action, action_index)
            );
            assert_eq!(
                instance.concrete_params,
                BTreeMap::from([(name.into(), value.clone())])
            );
            assert_eq!(instance.params.keys().cloned().collect::<Vec<_>>(), [name]);
            let r = record(&instance.params, name);
            assert_eq!(r.binding.1.as_ref(), Some(&ty));
            assert_eq!(r.binding.0.ty(), Some(&ty));
            assert_eq!(binding_types(&instance.params), [(name.into(), ty.clone())]);
            match value {
                FslValue::Int(n) => {
                    binding(&mut s, r, &ty, n);
                    fact(&mut s, r, (n, n), false, true);
                }
                FslValue::Bool(v) => {
                    truth(&mut s, bool_term(&r.binding.0).unwrap(), v);
                    assert!(r.range_fact.is_none());
                }
                FslValue::Enum { member, .. } => {
                    let ordinal = match member.as_str() {
                        "Red" => 0,
                        "Blue" => 1,
                        _ => unreachable!(),
                    };
                    binding(&mut s, r, &ty, ordinal); // Preserve encoding, never invent an integer fact.
                    assert!(r.range_fact.is_none());
                }
                _ => panic!("unexpected fixture value"),
            }
            offset += 1;
        }
    }
}

// b: let capture, read-frame prerequisites and status. No a-only qualification.
fn frame(s: &mut Z, m: &KernelModel, index: usize, n: i64) -> SymbolicState<T> {
    let state = crate::value::symbolic_state(s, m, index).unwrap();
    pin(s, &state["n"], n);
    state
}
fn capture(
    s: &Z,
    m: &KernelModel,
    state: &SymbolicState<T>,
    scope: &mut Scope,
    name: &str,
    rhs: &str,
) {
    let result = eval_binding(s, m, &expr(rhs), state, scope, None).unwrap();
    eprintln!("U3_CAPTURE {name}={rhs} {result:?}");
    assert_eq!(result.binding.0.ty(), result.binding.1.as_ref());
    scope.insert(name.into(), result);
}
fn finite(lo: i64, hi: i64, prerequisites: Vec<RangePrerequisite>) -> RangePlan {
    RangePlan::Finite {
        candidates: InclusiveI64::new(lo, hi).unwrap(),
        prerequisites,
    }
}
fn guard(s: &mut Z, d: &crate::transition::ActionGuardDefinedness<T>, partial: bool) {
    truth(s, &d.enabled, true);
    truth(s, &d.defined, true);
    truth(s, &d.first_partial, false);
    assert_eq!(d.has_partial_operation, partial);
}

#[test]
fn b_preinsert_scope_closed_children_and_both_chained_let_paths() {
    let mut s = Z::new().unwrap();
    let m = model(BASE);
    let state = frame(&mut s, &m, 0, 1);
    let mut scope = params(&s, &m, "prior", 2);
    let old = scope.clone();
    let r = eval_binding(&s, &m, &expr("x+1"), &state, &mut scope, None).unwrap();
    binding(&mut s, &r, &TypeRef::Int, 3);
    fact(&mut s, &r, (3, 3), true, true);
    binding(&mut s, record(&scope, "x"), &TypeRef::Named("K".into()), 2);
    scope.insert("x".into(), r);
    scope.clone_from(&old);
    let closed = eval(
        &s,
        &m,
        &expr("not (some(1) is some(x))"),
        &state,
        &mut scope,
        None,
    )
    .unwrap();
    truth(&mut s, bool_term(&closed).unwrap(), false);
    fact(&mut s, record(&scope, "x"), (2, 2), false, true);
    binding(&mut s, record(&scope, "x"), &TypeRef::Named("K".into()), 2);
    let chain = model(
        "spec Chain {type K=0..2 state {n:K} init {n=1} action go(lim:K){let top=lim+1 let next=top+1 requires next>=0}}",
    );
    let p = params(&s, &chain, "go", 1);
    let read = frame(&mut s, &chain, 1, 1);
    let (guards, ordinary) =
        crate::transition::action_guards(&s, &chain, &chain.actions[0], &read, &p).unwrap();
    let status =
        crate::transition::action_guard_definedness(&s, &chain, &chain.actions[0], &read, &p)
            .unwrap();
    for bindings in [&ordinary, &status.bindings] {
        assert_eq!(
            binding_types(bindings),
            [
                ("lim".into(), TypeRef::Named("K".into())),
                ("next".into(), TypeRef::Int),
                ("top".into(), TypeRef::Int)
            ]
        );
        for (name, value) in [("top", 2), ("next", 3)] {
            binding(&mut s, record(bindings, name), &TypeRef::Int, value);
            fact(&mut s, record(bindings, name), (value, value), true, true);
        }
    }
    truth(&mut s, &guards[0], true);
    guard(&mut s, &status, false);
}

#[test]
fn b_transitive_capture_and_clone_keep_the_original_frame_and_exact_indices() {
    for (n0, n1, top, last, valid) in [(1, 2, 2, 3, true), (1, 3, 2, 3, true), (3, 1, 4, 5, false)]
    {
        let mut s = Z::new().unwrap();
        let m = model(BASE);
        let read = frame(&mut s, &m, 0, n0);
        let next = frame(&mut s, &m, 1, n1);
        let mut scope = Scope::new();
        capture(&s, &m, &read, &mut scope, "top", "n+1");
        capture(&s, &m, &next, &mut scope, "last", "top+1");
        let closed = eval(
            &s,
            &m,
            &expr("not (some(1) is some(top))"),
            &read,
            &mut scope,
            None,
        )
        .unwrap();
        truth(&mut s, bool_term(&closed).unwrap(), false);
        binding(&mut s, record(&scope, "top"), &TypeRef::Int, top);
        binding(&mut s, record(&scope, "last"), &TypeRef::Int, last);
        fact(&mut s, record(&scope, "top"), (1, 3), true, valid);
        fact(&mut s, record(&scope, "last"), (2, 4), true, valid);
        let outer = scope.clone();
        scope.insert(
            "top".into(),
            SymbolicValue::Scalar {
                ty: TypeRef::Bool,
                term: s.bool_value(true),
            },
        );
        assert!(record(&scope, "top").range_fact.is_none());
        scope.clone_from(&outer);
        binding(&mut s, record(&scope, "top"), &TypeRef::Int, top);
        fact(&mut s, record(&scope, "top"), (1, 3), true, valid);
    }
    let mut s = Z::new().unwrap();
    let m = model(BASE);
    let read = frame(&mut s, &m, 0, 3);
    let mut scope = params(&s, &m, "fill", 1);
    capture(&s, &m, &read, &mut scope, "a", "n+1");
    capture(&s, &m, &read, &mut scope, "top", "lim+1");
    assert_eq!(
        scope.keys().cloned().collect::<Vec<_>>(),
        ["a", "lim", "top"]
    );
    capture(&s, &m, &read, &mut scope, "last", "top+1");
    fact(&mut s, record(&scope, "a"), (1, 3), true, false);
    binding(&mut s, record(&scope, "last"), &TypeRef::Int, 3);
    fact(&mut s, record(&scope, "last"), (3, 3), true, true);
}

#[test]
fn b_supplied_d_forwarding_and_conditional_singleton_overflow_preservation() {
    let mut s = Z::new().unwrap();
    let m = model(BASE);
    let state = frame(&mut s, &m, 0, 1);
    for supplied in [false, true] {
        let d = s.bool_value(supplied);
        let r = eval_binding(&s, &m, &expr("1"), &state, &mut Scope::new(), Some(&d)).unwrap();
        binding(&mut s, &r, &TypeRef::Int, 1);
        fact(&mut s, &r, (1, 1), true, supplied); // Helper forwarding, NOT partial-op detection.
        let forwarded_fact =
            capture_let_range_fact(&s, &m, &expr("1"), &state, &Scope::new(), Some(&d)).unwrap();
        truth(&mut s, forwarded_fact.valid_when().unwrap(), supplied);
    }
    for (n, valid) in [(1, true), (0, false)] {
        let mut s = Z::new().unwrap();
        let one = model("spec One {type K=1..1 state {n:K} init {n=1} action go(){}}");
        let read = frame(&mut s, &one, 0, n);
        let r = eval_binding(&s, &one, &expr("n"), &read, &mut Scope::new(), None).unwrap();
        binding(&mut s, &r, &TypeRef::Named("K".into()), n);
        fact(&mut s, &r, (1, 1), true, valid);
    }
    let mut s = Z::new().unwrap();
    let read = frame(&mut s, &m, 0, i64::MAX);
    let status = evaluation_status(&s, &m, &expr("n+1"), &read, &Scope::new(), None).unwrap();
    truth(&mut s, &status.fully_defined, false);
    let r = eval_binding(
        &s,
        &m,
        &expr("n+1"),
        &read,
        &mut Scope::new(),
        Some(&status.fully_defined),
    )
    .unwrap();
    fact(&mut s, &r, (1, 3), true, false); // Carrier also false: preservation, not isolated D-removal detector.
}

#[test]
fn b_unknown_overwrites_missing_types_and_required_rhs_errors_are_distinct() {
    let mut s = Z::new().unwrap();
    let m = model(BASE);
    let state = frame(&mut s, &m, 0, 1);
    pin(&mut s, &state["u"], 4);
    let mut scope = params(&s, &m, "fill", 1);
    for (rhs, expected) in [("u", 4), ("lim/1", 1)] {
        capture(&s, &m, &state, &mut scope, "top", "lim+1");
        capture(&s, &m, &state, &mut scope, "top", rhs);
        binding(&mut s, record(&scope, "top"), &TypeRef::Int, expected);
        assert!(record(&scope, "top").range_fact.is_none());
    }
    capture(&s, &m, &state, &mut scope, "top", "true");
    assert_eq!(record(&scope, "top").binding.1, Some(TypeRef::Bool));
    assert!(record(&scope, "top").range_fact.is_none());
    for (name, ty) in [("n", TypeRef::Named("K".into())), ("c", TypeRef::Int)] {
        let mut unknown = Scope::new();
        unknown.insert(name.into(), TypedBinding(state["u"].clone(), Some(ty)));
        let withheld = capture_let_range_fact(&s, &m, &expr(name), &state, &unknown, None);
        assert!(withheld.is_none());
    }
    let divide_params = params(&s, &m, "divide", 1);
    let (guards, ordinary) =
        crate::transition::action_guards(&s, &m, &m.actions[4], &state, &divide_params).unwrap();
    let d =
        crate::transition::action_guard_definedness(&s, &m, &m.actions[4], &state, &divide_params)
            .unwrap();
    for bindings in [&ordinary, &d.bindings] {
        binding(&mut s, record(bindings, "q"), &TypeRef::Int, 1);
        assert!(record(bindings, "q").range_fact.is_none());
    }
    truth(&mut s, &guards[0], true);
    guard(&mut s, &d, true); // Corrected U15: syntactic division presence, no actual partial failure.
    scope.insert("c".into(), TypedBinding(SymbolicValue::None, None));
    assert!(capture_let_range_fact(&s, &m, &expr("1"), &state, &scope, None).is_none());
    assert!(capture_let_range_fact(&s, &m, &expr("c"), &state, &scope, None).is_none());
    let missing = SymbolicState::new(); // Genuine missing lookup, not a foreign/invalid solver term.
    assert!(capture_let_range_fact(&s, &m, &expr("n"), &missing, &Scope::new(), None).is_none());
    let required = eval_binding(&s, &m, &expr("n"), &missing, &mut Scope::new(), None).unwrap_err();
    assert_eq!(required, VerifyError::new("unknown identifier 'n'"));
    let full = model(
        "spec Full {const MIN=(-9223372036854775807-1) const MAX=9223372036854775807 type K=MIN..MAX state {n:K} init {n=0} action go(){}}",
    );
    assert_eq!(
        full.plan_integer_range(&expr("n-n"), &expr("n-n"), &[]),
        RangePlan::Unknown(RangeUnknown::EnvelopeUnrepresentable)
    );
    let read = frame(&mut s, &full, 2, 0);
    let r = eval_binding(&s, &full, &expr("n-n"), &read, &mut Scope::new(), None).unwrap();
    binding(&mut s, &r, &TypeRef::Int, 0);
    assert!(r.range_fact.is_none()); // No correlation-based replacement for planner Unknown.
}

#[test]
fn b_public_core_reference_plans_preserve_complete_prerequisites_and_unknown_reasons() {
    use RangePrerequisite::{LexicalFact, StateCarrier};
    let mut s = Z::new().unwrap();
    let m = model(BASE);
    let state = frame(&mut s, &m, 0, 1);
    let params = params(&s, &m, "fill", 1);
    let lim = record(&params, "lim");
    let k = TypeRef::Named("K".into());
    let lexical = [RangeBinding {
        name: "lim",
        semantic_ty: &k,
        fact: Some(lim.range_fact.as_ref().unwrap().envelope()),
    }];
    assert_eq!(
        m.plan_integer_range(&expr("lim+1"), &expr("lim+1"), &lexical),
        finite(2, 2, vec![LexicalFact { input_index: 0 }])
    );
    assert_eq!(
        m.plan_integer_range(&expr("lim/1"), &expr("lim/1"), &lexical),
        RangePlan::Unknown(RangeUnknown::UnsupportedExpression)
    );
    let n_carrier = StateCarrier {
        name: "n".into(),
        envelope: InclusiveI64::new(0, 2).unwrap(),
    };
    assert_eq!(
        m.plan_integer_range(&expr("n+1"), &expr("n+1"), &[]),
        finite(1, 3, vec![n_carrier])
    );
    let captured =
        capture_let_range_fact(&s, &m, &expr("n+1"), &state, &Scope::new(), None).unwrap();
    let lexical = [RangeBinding {
        name: "top",
        semantic_ty: &TypeRef::Int,
        fact: Some(captured.envelope()),
    }];
    assert_eq!(
        m.plan_integer_range(&expr("top"), &expr("top"), &lexical),
        finite(1, 3, vec![LexicalFact { input_index: 0 }])
    );
    assert_eq!(
        m.plan_integer_range(&expr("top"), &expr("0"), &lexical),
        RangePlan::Empty {
            prerequisites: vec![LexicalFact { input_index: 0 }]
        }
    );
    for (name, ty) in [("n", k), ("c", TypeRef::Int), ("top", TypeRef::Int)] {
        let lexical = [RangeBinding {
            name,
            semantic_ty: &ty,
            fact: None,
        }];
        assert_eq!(
            m.plan_integer_range(&expr("0"), &expr(name), &lexical),
            RangePlan::Unknown(RangeUnknown::MissingFiniteFact)
        );
    }
    for ty in [TypeRef::Bool, TypeRef::Named("Tag".into())] {
        let lexical = [RangeBinding {
            name: "n",
            semantic_ty: &ty,
            fact: None,
        }];
        assert_eq!(
            m.plan_integer_range(&expr("0"), &expr("n"), &lexical),
            RangePlan::Unknown(RangeUnknown::UnsupportedExpression)
        );
    }
}

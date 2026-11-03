// SPDX-License-Identifier: Apache-2.0
//! `UNEXECUTED/API_ABSENT` until the accepted b00d6306 R1 surface exists.
//! Pure eligibility/envelope plans, NEVER membership or caller-policy evidence.
use fsl_core::binder_range::{
    InclusiveI64 as I, IntegerEnvelopeFact as F, RangeBinding as B, RangePlan as R,
    RangePrerequisite as P, RangeUnknown as U,
};
use fsl_core::{
    FsResolver, KernelExpr as E, KernelModel, TypeRef, build_model, parse_kernel_source,
};

const BASE: &str = r"spec Planner {
const MAX=9223372036854775807 const MIN=(-9223372036854775807-1) const n=0
type K=0..2 type One=1..1 type Full=MIN..MAX
type Near=9223372036854775806..9223372036854775807
enum Tag { Red, Blue } struct Cell { flag:Bool }
state { n:K, a:K, b:K, u:Int, one:One, full:Full, top:Near, flag:Bool }
init { n=1 a=0 b=0 u=0 one=1 full=0 top=9223372036854775806 flag=false }
action go() {} invariant P { true }
}";

fn model() -> KernelModel {
    build_model(parse_kernel_source(BASE, &FsResolver::new(".")).unwrap()).unwrap()
}
fn e(source: &str) -> E {
    fsl_syntax::parse_expr(source).expect("raw AST grammar, intentionally not endpoint validation")
}
fn iv(lo: i64, hi: i64) -> I {
    I::new(lo, hi).unwrap()
}
fn state(name: &str, lo: i64, hi: i64) -> P {
    P::StateCarrier {
        name: name.into(),
        envelope: iv(lo, hi),
    }
}
fn lex(index: usize) -> P {
    P::LexicalFact { input_index: index }
}
fn finite(lo: i64, hi: i64, prerequisites: Vec<P>) -> R {
    R::Finite {
        candidates: iv(lo, hi),
        prerequisites,
    }
}
fn closed(lo: i64, hi: i64) -> R {
    R::Static { lo, hi }
}
fn empty(prerequisites: Vec<P>) -> R {
    R::Empty { prerequisites }
}
fn binding<'a>(name: &'a str, semantic_ty: &'a TypeRef, fact: Option<&'a F>) -> B<'a> {
    B {
        name,
        semantic_ty,
        fact,
    }
}
fn check(m: &KernelModel, lo: &E, hi: &E, lexical: &[B<'_>], expected: &R) {
    let actual = m.plan_integer_range(lo, hi, lexical);
    eprintln!(
        "PURE_PLAN lo={lo:?} hi={hi:?} lexical={lexical:?} actual={actual:?} expected={expected:?}"
    );
    assert_eq!(&actual, expected);
}

#[test]
fn compact_constructor_and_full_width_count_have_no_execution_budget() {
    assert_eq!(I::new(1, 0), None);
    for (lo, hi, count) in [
        (i64::MIN, i64::MAX, 18_446_744_073_709_551_616_u128),
        (i64::MIN, i64::MIN, 1),
        (i64::MAX, i64::MAX, 1),
        (-2, 3, 6),
    ] {
        let interval = I::new(lo, hi).unwrap();
        assert_eq!((interval.lo(), interval.hi()), (lo, hi));
        assert_eq!(interval.checked_cardinality(), Some(count));
        assert_eq!(F::new(interval).envelope(), interval);
    }
}

#[test]
fn closed_plans_preserve_reversal_and_delegate_checked_constant_semantics() {
    let m = model();
    for (lo, hi, expected) in [
        (e("-1"), e("2-1"), closed(-1, 1)),
        (e("1"), e("0"), closed(1, 0)),
        (e("MIN"), e("MAX"), closed(i64::MIN, i64::MAX)),
        (e("(-5/2)"), e("-3"), closed(-3, -3)),
        (e("(-5%2)"), e("1"), closed(1, 1)),
        (e("if true then 1 else (1/0)"), e("1"), closed(1, 1)),
        (e("if false then (1/0) else 2"), e("2"), closed(2, 2)),
        (e("abs(-2)"), e("max(2,1)"), closed(2, 2)),
    ] {
        check(&m, &lo, &hi, &[], &expected);
    }
}

#[test]
fn state_before_namesake_constant_and_ordered_state_dependencies() {
    let m = model();
    for (lo, hi, expected) in [
        (e("0"), e("n"), finite(0, 2, vec![state("n", 0, 2)])),
        (e("n"), e("n"), finite(0, 2, vec![state("n", 0, 2)])),
        (e("-n"), e("n"), finite(-2, 2, vec![state("n", 0, 2)])),
        (e("n+1"), e("n+3"), finite(1, 5, vec![state("n", 0, 2)])),
        (
            e("b"),
            e("a+b"),
            finite(0, 4, vec![state("b", 0, 2), state("a", 0, 2)]),
        ),
        (e("one"), e("1"), finite(1, 1, vec![state("one", 1, 1)])),
        (e("one"), e("0"), empty(vec![state("one", 1, 1)])),
        (e("n"), e("-1"), empty(vec![state("n", 0, 2)])),
        (
            e("full"),
            e("full"),
            finite(i64::MIN, i64::MAX, vec![state("full", i64::MIN, i64::MAX)]),
        ),
    ] {
        check(&m, &lo, &hi, &[], &expected);
    }
}

#[test]
fn last_lexical_entry_uses_original_index_and_never_falls_back() {
    let m = model();
    let integer = TypeRef::Int;
    let older = F::new(iv(-2, 3));
    let newer = F::new(iv(4, 6));
    let mut bindings = [
        binding("n", &integer, Some(&older)),
        binding("pad", &integer, None),
        binding("n", &integer, Some(&newer)),
    ];
    check(&m, &e("n"), &e("n"), &bindings, &finite(4, 6, vec![lex(2)]));
    assert_eq!(bindings[2].semantic_ty, &TypeRef::Int); // fact is NOT a semantic carrier.
    bindings[2].fact = None;
    check(
        &m,
        &e("n"),
        &e("n"),
        &bindings,
        &R::Unknown(U::MissingFiniteFact),
    );
    let bounded = TypeRef::Named("K".into());
    let missing = [binding("n", &bounded, None)];
    check(
        &m,
        &e("0"),
        &e("n"),
        &missing,
        &R::Unknown(U::MissingFiniteFact),
    );
    let narrow = TypeRef::Range(0, 1);
    let wider_fact = [binding("n", &narrow, Some(&older))];
    check(
        &m,
        &e("n"),
        &e("n"),
        &wider_fact,
        &finite(-2, 3, vec![lex(0)]),
    );
    let near = F::new(iv(9_223_372_036_854_775_806, i64::MAX));
    bindings[2].fact = Some(&near);
    check(
        &m,
        &e("n+1"),
        &e("0"),
        &bindings,
        &R::Unknown(U::EnvelopeUnrepresentable),
    );
}

#[test]
fn noninteger_semantic_types_cannot_become_numeric_from_a_fact() {
    let m = model();
    let fact = F::new(iv(-2, 3));
    for ty in [
        TypeRef::Bool,
        TypeRef::Named("Tag".into()),
        TypeRef::Named("Cell".into()),
        TypeRef::Set(Box::new(TypeRef::Int)),
    ] {
        let bindings = [binding("n", &ty, Some(&fact))];
        check(
            &m,
            &e("n"),
            &e("1"),
            &bindings,
            &R::Unknown(U::UnsupportedExpression),
        );
    }
}

#[test]
fn signed_corner_envelopes_and_mixed_dependencies_are_not_membership() {
    let m = model();
    let integer = TypeRef::Int;
    let x = F::new(iv(-2, 3));
    let y = F::new(iv(-4, 1));
    let bindings = [
        binding("x", &integer, Some(&x)),
        binding("y", &integer, Some(&y)),
    ];
    for (lo, hi, expected) in [
        (e("x"), e("x"), finite(-2, 3, vec![lex(0)])),
        (e("-x"), e("-x"), finite(-3, 2, vec![lex(0)])),
        (e("x+y"), e("x+y"), finite(-6, 4, vec![lex(0), lex(1)])),
        (e("x-y"), e("x-y"), finite(-3, 7, vec![lex(0), lex(1)])),
        (e("x*y"), e("x*y"), finite(-12, 8, vec![lex(0), lex(1)])),
        (
            e("n+x"),
            e("x+n+y"),
            finite(-2, 6, vec![state("n", 0, 2), lex(0), lex(1)]),
        ),
    ] {
        check(&m, &lo, &hi, &bindings, &expected);
    }
    // x=0 is eligible for [-2,3], but candidate2 is NOT in the written x..x.
    // No RangePlan result discharges this actual membership or caller constraints.
    let point = F::new(iv(1, 1));
    let captured = [binding("x", &integer, Some(&point))];
    check(&m, &e("x"), &e("1"), &captured, &finite(1, 1, vec![lex(0)]));
    check(&m, &e("x"), &e("0"), &captured, &empty(vec![lex(0)]));
}

#[test]
fn full_validator_checks_both_conditional_arms_not_only_result_inference() {
    let m = model();
    for bad in [
        e("if true then 1 else false"),
        e("if false then (true+1) else 0"),
        e("if 0 then 1 else 2"),
        e("if true then (1/0) else false"),
    ] {
        // Existing inference returns Int here; it is NOT sufficient endpoint validation.
        assert_eq!(
            m.expression_type(&bad, &[], None).unwrap(),
            Some(TypeRef::Int)
        );
        check(
            &m,
            &bad,
            &e("2"),
            &[],
            &R::Unknown(U::UnsupportedExpression),
        );
    }
    check(
        &m,
        &e("true+1"),
        &e("2"),
        &[],
        &R::Unknown(U::UnsupportedExpression),
    );
    check(
        &m,
        &e("flag"),
        &e("2"),
        &[],
        &R::Unknown(U::UnsupportedExpression),
    );
}

#[test]
fn unknown_classes_and_lo_then_hi_left_to_right_precedence_are_exact() {
    let m = model();
    for (lo, hi, why) in [
        (e("u"), e("2"), U::MissingFiniteFact),
        (e("true"), e("2"), U::UnsupportedExpression),
        (e("ghost"), e("2"), U::UnsupportedExpression),
        (e("MAX+1"), e("0"), U::ConstantFoldFailed),
        (e("1/0"), e("0"), U::ConstantFoldFailed),
        (e("MIN/-1"), e("0"), U::ConstantFoldFailed),
        (e("top+1"), e("-1"), U::EnvelopeUnrepresentable),
        (e("-full"), e("0"), U::EnvelopeUnrepresentable),
        (e("u/0"), e("0"), U::UnsupportedExpression),
        (e("u"), e("ghost"), U::MissingFiniteFact),
        (e("u+(top+1)"), e("0"), U::MissingFiniteFact),
        (e("(top+1)+u"), e("0"), U::EnvelopeUnrepresentable),
        (e("u+(MAX+1)"), e("0"), U::MissingFiniteFact),
        (e("(MAX+1)+u"), e("0"), U::ConstantFoldFailed),
        (e("u+flag"), e("0"), U::UnsupportedExpression),
        (e("full-full"), e("0"), U::EnvelopeUnrepresentable),
        (e("top*2"), e("0"), U::EnvelopeUnrepresentable),
        (e("if true then 1 else n"), e("0"), U::UnsupportedExpression),
    ] {
        check(&m, &lo, &hi, &[], &R::Unknown(why));
    }
}
#[test]
fn returned_metadata_owns_names_after_model_and_endpoint_temporaries_drop() {
    let owned = {
        let inner = model();
        inner.plan_integer_range(&e("n"), &e("n"), &[])
    };
    assert_eq!(owned, finite(0, 2, vec![state("n", 0, 2)]));
}

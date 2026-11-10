// SPDX-License-Identifier: Apache-2.0
//! DRAFT / UNEXECUTED / Native0: PUBLIC authored validation, not strict projection.
use fsl_core::{
    FsResolver, KernelExpr as E, KernelModel, build_model, parse_kernel_source,
    validate_boolean_expression,
};

fn model(n_type: &str, initial: &str) -> KernelModel {
    let source = format!(
        "spec AuthoredAgg {{ type K=0..2 state {{n:{n_type}}} init {{n={initial}}} action idle(){{}} invariant P {{true}} }}"
    );
    build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap()).unwrap()
}
fn expression(source: &str) -> E {
    fsl_syntax::parse_expr(source).unwrap()
}
fn validate(m: &KernelModel, source: &str) -> Result<(), String> {
    let actual = validate_boolean_expression(&expression(source), m);
    eprintln!("U5_AUTHORED source={source:?} actual={actual:?}");
    actual
}

#[test]
fn authored_valid_deferred_and_unconditional_forms_both_validate() {
    let m = model("K", "1");
    for source in [
        "count(i in 0..n where i<=n)>=0",
        "sum(i in 0..n where i<=n of (i+n))>=0",
        "count(i in 1..0 where true)==0",
        "sum(i in 1..0 where true of (1/0))==0",
        "count(i in 0..2 where i!=1)==2",
        "sum(i in 0..2 where i!=1 of i)==2",
        "sum(i in 0..1 of count(j in i..1))==3",
        "sum(i in 0..n of sum(j in i..n of j))>=0",
    ] {
        assert_eq!(validate(&m, source), Ok(()));
    }
}

#[test]
fn stored_authored_ast_is_preserved_not_a_carrier_clipped_conditional_sum() {
    // AST retention is already supported: this is preservation, NOT evidence of
    // a new value-producing normalizer or established runtime eligibility.
    for predicate in [
        "count(i in 0..n where i!=1)>=0",
        "sum(i in 0..n where i!=1 of i)>=0",
        "sum(i in 0..2 of i)==3",
    ] {
        let source = format!(
            "spec StoredAgg {{ type K=0..2 state {{n:K}} init {{n=1}} action idle(){{}} invariant P {{{predicate}}} }}"
        );
        let m = build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap()).unwrap();
        assert_eq!(
            m.invariants[0].expr.kernel_ast_v1(),
            expression(predicate).kernel_ast_v1()
        );
    }
}

#[test]
fn authored_bounds_filter_and_sum_types_are_checked_before_defer_or_copies() {
    let m = model("K", "1");
    for (source, expected) in [
        (
            "sum(i in missing_lo..n where missing_filter of missing_value)==0",
            "public Kernel cannot type identifier 'missing_lo'",
        ),
        (
            "sum(i in 0..n where missing_filter of missing_value)==0",
            "public Kernel cannot type identifier 'missing_filter'",
        ),
        (
            "sum(i in 0..n where 1 of missing_value)==0",
            "expression of type Int is not assignable to Bool",
        ),
        (
            "sum(i in 0..n where true of true)==0",
            "expression of type Bool is not assignable to Int",
        ),
        (
            "sum(i in 0..n where false of missing_value)==0",
            "public Kernel cannot type identifier 'missing_value'",
        ),
        (
            "sum(i in 1..0 where 1 of missing_value)==0",
            "expression of type Int is not assignable to Bool",
        ),
        (
            "sum(i in 1..0 where true of true)==0",
            "expression of type Bool is not assignable to Int",
        ),
        (
            "sum(i in 0..(1/0) where 1 of missing_value)==0",
            "expression of type Int is not assignable to Bool",
        ),
    ] {
        // Error text follows parent-copied CURRENT SourceA shared diagnostics.
        assert_eq!(validate(&m, source), Err(expected.into()));
    }
}

#[test]
fn statically_failed_folding_is_not_reclassified_as_valid_deferred() {
    let m = model("K", "1");
    for source in [
        "count(i in 0..(1/0))==0",
        "sum(i in 0..(9223372036854775807+1) of i)==0",
    ] {
        // All authored names/types/filter/value are valid. Only the closed
        // written endpoint fails checked folding: catch-all Err=>Deferred is wrong.
        assert_eq!(
            validate(&m, source),
            Err("public Kernel requires static aggregate range bounds".into())
        );
    }
}

#[test]
fn range_source_is_outer_scope_but_filter_and_sum_value_are_inner_scope() {
    let m = model("Bool", "false");
    assert_eq!(validate(&m, "sum(n in 0..1 where n>=0 of n)==1"), Ok(()));
    assert_eq!(
        validate(&m, "sum(n in 0..n of n)==0"),
        Err("expression of type Bool is not assignable to Int".into())
    );
}

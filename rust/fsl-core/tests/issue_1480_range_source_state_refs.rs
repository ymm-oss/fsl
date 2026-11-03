// SPDX-License-Identifier: Apache-2.0
//! Pure direct Range-source projection; absent API is setup, not semantic RED.
use fsl_core::{
    FsResolver, build_model, integer_range_state_references, parse_kernel_source, state_references,
};
use std::collections::BTreeSet;

const SOURCE: &str = "spec Refs {const top=3 type K=0..2 state {n:K,m:K,u:Int,flag:Bool,opt:Option<K>} init {n=0 m=1 u=0 flag=false opt=none} action idle(){}}";
#[test]
fn only_direct_free_bounded_state_in_range_sources_is_selected() {
    let parsed = parse_kernel_source(SOURCE, &FsResolver::new("."));
    eprintln!("RANGE_SOURCE_MODEL source={SOURCE:?} parsed={parsed:?}");
    let model = build_model(parsed.unwrap()).unwrap();
    let rows: &[(&str, &[&str], &[&str])] = &[
        ("n", &[], &[]),
        ("forall i in 0..2 {n==0}", &[], &[]),
        ("forall i in 0..2 where n==0 {true}", &[], &[]),
        ("forall i in 2..1 {true}", &[], &[]),
        ("forall i in 0..top {true}", &[], &[]),
        ("forall i in 0..u {true}", &[], &[]),
        ("forall i in 0..flag {true}", &[], &[]),
        ("true and (forall i in 0..n {true})", &[], &["n"]),
        ("count(i in m..n)", &[], &["m", "n"]),
        ("forall i in n..(n+1) {true}", &[], &["n"]),
        ("forall i in 0..n {true}", &["n"], &[]),
        ("forall n:K {forall i in 0..n {true}}", &[], &[]),
        ("forall n in 0..n where n==0 {n==0}", &[], &["n"]),
        (
            "(forall n:K {forall i in 0..n {true}}) and (forall i in 0..n {true})",
            &[],
            &["n"],
        ),
        ("forall i in 0..1 {forall j in 0..n {true}}", &[], &["n"]),
        (
            "forall i in 0..1 where (exists j in 0..m {true}) {true}",
            &[],
            &["m"],
        ),
        ("sum(i in 0..1 of count(j in 0..n))", &[], &["n"]),
        (
            "forall i in 0..count(j in 0..1 where n==0) {true}",
            &[],
            &["n"],
        ),
        (
            "forall i in 0..count(n in 0..1 where n==0) {true}",
            &[],
            &[],
        ),
        ("(opt is some(n)) and (forall i in 0..n {true})", &[], &[]),
        ("(opt is some(n)) or (forall i in 0..n {true})", &[], &["n"]),
        (
            "(not (opt is some(n))) and (forall i in 0..n {true})",
            &[],
            &["n"],
        ),
    ];
    for &(source, bound, expected) in rows {
        let expr = fsl_syntax::parse_expr(source).unwrap(); // Parse only; no normalization/evaluation.
        let actual = integer_range_state_references(&expr, &model, bound.iter().copied());
        let literal: BTreeSet<String> = expected.iter().map(|name| (*name).into()).collect();
        eprintln!(
            "RANGE_SOURCE expr={source:?} bound={bound:?} actual={actual:?} expected={literal:?}"
        );
        assert_eq!(actual, literal);
    }
    // Preserve distinct ordinary-name projections; these are independent literals.
    for (source, expected) in [
        ("n", vec!["n"]),
        ("forall i in 0..2 where n==0 {true}", vec!["n"]),
        (
            "(opt is some(n)) and (forall i in 0..n {true})",
            vec!["opt"],
        ),
        (
            "(opt is some(n)) or (forall i in 0..n {true})",
            vec!["n", "opt"],
        ),
    ] {
        let expr = fsl_syntax::parse_expr(source).unwrap();
        let actual = state_references(&expr, &model, []);
        let literal: BTreeSet<String> = expected.into_iter().map(String::from).collect();
        eprintln!("ORDINARY_NAMES expr={source:?} actual={actual:?} expected={literal:?}");
        assert_eq!(actual, literal);
    }
}

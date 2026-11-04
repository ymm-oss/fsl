// SPDX-License-Identifier: Apache-2.0
//! Pure action Range-syntax dispatch; an absent prospective API is setup only.
use fsl_core::{FsResolver, action_has_integer_range_binder, build_model, parse_kernel_source};
#[test]
fn range_syntax_anywhere_in_action_selects_dispatch_but_typed_binders_do_not() {
    for (body, expected) in [
        ("n=n", false),
        ("forall i:K {m[i]=0}", false),
        ("forall i in 0..1 {m[i]=0}", true),
        ("forall i in 0..n {m[i]=0}", true),
        ("requires count(i in 0..n)>=0 n=n", true),
        ("let top=count(i in 0..n) n=top", true),
        ("n=count(i in 0..n)", true),
        ("n=n ensures count(i in 0..n)>=0", true),
        ("if n==0 {forall i in 0..n {m[i]=0}} else {n=n}", true),
        ("forall i:K where (exists j in 0..n {j==i}) {m[i]=0}", true),
        ("n=if true then count(i in 0..n) else 0", true),
    ] {
        let source = format!(
            "spec Classify {{ type K=0..2 state {{n:K,m:Map<K,Int>}} init {{n=0 forall k:K {{m[k]=0}}}} action go() {{{body}}} }}"
        );
        let parsed = parse_kernel_source(&source, &FsResolver::new("."));
        eprintln!("ACTION_CLASSIFIER source={source:?} parsed={parsed:#?} expected={expected}");
        let model = build_model(parsed.unwrap()).unwrap();
        let actual = action_has_integer_range_binder(&model, &model.actions[0]);
        eprintln!("ACTION_CLASSIFIER model={model:#?} actual={actual}");
        assert_eq!(actual, expected, "body={body}");
    }
}

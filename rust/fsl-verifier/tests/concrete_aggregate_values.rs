// SPDX-License-Identifier: Apache-2.0
//! DRAFT / UNEXECUTED / Native0. Existing direct eval, never Monitor/BMC.
//! Current `SourceA` eval/RuntimeError declarations are confirmed in copied context.
use fsl_core::{
    FsResolver, FslValue as V, KernelModel, PartialOperation, build_model, parse_kernel_source,
};
use fsl_runtime::{Bindings, RuntimeError, State, eval};
use std::collections::BTreeMap;

fn model(domain: &str) -> KernelModel {
    let source = format!(
        "spec ConcreteAgg {{ type K={domain} state {{n:K}} init {{n=1}} action idle(){{}} invariant P {{true}} }}"
    );
    build_model(parse_kernel_source(&source, &FsResolver::new(".")).unwrap()).unwrap()
}
fn state(n: i64) -> State {
    BTreeMap::from([("n".into(), V::Int(n))])
}
fn direct(m: &KernelModel, n: i64, source: &str) -> Result<V, RuntimeError> {
    let expr = fsl_syntax::parse_expr(source).unwrap();
    let mut bindings = Bindings::from([("i".into(), V::Int(91))]);
    let before = bindings.clone();
    let actual = eval(&expr, &state(n), &mut bindings, m, None);
    eprintln!(
        "U5_DIRECT source={source:?} state={:?} actual={actual:?}",
        state(n)
    );
    assert_eq!(
        bindings, before,
        "aggregate-local bindings must not escape, even on Err"
    );
    actual
}

#[test]
fn supplied_state_carriers_are_not_concrete_aggregate_premises() {
    for (domain, n, expression, expected) in [
        ("0..2", 3, "count(i in 0..n)", 4),
        ("0..2", 3, "sum(i in 0..n of i)", 6),
        ("0..2", 0, "count(i in 0..n)", 1),
        ("0..2", 0, "sum(i in 0..n of i)", 0),
        ("1..2", 0, "count(i in n..0)", 1),
        ("1..2", 0, "sum(i in n..0 of (i+7))", 7),
        ("1..2", 1, "count(i in n..0)", 0),
        ("1..2", 1, "sum(i in n..0 of (i+7))", 0),
        ("0..2", 3, "count(i in 0..n where i==3)", 1),
        ("0..2", 3, "sum(i in 0..n where i==3 of 11)", 11),
        ("0..2", 3, "sum(n in 0..n of n)", 6),
    ] {
        let m = model(domain);
        assert_eq!(direct(&m, n, expression), Ok(V::Int(expected)));
    }
}

#[test]
fn actual_endpoint_filter_and_body_status_are_not_a_clipped_empty_value() {
    let m = model("1..2");
    for (n, source) in [
        (1, "sum(i in 2..1 where 1/0>0 of (1%0))"),
        (1, "count(i in 2..1 where 1/0>0)"),
        (0, "sum(i in 0..n where false of (1%0))"),
        (1, "sum(i in n..0 of (1%0))"),
    ] {
        assert_eq!(direct(&m, n, source), Ok(V::Int(0)));
    }
    for (n, source, operation) in [
        (
            1,
            "sum(i in (n/(n-n))..0 of (1%0))",
            PartialOperation::Divide,
        ),
        (
            1,
            "sum(i in 2..(n/(n-n)) of (1%0))",
            PartialOperation::Divide,
        ),
        (
            1,
            "sum(i in 0..0 where 1/0>0 of (1%0))",
            PartialOperation::Divide,
        ),
        (
            1,
            "sum(i in 0..0 where true of (1%0))",
            PartialOperation::Remainder,
        ),
        (0, "sum(i in n..0 of (1%0))", PartialOperation::Remainder),
    ] {
        let actual = direct(&m, n, source);
        let error =
            actual.expect_err("a reached actual partial is not count/sum0 or generic unsupported");
        assert_eq!(error.partial_operation, Some(operation));
        assert!(!error.map_key_outside_domain);
    }
}

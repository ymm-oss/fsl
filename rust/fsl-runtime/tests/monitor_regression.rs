// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_core::{FsResolver, FslValue, build_model, parse_kernel_source};

fn model(source: impl AsRef<str>) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source.as_ref(), &FsResolver::new(".")).expect("parse model"))
        .expect("build model")
}

#[test]
fn domain_empty_membership_is_an_initial_invariant_violation_in_monitor_and_bfs() {
    let model = model(include_str!(
        "../../fslc/tests/fixtures/domain_empty_membership.fsl"
    ));
    let monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize domain Monitor");
    let expected = fsl_runtime::Violation {
        kind: "invariant".to_owned(),
        name: "Order_empty".to_owned(),
        step: 0,
    };
    assert_eq!(
        monitor
            .current_violation()
            .expect("check initial invariant"),
        Some(expected.clone())
    );
    let explored = fsl_runtime::bfs(model, 2).expect("solver-free BFS");
    assert_eq!(explored.violation, Some(expected));
}

#[test]
fn domain_nonempty_membership_still_holds_in_monitor_and_bfs() {
    let source = include_str!("../../fslc/tests/fixtures/domain_empty_membership.fsl")
        .replace("status in []", "status in [Draft]");
    let model = model(source);
    let monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize domain Monitor");
    assert_eq!(monitor.current_violation().expect("check invariant"), None);
    assert_eq!(
        fsl_runtime::bfs(model, 2)
            .expect("solver-free BFS")
            .violation,
        None
    );
}

#[test]
fn monitor_enforces_guards_and_updates_state() {
    let model = model(
        "spec Counter { type Count = 0..2 state { count: Count } init { count = 0 } ".to_owned()
            + "action add() { requires count < 2 count = count + 1 } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    for expected in [1, 2] {
        let action = monitor.enabled().expect("enabled actions")[0].clone();
        let stepped = monitor.step(&action).expect("step monitor");
        assert_eq!(stepped.violation, None, "clean increment: {stepped:?}");
        assert_eq!(monitor.state["count"], FslValue::Int(expected));
    }
    assert!(monitor.enabled().expect("enabled actions").is_empty());
}

#[test]
fn concrete_arithmetic_uses_smt_euclidean_division() {
    let model = model(
        "spec Arithmetic { state { x: Int } init { x = -1 } action stay() { x = x } ".to_owned()
            + "invariant Division { x / 2 == -1 and x % 2 == 1 } }",
    );
    let monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize monitor");
    let value = fsl_runtime::eval(
        &model.invariants[0].expr,
        &monitor.state,
        &mut BTreeMap::new(),
        &model,
        None,
    )
    .expect("evaluate expression");
    assert_eq!(value, FslValue::Bool(true));
}

/// DESIGN-divmod.md §2.1/§2.3: an invariant's own `/0`/`%0` is totally defined
/// as `0`, so it must never mask a genuine, independent invariant violation
/// behind a misattributed `partial_op` on the last-executed action. Reverting
/// the `TOTAL_DIVISION` scoping in `check_state_selected` makes both bumps
/// below report `partial_op`/`_partial_bump` (from `ZeroDivTotal`'s `5 / 0`)
/// instead of ever reaching the real `GenuineViolation` (issue #477).
#[test]
fn property_context_zero_division_does_not_mask_a_genuine_invariant_violation() {
    let model = model(
        "spec MaskTest { type Qty = 0..3 state { n: Qty } init { n = 0 } ".to_owned()
            + "action bump() { requires n < 3 n = n + 1 } "
            + "invariant GenuineViolation { n <= 1 } "
            + "invariant ZeroDivTotal { 5 / 0 == 0 } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    let first = monitor.enabled().expect("enabled actions")[0].clone();
    let stepped = monitor
        .step(&first)
        .expect("first bump must not error on /0");
    assert!(
        stepped.violation.is_none(),
        "ZeroDivTotal must not be checked/violated at n=1, got {:?}",
        stepped.violation
    );
    let second = monitor.enabled().expect("enabled actions")[0].clone();
    let stepped = monitor
        .step(&second)
        .expect("second bump must not error on /0");
    let violation = stepped.violation.expect("n=2 violates GenuineViolation");
    assert_eq!(violation.kind, "invariant");
    assert_eq!(violation.name, "GenuineViolation");
}

/// DESIGN-divmod.md §2.2: totality of `/0`/`%0` in property contexts must not
/// weaken the unrelated, pre-existing action-context `partial_op` check. An
/// action that unconditionally divides by a variable that can be zero is
/// still `partial_op`, attributed to the action that actually performs it.
#[test]
fn action_context_zero_division_is_still_partial_op() {
    let model = model(
        "spec ActionDiv0 { state { x: 0..10, d: 0..3 } init { x = 10 d = 0 } ".to_owned()
            + "action divide() { x = x / d } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    let action = monitor.enabled().expect("enabled actions")[0].clone();
    let stepped = monitor
        .step(&action)
        .expect("partial operations are outcomes, not errors");
    let violation = stepped
        .violation
        .expect("unguarded x / d must be partial_op");
    assert_eq!(violation.kind, "partial_op");
    assert_eq!(violation.name, "_partial_divide");
}

#[test]
fn replay_rejects_a_trace_that_is_not_enabled() {
    let model = model(
        "spec Once { state { done: Bool } init { done = false } ".to_owned()
            + "action finish() { requires not done done = true } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize monitor");
    let action = monitor.enabled().expect("enabled actions")[0].clone();
    let stepped = monitor.step(&action).expect("step monitor");
    assert_eq!(stepped.violation, None, "clean finish: {stepped:?}");
    let trace_action = fsl_core::TraceAction {
        name: action.action.clone(),
        params: action.params.clone(),
    };
    let trace = vec![
        fsl_core::TraceStep {
            step: 0,
            state: BTreeMap::from([("done".to_owned(), FslValue::Bool(false))]),
            action: None,
            changes: BTreeMap::new(),
        },
        fsl_core::TraceStep {
            step: 1,
            state: monitor.state.clone(),
            action: Some(trace_action),
            changes: BTreeMap::from([(
                "done".to_owned(),
                fsl_core::TraceChange {
                    from: FslValue::Bool(false),
                    to: FslValue::Bool(true),
                },
            )]),
        },
    ];
    fsl_runtime::replay_trace(model.clone(), &trace).expect("replay valid trace");
    let mut invalid = trace;
    invalid.push(invalid[1].clone());
    assert!(fsl_runtime::replay_trace(model, &invalid).is_err());
}

#[test]
fn action_cover_trace_uses_the_enabling_bool_value_and_may_end_at_terminal() {
    let model = model(
        "spec BoolGuard { state { done: Bool } init { done = false } ".to_owned()
            + "action accept(allowed: Bool) { requires not done requires allowed done = true } "
            + "terminal { done } }",
    );

    let traces = fsl_runtime::action_cover_traces(model.clone(), 8).expect("cover traces");
    let trace = &traces["accept"];
    assert_eq!(trace.len(), 2, "{trace:?}");
    assert_eq!(
        trace[1].action.as_ref().expect("covered action").params["allowed"],
        FslValue::Bool(true)
    );
    fsl_runtime::replay_trace(model.clone(), trace).expect("replay cover trace");

    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    let rejected = monitor
        .attempt(
            "accept",
            &BTreeMap::from([("allowed".to_owned(), FslValue::Bool(false))]),
        )
        .expect("observe disabled Bool binding");
    assert_eq!(
        rejected.violation.expect("false must be rejected").kind,
        "requires_failed"
    );
}

#[test]
fn action_cover_traces_exclude_violating_steps() {
    let model = model(
        "spec UnsafeCover { state { safe: Bool } init { safe = true } ".to_owned()
            + "action break_safety() { safe = false } invariant Safe { safe } }",
    );

    let traces = fsl_runtime::action_cover_traces(model, 1).expect("cover traces");
    assert!(!traces.contains_key("break_safety"), "{traces:?}");
}

#[test]
fn failed_steps_leave_the_monitor_state_unchanged() {
    for (suffix, expected_kind) in [
        ("invariant Safe { x == 0 }", "invariant"),
        ("trans Stable { x == old(x) }", "trans"),
        ("", "ensures"),
    ] {
        let ensures = if expected_kind == "ensures" {
            "ensures x == 0"
        } else {
            ""
        };
        let model = model(format!(
            "spec Rollback {{ state {{ x: Int }} init {{ x = 0 }} action break_it() {{ x = 1 {ensures} }} {suffix} }}"
        ));
        let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
        let before = monitor.state.clone();
        let action = monitor.enabled().expect("enabled actions")[0].clone();
        let result = monitor.step(&action).expect("step monitor");
        assert_eq!(
            result
                .violation
                .as_ref()
                .map(|violation| violation.kind.as_str()),
            Some(expected_kind)
        );
        assert_eq!(result.state, before);
        assert_eq!(monitor.state, before);
    }
}

#[test]
fn stale_enabled_action_is_rejected_after_state_change() {
    let model = model(
        "spec Once { state { done: Bool } init { done = false } ".to_owned()
            + "action finish() { requires not done done = true } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    let action = monitor.enabled().expect("enabled actions")[0].clone();

    let stepped = monitor.step(&action).expect("first step");
    assert_eq!(stepped.violation, None, "clean first step: {stepped:?}");
    let state_after_first_step = monitor.state.clone();
    let error = monitor.step(&action).expect_err("stale action must fail");

    assert!(error.message.contains("stale"), "{error:?}");
    assert_eq!(monitor.state, state_after_first_step);
    let failed = monitor
        .attempt("finish", &BTreeMap::new())
        .expect("disabled attempt is observable");
    assert_eq!(failed.violation.expect("requires violation").step, 2);
}

#[test]
fn action_parameters_must_belong_to_their_declared_domains() {
    let model = model(
        "spec Parameters { type Small = 0..1 enum Choice { A, B } state { done: Bool } ".to_owned()
            + "init { done = false } action inline(v in 0..1) { } "
            + "action domain(v: Small) { } action choice(v: Choice) { } "
            + "action flag(v: Bool) { } }",
    );
    let valid = [
        ("inline", FslValue::Int(0)),
        ("inline", FslValue::Int(1)),
        ("domain", FslValue::Int(0)),
        ("domain", FslValue::Int(1)),
        (
            "choice",
            FslValue::Enum {
                type_name: "Choice".to_owned(),
                member: "A".to_owned(),
            },
        ),
        ("flag", FslValue::Bool(false)),
        ("flag", FslValue::Bool(true)),
    ];
    for (action, value) in valid {
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize monitor");
        let result = monitor
            .attempt(action, &BTreeMap::from([("v".to_owned(), value)]))
            .unwrap_or_else(|error| panic!("valid {action} parameter failed: {error:?}"));
        assert!(result.violation.is_none(), "{action}: {result:?}");
    }

    let invalid = [
        ("inline", FslValue::Int(-1)),
        ("inline", FslValue::Int(2)),
        ("domain", FslValue::Int(-1)),
        ("domain", FslValue::Int(2)),
        ("choice", FslValue::Bool(false)),
        (
            "choice",
            FslValue::Enum {
                type_name: "Other".to_owned(),
                member: "A".to_owned(),
            },
        ),
        (
            "choice",
            FslValue::Enum {
                type_name: "Choice".to_owned(),
                member: "Missing".to_owned(),
            },
        ),
        ("flag", FslValue::Int(0)),
    ];
    for (action, value) in invalid {
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize monitor");
        let state_before_attempt = monitor.state.clone();
        let error = monitor
            .attempt(action, &BTreeMap::from([("v".to_owned(), value)]))
            .expect_err("out-of-domain parameter must fail");
        assert!(error.message.contains("parameter"), "{error:?}");
        assert_eq!(monitor.state, state_before_attempt);
    }
}

#[test]
fn wide_named_domain_parameter_validation_does_not_enumerate_for_direct_attempts() {
    let model = model(
        "spec Wide { type Value = 0..1000000000 state { done: Bool } init { done = false } action accept(v: Value) { done = true } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");

    let attempted = monitor
        .attempt(
            "accept",
            &BTreeMap::from([("v".to_owned(), FslValue::Int(1_000_000_000))]),
        )
        .expect("upper bound validates without domain enumeration");
    assert_eq!(
        attempted.violation, None,
        "the upper bound of the declared domain is in range: {attempted:?}"
    );
}

#[test]
fn invalid_parameters_do_not_consume_a_logical_step() {
    let model = model(
        "spec Steps { type Small = 0..1 state { done: Bool } init { done = false } action blocked(v: Small) { requires false } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    monitor
        .attempt(
            "blocked",
            &BTreeMap::from([("v".to_owned(), FslValue::Int(2))]),
        )
        .expect_err("invalid parameter must fail before attempting a step");
    let failed = monitor
        .attempt(
            "blocked",
            &BTreeMap::from([("v".to_owned(), FslValue::Int(1))]),
        )
        .expect("valid disabled attempt is observable");
    assert_eq!(failed.violation.expect("requires violation").step, 1);
}

/// #1119: a satisfied `exists` used to copy its whole local scope -- the
/// witnessing binder included -- back into the caller's bindings, so a
/// binder shadowing an action parameter overwrote it. `act(0)`'s only
/// witness here is `r = 1`, so the effects wrote `w[1]`/`a[1]` and the
/// explicit engine reported `proved` for a violated `NoW0`. The binder must
/// stay inside the quantifier: `typecheck.rs` types the body in a cloned
/// environment, so nothing outside can legitimately name it.
#[test]
fn exists_binder_does_not_overwrite_a_shadowed_action_parameter() {
    let model = model(
        "spec Shadow { type R = 0..1 state { a: Map<R, Bool>, w: Map<R, Bool> } \
         init { forall r: R { a[r] = true  w[r] = false } } \
         action act(r: R) { \
           requires a[r] and (exists r: R { a[r] and not (r == 0) }) \
           w[r] = true  a[r] = false } \
         invariant NoW0 { not w[0] } }",
    );
    let mut monitor = fsl_runtime::Monitor::new(model.clone()).expect("initialize monitor");

    // The quantifier leaves the caller's `r` alone even when it witnesses.
    let mut bindings = BTreeMap::from([("r".to_owned(), FslValue::Int(0))]);
    for guard in &model.actions[0].requires {
        let value = fsl_runtime::eval(guard, &monitor.state, &mut bindings, &model, None)
            .expect("evaluate the guard");
        assert_eq!(value, FslValue::Bool(true), "the guard holds for act(0)");
    }
    assert_eq!(
        bindings["r"],
        FslValue::Int(0),
        "the exists witness (r = 1) escaped into the caller's scope: {bindings:?}"
    );

    // ...and the effects therefore write the parameter's index.
    let stepped = monitor
        .attempt("act", &BTreeMap::from([("r".to_owned(), FslValue::Int(0))]))
        .expect("act(0) is enabled");
    let violation = stepped.violation.clone().expect("act(0) breaks NoW0");
    assert_eq!(violation.name, "NoW0", "{stepped:?}");
    let attempted = stepped
        .attempted_state
        .as_ref()
        .expect("the violating successor state");
    let map = |zero: bool, one: bool| {
        FslValue::Map(BTreeMap::from([
            (FslValue::Int(0), FslValue::Bool(zero)),
            (FslValue::Int(1), FslValue::Bool(one)),
        ]))
    };
    assert_eq!(
        attempted["w"],
        map(true, false),
        "act(0) must raise w[0], not the witness's w[1]: {stepped:?}"
    );
    assert_eq!(
        attempted["a"],
        map(false, true),
        "act(0) must clear a[0], not the witness's a[1]: {stepped:?}"
    );
}

/// Issue #1247 reads a `Var`/`Index`/`Field` chain (`design[c].st`) by
/// reference instead of cloning the whole state variable at each level. The
/// order a chain fails in must stay the bottom-up one: each step evaluates
/// its index and then looks it up before the next index is evaluated. Both
/// chains below have two failing steps; reading every index first (or every
/// lookup last) would report the other one. The nested map is built by hand:
/// `check` admits no `Map` of `Map`s, but the evaluator is shared by every
/// shape a chain can take.
#[test]
fn place_read_chain_keeps_the_bottom_up_failure_order() {
    use fsl_core::KernelExpr as Expr;

    let model = model("spec Place { state { z: Int } init { z = 0 } action stay() { z = z } }");
    let leaf = |a: i64| FslValue::Struct {
        type_name: "P".to_owned(),
        fields: BTreeMap::from([("a".to_owned(), FslValue::Int(a))]),
    };
    let row = |i: i64| FslValue::Map((0..2).map(|j| (FslValue::Int(j), leaf(i + j))).collect());
    let state = BTreeMap::from([
        (
            "m".to_owned(),
            FslValue::Map((0..2).map(|i| (FslValue::Int(i), row(i))).collect()),
        ),
        ("z".to_owned(), FslValue::Int(0)),
    ]);
    let boxed = Box::new;
    let outside_key = || Expr::Num(9);
    let zero_division = || Expr::Binary {
        op: "/".to_owned(),
        left: boxed(Expr::Num(1)),
        right: boxed(Expr::Var("z".to_owned())),
    };
    let chain = |inner: Expr, outer: Expr| {
        Expr::Field(
            boxed(Expr::Index(
                boxed(Expr::Index(boxed(Expr::Var("m".to_owned())), boxed(inner))),
                boxed(outer),
            )),
            "a".to_owned(),
        )
    };
    let eval = |expr: &Expr| fsl_runtime::eval(expr, &state, &mut BTreeMap::new(), &model, None);

    let inner_key = eval(&chain(outside_key(), zero_division())).expect_err("m[9] is outside");
    assert!(
        inner_key.map_key_outside_domain && inner_key.partial_operation.is_none(),
        "the inner lookup fails before the outer index is evaluated: {inner_key:?}"
    );
    let inner_division =
        eval(&chain(zero_division(), outside_key())).expect_err("1 / 0 in action context");
    assert!(
        inner_division.partial_operation.is_some() && !inner_division.map_key_outside_domain,
        "the inner index is evaluated before the outer lookup: {inner_division:?}"
    );
    let not_a_struct = eval(&Expr::Field(
        boxed(Expr::Index(
            boxed(Expr::Var("m".to_owned())),
            boxed(Expr::Num(1)),
        )),
        "a".to_owned(),
    ))
    .expect_err("m[1] is a map");
    assert_eq!(not_a_struct.message, "field access requires a struct");
    assert_eq!(
        eval(&chain(Expr::Num(1), Expr::Num(1))).expect("leaf read"),
        FslValue::Int(2)
    );
}

/// `enabled` lists instances action by action, each action's parameters as
/// an odometer whose first parameter varies slowest. The bindings are
/// generated one at a time (a large `values` scope is never listed), and this
/// order is the one the listed product had.
#[test]
fn enabled_lists_parameter_bindings_first_parameter_slowest() {
    let model = model(
        "spec Order { type Small = 0..1 enum Color { Red, Blue } state { n: 0..3 } init { n = 0 } "
            .to_owned()
            + "action pick(a: Small, c: Color, b: Bool, r in 2..3) { requires n == 0 n = 1 } "
            + "action skip() { requires n == 0 n = 2 } }",
    );
    let monitor = fsl_runtime::Monitor::new(model).expect("initialize monitor");
    let listed = monitor
        .enabled()
        .expect("enabled actions")
        .into_iter()
        .map(|instance| {
            let params = ["a", "c", "b", "r"]
                .iter()
                .filter_map(|name| instance.params.get(*name))
                .map(|value| match value {
                    FslValue::Int(value) => value.to_string(),
                    FslValue::Bool(value) => value.to_string(),
                    FslValue::Enum { member, .. } => member.clone(),
                    other => format!("{other:?}"),
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{}({params})", instance.action)
        })
        .collect::<Vec<_>>();
    let mut expected = Vec::new();
    for a in ["0", "1"] {
        for c in ["Red", "Blue"] {
            for b in ["false", "true"] {
                for r in ["2", "3"] {
                    expected.push(format!("pick({a},{c},{b},{r})"));
                }
            }
        }
    }
    expected.push("skip()".to_owned());
    assert_eq!(listed, expected);
}

/// `enabled_where` keeps only the instances its filter accepts, and still
/// evaluates every other instance's guards: a guard that is undefined for an
/// instance the filter rejects fails it exactly as it fails `enabled`, so a
/// requirements trace step matched through it keeps the full listing's
/// errors.
#[test]
fn enabled_where_filters_instances_but_keeps_every_guard_error() {
    let total = model(
        "spec Pick { type X = 0..2 state { n: 0..3 } init { n = 0 } ".to_owned()
            + "action a(x: X) { requires n == 0 n = 1 } "
            + "action b() { requires n == 0 n = 2 } }",
    );
    let monitor = fsl_runtime::Monitor::new(total).expect("initialize monitor");
    let kept = monitor
        .enabled_where(|action, params| action == "a" && params["x"] == FslValue::Int(2))
        .expect("enabled actions");
    let all = monitor.enabled().expect("enabled actions");
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(Some(&kept[0]), all.get(2), "{all:?}");

    let undefined = model(
        "spec Undefined { type X = 0..2 state { n: 0..3 } init { n = 0 } ".to_owned()
            + "action a(x: X) { requires 10 / x > 0 n = 1 } "
            + "action b() { requires n == 0 n = 2 } }",
    );
    let monitor = fsl_runtime::Monitor::new(undefined).expect("initialize monitor");
    let listed = monitor.enabled().expect_err("a(0) divides by zero");
    let filtered = monitor
        .enabled_where(|action, _| action == "b")
        .expect_err("a(0) divides by zero");
    assert_eq!(filtered.to_string(), listed.to_string());
}

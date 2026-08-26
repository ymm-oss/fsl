// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_core::{FsResolver, TypeDef, TypeRef, build_model, parse_kernel_source};

fn model(source: &str) -> fsl_core::KernelModel {
    build_model(parse_kernel_source(source, &FsResolver::new(".")).expect("parse kernel"))
        .expect("build model")
}

#[test]
fn explicit_bfs_proves_at_state_space_closure() {
    let model = model(
        "spec Once { state { done: Bool } init { done = false } \
         action finish() { requires not done done = true } \
         invariant BooleanState { done or not done } terminal { done } }",
    );
    let result = fsl_runtime::verify_explicit(model, 4, 100).expect("explicit verification");
    assert!(result.closure);
    assert!(!result.budget_exceeded);
    assert!(result.violation.is_none());
    assert_eq!(result.states_explored, 2);
    assert_eq!(result.depth_reached, 1);
    assert_eq!(result.deadlock_step, None);
}

/// DESIGN-divmod.md §2.1/§2.3: an invariant's own `5 / 0` must not turn
/// `--engine explicit` verification into a raw `RuntimeError` ("division by
/// zero"). Reverting the concrete evaluator's zero-divisor totality (or its
/// `TOTAL_DIVISION` property-context scoping) makes this `Err` instead of
/// reporting the real `GenuineViolation` (issue #477, symptom 2).
#[test]
fn explicit_engine_totalizes_property_context_zero_division() {
    let model = model(
        "spec MaskTest { type Qty = 0..3 state { n: Qty } init { n = 0 } \
         action bump() { requires n < 3 n = n + 1 } \
         invariant GenuineViolation { n <= 1 } \
         invariant ZeroDivTotal { 5 / 0 == 0 } }",
    );
    let result =
        fsl_runtime::verify_explicit(model, 4, 100).expect("explicit verification must not error");
    let violation = result
        .violation
        .expect("depth 4 reaches n=2, which violates GenuineViolation");
    assert_eq!(violation.violation.kind, "invariant");
    assert_eq!(violation.violation.name, "GenuineViolation");
}

#[test]
fn explicit_bfs_fails_closed_at_the_state_budget() {
    let model = model(
        "spec Counter { type Count = 0..2 state { count: Count } init { count = 0 } \
         action add() { requires count < 2 count = count + 1 } }",
    );
    let result = fsl_runtime::verify_explicit(model, 4, 1).expect("budget verdict");
    assert!(result.budget_exceeded);
    assert!(!result.closure);
    assert!(result.violation.is_none());
    assert_eq!(result.states_explored, 1);
    assert_eq!(result.depth_reached, 0);
}

#[test]
fn explicit_bfs_rejects_underconstrained_or_order_dependent_init() {
    let missing = model(
        "spec Missing { state { x: Bool, y: Bool } init { x = false } \
         action stay() { x = x y = y } }",
    );
    let error = fsl_runtime::verify_explicit(missing, 2, 100).expect_err("missing init rejected");
    assert_eq!(error.message, "init does not assign state variable(s): y");

    let read_before_write = model(
        "spec ReadBeforeWrite { state { x: Bool, y: Bool } \
         init { x = y y = false } action stay() { x = x y = y } }",
    );
    let error = fsl_runtime::verify_explicit(read_before_write, 2, 100)
        .expect_err("order-dependent init rejected");
    assert_eq!(
        error.message,
        "init references state variable 'y' before it is assigned"
    );
}

#[test]
fn deterministic_init_rejects_state_dependent_forall_domains() {
    let unassigned_range = model(
        "spec UnassignedRange { type Slot = 0..2 state { n: Slot, m: Map<Slot, Bool> } \
         init { forall i in 0..n { m[i] = true } n = 0 } \
         action stay() { n = n } }",
    );
    let error = fsl_runtime::verify_explicit(unassigned_range, 2, 100)
        .expect_err("state-dependent init range rejected");
    assert_eq!(
        error.message,
        "init forall range bounds must be compile-time constants; state variable 'n' is not allowed"
    );

    let assigned_range = model(
        "spec AssignedRange { type Slot = 0..2 state { n: Slot, m: Map<Slot, Bool> } \
         init { n = 2 forall i in 0..n { m[i] = true } } \
         action stay() { n = n } }",
    );
    let error = fsl_runtime::verify_explicit(assigned_range, 2, 100)
        .expect_err("assigned state still cannot bound an init range");
    assert_eq!(
        error.message,
        "init forall range bounds must be compile-time constants; state variable 'n' is not allowed"
    );

    let state_collection = model(
        "spec StateCollection { type Slot = 0..2 \
         state { s: Set<Slot>, m: Map<Slot, Bool> } \
         init { s = Set { 0, 1 } forall i in s { m[i] = true } } \
         action stay() { s = s } }",
    );
    let error = fsl_runtime::verify_explicit(state_collection, 2, 100)
        .expect_err("state collection init domain rejected");
    assert_eq!(
        error.message,
        "init forall over a state collection is not supported; state variable 's' is not allowed"
    );

    let state_range_filter = model(
        "spec StateRangeFilter { type Slot = 0..2 state { n: Slot, m: Map<Slot, Bool> } \
         init { forall i in 0..2 where n == 0 { m[i] = true } n = 0 } \
         action stay() { n = n } }",
    );
    let error = fsl_runtime::verify_explicit(state_range_filter, 2, 100)
        .expect_err("state-dependent init range filter rejected");
    assert_eq!(
        error.message,
        "init references state variable 'n' before it is assigned"
    );

    let const_range = model(
        "spec ConstRange { const CAP = 2 type Slot = 0..2 \
         state { m: Map<Slot, Bool> } \
         init { forall i in 0..CAP { m[i] = true } } \
         action stay() { m[0] = m[0] } }",
    );
    let result = fsl_runtime::verify_explicit(const_range, 2, 100)
        .expect("compile-time const init range accepted");
    assert!(result.closure);
    assert!(result.violation.is_none());
}

#[test]
fn deterministic_init_tracks_branches_foralls_and_duplicate_locations() {
    let both_branches = model(
        "spec Branches { state { flag: Bool, value: Bool } \
         init { flag = false if flag { value = true } else { value = false } } \
         action stay() { flag = flag value = value } }",
    );
    let both = fsl_runtime::verify_explicit(both_branches, 1, 100)
        .expect("both init branches definitely assign value");
    assert_eq!(both.violation, None, "{both:?}");

    let one_branch = model(
        "spec OneBranch { state { flag: Bool, value: Bool } \
         init { flag = false if flag { value = true } } \
         action stay() { flag = flag value = value } }",
    );
    let error =
        fsl_runtime::verify_explicit(one_branch, 1, 100).expect_err("one branch is incomplete");
    assert_eq!(
        error.message,
        "init does not assign state variable(s): value"
    );

    let distinct_keys = model(
        "spec DistinctKeys { enum Key { A, B } state { values: Map<Key, Bool> } \
         init { values[A] = false values[B] = true } \
         action stay() { values[A] = values[A] } }",
    );
    let distinct = fsl_runtime::verify_explicit(distinct_keys, 1, 100)
        .expect("separate concrete map keys are not duplicate init writes");
    assert_eq!(distinct.violation, None, "{distinct:?}");

    let duplicate = model(
        "spec Duplicate { state { value: Bool } init { value = false value = true } \
         action stay() { value = value } }",
    );
    let error =
        fsl_runtime::verify_explicit(duplicate, 1, 100).expect_err("duplicate init rejected");
    assert_eq!(
        error.message,
        "state variable 'value' assigned more than once in init"
    );
}

#[test]
fn deterministic_init_rejects_nested_forall_with_bounded_map_keys() {
    let nested_forall = model(
        "spec Nested { const MAX = 1 type Key = 0..1 state { values: Map<Key, Int> } \
         init { forall i in 0..MAX: { forall j in 0..MAX: { values[i] = j } } } \
         action stay() { values[0] = values[0] } }",
    );
    let error = fsl_runtime::verify_explicit(nested_forall, 1, 100)
        .expect_err("nested init forall rejected");
    assert_eq!(error.message, "nested forall in init is not supported");
}

#[test]
fn init_accepts_distinct_integer_const_keys_for_bounded_maps() {
    // A named integer domain lowers to TypeRef::Named. Keep this control so
    // the TypeDef::Domain arm of resolve_integer_const_key remains covered.
    let named_domain_consts = model(
        "spec NamedDomainConstKeys { const K = 0 const J = 1 type Key = 0..1 \
         state { m: Map<Key, Bool> } \
         init { m[K] = true m[J] = false } action noop() { } }",
    );
    let TypeRef::Map(key_type, _) = named_domain_consts.state_type("m").expect("map state type")
    else {
        panic!("m must be a map");
    };
    assert!(matches!(
        key_type.as_ref(),
        TypeRef::Named(type_name)
            if matches!(named_domain_consts.types.get(type_name), Some(TypeDef::Domain { .. }))
    ));
    fsl_runtime::Monitor::new(named_domain_consts.clone())
        .expect("Monitor accepts fully covered named-domain const keys");
    let result = fsl_runtime::verify_explicit(named_domain_consts, 1, 100)
        .expect("explicit BFS accepts fully covered named-domain const keys");
    assert!(result.closure);
    assert!(result.violation.is_none());

    // Inline map bounds lower directly to TypeRef::Range rather than the
    // named-domain representation above, so exercise that separate type gate.
    let inline_range_consts = model(
        "spec InlineRangeConstKeys { const K = 0 const J = 1 \
         state { m: Map<0..1, Bool> } \
         init { m[K] = true m[J] = false } action noop() { } }",
    );
    let TypeRef::Map(key_type, _) = inline_range_consts.state_type("m").expect("map state type")
    else {
        panic!("m must be a map");
    };
    assert!(matches!(key_type.as_ref(), TypeRef::Range(0, 1)));
    fsl_runtime::Monitor::new(inline_range_consts.clone())
        .expect("Monitor accepts fully covered inline-range const keys");
    let result = fsl_runtime::verify_explicit(inline_range_consts, 1, 100)
        .expect("explicit BFS accepts fully covered inline-range const keys");
    assert!(result.closure);
    assert!(result.violation.is_none());

    // Regression control: enum-key coverage is still resolved as enum values.
    let enum_keys = model(
        "spec EnumKeys { enum Key { A, B } state { m: Map<Key, Bool> } \
         init { m[A] = true m[B] = false } action noop() { } }",
    );
    fsl_runtime::Monitor::new(enum_keys.clone())
        .expect("Monitor still accepts fully covered enum keys");
    let result = fsl_runtime::verify_explicit(enum_keys, 1, 100)
        .expect("explicit BFS still accepts fully covered enum keys");
    assert!(result.closure);
    assert!(result.violation.is_none());
}

#[test]
fn init_integer_const_key_duplicate_assignment_is_rejected_for_bounded_maps() {
    let duplicate = model(
        "spec DuplicateConst { const K = 0 type Key = 0..1 state { m: Map<Key, Bool> } \
         init { m[K] = true m[K] = false } action noop() { } }",
    );
    let error = fsl_runtime::verify_explicit(duplicate, 1, 100)
        .expect_err("the same integer const key remains a duplicate init write");
    assert_eq!(
        error.message,
        "state variable 'm' assigned more than once in init"
    );
}

#[test]
fn init_integer_const_key_bound_aliasing_is_rejected_for_bounded_maps() {
    let bound_alias = model(
        "spec BoundAlias { const K = 0 const MAX = 1 type Key = 0..1 state { m: Map<Key, Bool> } \
         init { forall i in 0..MAX { m[i] = true } m[K] = false } action noop() { } }",
    );
    let error = fsl_runtime::verify_explicit(bound_alias, 1, 100)
        .expect_err("a const key still collides with a bound key that can alias it");
    assert_eq!(
        error.message,
        "state variable 'm' assigned more than once in init"
    );
}

#[test]
fn init_integer_const_keys_reject_partial_bounded_map_initialization() {
    let partial = model(
        "spec PartialConst { const K = 0 const J = 1 type Key = 0..2 \
         state { m: Map<Key, Bool> } init { m[K] = true m[J] = false } action noop() { } }",
    );
    let error = fsl_runtime::verify_explicit(partial, 1, 100)
        .expect_err("integer constants must not make a partially initialized map complete");
    assert_eq!(
        error.message,
        "init does not assign state variable(s): m (partial component initialization is rejected by the explicit engine)"
    );
}

/// Negative control for #480: a `forall` binder over more than one value
/// that writes to a target *not* indexed by the binder demands the same
/// location equal every binder value simultaneously. When those values
/// differ, no initial state can satisfy every iteration at once — the same
/// contradiction the symbolic engine already reports as unsatisfiable init.
/// Before the fix, `Monitor::new` executed the forall as an imperative
/// last-write-wins loop and silently produced a (bogus) initial state.
#[test]
fn forall_init_writing_conflicting_values_to_the_same_location_is_unsatisfiable() {
    let contradictory = model(
        "spec Contradictory { type K = 0..1 state { x: Int } \
         init { forall k: K { x = k } } action noop() { } }",
    );
    let error = fsl_runtime::verify_explicit(contradictory, 4, 100)
        .expect_err("contradictory forall init rejected");
    assert_eq!(error.message, "init constraints are unsatisfiable");

    // Regression control: repeating the *same* value on every iteration is
    // satisfiable and must not be flagged.
    let consistent = model(
        "spec Consistent { type K = 0..2 state { ready: Bool } \
         init { forall k: K { ready = true } } \
         action noop() { } invariant AlwaysReady { ready } }",
    );
    let result =
        fsl_runtime::verify_explicit(consistent, 4, 100).expect("consistent forall init accepted");
    assert!(result.closure);
    assert!(result.violation.is_none());
}

/// Issue #821: `docs/LANGUAGE.md`'s "assign exactly once" rule is per
/// concrete key, not per variable. A `forall i { m[i] = ... }` write and a
/// later flat `m[K] = ...` write must collide when `K` is one of the keys
/// the `forall` already covered, whether or not the values agree — before
/// the fix, `init_write_key` collapsed every forall-indexed write to
/// `Root(name)` (whole-variable), a different bucket from the flat write's
/// `ConcreteIndex`, so the overlap was never detected.
#[test]
fn init_forall_write_collides_with_later_concrete_write_to_the_same_key() {
    // Rejecting control (case 1, issue #821): conflicting values on the
    // same key. Before the fix this fell through to the unrelated
    // "init constraints are unsatisfiable" diagnostic instead of the
    // duplicate-write rule's own message.
    let conflicting = model(
        "spec Probe { type Idx = 0..2 state { m: Map<Idx, Bool> } \
         init { forall i: Idx { m[i] = true } m[0] = false } \
         action noop() { } }",
    );
    let error = fsl_runtime::verify_explicit(conflicting, 1, 100)
        .expect_err("forall write and overlapping concrete write to the same key are rejected");
    assert_eq!(
        error.message,
        "state variable 'm' assigned more than once in init"
    );

    // Rejecting control (case 2, issue #821): the values happen to agree.
    // Before the fix this was silently accepted with no error at all.
    let agreeing = model(
        "spec Probe { type Idx = 0..2 state { m: Map<Idx, Bool> } \
         init { forall i: Idx { m[i] = true } m[0] = true } \
         action noop() { } }",
    );
    let error = fsl_runtime::verify_explicit(agreeing, 1, 100).expect_err(
        "forall write and overlapping concrete write to the same key are rejected even when values agree",
    );
    assert_eq!(
        error.message,
        "state variable 'm' assigned more than once in init"
    );

    // Rejecting control: the same overlap through an enum-member key
    // instead of a numeric literal, exercising the enum-resolution arm of
    // `assignment_coverage` rather than `Expr::Num`.
    let enum_overlap = model(
        "spec Probe { enum Key { A, B } state { m: Map<Key, Bool> } \
         init { forall k: Key { m[k] = true } m[A] = true } \
         action noop() { } }",
    );
    let error = fsl_runtime::verify_explicit(enum_overlap, 1, 100)
        .expect_err("forall write and overlapping enum-member concrete write are rejected");
    assert_eq!(
        error.message,
        "state variable 'm' assigned more than once in init"
    );

    // Accepting control: an injective `forall` write on its own, with no
    // overlapping concrete write, must stay accepted.
    let lone_forall = model(
        "spec Probe { type Idx = 0..2 state { m: Map<Idx, Bool> } \
         init { forall i: Idx { m[i] = true } } \
         action noop() { } }",
    );
    let result =
        fsl_runtime::verify_explicit(lone_forall, 1, 100).expect("lone injective forall accepted");
    assert!(result.violation.is_none());

    // Accepting control: a forall on one map and flat writes to a wholly
    // separate map are trivially not duplicates (different logical roots,
    // not disjoint keys on the same map). See the three same-map controls
    // below for the case this change actually had to get right.
    let disjoint_variables = model(
        "spec Probe { type Idx = 0..2 state { m: Map<Idx, Bool>, n: Map<Idx, Bool> } \
         init { forall i: Idx { m[i] = true } n[0] = true n[1] = false n[2] = true } \
         action noop() { } }",
    );
    let result = fsl_runtime::verify_explicit(disjoint_variables, 1, 100)
        .expect("forall write and flat writes to an unrelated map's keys are not duplicates");
    assert!(result.violation.is_none());

    // Accepting control: two `forall` blocks over disjoint subranges of the
    // *same* map's key domain must not collide with each other.
    let disjoint_foralls = model(
        "spec Probe { type Idx = 0..3 state { m: Map<Idx, Bool> } \
         init { forall i in 0..1 { m[i] = true } forall j in 2..3 { m[j] = false } } \
         action noop() { } }",
    );
    let result = fsl_runtime::verify_explicit(disjoint_foralls, 1, 100)
        .expect("two foralls over disjoint subranges of the same map do not collide");
    assert!(result.violation.is_none());

    // Accepting control: a forall covering one subrange and a flat write to
    // a key outside that subrange, on the *same* map, must not collide.
    let forall_then_flat = model(
        "spec Probe { type Idx = 0..2 state { m: Map<Idx, Bool> } \
         init { forall i in 0..1 { m[i] = true } m[2] = false } \
         action noop() { } }",
    );
    let result = fsl_runtime::verify_explicit(forall_then_flat, 1, 100)
        .expect("a forall and a same-map flat write to a key it does not cover do not collide");
    assert!(result.violation.is_none());

    // Accepting control: the same shape in the opposite order -- the flat
    // write first, the forall second -- since detection must not depend on
    // statement order.
    let flat_then_forall = model(
        "spec Probe { type Idx = 0..2 state { m: Map<Idx, Bool> } \
         init { m[2] = false forall i in 0..1 { m[i] = true } } \
         action noop() { } }",
    );
    let result = fsl_runtime::verify_explicit(flat_then_forall, 1, 100).expect(
        "a same-map flat write followed by a forall that does not cover its key do not collide",
    );
    assert!(result.violation.is_none());
}

/// Issue #826: an indexed init write whose key coverage cannot be resolved
/// may touch every key of its logical map root. It therefore overlaps both
/// concrete-key writes and another unresolved indexed write, while a lone
/// unresolved write has no competing owner and remains admissible to BMC.
#[test]
fn unresolved_init_index_writes_collide_by_logical_root() {
    let root_concrete = model(
        "spec RootConcrete { type Idx = 0..2 state { m: Map<Idx, Bool> } \
         init { forall i: Idx { m[i - i] = true } forall j: Idx { m[j] = true } } \
         action noop() { } }",
    );
    let error = fsl_runtime::check_init_write_ownership(&root_concrete)
        .expect_err("unresolved write must overlap every concrete key on the same root");
    assert_eq!(
        error.message,
        "state variable 'm' assigned more than once in init forall"
    );
    assert!(
        error.span.is_some(),
        "collision must retain the second write span"
    );

    let root_root = model(
        "spec RootRoot { type Idx = 0..2 state { m: Map<Idx, Bool> } \
         init { forall i: Idx { m[i - i] = true } \
                forall j: Idx { m[j - j] = true } } action noop() { } }",
    );
    let error = fsl_runtime::check_init_write_ownership(&root_root)
        .expect_err("two unresolved writes must overlap on the same logical root");
    assert_eq!(
        error.message,
        "state variable 'm' assigned more than once in init forall"
    );
    assert!(
        error.span.is_some(),
        "collision must retain the second write span"
    );

    let lone_root = model(
        "spec LoneRoot { type Idx = 0..2 state { m: Map<Idx, Bool> } \
         init { forall i: Idx { m[i - i] = true } } action noop() { } }",
    );
    fsl_runtime::check_init_write_ownership(&lone_root)
        .expect("a lone unresolved write has no duplicate owner");
}

#[test]
fn explicit_violation_trace_replays_through_the_monitor() {
    let model = model(
        "spec Overflow { type Count = 0..1 state { count: Count } init { count = 0 } \
         action add() { count = count + 1 } }",
    );
    let result =
        fsl_runtime::verify_explicit(model.clone(), 3, 100).expect("explicit verification");
    let violation = result.violation.expect("type-bound violation");
    assert_eq!(violation.violation.kind, "type_bound");
    assert_eq!(violation.violation.step, 2);
    fsl_runtime::replay_trace(model, &violation.trace).expect("replay explicit trace");
}

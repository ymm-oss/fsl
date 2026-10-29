// SPDX-License-Identifier: Apache-2.0

//! Out-of-carrier values are real members, not absent grid cells.
//! Monitor expectations and measured base verdicts are recorded beside the cases.

#[path = "support/collection_replay.rs"]
mod collection_replay;

use collection_replay::{ReplayCase, finish, require};

// Relations have no size() method: cardinality consumers use domain/range Sets.
// bounds() is internal; bounded-state assignments exercise _bounds_s/_bounds_r.
//
// Measured base 1844fa74, depth 4 (BMC / induction / explicit):
// V = (0, verified, bounded), P = (0, proved, unbounded),
// X = (1, violated, bounded), C = (1, unknown_cti, bounded),
// E = (3, error, internal: "trace state mismatch at step 2").
// These are observations, never allowed alternatives to Monitor agreement.
//
// For BOTH Set and relation, each non-remove property-holds case is X/X/P,
// and each non-remove effect-holds case is E/E/P. Monitor is (0, conformant).
// Rejects cases in all three contexts, Monitor always (1, nonconformant):
//   contains, size, domain-size, range-size, equality, duplicate, remove-one,
//   chain: V/P/X
// Cancelling remove is a preservation control in every context:
//   holds: V/P/P with Monitor (0, conformant);
//   rejects: X/X/X with Monitor (1, nonconformant).
// Guard cases use only rejects, so the fixed trace always has an enabled probe.
fn predicates(relation: bool) -> Vec<(&'static str, String)> {
    let value = "if done then 2 else 0";
    let other = "if done then 3 else 0";
    let third = "if done then 4 else 0";
    if relation {
        let added = format!("r.add({value}, {other})");
        vec![
            ("contains", format!("{added}.contains(2, 3)")),
            ("domain-size", format!("domain({added}).size() == 1")),
            ("range-size", format!("range({added}).size() == 1")),
            ("equality", format!("{added} != r")),
            (
                "duplicate",
                format!("domain({added}.add({value}, {other})).size() == 1"),
            ),
            ("remove", format!("{added}.remove({value}, {other}) == r")),
            (
                "remove-one",
                format!("{added}.add({other}, {third}).remove({value}, {other}).contains(3, 4)"),
            ),
            (
                "chain",
                format!("domain({added}.add({other}, {third}).add({third}, {value})).size() == 3"),
            ),
        ]
    } else {
        let added = format!("s.add({value})");
        vec![
            ("contains", format!("{added}.contains(2)")),
            ("size", format!("{added}.size() == 1")),
            ("equality", format!("{added} != s")),
            ("duplicate", format!("{added}.add({value}).size() == 1")),
            ("remove", format!("{added}.remove({value}) == s")),
            (
                "remove-one",
                format!("{added}.add({other}).remove({value}).contains(3)"),
            ),
            (
                "chain",
                format!("{added}.add({other}).add({third}).size() == 3"),
            ),
        ]
    }
}

fn source(relation: bool, action: &str, invariant: &str) -> String {
    let (state, init) = if relation {
        ("r: relation K -> K", "r = Set {}")
    } else {
        ("s: Set<K>", "s = Set {}")
    };
    let empty_receiver = if relation {
        "domain(r).size() == 0"
    } else {
        "s.size() == 0"
    };
    format!(
        "spec Retention {{
  type K = 0..1
  state {{ {state}, done: Bool, fin: Bool }}
  init {{ {init} done = false fin = false }}
  action go() {{ requires not done done = true }}
  {action}
  invariant ReceiverFrame {{ {empty_receiver} }}
  invariant P {{ {invariant} }}
}}\n"
    )
}

fn consumers(relation: bool, context: &str) {
    let mut errors = Vec::new();
    for (form, predicate) in predicates(relation) {
        // The cancelling remove is a preservation control; remove-one retains
        // a different outside member and rejects the dropped-member mutant.
        for conformant in [true, false] {
            if context == "guard" && conformant {
                continue; // A false guard would make the fixed trace inapplicable.
            }
            let condition = if conformant {
                predicate.clone()
            } else {
                format!("not ({predicate})")
            };
            let (action, invariant, trace) = match context {
                "property" => (
                    String::new(),
                    format!("not done or ({condition})"),
                    vec!["go"],
                ),
                "guard" => (
                    format!(
                        "action probe() {{ requires done and not fin requires {predicate} fin = true }}"
                    ),
                    "not fin".to_owned(),
                    vec!["go", "probe"],
                ),
                "effect" => (
                    format!(
                        "action probe() {{ requires done and not fin fin = not ({condition}) }}"
                    ),
                    "not fin".to_owned(),
                    vec!["go", "probe"],
                ),
                _ => unreachable!(),
            };
            let name = format!(
                "{}-{context}-{form}-{}",
                if relation { "relation" } else { "set" },
                if conformant { "holds" } else { "rejects" }
            );
            ReplayCase::new(&name, &source(relation, &action, &invariant), &trace).agreement(
                4,
                conformant,
                None,
                &mut errors,
            );
        }
    }
    finish(&errors);
}

// Base measurements are verdict tuples (exit, result), not permitted outcomes.
// Each function includes a rejecting outside-member control; exit 2 never passes.
#[test]
fn set_property_consumers_retain_members() {
    consumers(false, "property");
}

#[test]
fn relation_property_consumers_retain_pairs() {
    consumers(true, "property");
}

#[test]
fn set_guard_consumers_retain_members() {
    consumers(false, "guard");
}

#[test]
fn relation_guard_consumers_retain_pairs() {
    consumers(true, "guard");
}

#[test]
fn set_effect_consumers_retain_members() {
    consumers(false, "effect");
}

#[test]
fn relation_effect_consumers_retain_pairs() {
    consumers(true, "effect");
}

const FG16: &str = "spec FG16 {
  type K = 0..1
  type C = 0..5
  state { s: Set<K>, c: C }
  init { s = Set {} c = 0 }
  action tick() { requires c < 5 c = c + 1 }
  action go() { requires c == 5 s = s.add(if c == 5 then 2 else 0) }
  invariant P { not s.contains(2) }
}\n";

#[test]
fn fg16_depth_rule_does_not_promote_a_bounded_result() {
    // Measured base: depth 4 = verified / proved(unbounded) / verified;
    // depth 8 = violated / violated / violated; Monitor = nonconformant _bounds_s.
    let case = ReplayCase::new(
        "fg16-depth",
        FG16,
        &["tick", "tick", "tick", "tick", "tick", "go"],
    );
    let mut errors = Vec::new();
    for engine in ["bmc", "explicit"] {
        let (report, exit) = case.observe(engine, 4);
        require(
            &mut errors,
            exit == 0 && report["result"] == "verified" && report["completeness"] == "bounded",
            &case.name,
            engine,
            &report,
            exit,
        );
    }
    let (report, exit) = case.observe("induction", 4);
    // The shallow induction step may expose a CTI, not invent a reachable trace.
    require(
        &mut errors,
        exit == 1 && report["result"] == "unknown_cti" && report["completeness"] != "unbounded",
        &case.name,
        "induction depth 4",
        &report,
        exit,
    );
    case.agreement(8, false, Some("_bounds_s"), &mut errors);
    finish(&errors);
}

#[test]
fn relation_bounded_storage_exposes_both_outside_endpoints() {
    // Base depth 4 induction: P; depth 8 all X for _bounds_r.
    // Monitor: (1, nonconformant), type_bound _bounds_r at event 5.
    let source = "spec RelationStorage {
  type K = 0..1
  type C = 0..5
  state { r: relation K -> K, c: C }
  init { r = Set {} c = 0 }
  action tick() { requires c < 5 c = c + 1 }
  action go() { requires c == 5 r = r.add(if c == 5 then 2 else 0, if c == 5 then 3 else 0) }
  invariant P { not r.contains(2, 3) }
}\n";
    let case = ReplayCase::new(
        "relation-storage",
        source,
        &["tick", "tick", "tick", "tick", "tick", "go"],
    );
    let mut errors = Vec::new();
    let (report, exit) = case.observe("induction", 4);
    require(
        &mut errors,
        exit == 1 && report["result"] == "unknown_cti" && report["completeness"] != "unbounded",
        &case.name,
        "induction depth 4",
        &report,
        exit,
    );
    case.agreement(8, false, Some("_bounds_r"), &mut errors);
    finish(&errors);
}

#[test]
fn existing_typed_binders_retain_outside_members() {
    // These single-item controls already have typed values on base; sibling
    // none/empty-literal inference remains the separate composite-value work.
    // Base option/some/Set/relation: V/P/X; Seq: V/C/X.
    // All five Monitor replays: (1, nonconformant).
    let mut errors = Vec::new();
    for (name, state, init, update, predicate) in [
        (
            "option-single",
            "o: Option<Int>",
            "o = none",
            "o = some(2)",
            "exists z in Seq { o } { z is some(v) and s.add(v).contains(2) }",
        ),
        (
            "some-single",
            "n: Int",
            "n = 0",
            "n = 2",
            "exists z in Seq { some(n) } { z is some(v) and s.add(v).contains(2) }",
        ),
        (
            "seq-single",
            "ys: Seq<K, 3>",
            "ys = Seq { 0 }",
            "ys = ys.push(1)",
            "exists z in Seq { ys } { s.add(z.size()).contains(2) }",
        ),
        (
            "set-single",
            "n: Int",
            "n = 0",
            "n = 2",
            "exists z in Seq { s } { z.add(n).contains(2) }",
        ),
        (
            "relation-single",
            "n: Int",
            "n = 0",
            "n = 2",
            "exists z in Seq { r } { z.add(n, 0).contains(2, 0) }",
        ),
    ] {
        let source = format!(
            "spec Binder {{
  type K = 0..1
  state {{ s: Set<K>, r: relation K -> K, {state}, done: Bool }}
  init {{ s = Set {{}} r = Set {{}} {init} done = false }}
  action go() {{ requires not done {update} done = true }}
  invariant P {{ not done or not ({predicate}) }}
}}\n"
        );
        ReplayCase::new(name, &source, &["go"]).agreement(4, false, None, &mut errors);
    }
    finish(&errors);
}

#[test]
fn unknown_range_scalar_sources_retain_members() {
    // Scalar-producing methods/aggregates must not depend on the source path.
    // Base all nine: V/C/X; Monitor all nine: (1, nonconformant).
    let mut errors = Vec::new();
    for (name, expression, member) in [
        ("size-source", "xs.size()", 2),
        ("if-source", "if done then 2 else 0", 2),
        ("min-source", "min(n, 2)", 2),
        ("max-source", "max(n, 2)", 2),
        ("abs-source", "abs(0 - n)", 2),
        ("count-source", "count(q: Q where done)", 3),
        ("sum-source", "sum(x in xs of x)", 2),
        ("int-source", "n", 2),
        ("map-source", "m[0]", 2),
    ] {
        let source = format!(
            "spec ScalarSource {{
  type K = 0..1
  type Q = 0..2
  state {{ s: Set<K>, xs: Seq<K, 3>, n: Int, m: Map<K, Int>, done: Bool }}
  init {{ s = Set {{}} xs = Seq {{ 1 }} n = 0 m[0] = 0 m[1] = 0 done = false }}
  action go() {{ requires not done xs = xs.push(1) n = 2 m[0] = 2 done = true }}
  invariant P {{ not done or not s.add({expression}).contains({member}) }}
}}\n"
        );
        ReplayCase::new(name, &source, &["go"]).agreement(4, false, None, &mut errors);
    }
    finish(&errors);
}

// No repository verifier executes the exported JSON expression tree. Compare
// its normalized live candidate set with the fixed trace's actual {n}, then
// require all native verdict classes to agree with Monitor as well.
fn alias_exports(aggregate: &str, version: usize) {
    let mut errors = Vec::new();
    for collection in ["set", "domain", "range"] {
        for (control, member, conformant) in [
            ("alias", 2, false),
            ("direct", 2, false),
            ("in-domain", 1, false),
            ("export-holds", 2, true),
        ] {
            let (state, init, added) = if collection == "set" {
                ("s: Set<K>", "s = Set {}", "s.add(n)".to_owned())
            } else {
                (
                    "r: relation K -> K",
                    "r = Set {}",
                    format!("{collection}(r.add(n, n))"),
                )
            };
            let receiver = if control == "direct" {
                added.as_str()
            } else {
                "z"
            };
            let sum = format!("sum(x in {receiver} of x)");
            // FSL min/max are binary arithmetic, not collection aggregates.
            let (expression, value) = match aggregate {
                "count" => (format!("count(x in {receiver} where x == {member})"), 1),
                "sum" => (sum, member),
                "min" => (format!("min({sum}, 3)"), member),
                "max" => (format!("max({sum}, 0)"), member),
                _ => unreachable!(),
            };
            let property = if conformant {
                "done or not done"
            } else {
                "not done"
            };
            let source = format!(
                "spec AliasExport {{
  type K = 0..1
  state {{ {state}, n: Int, done: Bool }}
  init {{ {init} n = {member} done = false }}
  action go() {{
    requires not done
    requires n == {member}
    let z = {added}
    requires {expression} == {value}
    done = true
  }}
  invariant P {{ {property} }}
}}\n"
            );
            let name = format!("kernel-v{version}-{collection}-{aggregate}-{control}");
            let case = ReplayCase::new(&name, &source, &["go"]);
            case.agreement(4, conformant, None, &mut errors);
            case.kernel_candidates(version, member, &mut errors);
        }
    }
    finish(&errors);
}

#[test]
fn kernel_v1_count_alias_retains_outside_candidates() {
    alias_exports("count", 1);
}

#[test]
fn kernel_v2_count_alias_retains_outside_candidates() {
    alias_exports("count", 2);
}

#[test]
fn kernel_v1_sum_alias_retains_outside_candidates() {
    alias_exports("sum", 1);
}

#[test]
fn kernel_v2_sum_alias_retains_outside_candidates() {
    alias_exports("sum", 2);
}

#[test]
fn kernel_v1_min_alias_retains_outside_candidates() {
    alias_exports("min", 1);
}

#[test]
fn kernel_v2_min_alias_retains_outside_candidates() {
    alias_exports("min", 2);
}

#[test]
fn kernel_v1_max_alias_retains_outside_candidates() {
    alias_exports("max", 1);
}

#[test]
fn kernel_v2_max_alias_retains_outside_candidates() {
    alias_exports("max", 2);
}

// Select the bound to bypass concrete boundary discovery: P cannot mask a
// symbolic bounds defect, and the symbolic evidence must retain the bad value.
#[test]
fn stored_outside_members_violate_bounds_and_survive_projection() {
    let mut errors = Vec::new();
    for (name, state, init, effect, bound, variable, expected) in [
        (
            "set-bound-evidence",
            "s: Set<K>",
            "s = Set {}",
            "s = s.add(n)",
            "_bounds_s",
            "s",
            serde_json::json!([2]),
        ),
        (
            "relation-bound-evidence",
            "r: relation K -> K",
            "r = Set {}",
            "r = r.add(n, n)",
            "_bounds_r",
            "r",
            serde_json::json!([[2, 2]]),
        ),
    ] {
        let source = format!(
            "spec BoundEvidence {{
 type K = 0..1
 state {{ {state}, n: Int }}
 init {{ {init} n = 2 }}
 action go() {{ {effect} }}
 invariant P {{ true }}
}}\n"
        );
        ReplayCase::new(name, &source, &["go"])
            .with_verify_options(&["--property", bound])
            .with_expected_trace_state(variable, expected)
            .agreement(1, false, Some(bound), &mut errors);
    }
    finish(&errors);
}

const OUTSIDE_INITIAL_MEMBER: &str = "spec Initial {
 type K = 0..1
 state { s: Set<K>, n: Int }
 init { n = 2 s = Set { n } }
 action go() {}
 invariant P { true }
}\n";

#[test]
fn computed_initial_members_violate_bounds_without_disappearing() {
    let mut errors = Vec::new();
    ReplayCase::new("computed-initial-member", OUTSIDE_INITIAL_MEMBER, &["go"])
        .with_verify_options(&["--property", "_bounds_s"])
        .with_expected_trace_state("s", serde_json::json!([2]))
        .agreement(1, false, Some("_bounds_s"), &mut errors);
    finish(&errors);
}

#[test]
fn supplied_snapshot_preserves_the_outside_member() {
    let mut errors = Vec::new();
    let case = ReplayCase::new("outside-snapshot", OUTSIDE_INITIAL_MEMBER, &["go"])
        .with_verify_options(&["--property", "_bounds_s"])
        .with_expected_trace_state("s", serde_json::json!([2]));
    // CLI induction/explicit replay the equivalent init. The public snapshot
    // API and Monitor::from_state then compare the identical logical state.
    case.agreement(1, false, Some("_bounds_s"), &mut errors);
    case.snapshot_bounds(OUTSIDE_INITIAL_MEMBER, 2, &mut errors);
    finish(&errors);
}

#[test]
fn relation_graph_closure_includes_outside_vertices() {
    let mut errors = Vec::new();
    for (name, condition) in [
        ("outside-cycle", "acyclic(r.add(n, n))"),
        (
            "outside-reachable",
            "not reachable(r.add(n, n + 1).add(n + 1, 0), n, 0)",
        ),
    ] {
        let source = format!(
            "spec Graph {{
 type K = 0..1
 state {{ r: relation K -> K, n: Int, done: Bool }}
 init {{ r = Set {{}} n = 2 done = false }}
 action go() {{ requires not done done = true }}
 invariant P {{ not done or {condition} }}
}}\n"
        );
        ReplayCase::new(name, &source, &["go"]).agreement(1, false, None, &mut errors);
    }
    finish(&errors);
}

fn shadowed_alias_source(shadow: &str, target: i64) -> String {
    format!("spec Shadow {{
 type K = 0..1
 state {{ s: Set<K>, n: Int, done: Bool }}
 init {{ s = Set {{}} n = 2 done = false }}
 action go() {{ requires not done requires n == 2 let z = s.add(n) {shadow} requires count(x in z where x == {target}) == 1 done = true }}
 invariant P {{ not done }}
}}\n")
}

#[test]
fn kernel_alias_candidates_capture_values_before_scalar_shadowing() {
    let source = shadowed_alias_source("let n = 0", 2);
    let mut errors = Vec::new();
    for version in [1, 2] {
        let case = ReplayCase::new(
            &format!("kernel-v{version}-scalar-shadow"),
            &source,
            &["go"],
        );
        case.agreement(1, false, None, &mut errors);
        case.kernel_captured_candidates(version, &[2], &mut errors);
    }
    finish(&errors);
}

#[test]
fn kernel_collection_shadow_uses_the_previous_candidate_scope() {
    let source = shadowed_alias_source("let z = z.add(sum(x in z of x) + 1)", 3);
    let mut errors = Vec::new();
    for version in [1, 2] {
        let case = ReplayCase::new(
            &format!("kernel-v{version}-collection-shadow"),
            &source,
            &["go"],
        );
        case.agreement(1, false, None, &mut errors);
        case.kernel_captured_candidates(version, &[2, 3], &mut errors);
    }
    finish(&errors);
}

fn verify_deep_collection_with_deadline(name: &str, source: &str) {
    let case = ReplayCase::new(name, source, &[]);
    let mut errors = Vec::new();
    // Healthy base runs are sub-second; 30s leaves >100x room for scheduling noise.
    let limit = std::time::Duration::from_secs(30);
    for engine in ["bmc", "induction"] {
        case.verify_with_deadline(engine, 12, limit, &mut errors);
    }
    finish(&errors);
}

#[test]
fn deep_set_verification_stays_within_deadline() {
    verify_deep_collection_with_deadline(
        "deep-set",
        "spec PerfSet {
  type K = 0..4
  state { s: Set<K>, t: Set<K> }
  init { s = Set {} t = Set {} }
  action put(x: K) { s = s.add(x) }
  action take(x: K) { requires s.contains(x) s = s.remove(x) t = t.add(x) }
  invariant P { s.size() + t.size() <= 10 }
  invariant Q { forall x in s: x <= 4 }
}\n",
    );
}

#[test]
fn deep_relation_verification_stays_within_deadline() {
    verify_deep_collection_with_deadline(
        "deep-relation",
        include_str!("fixtures/issue_467_relation_demo.fsl"),
    );
}

#[test]
fn implicit_bound_cti_retains_scalar_suggestions() {
    let source = "spec C03 {
  type C = 0..3
  state { c: C, n: Int }
  init { c = 0 n = 0 }
  action inc() { requires c < 3 or n > 0 c = c + 1 n = n - 1 }
  action reset() { requires c == 3 c = 0 }
}\n";
    let case = ReplayCase::new("implicit-bound-suggestion", source, &[]);
    let (report, exit) = case.observe("induction", 3);
    assert_eq!(exit, 1, "{report}");
    assert_eq!(report["result"], "unknown_cti", "{report}");
    assert_eq!(report["invariant"], "_bounds_c", "{report}");
    assert_eq!(report["completeness"], "bounded", "{report}");
    assert_eq!(
        report["suggested_invariants"],
        serde_json::json!(["n <= 0"]),
        "{report}"
    );
    assert_eq!(
        report["hint"],
        "this state sequence satisfies all invariants but leads to a violation; the start state may be unreachable — add an auxiliary invariant that excludes it, then re-run 'n' only decreasing in this CTI but starts above its initial value 0; adding invariant n <= 0 may exclude this unreachable start state",
        "{report}"
    );
}

#[test]
fn initial_collection_copies_agree_with_concrete_execution() {
    let mut errors = Vec::new();
    let fields = "s: Set<K>, t: Set<K>, u: Set<K>, x: Int, done: Bool";
    for (name, fields, init, condition) in [
        (
            "initial-copy",
            "s: Set<K>, t: Set<K>, x: Int, done: Bool",
            "x = 2 done = false t = Set { x } s = t",
            "not s.contains(2)",
        ),
        (
            "initial-copy-chain",
            fields,
            "x = 2 done = false t = Set { x } u = t s = u",
            "not s.contains(2)",
        ),
        (
            "initial-copy-then",
            fields,
            "x = 2 done = false t = Set { x } u = Set { x + 1 } s = if x == 2 then t else u",
            "not s.contains(2)",
        ),
        (
            "initial-copy-else",
            fields,
            "x = 3 done = false t = Set { x } u = Set { x + 1 } s = if x == 2 then t else u",
            "not s.contains(4)",
        ),
        (
            "initial-copy-mixed",
            fields,
            "x = 2 done = false t = Set { 0, x, 1, x + 1 } u = t s = u",
            "not s.contains(3)",
        ),
        (
            "initial-relation-copy",
            "empty: relation K -> K, t: relation K -> K, s: relation K -> K, x: Int, done: Bool",
            "x = 2 done = false empty = Set {} t = empty.add(x, 0) s = t",
            "not s.contains(2, 0)",
        ),
    ] {
        let source = format!(
            "spec InitCopy {{
  type K = 0..1
  state {{ {fields} }}
  init {{ {init} }}
  action go() {{ requires not done done = true }}
  invariant P {{ {condition} }}
}}\n"
        );
        // The selected invariant must fail, not merely an unselected bound.
        ReplayCase::new(name, &source, &["go"])
            .with_verify_options(&["--property", "P"])
            .with_expected_property("P")
            .agreement(3, false, None, &mut errors);
    }
    finish(&errors);
}

#[test]
fn free_scalar_init_preserves_symbolic_bound_violations() {
    let source = "spec FreeInitialScalar {
  type K = 0..1
  type V = 0..3
  state { s: Set<K>, t: Set<K>, x: V, done: Bool }
  init { done = false t = Set { x + 0 } s = t }
  action go() { requires not done done = true }
  invariant P { not s.contains(2) }
}\n";
    let case = ReplayCase::new("free-initial-scalar", source, &["go"]);
    let mut errors = Vec::new();
    check_fixture(&case, 3, &mut errors);
    for engine in ["bmc", "induction"] {
        let (report, exit) = bound_violation_at(&case, engine, 3, "_bounds_s", 0, &mut errors);
        let state = &report["trace"][0]["state"];
        let s = state["s"].as_array();
        let t = state["t"].as_array();
        // Pin the admitted valuation, not a particular scalar chosen by Z3.
        let same_members = s
            .zip(t)
            .is_some_and(|(s, t)| s.len() == t.len() && s.iter().all(|member| t.contains(member)));
        require(
            &mut errors,
            same_members,
            &case.name,
            &format!("{engine} initial s equals t"),
            &report,
            exit,
        );
        require(
            &mut errors,
            s.is_some_and(|values| {
                values.iter().any(|member| {
                    member
                        .as_i64()
                        .is_some_and(|value| !(0..=1).contains(&value))
                })
            }),
            &case.name,
            &format!("{engine} initial outside member"),
            &report,
            exit,
        );
        require(
            &mut errors,
            state["x"]
                .as_i64()
                .is_some_and(|value| (0..=3).contains(&value)),
            &case.name,
            &format!("{engine} initial x within V"),
            &report,
            exit,
        );
    }
    // Concrete init needs an assigned x; this is a preserved policy exception,
    // not permission for the symbolic engines to discard its free valuations.
    concrete_initial_error(
        &case,
        "init references state variable 'x' before it is assigned",
        &mut errors,
    );
    finish(&errors);
}

#[test]
fn reversed_initial_copy_preserves_read_before_assignment_rejection() {
    let source = "spec ReversedInitialCopy {
  type K = 0..1
  state { s: Set<K>, t: Set<K>, x: Int, done: Bool }
  init { x = 2 done = false s = t t = Set { x } }
  action go() { requires not done done = true }
  invariant P { not s.contains(2) }
}\n";
    let case = ReplayCase::new("reversed-initial-copy", source, &[]);
    let (report, exit) = case.observe("check", 3);
    assert_eq!(exit, 2, "{report}");
    assert_eq!(report["result"], "error", "{report}");
    assert_eq!(
        report["message"], "init references state variable 't' before it is assigned",
        "{report}"
    );
}

fn verify_unchecked_collection_support(name: &str, source: &str) {
    let case = ReplayCase::new(name, source, &[]).with_verify_options(&["--property", "P"]);
    let mut errors = Vec::new();
    // Depth20 is the highest profiled sub-second base depth; 30s is >80x slower.
    for (engine, result, completeness) in [
        ("bmc", "verified", "bounded"),
        ("induction", "proved", "unbounded"),
    ] {
        case.verify_with_expected_deadline(
            engine,
            20,
            std::time::Duration::from_secs(30),
            Some((result, completeness)),
            &mut errors,
        );
    }
    finish(&errors);
}

#[test]
fn unchecked_single_collection_support_stays_within_deadline() {
    verify_unchecked_collection_support(
        "unchecked-single-support",
        "spec PerfMin {
  type K = 0..4
  state { s: Set<K>, x: K }
  init { s = Set {} x = 0 }
  action setx(v: K) { x = v }
  action put() { s = s.add(x) }
  invariant P { s.size() <= 5 }
}\n",
    );
}

#[test]
fn unchecked_collection_variables_stay_within_deadline() {
    verify_unchecked_collection_support(
        "unchecked-variable-support",
        "spec PerfVar {
  type K = 0..4
  state { s: Set<K>, t: Set<K>, x: K, y: K }
  init { s = Set {} t = Set {} x = 0 y = 0 }
  action setx(v: K) { x = v }
  action sety(v: K) { y = v }
  action put() { s = s.add(x) }
  action move() { requires s.contains(y) s = s.remove(y) t = t.add(y) }
  action both() { s = s.add(y).add(x) }
  invariant P { s.size() + t.size() <= 10 }
  invariant Q { s.size() <= 5 }
}\n",
    );
}

#[test]
fn ranked_progress_preserves_unchecked_collection_lassos() {
    let mut errors = Vec::new();
    for (root, collection) in [("s", "Set<K>"), ("r", "relation K -> K")] {
        for ranked in [true, false] {
            for (variant, upper, two_members) in [
                ("outside", 1, false),
                ("two-outside", 1, true),
                ("in-domain", 2, false),
            ] {
                let relation = root == "r";
                let arguments = if relation { "x, x" } else { "x" };
                let mut added = format!("{root}.add({arguments})");
                let mut members = vec![if relation {
                    serde_json::json!([2, 2])
                } else {
                    serde_json::json!(2)
                }];
                if two_members {
                    added.push_str(if relation {
                        ".add(x + 1, x + 1)"
                    } else {
                        ".add(x + 1)"
                    });
                    members.push(if relation {
                        serde_json::json!([3, 3])
                    } else {
                        serde_json::json!(3)
                    });
                }
                let contains = if relation { "2, 2" } else { "2" };
                let ranking = if ranked {
                    "helpful step() decreases level"
                } else {
                    ""
                };
                let source = format!("spec SelectedProgress {{
  type K = 0..{upper}
  type Level = 0..2
  state {{ {root}: {collection}, level: Level, x: Int, done: Bool }}
  init {{ {root} = Set {{}} level = 2 x = 2 done = false }}
  fair action step() {{ requires level > 0 requires not {root}.contains({contains}) level = level - 1 }}
  action poison() {{ requires not done {root} = {added} done = true }}
  action idle() {{ requires done }}
  leadsTo Drains {{ level > 0 ~> level == 0 {ranking} }}
}}\n");
                let case = ReplayCase::new(
                    &format!("selected-progress-{root}-{ranked}-{variant}"),
                    &source,
                    &["poison", "idle", "idle"],
                )
                .with_verify_options(&["--property", "Drains"]);
                require_selected_lasso(&case, root, &members, &mut errors);
            }
        }
    }
    finish(&errors);
}

fn require_selected_lasso(
    case: &ReplayCase,
    root: &str,
    members: &[serde_json::Value],
    errors: &mut Vec<String>,
) {
    let (report, exit) = case.observe("check", 4);
    require(
        errors,
        exit == 0 && report["result"] == "ok",
        &case.name,
        "check",
        &report,
        exit,
    );
    // Replay is scoped to selected bounds, not ordinary all-bounds Monitor.
    let (report, exit) = case.observe("bmc", 4);
    require(
        errors,
        exit == 1
            && report["result"] == "violated"
            && report["violation_kind"] == "leadsTo"
            && report["invariant"] == "Drains"
            && report["completeness"] == "bounded",
        &case.name,
        "bmc lasso",
        &report,
        exit,
    );
    let closed = report["trace"]
        .as_array()
        .zip(report["loop_start"].as_u64())
        .is_some_and(|(trace, start)| {
            let Ok(start) = usize::try_from(start) else {
                return false;
            };
            let Some(loop_state) = trace.get(start).map(|step| &step["state"]) else {
                return false;
            };
            trace
                .last()
                .is_some_and(|last| last["state"] == *loop_state)
                && loop_state["level"].as_i64().is_some_and(|level| level > 0)
                && loop_state[root]
                    .as_array()
                    .is_some_and(|values| members.iter().all(|member| values.contains(member)))
        });
    require(
        errors,
        closed,
        &case.name,
        "retained closed lasso",
        &report,
        exit,
    );
}

fn check_fixture(case: &ReplayCase, depth: usize, errors: &mut Vec<String>) {
    let (report, exit) = case.observe("check", depth);
    require(
        errors,
        exit == 0 && report["result"] == "ok",
        &case.name,
        "check",
        &report,
        exit,
    );
}

fn bound_violation_at(
    case: &ReplayCase,
    engine: &str,
    depth: usize,
    bound: &str,
    step: usize,
    errors: &mut Vec<String>,
) -> (serde_json::Value, i32) {
    let (report, exit) = case.observe(engine, depth);
    require(
        errors,
        exit == 1
            && report["result"] == "violated"
            && report["violation_kind"] == "type_bound"
            && report["invariant"] == bound
            && report["completeness"] == "bounded"
            && report["violated_at_step"] == step
            && report["trace"][0]["step"] == 0,
        &case.name,
        engine,
        &report,
        exit,
    );
    (report, exit)
}

fn concrete_initial_error(case: &ReplayCase, message: &str, errors: &mut Vec<String>) {
    for route in ["explicit", "monitor"] {
        let (report, exit) = case.observe(route, 3);
        require(
            errors,
            exit == 2 && report["result"] == "error" && report["message"] == message,
            &case.name,
            route,
            &report,
            exit,
        );
    }
}

#[test]
fn independent_free_scalar_init_reports_its_own_bound() {
    let source = "spec IndependentInitialScalar {
  type K = 0..1
  type V = 0..3
  state { s: Set<K>, t: Set<K>, x: V, done: Bool }
  init { done = false t = Set {} s = t }
  action go() { requires not done done = true }
  invariant P { true }
}\n";
    let case = ReplayCase::new("independent-initial-scalar", source, &["go"]);
    let mut errors = Vec::new();
    check_fixture(&case, 3, &mut errors);
    for engine in ["bmc", "induction"] {
        let (report, exit) = bound_violation_at(&case, engine, 3, "_bounds_x", 0, &mut errors);
        let state = &report["trace"][0]["state"];
        require(
            &mut errors,
            state["s"] == serde_json::json!([])
                && state["t"] == serde_json::json!([])
                && state["x"]
                    .as_i64()
                    .is_some_and(|value| !(0..=3).contains(&value)),
            &case.name,
            &format!("{engine} only scalar outside V"),
            &report,
            exit,
        );
    }
    // Without an assignment to x there is no concrete initial value to replay.
    concrete_initial_error(
        &case,
        "init does not assign state variable(s): x",
        &mut errors,
    );
    finish(&errors);
}

#[test]
fn growing_unchecked_support_never_reports_an_unbounded_proof() {
    let source = "spec GrowingSupport {
  type K = 0..4
  state { x: Int, s: Set<K> }
  init { x = 0 s = Set {} }
  action grow() { x = x + 1 s = s.add(x) }
  invariant P { s.size() <= x + 1 }
}\n";
    let mut errors = Vec::new();
    for depth in [4, 8] {
        let case = ReplayCase::new(&format!("growing-selected-depth-{depth}"), source, &[])
            .with_verify_options(&["--property", "P"]);
        check_fixture(&case, depth, &mut errors);
        let (report, exit) = case.observe("induction", depth);
        require(
            &mut errors,
            exit == 1
                && matches!(report["result"].as_str(), Some("unknown_cti" | "violated"))
                && report["completeness"] == "bounded",
            &case.name,
            "selected induction without false proof",
            &report,
            exit,
        );
    }
    let case = ReplayCase::new("growing-all-bounds", source, &["grow"; 6]);
    check_fixture(&case, 6, &mut errors);
    let (report, exit) = case.observe("monitor", 6);
    require(
        &mut errors,
        exit == 1
            && report["result"] == "nonconformant"
            && report["violation"]["kind"] == "type_bound"
            && report["violation"]["name"] == "_bounds_s"
            && report["failed_at_event"] == 5,
        &case.name,
        "all-bounds Monitor",
        &report,
        exit,
    );
    for (engine, depth) in [("bmc", 6), ("explicit", 8)] {
        let _ = bound_violation_at(&case, engine, depth, "_bounds_s", 6, &mut errors);
    }
    finish(&errors);
}

#[test]
fn scalar_ranked_progress_keeps_selected_scope_discharge_fast() {
    let source = "spec ScalarRankedProgress {
  type Level = 0..6
  type N = 0..6
  state { level: Level, a: N, b: N, c: N }
  init { level = 6 a = 0 b = 0 c = 0 }
  fair action step() { requires level > 0 level = level - 1 }
  action ia(v: N) { a = v }
  action ib(v: N) { b = v }
  action ic(v: N) { c = v }
  action refill() { requires level == 0 level = 6 }
  leadsTo Drains {
    level > 0 ~> level == 0
    helpful step()
    decreases level
  }
}\n";
    let case = ReplayCase::new("scalar-ranked-selected-deadline", source, &[])
        .with_verify_options(&["--property", "Drains"]);
    let mut errors = Vec::new();
    // 30s is >50x the measured healthy depth32 run, leaving ample CI margin.
    case.verify_with_expected_deadline(
        "bmc",
        32,
        std::time::Duration::from_secs(30),
        Some(("verified", "bounded")),
        &mut errors,
    );
    finish(&errors);
}

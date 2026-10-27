// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Review5: legacy concrete outcomes, deterministic refine/auto consumers,
//! field-read diagnostic priority, and reads of never-assigned free roots.
//! Expectations follow the inline specs, not an engine-to-engine oracle.

use super::*;

const OVER_DOMAIN: &str = r"spec RImpl {
  type K = 0..1
  struct One { v: Bool }
  state { m: Map<K, One> }
  init { forall k in 0..2 { m[k].v = true } }
  action step() { }
  invariant P { m[0].v and m[1].v }
}
";
const REPEATED_FIXED_FIELD: &str = r"spec RImpl {
  type K = 0..0
  struct One { v: Bool }
  state { m: Map<K, One> }
  init { forall j in 0..1 { m[0].v = (j == 1) } }
  action step() { }
  invariant P { m[0].v }
}
";
const MIXED_FULL: &str = r"spec RImpl {
  type K = 0..0
  struct One { v: Bool }
  state { m: Map<K, One>, flag: Bool }
  init { flag = true if flag { m[0].v = true } else { m[0] = One { v: false } } }
  action step() { }
  invariant P { m[0].v }
}
";
const MIXED_PARTIAL: &str = r"spec RImpl {
  type K = 0..1
  struct One { v: Bool }
  state { m: Map<K, One>, flag: Bool }
  init { flag = true if flag { m[0].v = true } else { m[0] = One { v: false } } }
  action step() { }
}
";
const ONE_FIELD_FORALL: &str = r"spec RImpl {
  type K = 0..1
  struct One { v: Bool }
  state { m: Map<K, One>, t: Bool }
  init { forall k: K { m[k].v = true } t = m[0].v }
  action step() { }
  invariant P { t and m[1].v }
}
";
const FIXED_FIELD_ONCE: &str = r"spec RImpl {
  type K = 0..0
  struct One { v: Bool }
  state { m: Map<K, One> }
  init { forall j in 0..0 { m[0].v = true } }
  action step() { }
  invariant P { m[0].v }
}
";
const ORDER_BOTH_READS: &str = r"spec RImpl {
  type V = 0..1
  struct T { a: V, b: V }
  state { s: T, t: V, u: V, x: Bool }
  init {
    u = t
    s.a = s.b
    s.b = 0
    t = 1
  }
  action step() { }
}
";
const READ_FREE: &str = r"spec RImpl {
  struct Pair { a: Bool, b: Bool }
  state { s: Pair, free: Bool }
  init { s.a = free s.b = not free }
  action idle() { }
  invariant P { s.a != s.b }
}
";
const ABS_TRUE: &str = r"spec RAbs {
  state { y: Bool }
  init { y = true }
  action noop() { }
}
";
const ABS_FALSE: &str = r"spec RAbs {
  state { y: Bool }
  init { y = false }
  action noop() { }
}
";
const MAP_T: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = t
  action step() -> noop()
}
";
const MAP_M0V: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = m[0].v
  action step() -> noop()
}
";
const MAP_FREE: &str = r"refinement R {
  impl RImpl
  abs RAbs
  map y = s.a != s.b
  action idle() -> noop()
}
";

type ErrorExpectation = (&'static str, &'static str);

fn require_probe(
    source: &str,
    consumer: Consumer,
    symbolic_error: Option<ErrorExpectation>,
    concrete_error: Option<ErrorExpectation>,
    initial: &Value,
) {
    let error = match consumer {
        Consumer::Check => None,
        Consumer::Bmc | Consumer::Induction => symbolic_error,
        Consumer::Explicit | Consumer::Monitor => concrete_error,
    };
    let (actual, exit) = observe(source, consumer);
    if let Some((kind, message)) = error {
        assert_eq!(exit, 2, "{consumer:?}: {actual:#}");
        assert_eq!(
            without_versions(actual, consumer),
            json!({"fsl": "1.0", "result": "error", "kind": kind, "message": message}),
            "{consumer:?}: complete error envelope, no loc"
        );
        return;
    }
    assert_eq!(exit, 0, "{consumer:?}: {actual:#}");
    match consumer {
        Consumer::Check => {
            let warnings = if source == MIXED_PARTIAL {
                json!([{
                    "kind": "no_user_invariants",
                    "message": "spec declares no user invariants (only implicit type bounds are checked)"
                }])
            } else {
                json!([])
            };
            assert_eq!(
                without_versions(actual, consumer),
                json!({
                    "fsl": "1.0", "result": "ok", "spec": "RImpl", "warnings": warnings
                })
            );
        }
        Consumer::Monitor => assert_eq!(
            actual,
            json!({
                "fsl": "1.0", "result": "conformant", "spec": "RImpl",
                "steps_checked": 0, "final_state": initial,
                "note": "leadsTo properties are not checked by replay (finite logs only)"
            })
        ),
        Consumer::Bmc | Consumer::Induction | Consumer::Explicit => {
            assert_eq!(
                actual["result"],
                if matches!(consumer, Consumer::Bmc) {
                    "verified"
                } else {
                    "proved"
                }
            );
            assert_eq!(
                actual["invariants_checked"],
                if source == MIXED_PARTIAL {
                    json!([])
                } else {
                    json!(["P"])
                }
            );
            assert_eq!(
                actual["completeness"],
                if matches!(consumer, Consumer::Bmc) {
                    "bounded"
                } else {
                    "unbounded"
                }
            );
            for key in [
                "kind",
                "message",
                "loc",
                "invariant",
                "violation_kind",
                "action",
                "last_action",
                "violation",
            ] {
                assert!(actual.get(key).is_none(), "{consumer:?}: {actual:#}");
            }
            if matches!(consumer, Consumer::Explicit) {
                assert_eq!(actual["closure"], true, "{actual:#}");
            }
        }
    }
}

macro_rules! probe_cell {
    ($name:ident, $source:expr, $symbolic_error:expr, $concrete_error:expr, $initial:expr) => {
        mod $name {
            use super::*;
            fn require_case(consumer: Consumer) {
                require_probe(
                    $source,
                    consumer,
                    $symbolic_error,
                    $concrete_error,
                    &$initial,
                );
            }
            #[test]
            fn check() {
                require_case(Consumer::Check);
            }
            #[test]
            fn bmc() {
                require_case(Consumer::Bmc);
            }
            #[test]
            fn induction() {
                require_case(Consumer::Induction);
            }
            #[test]
            fn explicit() {
                require_case(Consumer::Explicit);
            }
            #[test]
            fn monitor() {
                require_case(Consumer::Monitor);
            }
        }
    };
}

// Superset coverage does not authorize an out-of-domain write. Preserve the
// symbolic diagnostic's location TEXT and concrete diagnostic's missing loc.
probe_cell!(
    l3_range_over_domain,
    OVER_DOMAIN,
    Some((
        "semantics",
        "map assignment index outside key domain in init at 5:29"
    )),
    Some(("semantics", "map assignment index outside key domain")),
    Value::Null
);
// Relational repetition requires both false and true in the same slot, not
// last-write-wins true. Every verification/construction consumer must refuse.
probe_cell!(
    l5_fixed_field_repeat,
    REPEATED_FIXED_FIELD,
    Some(("vacuous", "init constraints are unsatisfiable")),
    Some(("vacuous", "init constraints are unsatisfiable")),
    Value::Null
);
probe_cell!(
    l1_mixed_whole_field_branch,
    MIXED_FULL,
    None,
    None,
    json!({"m": {"0": {"v": true}}, "flag": true})
);
probe_cell!(
    l2_mixed_whole_field_branch_partial,
    MIXED_PARTIAL,
    None,
    Some((
        "semantics",
        "init does not assign state variable(s): m (partial component initialization is rejected by the explicit engine)"
    )),
    Value::Null
);
probe_cell!(
    e1_onefield_forall,
    ONE_FIELD_FORALL,
    None,
    None,
    json!({"m": {"0": {"v": true}, "1": {"v": true}}, "t": true})
);
probe_cell!(
    l6_fixed_field_once,
    FIXED_FIELD_ONCE,
    None,
    None,
    json!({"m": {"0": {"v": true}}})
);
// This read is intentionally of a NEVER-assigned root. A gate built from all
// state roots instead of may-assigned roots would incorrectly refuse it.
probe_cell!(
    p4_read_never_assigned_root,
    READ_FREE,
    None,
    Some((
        "semantics",
        "init references state variable 'free' before it is assigned"
    )),
    Value::Null
);

// Gate (a) runs before the nonfield pass, even when its source read is later.
read_rejection_cell!(
    g1_field_read_priority,
    ORDER_BOTH_READS,
    "init cannot read 's.b' before state variable 's' is fully assigned",
    7,
    5
);
#[test]
fn g1_refine_field_read_priority() {
    require_refine_error(
        ORDER_BOTH_READS,
        RB_ABS,
        RB_MAP_SA,
        "init cannot read 's.b' before state variable 's' is fully assigned",
    );
}

fn require_refine_pair(source: &str, mapping: &str, negative: bool) {
    let (actual, exit) =
        observe_refine(source, if negative { ABS_FALSE } else { ABS_TRUE }, mapping);
    let action_map = if source == READ_FREE {
        json!({"idle": "noop"})
    } else {
        json!({"step": "noop"})
    };
    if negative {
        // The free valuation is intentionally unconstrained. Split its Boolean
        // membership from the independent spec equations a=free, b=not free;
        // never infer either field's expected value from the observed fields.
        let initial = match source {
            ONE_FIELD_FORALL => json!({"m": {"0": {"v": true}, "1": {"v": true}}, "t": true}),
            MIXED_FULL => json!({"m": {"0": {"v": true}}, "flag": true}),
            FIXED_FIELD_ONCE => json!({"m": {"0": {"v": true}}}),
            READ_FREE => {
                let free = actual["impl_trace"][0]["state"]["free"]
                    .as_bool()
                    .expect("free Bool valuation");
                json!({"free": free, "s": {"a": free, "b": !free}})
            }
            _ => panic!("only declared refine controls"),
        };
        assert_eq!(exit, 1, "{actual:#}");
        assert_eq!(
            actual,
            json!({
                "fsl": "1.0", "impl": "RImpl", "abs": "RAbs", "result": "refinement_failed",
                "kind": "abs_state_mismatch", "at": "init", "violated_at_step": 0,
                "abs_after_expected": {"y": false}, "abs_after_actual": {"y": true}, "mismatch": ["y"],
                "impl_trace": [{"step": 0, "state": initial}],
                "hint": "the impl step does not correspond to the mapped abs action; fix the map expressions, the action correspondence, or guard the impl action",
                "trace_type": "refinement"
            })
        );
    } else {
        assert_eq!(exit, 0, "{actual:#}");
        assert_eq!(
            actual,
            json!({
                "fsl": "1.0", "impl": "RImpl", "abs": "RAbs", "result": "refines",
                "checked_to_depth": 4, "action_map": action_map
            })
        );
    }
}

macro_rules! refine_pair {
    ($name:ident, $source:expr, $mapping:expr) => {
        mod $name {
            use super::*;
            #[test]
            fn refines() {
                require_refine_pair($source, $mapping, false);
            }
            #[test]
            fn wrong_abs_rejects() {
                require_refine_pair($source, $mapping, true);
            }
        }
    };
}
// Unlike the free-root controls, these three sources have NO free roots. Their
// refine construction must exercise Monitor::new on the deterministic path.
refine_pair!(e1_deterministic_refine, ONE_FIELD_FORALL, MAP_T);
refine_pair!(l1_deterministic_refine, MIXED_FULL, MAP_M0V);
refine_pair!(l6_deterministic_refine, FIXED_FIELD_ONCE, MAP_M0V);
refine_pair!(p4_free_read_refine, READ_FREE, MAP_FREE);

fn observe_auto(source: &str) -> (Value, i32) {
    let fixture = Fixture::new(source);
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            "model.fsl",
            "--engine",
            "auto",
            "--depth",
            "4",
            "--no-cache",
        ])
        .current_dir(&fixture.0)
        .env_remove("FSLC_CACHE")
        .output()
        .expect("run working-tree auto engine");
    let actual: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("auto: invalid JSON: {error}; output={output:?}"));
    assert!(output.stderr.is_empty(), "auto: {output:?}");
    let exit = output.status.code().expect("auto exit code");
    println!("Auto exit={exit} output={actual}");
    (actual, exit)
}

fn require_auto(source: &str) {
    let (actual, exit) = observe_auto(source);
    if source == OVER_DOMAIN {
        assert_eq!(exit, 2, "{actual:#}");
        assert_eq!(
            without_versions(actual, Consumer::Explicit),
            json!({
                "fsl": "1.0", "result": "error", "kind": "semantics",
                "message": "map assignment index outside key domain"
            })
        );
    } else {
        assert_eq!(exit, 0, "{actual:#}");
        assert_eq!(actual["result"], "proved", "{actual:#}");
        assert_eq!(actual["engine"], "explicit", "{actual:#}");
        assert_eq!(actual["completeness"], "unbounded", "{actual:#}");
        assert_eq!(actual["closure"], true, "{actual:#}");
        assert_eq!(actual["invariants_checked"], json!(["P"]));
        for key in [
            "engine_fallback",
            "kind",
            "message",
            "loc",
            "invariant",
            "violation_kind",
            "violation",
            "last_action",
        ] {
            assert!(actual.get(key).is_none(), "auto: {actual:#}");
        }
    }
}
#[test]
fn e1_auto_uses_explicit_without_fallback() {
    require_auto(ONE_FIELD_FORALL);
}
#[test]
fn l1_auto_uses_explicit_without_fallback() {
    require_auto(MIXED_FULL);
}
#[test]
fn l6_auto_uses_explicit_without_fallback() {
    require_auto(FIXED_FIELD_ONCE);
}
#[test]
fn l3_auto_preserves_concrete_domain_diagnostic() {
    require_auto(OVER_DOMAIN);
}

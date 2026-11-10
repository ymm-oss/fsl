// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Legacy-probe-accepted init: concrete coverage deltas and preservation controls.
//! Each source is the corresponding review4 L-family probe, not a weakened spec.
//! Incomplete Bool components remain symbolic free values; a clean symbolic
//! verdict is not evidence that explicit/Monitor can construct their init.

use super::*;

const L2_CONST_KEY: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  const zero = 0
  type K = 0..1
  struct T { a: Bool, b: Bool }
  state { m: Map<K, T> }
  init { m[zero].a = true }
  action step() { }
}
";
const L2_ENUM_KEY: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  enum K { A, B }
  struct T { a: Bool, b: Bool }
  state { m: Map<K, T> }
  init { m[A].a = true }
  action step() { }
}
";
const L4_RANGE: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  type K = 0..1
  struct One { v: Bool }
  state { m: Map<K, One> }
  init { forall k in 0..1 { m[k].v = true } }
  action step() { }
  invariant P { m[0].v and m[1].v }
}
";
const L4_SINGLETON: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  type K = 0..0
  struct One { v: Bool }
  state { m: Map<K, One> }
  init { m[0].v = true }
  action step() { }
  invariant P { m[0].v }
}
";
const L4_OTHER_MISSING: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  type K = 0..0
  struct One { v: Bool }
  state { m: Map<K, One>, free: Bool }
  init { m[0].v = true }
  action step() { }
  invariant P { m[0].v }
}
";
const L6_SAME_FIELD: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  type K = 0..0
  struct One { v: Bool }
  state { m: Map<K, One>, flag: Bool }
  init { flag = true if flag { m[0].v = true } else { m[0].v = true } }
  action step() { }
  invariant P { m[0].v }
}
";
const L6_DIFFERENT_FIELDS: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  type K = 0..0
  struct T { a: Bool, b: Bool }
  state { m: Map<K, T>, flag: Bool }
  init { flag = true if flag { m[0].a = true } else { m[0].b = true } }
  action step() { }
}
";
const L6_MISSING_ARM: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  type K = 0..0
  struct One { v: Bool }
  state { m: Map<K, One>, flag: Bool }
  init { flag = true if flag { m[0].v = true } }
  action step() { }
}
";
const L0_ROOT_FIELD: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  struct One { v: Bool }
  state { s: One }
  init { s.v = true }
  action step() { }
  invariant P { s.v }
}
";
const L0_WHOLE_ENTRIES: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  type K = 0..1
  struct T { a: Bool, b: Bool }
  state { m: Map<K, T> }
  init { m[0] = T { a: true, b: false } m[1] = T { a: false, b: true } }
  action step() { }
  invariant P { m[0].a and m[1].b }
}
";
const L1_UNRESOLVED_KEY: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  type K = 0..1
  struct T { a: Bool, b: Bool }
  state { m: Map<K, T>, k: K }
  init { k = 0 m[k].a = true }
  action step() { }
}
";
const L7_EMPTY_FORALL: &str = r"// SPDX-License-Identifier: Apache-2.0
spec RImpl {
  type K = 0..1
  struct One { v: Bool }
  state { m: Map<K, One> }
  init { forall k in 1..0 { m[k].v = true } }
  action step() { }
}
";

fn require_legacy(
    source: &str,
    consumer: Consumer,
    invariants: &[&str],
    has_user_invariant: bool,
    concrete_error: Option<&str>,
    initial: &Value,
) {
    if matches!(consumer, Consumer::Explicit | Consumer::Monitor)
        && let Some(message) = concrete_error
    {
        // Literal message, exit 2, and complete error envelope, including the
        // absence of loc. Do not treat any generic error as this refusal.
        require(source, consumer, &semantics(message), None);
        return;
    }
    let (actual, exit) = observe(source, consumer);
    assert_eq!(exit, 0, "{consumer:?}: {actual:#}");
    match consumer {
        Consumer::Check => {
            let warnings = if has_user_invariant {
                json!([])
            } else {
                json!([{
                    "kind": "no_user_invariants",
                    "message": "spec declares no user invariants (only implicit type bounds are checked)"
                }])
            };
            assert_eq!(
                without_versions(actual, consumer),
                json!({"fsl": "1.0", "result": "ok", "spec": "RImpl", "warnings": warnings})
            );
        }
        Consumer::Monitor => assert_eq!(
            actual,
            json!({
                "fsl": "1.0", "result": "conformant", "spec": "RImpl",
                "steps_checked": 0, "final_state": initial,
                "note": "leadsTo properties are not checked by replay (finite logs only)"
            }),
            "Monitor: complete initial state, no exclusions"
        ),
        Consumer::Bmc | Consumer::Induction | Consumer::Explicit => {
            // Independent verdict/property assertions, not equality between
            // distinct engines' proof/cost reports. Preserve empty property
            // lists for no-invariant probes rather than inventing assurance.
            assert_eq!(
                actual["result"],
                if matches!(consumer, Consumer::Bmc) {
                    "verified"
                } else {
                    "proved"
                },
                "{consumer:?}: {actual:#}"
            );
            assert_eq!(actual["invariants_checked"], json!(invariants));
            assert_eq!(
                actual["completeness"],
                if matches!(consumer, Consumer::Bmc) {
                    "bounded"
                } else {
                    "unbounded"
                }
            );
            for field in [
                "kind",
                "message",
                "loc",
                "violation_kind",
                "invariant",
                "action",
                "last_action",
                "violation",
            ] {
                assert!(actual.get(field).is_none(), "{consumer:?}: {actual:#}");
            }
            if matches!(consumer, Consumer::Explicit) {
                assert_eq!(actual["closure"], true, "{actual:#}");
            }
        }
    }
}

macro_rules! legacy_cell {
    ($name:ident, $source:expr, $invariants:expr, $has_invariant:expr, $error:expr, $initial:expr) => {
        mod $name {
            use super::*;
            fn require_case(consumer: Consumer) {
                require_legacy(
                    $source,
                    consumer,
                    $invariants,
                    $has_invariant,
                    $error,
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

// Resolved keys now contribute partial coverage. Neither missing fields nor
// missing keys may be default-filled into a conformant concrete state.
legacy_cell!(
    l2_const_key,
    L2_CONST_KEY,
    &[],
    false,
    Some(
        "init does not assign state variable(s): m (partial component initialization is rejected by the explicit engine)"
    ),
    Value::Null
);
legacy_cell!(
    l2_enum_key,
    L2_ENUM_KEY,
    &[],
    false,
    Some(
        "init does not assign state variable(s): m (partial component initialization is rejected by the explicit engine)"
    ),
    Value::Null
);
// One field per entry is enough only when every key and every other root is
// covered. The original invariants and complete Monitor states pin true, not
// the default false. The free-root case is still unsupported concretely.
legacy_cell!(
    l4_range,
    L4_RANGE,
    &["P"],
    true,
    None,
    json!({"m": {"0": {"v": true}, "1": {"v": true}}})
);
legacy_cell!(
    l4_singleton,
    L4_SINGLETON,
    &["P"],
    true,
    None,
    json!({"m": {"0": {"v": true}}})
);
legacy_cell!(
    l4_other_missing,
    L4_OTHER_MISSING,
    &["P"],
    true,
    Some("init does not assign state variable(s): free"),
    Value::Null
);
// Branch meet: common full coverage constructs init; different fields retain
// partial coverage; a missing arm drops it entirely (hence NO partial suffix).
legacy_cell!(
    l6_same_field,
    L6_SAME_FIELD,
    &["P"],
    true,
    None,
    json!({"m": {"0": {"v": true}}, "flag": true})
);
legacy_cell!(
    l6_different_fields,
    L6_DIFFERENT_FIELDS,
    &[],
    false,
    Some(
        "init does not assign state variable(s): m (partial component initialization is rejected by the explicit engine)"
    ),
    Value::Null
);
legacy_cell!(
    l6_missing_arm,
    L6_MISSING_ARM,
    &[],
    false,
    Some("init does not assign state variable(s): m"),
    Value::Null
);
// Preservation controls: root fields/whole entries were already constructible;
// unresolved state keys and empty loops still contribute no coverage/suffix.
legacy_cell!(
    l0_root_field,
    L0_ROOT_FIELD,
    &["P"],
    true,
    None,
    json!({"s": {"v": true}})
);
legacy_cell!(
    l0_whole_entries,
    L0_WHOLE_ENTRIES,
    &["P"],
    true,
    None,
    json!({"m": {"0": {"a": true, "b": false}, "1": {"a": false, "b": true}}})
);
legacy_cell!(
    l1_unresolved_key,
    L1_UNRESOLVED_KEY,
    &["_bounds_k"],
    false,
    Some("init does not assign state variable(s): m"),
    Value::Null
);
legacy_cell!(
    l7_empty_forall,
    L7_EMPTY_FORALL,
    &[],
    false,
    Some("init does not assign state variable(s): m"),
    Value::Null
);

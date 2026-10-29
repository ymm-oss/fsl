// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1591: an init type-bound failure precedes every action, even at depth zero.
//! Expectations follow the source values and declared bounds, not base output.

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

const INITOVER: &str = r"spec ReviewInitOver {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { 0, 1 } }
  action idle() {}
  invariant P { true }
}
";

const INITRANGE: &str = r"spec InitRange {
  state { n: 0..1 }
  init { n = 2 }
  action idle() {}
  invariant P { true }
}
";

const FREE_INIT: &str = r"spec FreeInit {
  state { n: 0..1, go: Bool }
  init { n = 2 }
  action idle() {}
  invariant P { true }
}
";

const FIRST_STEP: &str = r"spec FirstStep {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { 0 } }
  action take() { xs = xs.push(1) }
  invariant P { true }
}
";

const VALID_INIT: &str = r"spec ValidInit {
  type K = 0..1
  state { xs: Seq<K, 1> }
  init { xs = Seq { 0 } }
  action idle() {}
  invariant P { true }
}
";

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn verify(source: &str, engine: &str, depth: usize, selected: bool) -> (i32, Value) {
    verify_property(source, engine, depth, selected.then_some("P"))
}

fn verify_property(
    source: &str,
    engine: &str,
    depth: usize,
    property: Option<&str>,
) -> (i32, Value) {
    // Use the caller's TMPDIR (task-local during calibration). Leave artifacts
    // available for diagnosis rather than deleting a failed cell's evidence.
    let directory = std::env::temp_dir().join(format!(
        "fslc-1591-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).expect("create fixture directory");
    std::fs::write(directory.join("model.fsl"), source).expect("write fixture");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .current_dir(&directory)
        .env_remove("FSLC_CACHE")
        .args(["verify", "model.fsl", "--engine", engine, "--depth"])
        .arg(depth.to_string())
        .arg("--no-cache");
    if let Some(property) = property {
        command.args(["--property", property]);
    }
    let output = command.output().expect("run native CLI");
    let exit = output.status.code().expect("native exit code");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON {error}: exit={exit}, stdout={}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    eprintln!(
        "fixture={} engine={engine} depth={depth} property={property:?} exit={exit} {value}",
        directory.display()
    );
    (exit, value)
}

fn assert_bound(exit: i32, value: &Value, name: &str, step: usize) {
    // Keep exit/verdict/kind/name/step in one assertion so fail-first records
    // both the step-1 bug and depth-zero's fail-closed semantics error.
    assert_eq!(
        (
            exit,
            &value["result"],
            &value["violation_kind"],
            &value["invariant"],
            &value["violated_at_step"],
        ),
        (
            1,
            &json!("violated"),
            &json!("type_bound"),
            &json!(name),
            &json!(step),
        ),
        "{value}"
    );
}

fn init_bound(source: &str, engine: &str, depth: usize, name: &str, state: &Value) {
    let (exit, value) = verify(source, engine, depth, false);
    assert_bound(exit, &value, name, 0);
    assert_eq!(
        value["trace"],
        json!([{"step": 0, "state": state}]),
        "{value}"
    );
    assert_eq!(value["last_action"], Value::Null, "{value}");
}

macro_rules! depths {
    ($source:ident, $engine:literal, $name:literal, $state:expr) => {
        #[test]
        fn depth_0() {
            init_bound($source, $engine, 0, $name, &$state);
        }
        #[test]
        fn depth_1() {
            init_bound($source, $engine, 1, $name, &$state);
        }
        #[test]
        fn depth_2() {
            init_bound($source, $engine, 2, $name, &$state);
        }
    };
}

macro_rules! init_cells {
    ($module:ident, $source:ident, $name:literal, $state:expr) => {
        mod $module {
            use super::*;
            mod bmc {
                use super::*;
                depths!($source, "bmc", $name, $state);
            }
            mod induction {
                use super::*;
                depths!($source, "induction", $name, $state);
            }
            mod explicit {
                use super::*;
                depths!($source, "explicit", $name, $state);
            }
        }
    };
}

// Length 2 > capacity 1; integer 2 > upper bound 1. Neither needs an action.
init_cells!(initover, INITOVER, "_bounds_xs", json!({"xs": [0, 1]}));
init_cells!(initrange, INITRANGE, "_bounds_n", json!({"n": 2}));

fn success(exit: i32, value: &Value, engine: &str) {
    assert_eq!(exit, 0, "{value}");
    assert_eq!(
        value["result"],
        if engine == "bmc" {
            "verified"
        } else {
            "proved"
        },
        "{value}"
    );
    for key in ["violation_kind", "invariant", "violated_at_step", "trace"] {
        assert_eq!(value[key], Value::Null, "{key}: {value}");
    }
}

macro_rules! preservation {
    ($module:ident, $engine:literal) => {
        mod $module {
            use super::*;

            #[test]
            fn nondeterministic_init_bypasses_prepass() {
                for _ in 0..2 {
                    let (exit, value) = verify(FREE_INIT, $engine, 2, false);
                    if $engine == "explicit" {
                        assert_eq!(exit, 2, "{value}");
                        assert_eq!(value["result"], "error", "{value}");
                        assert_eq!(
                            value["message"],
                            "init does not assign state variable(s): go",
                            "{value}"
                        );
                    } else {
                        // Both choices of the free Bool leave n=2 at step 0.
                        assert_bound(exit, &value, "_bounds_n", 0);
                        let trace = value["trace"].as_array().expect("symbolic trace");
                        assert_eq!(trace.len(), 1, "{value}");
                        assert_eq!(trace[0]["step"], 0, "{value}");
                        assert_eq!(trace[0]["state"]["n"], 2, "{value}");
                        assert!(trace[0]["state"]["go"].is_boolean(), "{value}");
                    }
                }
            }

            #[test]
            fn selected_p_preserves_success() {
                for _ in 0..2 {
                    // P is true regardless of n; property selection excludes
                    // the synthetic bound and therefore the boundary pre-pass.
                    let (exit, value) = verify(INITRANGE, $engine, 2, true);
                    success(exit, &value, $engine);
                    // Induction's proved envelope also lists implicit bounds;
                    // the selected base case still bypasses the pre-pass.
                    assert_eq!(
                        value["invariants_checked"],
                        if $engine == "induction" {
                            json!(["_bounds_n", "P"])
                        } else {
                            json!(["P"])
                        },
                        "{value}"
                    );
                }
            }

            #[test]
            fn valid_init_first_step_overflow_stays_step_1() {
                for _ in 0..2 {
                    let (exit, value) = verify(FIRST_STEP, $engine, 2, false);
                    assert_bound(exit, &value, "_bounds_xs", 1);
                    let trace = value["trace"].as_array().expect("action trace");
                    assert_eq!(trace.len(), 2, "{value}");
                    assert_eq!(trace[0], json!({"step": 0, "state": {"xs": [0]}}));
                    assert_eq!(trace[1]["step"], 1, "{value}");
                    assert_eq!(trace[1]["state"], json!({"xs": [0, 1]}), "{value}");
                    assert_eq!(trace[1]["action"]["name"], "take", "{value}");
                    assert_eq!(trace[1]["action"]["params"], json!({}), "{value}");
                    assert_eq!(value["last_action"]["name"], "take", "{value}");
                }
            }

            #[test]
            fn valid_init_and_idle_preserve_success() {
                for _ in 0..2 {
                    let (exit, value) = verify(VALID_INIT, $engine, 2, false);
                    success(exit, &value, $engine);
                    assert_eq!(
                        value["invariants_checked"],
                        json!(["_bounds_xs", "P"]),
                        "{value}"
                    );
                }
            }
        }
    };
}

preservation!(control_bmc, "bmc");
preservation!(control_induction, "induction");
preservation!(control_explicit, "explicit");

const INVARIANT_OVERFLOW_NEXT_BOUND: &str = r"spec ReviewInvariantOverflowNextBound {
  state { n: 0..9223372036854775807 }
  init { n = 9223372036854775807 }
  action cross() { n = -1 }
  invariant P { n + 1 > n }
}
";

const INVARIANT_OVERFLOW: &str = r"spec ReviewInvariantOverflow {
  state { n: 0..9223372036854775807 }
  init { n = 9223372036854775807 }
  action reset() { n = 0 }
  invariant P { n + 1 > n }
}
";

const SELECTED_TRANS: &str = r"spec ReviewSelectedTrans {
  state { n: 0..1 }
  init { n = 2 }
  action idle() {}
  invariant P { true }
  trans T { true }
}
";

fn invariant_overflow_before_next_bound(engine: &str) {
    for _ in 0..2 {
        // Init satisfies the bound; cross is the first bound violation. The
        // bounds-only initial probe must not evaluate P's overflowing addition.
        let (exit, value) = verify(INVARIANT_OVERFLOW_NEXT_BOUND, engine, 1, false);
        assert_bound(exit, &value, "_bounds_n", 1);
        let action = json!({"name": "cross", "params": {}, "loc": {"line": 4, "column": 3}});
        assert_eq!(
            value["trace"],
            json!([
                {"step": 0, "state": {"n": i64::MAX}},
                {
                    "step": 1,
                    "state": {"n": -1},
                    "action": action,
                    "changes": {"n": {"from": i64::MAX, "to": -1}},
                    "blame": {"guards": [], "effects": []}
                }
            ]),
            "{value}"
        );
        assert_eq!(value["last_action"], action, "{value}");
        assert_eq!(value["checked_to_depth"], 1, "{value}");
    }
}

fn invariant_overflow_preserves_symbolic_diagnostic(engine: &str, depth: usize) {
    for _ in 0..2 {
        // Both initial and reset states satisfy the bound. P is undefined at
        // the reachable root, so preserve the symbolic invariant diagnostic
        // rather than leaking a concrete-evaluator error from the new probe.
        let (exit, value) = verify(INVARIANT_OVERFLOW, engine, depth, false);
        assert_eq!(
            (exit, &value["result"], &value["kind"], &value["message"]),
            (
                2,
                &json!("error"),
                &json!("semantics"),
                &json!("invariant 'P' is undefined for a reachable state"),
            ),
            "{value}"
        );
        assert_eq!(value["trace"], Value::Null, "{value}");
    }
}

macro_rules! invariant_error_cells {
    ($module:ident, $engine:literal) => {
        mod $module {
            use super::*;

            #[test]
            fn next_bound_depth_1() {
                invariant_overflow_before_next_bound($engine);
            }

            #[test]
            fn invariant_overflow_depth_0() {
                invariant_overflow_preserves_symbolic_diagnostic($engine, 0);
            }

            #[test]
            fn invariant_overflow_depth_1() {
                invariant_overflow_preserves_symbolic_diagnostic($engine, 1);
            }
        }
    };
}

invariant_error_cells!(non_bound_error_bmc, "bmc");
invariant_error_cells!(non_bound_error_induction, "induction");

const NO_ACTIONS: &str = r"spec NoActions {
  state { n: 0..1 }
  init { n = 2 }
  invariant P { true }
}
";

fn no_actions_preserves_semantics_error(engine: &str) {
    for _ in 0..2 {
        // Without actions the model is rejected before any verdict; the
        // out-of-bound init must not turn that error into a type-bound result.
        let (exit, value) = verify(NO_ACTIONS, engine, 1, false);
        assert_eq!(
            (exit, &value["result"], &value["kind"], &value["message"]),
            (
                2,
                &json!("error"),
                &json!("semantics"),
                &json!("spec has no actions"),
            ),
            "{value}"
        );
    }
}

macro_rules! no_actions_cells {
    ($module:ident, $engine:literal) => {
        mod $module {
            use super::*;

            #[test]
            fn no_actions_depth_1() {
                no_actions_preserves_semantics_error($engine);
            }
        }
    };
}

no_actions_cells!(no_actions_bmc, "bmc");
no_actions_cells!(no_actions_induction, "induction");
no_actions_cells!(no_actions_explicit, "explicit");
no_actions_cells!(no_actions_auto, "auto");

#[test]
fn selected_trans_induction_depth_1_retains_initial_bound_check() {
    for _ in 0..2 {
        // Unlike selected P, induction's selected transition keeps implicit
        // bounds in its base case and therefore still reaches the pre-pass.
        let (exit, value) = verify_property(SELECTED_TRANS, "induction", 1, Some("T"));
        assert_bound(exit, &value, "_bounds_n", 0);
        assert_eq!(
            value["trace"],
            json!([{"step": 0, "state": {"n": 2}}]),
            "{value}"
        );
        assert_eq!(value["last_action"], Value::Null, "{value}");
        assert_eq!(value["checked_to_depth"], 0, "{value}");
    }
}

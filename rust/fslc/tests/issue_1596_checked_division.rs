// SPDX-License-Identifier: Apache-2.0

//! #1596: signed-minimum division/remainder must diagnose overflow, not panic.
//! Action and constant envelopes are pinned to existing addition/subtraction
//! overflow behavior; init locations follow #1543's containing statement rule.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);
const MIN: &str = "-9223372036854775807 - 1";

fn run(source: &str, engine: Option<&str>) -> (Value, i32) {
    // Retain fixtures for fail-first inspection; no cleanup outside this test.
    let path = std::env::temp_dir().join(format!(
        "fsl-1596-{}-{}.fsl",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, source).expect("write inline fixture");
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command.arg(if engine.is_some() { "verify" } else { "check" });
    command.arg(&path).env_remove("FSLC_CACHE");
    if let Some(engine) = engine {
        command.args(["--engine", engine, "--depth", "2", "--no-cache"]);
    }
    if source.starts_with("fsl 5\n") {
        command.env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5");
    } else {
        command.env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE);
    }
    let output = command.output().expect("run working-tree CLI");
    let exit = output.status.code().expect("native exit status");
    println!("fixture={} engine={engine:?} exit={exit}", path.display());
    assert!(output.stderr.is_empty(), "exit={exit}: {output:?}");
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid JSON: {error}; exit={exit}: {output:?}"));
    (value, exit)
}

fn require_error(source: &str, engine: Option<&str>, message: &str) {
    let (value, exit) = run(source, engine);
    assert_eq!(exit, 2, "{value:#}");
    assert_eq!(value["fsl"], "1.0", "{value:#}");
    assert_eq!(value["result"], "error", "{value:#}");
    assert_eq!(value["kind"], "semantics", "{value:#}");
    assert_eq!(value["message"], message, "{value:#}");
    assert!(value.get("loc").is_none(), "{value:#}");
}

fn arithmetic(a: &str, expression: &str, action: bool, invariant: &str) -> String {
    let (init_value, body) = if action {
        ("0", expression)
    } else {
        (expression, "t")
    };
    format!(
        "spec Arithmetic {{\n  state {{ a: Int, t: Int }}\n  init {{\n    a = {a}\n    t = {init_value}\n  }}\n  action go() {{ t = {body} }}\n  invariant P {{ {invariant} }}\n}}\n"
    )
}

macro_rules! engine_cells {
    ($name:ident, $check:expr) => {
        mod $name {
            use super::*;
            #[test]
            fn bmc() {
                ($check)("bmc");
            }
            #[test]
            fn induction() {
                ($check)("induction");
            }
            #[test]
            fn explicit() {
                ($check)("explicit");
            }
            #[test]
            fn auto() {
                ($check)("auto");
            }
        }
    };
}

engine_cells!(init_division, |engine| require_error(
    &arithmetic(MIN, "a / -1", false, "true"),
    Some(engine),
    "integer overflow in division in init at 5:5"
));
engine_cells!(init_remainder, |engine| require_error(
    &arithmetic(MIN, "a % -1", false, "true"),
    Some(engine),
    "integer overflow in remainder in init at 5:5"
));
engine_cells!(action_division, |engine| require_error(
    &arithmetic(MIN, "a / -1", true, "true"),
    Some(engine),
    "integer overflow in division"
));
engine_cells!(action_remainder, |engine| require_error(
    &arithmetic(MIN, "a % -1", true, "true"),
    Some(engine),
    "integer overflow in remainder"
));

// Measured on the base CLI: all four engines return this error, not a
// partial_op verdict. Keep the oracle executable beside its new siblings.
engine_cells!(action_addition_baseline, |engine| require_error(
    &arithmetic("9223372036854775807", "a + 1", true, "true"),
    Some(engine),
    "integer overflow in addition"
));

fn property(expression: &str) -> String {
    format!(
        "spec PropertyDivision {{\n  state {{ a: Int }}\n  init {{ a = {MIN} }}\n  action step() {{ }}\n  invariant P {{ {expression} }}\n}}\n"
    )
}

// TOTAL_DIVISION totalizes zero denominators only: MIN / -1 and MIN % -1
// must still overflow when evaluated inside the invariant itself.
engine_cells!(property_division_overflow, |engine| require_error(
    &property("a / -1 == 0"),
    Some(engine),
    "integer overflow in division"
));
engine_cells!(property_remainder_overflow, |engine| require_error(
    &property("a % -1 == 0"),
    Some(engine),
    "integer overflow in remainder"
));

fn require_totalized_property(expression: &str, engine: &str) {
    let (value, exit) = run(&property(expression), Some(engine));
    assert_eq!(exit, 0, "{value:#}");
    assert_eq!(
        value["result"],
        if engine == "bmc" {
            "verified"
        } else {
            "proved"
        },
        "{value:#}"
    );
    for key in ["kind", "message", "loc"] {
        assert!(value.get(key).is_none(), "{key}: {value:#}");
    }
}

engine_cells!(property_division_zero, |engine| require_totalized_property(
    "a / 0 == 0",
    engine
));
engine_cells!(
    property_remainder_zero,
    |engine| require_totalized_property("a % 0 == 0", engine)
);

fn constant(expression: &str, in_type: bool) -> String {
    let (declaration, ty, initial) = if in_type {
        (format!("type T = 0..(({expression}) + 1)"), "T", "0")
    } else {
        (format!("const C = {expression}"), "Int", "C")
    };
    format!(
        "spec Constant {{\n  const M = {MIN}\n  {declaration}\n  state {{ t: {ty} }}\n  init {{ t = {initial} }}\n  action step() {{ }}\n  invariant P {{ true }}\n}}\n"
    )
}

macro_rules! constant_cells {
    ($name:ident, $expression:literal, $in_type:literal, $message:literal) => {
        mod $name {
            use super::*;
            #[test]
            fn check() {
                require_error(&constant($expression, $in_type), None, $message);
            }
            #[test]
            fn bmc() {
                require_error(&constant($expression, $in_type), Some("bmc"), $message);
            }
            #[test]
            fn explicit() {
                require_error(&constant($expression, $in_type), Some("explicit"), $message);
            }
        }
    };
}

constant_cells!(
    constant_division,
    "M / -1",
    false,
    "integer overflow in division"
);
constant_cells!(
    constant_remainder,
    "M % -1",
    false,
    "integer overflow in remainder"
);
constant_cells!(
    type_division,
    "M / -1",
    true,
    "integer overflow in division"
);
constant_cells!(
    type_remainder,
    "M % -1",
    true,
    "integer overflow in remainder"
);

fn controls(engine: &str) {
    for (a, expression, expected) in [
        (MIN, "a / 1", "a"),
        (MIN, "a % 1", "0"),
        ("-1", "a / -1", "1"),
        ("7", "a / -2", "-3"),
        ("7", "a % -2", "1"),
    ] {
        let (value, exit) = run(
            &arithmetic(a, expression, false, &format!("t == {expected}")),
            Some(engine),
        );
        assert_eq!(exit, 0, "{expression}: {value:#}");
        assert_eq!(
            value["result"],
            if engine == "bmc" {
                "verified"
            } else {
                "proved"
            },
            "{value:#}"
        );
        for key in ["kind", "message", "loc"] {
            assert!(value.get(key).is_none(), "{key}: {value:#}");
        }
    }
    for (expression, message) in [
        ("a / 0", "division by zero in init at 5:5"),
        ("a % 0", "remainder by zero in init at 5:5"),
    ] {
        require_error(
            &arithmetic(MIN, expression, false, "true"),
            Some(engine),
            message,
        );
    }
}
engine_cells!(preservation_controls, controls);

fn finite_domain(operator: &str) -> String {
    format!(
        "fsl 5\nrequirements Finite {{\n  const M = {MIN}\n  actor System\n  entity Item {{\n    lifecycle Ready -> Done\n  }}\n  concept Broken = Item where M {operator} -1 == 0\n  event finish = System finishes Item Ready -> Done\n  decide REQ-1 \"finish\" for Ready Item {{\n    Broken => System finishes\n    otherwise => System finishes\n  }}\n}}\nverify {{ instances Item = 1 }}\n"
    )
}

// Like checked subtraction here, an enumeration failure is deferred to the
// solver, not a constant-folding semantics error. The table has an otherwise
// row and no overlaps, so check succeeds. This guards the distinct evaluator.
#[test]
fn finite_domain_division_defers_without_panicking() {
    let (value, exit) = run(&finite_domain("/"), None);
    assert_eq!(exit, 0, "{value:#}");
    assert_eq!(value["result"], "ok", "{value:#}");
}

#[test]
fn finite_domain_remainder_defers_without_panicking() {
    let (value, exit) = run(&finite_domain("%"), None);
    assert_eq!(exit, 0, "{value:#}");
    assert_eq!(value["result"], "ok", "{value:#}");
}

#[test]
fn fixed_coverage_manifest_remains_safe() {
    // coverage_matrix has no user-source input; its manifest's divisions are
    // small values. Do not claim MIN is unrepresentable: literal_int also
    // evaluates arithmetic expressions, including -MAX - 1.
    let value = fslc_rust::coverage::coverage_matrix().expect("fixed coverage corpus");
    assert_eq!(value["result"], "conformance_coverage", "{value:#}");
}

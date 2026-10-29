// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1521: assigned-but-incomplete roots must not acquire defaults in refine.
//! Each consumer has its own test: a false refine verdict cannot hide another
//! engine's observation. Never-assigned roots remain free (singleton control).
//! Verdict contracts are separate, not envelope parity (assurance/cost differ).

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);

impl Fixture {
    fn new(source: &str, abstraction: &str, mapping: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "fslc-1521-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create #1521 fixture directory");
        for (file, contents) in [
            ("model.fsl", source),
            ("abs.fsl", abstraction),
            ("mapping.fsl", mapping),
            ("empty.json", "[]"),
        ] {
            std::fs::write(root.join(file), contents).expect("write #1521 fixture");
        }
        Self(root)
    }

    fn observe(&self, consumer: &str) -> (Value, i32) {
        let args = match consumer {
            "check" => vec!["check", "model.fsl"],
            "Monitor" => vec!["replay", "model.fsl", "--trace", "empty.json"],
            "refine" => vec![
                "refine",
                "model.fsl",
                "abs.fsl",
                "mapping.fsl",
                "--depth",
                "4",
            ],
            engine => vec![
                "verify",
                "model.fsl",
                "--engine",
                engine,
                "--depth",
                "4",
                "--no-cache",
            ],
        };
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env_remove("FSLC_CACHE")
            .output()
            .expect("run native fslc");
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{consumer}: invalid JSON: {error}; stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        let exit = output.status.code().expect("native exit code");
        eprintln!("OBS consumer={consumer} exit={exit} output={actual}");
        assert!(output.stderr.is_empty(), "{consumer}: {output:?}");
        (actual, exit)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned #1521 fixture");
    }
}

fn assert_verdict(consumer: &str, actual: &Value, exit: i32, result: &str, bounds: bool) {
    assert_eq!(exit, i32::from(bounds), "{consumer}: {actual:#}");
    assert_eq!(
        json!({
            "result": actual["result"], "violation_kind": actual["violation_kind"],
            "invariant": actual["invariant"], "action": actual["action"],
            "last_action": actual["last_action"], "kind": actual["kind"],
            "message": actual["message"], "violation": actual["violation"],
        }),
        json!({
            "result": result,
            "violation_kind": if bounds { json!("type_bound") } else { Value::Null },
            "invariant": if bounds { json!("_bounds_x") } else { Value::Null },
            "action": null, "last_action": null, "kind": null,
            "message": null, "violation": null,
        }),
        "{consumer}: verdict/name/error contract: {actual:#}"
    );
}

fn assert_error(consumer: &str, actual: &Value, exit: i32) {
    assert_eq!(exit, 2, "{consumer}: {actual:#}");
    assert_eq!(actual["result"], "error", "{consumer}: {actual:#}");
    let kind = if consumer == "refine" {
        "type"
    } else {
        "semantics"
    };
    assert_eq!(actual["kind"], kind, "{consumer}: {actual:#}");
    for field in [
        "violation_kind",
        "invariant",
        "action",
        "last_action",
        "violation",
        "final_state",
    ] {
        assert!(actual.get(field).is_none(), "{consumer}: {actual:#}");
    }
}

fn assert_unassigned(consumer: &str, actual: &Value, exit: i32, roots: &str, partial: bool) {
    assert_error(consumer, actual, exit);
    let suffix = if partial {
        " (partial component initialization is rejected by the explicit engine)"
    } else {
        ""
    };
    assert_eq!(
        actual["message"],
        format!("init does not assign state variable(s): {roots}{suffix}")
    );
    assert!(actual.get("loc").is_none(), "{consumer}: {actual:#}");
}

struct Cell {
    source: &'static str,
    abstraction: &'static str,
    mapping: &'static str,
    missing: &'static str,
    partial: bool,
    bounds: bool,
    read: Option<(&'static str, usize, usize)>,
    refuse: Option<&'static str>,
}
impl Cell {
    fn run(&self, consumer: &str) {
        let (actual, exit) =
            Fixture::new(self.source, self.abstraction, self.mapping).observe(consumer);
        if let Some((root, line, column)) = self.read {
            assert_error(consumer, &actual, exit);
            assert_eq!(
                actual["message"],
                format!("init references state variable '{root}' before it is assigned")
            );
            if matches!(consumer, "Monitor" | "refine") {
                assert!(actual.get("loc").is_none(), "{consumer}: {actual:#}");
            } else {
                assert_eq!(actual["loc"], json!({"line": line, "column": column}));
            }
        } else if consumer == "refine" {
            if let Some(root) = self.refuse {
                assert_error(consumer, &actual, exit);
                let message = actual["message"]
                    .as_str()
                    .expect("refine init error message");
                assert!(
                    message.starts_with("init does not assign state variable(s): ")
                        && message
                            .split(|c: char| !c.is_alphanumeric() && c != '_')
                            .any(|word| word == root),
                    "{actual:#}"
                );
                assert!(
                    message.contains("bmc") && message.contains("induction"),
                    "{actual:#}"
                );
                assert!(!self.partial || message.contains("partial"), "{actual:#}");
                assert!(
                    actual.get("loc").is_none(),
                    "refine stringifies runtime errors: {actual:#}"
                );
            } else {
                assert_verdict(consumer, &actual, exit, "refines", false);
            }
        } else if matches!(consumer, "explicit" | "Monitor") {
            assert_unassigned(consumer, &actual, exit, self.missing, self.partial);
        } else {
            let result = match consumer {
                "check" => "ok",
                _ if self.bounds => "violated",
                "bmc" => "verified",
                _ => "proved",
            };
            assert_verdict(
                consumer,
                &actual,
                exit,
                result,
                consumer != "check" && self.bounds,
            );
        }
    }
}

macro_rules! consumers {
    ($($name:ident => $consumer:literal),*) => {
        $(#[test] fn $name() { run($consumer); })*
    };
}

macro_rules! cell {
    ($id:ident, $source:expr, $abs:expr, $map:expr, ($missing:expr, $partial:expr, $bounds:expr, $read:expr, $refuse:expr)) => {
        mod $id {
            use super::*;
            fn run(consumer: &str) {
                Cell {
                    source: $source,
                    abstraction: $abs,
                    mapping: concat!(
                        "refinement R { impl RImpl abs RAbs ",
                        $map,
                        " action idle() -> idle() }"
                    ),
                    missing: $missing,
                    partial: $partial,
                    bounds: $bounds,
                    read: $read,
                    refuse: $refuse,
                }
                .run(consumer);
            }
            consumers!(check => "check", bmc => "bmc", induction => "induction",
                explicit => "explicit", monitor => "Monitor", refine => "refine");
        }
    };
}

const INT_ABS_ZERO: &str = "spec RAbs { state { a: 0..3 } init { a = 0 } action idle() {} }";
const INT_ABS_ONE: &str = "spec RAbs { state { a: 0..3 } init { a = 1 } action idle() {} }";
const BOOL_ABS: &str = "spec RAbs { state { c: Bool } init { c = false } action idle() {} }";

cell!(
    c1_empty_filter,
    r"spec RImpl {
  type K = 0..1
  state { x: 0..3, y: Bool }
  init { forall k: K where false { x = 1 } }
  action idle() {}
}",
    INT_ABS_ZERO,
    "map a = x",
    ("x, y", false, true, None, Some("x"))
);

cell!(
    c2_partial_field,
    r"spec RImpl {
  struct Pair { a: Bool, b: Bool }
  state { s: Pair, free: Bool }
  init { s.a = true }
  action idle() {}
  invariant Initialized { s.a }
}",
    BOOL_ABS,
    "map c = s.b",
    ("free, s", true, false, None, Some("s"))
);

cell!(
    c3_map_read_before_assignment,
    r"spec RImpl {
  type K = 0..1
  state { m: Map<K, Bool>, a: Bool, y: Bool }
  init { a = m[0] forall k: K { m[k] = true } }
  action idle() {}
}",
    BOOL_ABS,
    "map c = a",
    ("", false, false, Some(("m", 4, 10)), None)
);

cell!(
    c4_scalar_read_before_assignment,
    r"spec RImpl {
  type V = 0..1
  state { u: V, t: V, x: Bool }
  init { u = t t = 1 }
  action idle() {}
}",
    INT_ABS_ZERO,
    "map a = u",
    ("", false, false, Some(("t", 4, 10)), None)
);

// Positive preservation control: x is definitely assigned, y is genuinely free.
cell!(
    c5_nonempty_singleton,
    r"spec RImpl {
  type K = 0..1
  state { x: 0..3, y: Bool }
  init { forall k: K where k == 0 { x = 1 } }
  action idle() {}
}",
    INT_ABS_ONE,
    "map a = x",
    ("y", false, false, None, None)
);

cell!(
    c6a_missing_else,
    r"spec RImpl {
  state { x: Bool, free: Bool }
  init { if false { x = true } }
  action idle() {}
}",
    BOOL_ABS,
    "map c = x",
    ("free, x", false, false, None, Some("x"))
);

cell!(
    c6b_partial_map_key,
    r"spec RImpl {
  type K = 0..1
  state { m: Map<K, Bool>, free: Bool }
  init { m[0] = true }
  action idle() {}
}",
    BOOL_ABS,
    "map c = m[1]",
    ("free, m", true, false, None, Some("m"))
);

// Sibling caller: abstract initial states must not acquire defaults either.
cell!(
    c7_partial_abstraction,
    r"spec RImpl {
  state { c: Bool, free: Bool }
  init { c = false }
  action idle() {}
}",
    "spec RAbs { state { x: Bool, y: Bool } init { if false { x = true } } action idle() {} }",
    "map x = c map y = free",
    ("free", false, false, None, Some("x"))
);

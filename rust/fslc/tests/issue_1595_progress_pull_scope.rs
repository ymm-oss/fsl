// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1595: pulled progress retains abstract constant/binder scope or rejects capture.
//! These are verdict contracts, not envelope parity: refine reports implementation
//! traces and progress metadata, whereas BMC reports abstract traces and assurance.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);

impl Fixture {
    fn new(impl_const: &str, abs_const: &str, property: &str) -> Self {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "fslc-1595-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create #1595 fixture directory");
        let implementation = format!(
            "spec Impl {{ {impl_const} state {{ x: Int = 0 }}
              fair action go() {{ requires x == 0 x = 1 }} }}"
        );
        let abstraction = format!(
            "spec Abs {{ {abs_const} state {{ y: Int = 0 }}
              fair action step() {{ requires y == 0 y = 1 }}
              leadsTo L {{ {property} }} }}"
        );
        for (file, source) in [
            ("impl.fsl", implementation.as_str()),
            ("abs.fsl", abstraction.as_str()),
            (
                "map.fsl",
                "refinement R { impl Impl abs Abs map y = x action go() -> step()
                  preserve progress { respond L by go } }",
            ),
        ] {
            std::fs::write(root.join(file), source).expect("write #1595 fixture");
        }
        Self(root)
    }

    fn observe(&self, cell: &str, consumer: &str) -> (Value, i32) {
        let args = if consumer == "refine" {
            vec!["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "4"]
        } else {
            vec![
                "verify",
                "abs.fsl",
                "--engine",
                "bmc",
                "--depth",
                "4",
                "--no-cache",
            ]
        };
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env_remove("FSLC_CACHE")
            .output()
            .expect("run native fslc");
        for stream in [&output.stdout, &output.stderr] {
            assert!(
                !String::from_utf8_lossy(stream).contains("_substitution_"),
                "{cell}/{consumer}: internal name leaked: {output:?}"
            );
        }
        let actual: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{cell}/{consumer}: invalid JSON: {error}; {output:?}"));
        let exit = output.status.code().expect("native exit code");
        eprintln!("OBS cell={cell} consumer={consumer} exit={exit} output={actual}");
        assert!(output.stderr.is_empty(), "{cell}/{consumer}: {output:?}");
        (actual, exit)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned #1595 fixture");
    }
}

enum Expected {
    Lost(Value),
    Located(&'static str),
    StaticBound,
}
use Expected::{Located, Lost, StaticBound};

fn assert_error(actual: &Value, exit: i32, kind: &str) {
    assert_eq!(exit, 2, "{actual:#}");
    assert_eq!(actual["result"], "error", "{actual:#}");
    assert_eq!(actual["kind"], kind, "{actual:#}");
}

fn assert_lost(actual: &Value, exit: i32, result: &str, bindings: &Value) {
    assert_eq!(exit, 1, "{actual:#}");
    assert_eq!(actual["result"], result, "{actual:#}");
    assert_eq!(actual["invariant"], "L", "{actual:#}");
    assert_eq!(actual["violation_kind"], "leadsTo", "{actual:#}");
    assert_eq!(&actual["bindings"], bindings, "{actual:#}");
}

fn run(cell: &str, impl_const: &str, abs_const: &str, property: &str, expected: &Expected) {
    let fixture = Fixture::new(impl_const, abs_const, property);
    // Observe both commands twice before a failing regression can stop calibration.
    let observations: Vec<_> = (0..2)
        .map(|_| {
            (
                fixture.observe(cell, "refine"),
                fixture.observe(cell, "bmc"),
            )
        })
        .collect();
    let static_bound = "'N' is not an integer const";
    for (_, (abs, exit)) in &observations {
        if matches!(expected, StaticBound | Located("N")) {
            assert_error(abs, *exit, "semantics");
            assert_eq!(abs["message"], static_bound, "{abs:#}");
        } else {
            let bindings = match expected {
                Lost(bindings) => bindings.clone(),
                Located(name) => json!({*name: 2}),
                StaticBound => unreachable!(),
            };
            assert_lost(abs, *exit, "violated", &bindings);
        }
    }
    for ((actual, exit), (abs, abs_exit)) in &observations {
        match expected {
            Lost(bindings) => {
                assert_lost(actual, *exit, "refinement_failed", bindings);
                assert_eq!(actual["kind"], "progress_lost", "{actual:#}");
                assert_eq!(actual["bindings"], abs["bindings"]);
                assert_eq!(exit, abs_exit);
            }
            Located(name) => {
                assert_error(actual, *exit, "type");
                assert!(actual["loc"].is_object(), "{actual:#}");
                let message = actual["message"].as_str().expect("located error message");
                assert!(message.contains(name), "{actual:#}");
                if *name == "N" {
                    assert_eq!(exit, abs_exit);
                }
            }
            StaticBound => {
                assert_error(actual, *exit, "semantics");
                assert_eq!(actual["message"], static_bound, "{actual:#}");
                assert_eq!(actual["message"], abs["message"]);
                assert_eq!(exit, abs_exit);
            }
        }
    }
}

macro_rules! cells {
    ($($name:ident: ($ic:expr, $ac:expr, $property:expr, $expected:expr)),* $(,)?) => {
        $(#[test]
        fn $name() {
            run(stringify!($name), $ic, $ac, $property, &$expected);
        })*
    };
}

cells! {
    f1: ("const N = 1", "const N = 3",
        "forall z in 0..N { y == 0 ~> z <= 1 }", Lost(json!({"z": 2}))),
    f2: ("const N = 1", "",
        "forall N in 0..3 { forall z in 0..N { y == 0 ~> z <= 1 } }", Located("N")),
    c1: ("const M = 1", "const N = 3",
        "forall z in 0..N { y == 0 ~> z <= 1 }", Lost(json!({"z": 2}))),
    s1c: ("const M = 1", "const N = 3", "y == 0 ~> y >= N", Lost(json!({}))),
    c2: ("const M = 1", "",
        "forall N in 0..3 { forall z in 0..N { y == 0 ~> z <= 1 } }", StaticBound),
    c3: ("const N = 1", "",
        "forall K in 0..3 { forall z in 0..3 { (y == 0 and z <= K) ~> z <= 1 } }",
        Lost(json!({"K": 2, "z": 2}))),
    s1: ("const N = 1", "const N = 3", "y == 0 ~> y >= N", Lost(json!({}))),
    s2: ("", "const x = 3", "y == 0 ~> y >= x", Lost(json!({}))),
    s5: ("const N = 1", "const N = 3",
        "y == 0 ~> (forall k in 0..N { k <= 1 })", Lost(json!({}))),
    s4: ("", "", "forall y in 2..3 { y == 2 ~> y == 3 }", Lost(json!({"y": 2}))),
    s3: ("", "", "forall x in 2..3 { y == 0 ~> y == x }", Located("x")),
    s3c: ("", "", "forall w in 2..3 { y == 0 ~> y == w }", Lost(json!({"w": 2}))),
    s4c: ("", "", "forall w in 2..3 { w == 2 ~> w == 3 }", Lost(json!({"w": 2}))),
}

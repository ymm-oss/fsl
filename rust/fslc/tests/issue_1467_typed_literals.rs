// SPDX-License-Identifier: Apache-2.0

//! F: scalar/enum literals must decide through every consumer, including frozen
//! vacuity. Composite policy is deferred: preserve check acceptance, not errors.

#[path = "support/scratch_identity.rs"]
mod scratch_identity;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const TEMPLATE: &str = r"spec Literals {
  type K = 0..2
  enum Color { Zed, Able }
  state { n: K, p: K, c: Bool, e: Color, ys: Set<K>, zs: Seq<K, 2> }
  init { n = 0 p = 0 c = true e = Zed ys = Set { 1 } zs = Seq { 1 } }
  action go() { n = 1 p = 2 c = false e = Able }
  invariant Capture { $P }
}";

fn fixture(label: &str, source: &str) -> PathBuf {
    let parent = Path::new(env!("CARGO_TARGET_TMPDIR")).join("issue-1467");
    std::fs::create_dir_all(&parent).expect("fixture parent");
    let directory = scratch_identity::unique(parent.join(label));
    std::fs::create_dir(&directory).expect("fresh fixture");
    std::fs::write(directory.join("spec.fsl"), source).expect("spec");
    std::fs::write(
        directory.join("actions.json"),
        r#"[{"action":"go","params":{}}]"#,
    )
    .expect("Monitor trace");
    directory
}

fn observe(directory: &Path, route: &str) -> (Value, i32) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    match route {
        "check" => command.args(["check", "spec.fsl"]),
        "replay" => command.args(["replay", "spec.fsl", "--trace", "actions.json"]),
        engine => command.args([
            "verify",
            "spec.fsl",
            "--depth",
            "4",
            "--no-cache",
            "--engine",
            engine,
        ]),
    };
    let output = command
        .current_dir(directory)
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_CACHE_VERIFY")
        .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
        .output()
        .expect("native CLI");
    std::fs::write(
        directory.join(format!("{route}.stdout.json")),
        &output.stdout,
    )
    .expect("retain stdout");
    std::fs::write(
        directory.join(format!("{route}.stderr.txt")),
        &output.stderr,
    )
    .expect("retain stderr");
    assert!(output.stderr.is_empty(), "{route}: {output:?}");
    let value = serde_json::from_slice(&output.stdout).expect("CLI JSON");
    (value, output.status.code().expect("exit status"))
}

fn cell(label: &str, predicate: &str, negative: bool) {
    let source = TEMPLATE.replace("$P", predicate);
    let mut failures = Vec::new();
    // Run all consumers and both repetitions even on RED, so an early symbolic
    // refusal cannot conceal the concrete oracle or the other engine failures.
    for repeat in 1..=2 {
        let directory = fixture(label, &source);
        for route in ["check", "bmc", "induction", "explicit", "replay"] {
            let (value, exit) = observe(&directory, route);
            let (actual, expected) = match (route, negative) {
                ("check", _) => (
                    json!([exit, value["result"], value["kind"], value["message"]]),
                    json!([0, "ok", null, null]),
                ),
                ("replay", true) => (
                    json!([
                        exit,
                        value["result"],
                        value["violation"]["kind"],
                        value["violation"]["name"],
                        value["failed_at_event"]
                    ]),
                    json!([1, "nonconformant", "invariant", "Capture", 0]),
                ),
                ("replay", false) => (
                    json!([
                        exit,
                        value["result"],
                        value["steps_checked"],
                        value["violation"],
                        value["failed_at_event"]
                    ]),
                    json!([0, "conformant", 1, null, null]),
                ),
                (_, true) => (
                    json!([
                        exit,
                        value["result"],
                        value["violation_kind"],
                        value["invariant"],
                        value["violated_at_step"]
                    ]),
                    json!([1, "violated", "invariant", "Capture", 1]),
                ),
                (_, false) => (
                    json!([
                        exit,
                        value["result"],
                        value["completeness"],
                        value["invariant"],
                        value["violated_at_step"],
                        value["violation_kind"],
                        value["trace"]
                    ]),
                    json!([
                        0,
                        if route == "bmc" { "verified" } else { "proved" },
                        if route == "bmc" {
                            "bounded"
                        } else {
                            "unbounded"
                        },
                        null,
                        null,
                        null,
                        null
                    ]),
                ),
            };
            // A swallowed frozen-vacuity failure must not manufacture GREEN.
            let needs_frozen = route == "explicit"
                && [
                    "set_if_binder-positive",
                    "seq_if_binder-positive",
                    "typed_set_if-positive",
                    "typed_seq_if-positive",
                ]
                .contains(&label);
            let frozen_probe = !needs_frozen
                || value["warnings"].as_array().is_some_and(|warnings| {
                    warnings.iter().any(|warning| {
                        warning["kind"] == "tautology_over_frozen" && warning["name"] == "Capture"
                    })
                });
            let agrees = actual == expected && frozen_probe;
            println!(
                "CELL {label} repeat={repeat} {route} {} produced={actual} expected={expected} evidence={}",
                if agrees { "GREEN" } else { "RED" },
                directory.display()
            );
            if !agrees {
                failures.push(format!(
                    "{label} {route} frozen_probe={frozen_probe}: {value}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

macro_rules! pair {
    ($name:ident, $negative:expr, $positive:expr) => {
        mod $name {
            use super::*;
            #[test]
            fn negative() {
                cell(concat!(stringify!($name), "-negative"), $negative, true);
            }
            #[test]
            fn positive() {
                cell(concat!(stringify!($name), "-positive"), $positive, false);
            }
        }
    };
}

pair!(
    pop,
    "Seq { 1, p }.pop().head() == 0",
    "Seq { 1, p }.pop().head() >= 0"
);
pair!(at, "Seq { 1, p }.at(1) == 0", "Seq { 1, p }.at(0) == 1");
pair!(
    set_size,
    "Set { 0, n }.size() == 1",
    "Set { 0, n }.size() <= 2"
);
pair!(
    seq_size,
    "Seq { 1, p }.size() == 2 - n",
    "Seq { 1, p }.size() == 2"
);
pair!(
    seq_contains,
    "Seq { 1, p }.contains(0)",
    "Seq { 1, p }.contains(1)"
);
pair!(
    set_contains,
    "Set { 1, p }.contains(0)",
    "Set { 1, p }.contains(1)"
);
pair!(
    enum_contains,
    "Set { e }.contains(Zed)",
    "Set { Zed, e }.contains(Zed)"
);
pair!(
    enum_at,
    "Seq { Zed, e }.at(1) == Zed",
    "Seq { Zed, e }.at(0) == Zed"
);
pair!(
    set_equality,
    "Set { 0 } == Set { n }",
    "Set { 0, n } == Set { n, 0 }"
);
pair!(
    seq_equality,
    "Seq { 0 } == Seq { n }",
    "Seq { 0, n } == Seq { 0, n }"
);

// ys/zs are intentionally frozen: explicit's symbolic vacuity probe must use
// the same materialization as verification, not discard its failure or skip it.
pair!(
    set_if_binder,
    "forall x in (if c then ys else Set { 2 }): x == 1",
    "forall x in (if c then ys else Set { 2 }): x >= 1"
);
pair!(
    seq_if_binder,
    "forall x in (if c then zs else Seq { 2 }): x == 1",
    "forall x in (if c then zs else Seq { 2 }): x >= 1"
);
pair!(
    set_literal_if_binder,
    "forall x in (if c then Set { 1 } else Set { 2 }): x == 1",
    "forall x in (if c then Set { 1 } else Set { 2 }): x >= 1"
);
pair!(
    seq_literal_if_binder,
    "forall x in (if c then Seq { 1 } else Seq { 2 }): x == 1",
    "forall x in (if c then Seq { 1 } else Seq { 2 }): x >= 1"
);
pair!(
    set_if_equality,
    "(if c then ys else Set { 2 }) == Set { 1 }",
    "(if c then ys else Set { 1 }) == Set { 1 }"
);
pair!(
    seq_if_equality,
    "(if c then zs else Seq { 2 }) == Seq { 1 }",
    "(if c then zs else Seq { 1 }) == Seq { 1 }"
);
pair!(
    set_literal_if_equality,
    "(if c then Set { 1 } else Set { 2 }) == Set { 1 }",
    "(if c then Set { 1 } else Set { 2 }) == (if c then Set { 1 } else Set { 2 })"
);
pair!(
    seq_literal_if_equality,
    "(if c then Seq { 1 } else Seq { 2 }) == Seq { 1 }",
    "(if c then Seq { 1 } else Seq { 2 }) == (if c then Seq { 1 } else Seq { 2 })"
);

// Base-GREEN preservation: an existing typed equality anchor already supplies
// the expected type. The positive is inductive even for arbitrary frozen ys/zs:
// go clears c. F must also preserve direct scalar/enum binder behavior.
pair!(
    typed_set_if,
    "ys == (if c then ys else Set { 2 })",
    "ys == (if c then Set { 1 } else ys)"
);
pair!(
    typed_seq_if,
    "zs == (if c then zs else Seq { 2 })",
    "zs == (if c then Seq { 1 } else zs)"
);
pair!(
    scalar_binder,
    "forall x in Set { 0, n }: x == 0",
    "forall x in Set { 0, n }: x >= 0"
);
pair!(
    enum_binder,
    "forall x in Set { Zed, e }: x == Zed",
    "forall x in Set { Zed, e }: x == Zed or x == Able"
);

#[test]
fn composite_literal_check_acceptance_is_preserved() {
    let template = r"spec Composite {
  type N = 0..2
  type M = 0..2
  struct P { a: N }
  state { r: relation N -> N, q: relation N -> M }
  init { r = Set {} q = Set {} }
  action go() {}
  invariant Capture { $P }
}";
    for predicate in [
        "Set { P { a: 0 } }.size() == 1",
        "Set { some(0) }.size() == 1",
        "Set { Set { 0 } }.size() == 1",
        "Seq { Seq { 0 } }.size() == 1",
        "Seq { r, q }.at(1).contains(0, 0)",
    ] {
        for _ in 0..2 {
            let directory = fixture("composite-check", &template.replace("$P", predicate));
            let (value, exit) = observe(&directory, "check");
            assert_eq!((exit, value["result"].as_str()), (0, Some("ok")), "{value}");
        }
    }
}

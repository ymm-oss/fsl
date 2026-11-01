// SPDX-License-Identifier: Apache-2.0

//! Bool closure must preserve evaluated endpoints in appended relation slots.
//! Reachable states are exactly {} and {(false,true)}; link is idempotent.

use serde_json::{Value, json};
use std::{path::Path, process::Command};

const EDGE: &str = "r.add(r.contains(true, false), not r.contains(true, false))";
const LOOP: &str =
    "r == Set {} or acyclic(r.add(r.contains(true, false), r.contains(true, false)))";

fn duration(value: &mut Value) {
    let number = value.as_f64().expect("observed duration leaf must exist");
    assert!(number.is_finite() && number >= 0.0, "{value}");
    *value = Value::Null;
}

fn stable(mut value: Value) -> Value {
    if value["result"] != "ok" {
        // Pristine full repeats differed only at these measured wall-clock leaves.
        duration(&mut value["cost"]["elapsed_s"]);
        if value["engine"] != "explicit" {
            duration(&mut value["cost"]["solver"]["check_elapsed_s"]);
        }
        for property in value["cost"]["properties"].as_array_mut().expect("costs") {
            duration(&mut property["elapsed_s"]);
        }
    }
    value
}

fn run(label: &str, relation: &str, pair: &str, predicate: &str, outcome: &str) {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("issue_1467_bool_closure")
        .join(label);
    std::fs::create_dir_all(&directory).unwrap();
    let source = format!(
        "spec BoolClosure {{\n  enum Color {{ Red, Blue }}\n  type N = 0..1\n  state {{ r: relation {relation} }}\n  init {{ r = Set {{}} }}\n  action link() {{ r = r.add({pair}) }}\n  invariant P {{ {predicate} }}\n}}\n"
    );
    std::fs::write(directory.join("closure.fsl"), source).unwrap();
    // Execute all routes/repeats before any aggregate assertion; retain raw packets.
    let outputs = ["check", "bmc", "explicit", "induction"].map(|mode| {
        let repeats = [0, 1].map(|round| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
            command.args([
                if mode == "check" { "check" } else { "verify" },
                "closure.fsl",
            ]);
            if mode != "check" {
                command.args(["--engine", mode, "--depth", "2", "--no-cache"]);
            }
            let output = command
                .current_dir(&directory)
                .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
                .env("FSLC_CACHE", "off")
                .output()
                .unwrap();
            for (suffix, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
                std::fs::write(directory.join(format!("{mode}-{round}.{suffix}")), bytes).unwrap();
            }
            std::fs::write(
                directory.join(format!("{mode}-{round}.exit")),
                output.status.code().unwrap().to_string(),
            )
            .unwrap();
            output
        });
        (mode, repeats)
    });
    for (mode, repeats) in outputs {
        let packets = repeats.map(|output| {
            assert!(output.stderr.is_empty(), "{label} {mode}: {output:?}");
            (
                output.status.code().unwrap(),
                stable(serde_json::from_slice::<Value>(&output.stdout).unwrap()),
            )
        });
        assert_eq!(packets[0], packets[1], "{label} {mode}");
        let (status, value) = &packets[0];
        let expected = match (mode, outcome) {
            ("check", _) => (0, "ok"),
            (_, "violated") => (1, "violated"),
            ("induction", "cti") => (1, "unknown_cti"),
            ("bmc", _) => (0, "verified"),
            _ => (0, "proved"),
        };
        assert_eq!(
            (*status, value["result"].as_str()),
            (expected.0, Some(expected.1)),
            "{label} {mode}: {value:#}"
        );
        assert_eq!(value.get("kind"), None, "{value:#}");
        assert_eq!(value.get("message"), None, "{value:#}");
        assert_eq!(value["spec"], "BoolClosure");
        if mode == "check" {
            continue;
        }
        let contract = if outcome == "violated" {
            let action = json!({"name":"link","params":{},"loc":{"line":6,"column":3}});
            json!({"violation_kind":"invariant","trace_type":"invariant","invariant":"P",
            "loc":{"line":7,"column":3},"violated_at_step":1,"violating_bindings":[{}],
            "last_action":action,"checked_to_depth":1,"completeness":"bounded",
                "blame":{"conjuncts":[{"index":0,"text":predicate,"holds":false,"violating_bindings":[{}]}]},
            "trace":[
                {"step":0,"state":{"r":[]}},
                {"step":1,"state":{"r":[[false,true]]},"action":action,
                 "changes":{"r":{"from":[],"to":[[false,true]]}},"blame":{"guards":[],"effects":[]}}
            ]})
        } else if mode == "induction" && outcome == "cti" {
            // A true reachable-state property need not be inductive from arbitrary r.
            json!({"invariant":"P","trace_type":"induction_cti","k":1,
                "completeness":"bounded","checked_to_depth":2,"cti":{"states":[
                {"step":0,"state":{"r":[[true,false]]}},
                {"step":1,"state":{"r":[[false,true],[true,false]]},
                 "action":{"name":"link","params":{},"loc":{"line":6,"column":3}},
                 "changes":{"r":{"from":[[true,false]],"to":[[false,true],[true,false]]}}}
            ],"violated_at":1}})
        } else if mode == "explicit" {
            json!({"closure":true,"states_explored":2,"completeness":"unbounded",
                "invariants_checked":["_bounds_r","P"],"action_coverage":{"link":true},
                "action_profile":{"link":{"enabled":2,"fired":2,"no_op":1}}})
        } else {
            let completeness = if mode == "bmc" {
                "bounded"
            } else {
                "unbounded"
            };
            json!({"checked_to_depth":2,"completeness":completeness,
                "invariants_checked":["_bounds_r","P"],"action_coverage":{"link":true}})
        };
        for (key, expected) in contract.as_object().unwrap() {
            assert_eq!(&value[key], expected, "{label} {mode} {key}: {value:#}");
        }
    }
}

fn boolean(label: &str, predicate: &str, outcome: &str) {
    run(label, "Bool -> Bool", "false, true", predicate, outcome);
}

#[test]
fn reachable_dynamic_bool_endpoints() {
    boolean(
        "reachable-dynamic",
        &format!("reachable({EDGE}, false, true) and not reachable({EDGE}, true, false)"),
        "pass",
    );
}

#[test]
fn acyclic_dynamic_bool_endpoints() {
    boolean("acyclic-dynamic", &format!("acyclic({EDGE})"), "cti");
}

#[test]
fn acyclic_dynamic_self_loop_rejects() {
    boolean("self-loop-dynamic", LOOP, "violated");
}

#[test]
fn literal_bool_closure_preservation_twin() {
    boolean(
        "literal-twin",
        "reachable(r.add(false, true), false, true) and not reachable(r.add(false, true), true, false) and acyclic(r.add(false, true))",
        "pass",
    );
}

#[test]
fn literal_bool_self_loop_rejects() {
    boolean(
        "self-loop-literal",
        "r == Set {} or acyclic(r.add(false, false))",
        "violated",
    );
}

#[test]
fn numeric_and_enum_closure_preservation_controls() {
    run(
        "numeric-control",
        "N -> N",
        "0, 1",
        "reachable(r.add(0, 1), 0, 1) and acyclic(r.add(0, 1))",
        "pass",
    );
    run(
        "enum-control",
        "Color -> Color",
        "Red, Blue",
        "reachable(r.add(Red, Blue), Red, Blue) and acyclic(r.add(Red, Blue))",
        "pass",
    );
}

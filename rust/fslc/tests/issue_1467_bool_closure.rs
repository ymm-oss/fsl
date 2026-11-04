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

fn validate_memory(memory: Option<f64>) {
    let memory = memory.expect("queried native solver memory must be numeric and present");
    assert!(memory.is_finite() && memory >= 0.0, "{memory}");
}

fn stable(mut value: Value, mode: &str) -> Value {
    // The retained numeric-control BMC pair differs at 17.68/17.67 MB after
    // duration normalization, and the numeric-control induction pair differed
    // at the same two values on the #1680 binary. Follow
    // collection_replay::stable_full_packet's live validation, restricted here
    // to the actually observed proof classes.
    let observed_class = match mode {
        "bmc" => {
            value["result"] == "verified"
                && value["completeness"] == "bounded"
                && value["depth"] == 2
                && value["checked_to_depth"] == 2
        }
        "induction" => value["result"] == "proved" && value["completeness"] == "unbounded",
        _ => false,
    };
    if observed_class && value["versions"]["solver"]["backend"] == "native-z3" {
        let checks = value["cost"]["solver"]["checks"]
            .as_u64()
            .expect("solver checks must be an unsigned integer");
        if checks > 0 {
            validate_memory(value["cost"]["solver"]["memory_mb"].as_f64());
            value["cost"]["solver"]
                .as_object_mut()
                .unwrap()
                .remove("memory_mb");
        }
    }
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
            let status = output.status.code().unwrap();
            let value = serde_json::from_slice::<Value>(&output.stdout).unwrap();
            (status, stable(value, mode))
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

// Synthetic full-body comparator input, not an edited CLI observation. The
// original failing pair remains raw; only clones of this packet are mutated.
fn telemetry_packet() -> Value {
    json!({
        "fsl":"1.0", "spec":"BoolClosure", "result":"verified", "depth":2,
        "checked_to_depth":2, "completeness":"bounded",
        "invariants_checked":["_bounds_r","P"], "transitions_checked":[],
        "reachables":{}, "action_coverage":{"link":true}, "deadlock":{"found":false},
        "warnings":[], "note":"bounded verification: no violation within depth 2",
        "cost":{"elapsed_s":0.01,
            "solver":{"checks":17,"check_elapsed_s":0.001,"conflicts":2,
                "decisions":1,"propagations":10,"memory_mb":17.68},
            "properties":[
                {"kind":"action_coverage","name":"link","checks":1,"elapsed_s":0.001},
                {"kind":"deadlock","name":"deadlock","checks":3,"elapsed_s":0.001},
                {"kind":"init","name":"initial_state","checks":1,"elapsed_s":0.001},
                {"kind":"invariant","name":"P","checks":3,"elapsed_s":0.001},
                {"kind":"partial_op","name":"P","checks":3,"elapsed_s":0.001},
                {"kind":"partial_op","name":"actions","checks":2,"elapsed_s":0.001},
                {"kind":"type_bound","name":"_bounds_r","checks":3,"elapsed_s":0.001},
                {"kind":"vacuity","name":"session","checks":1,"elapsed_s":0.001}
            ]},
        "versions":{"verifier":{"name":"fslc-rust","version":"4.12.0"},
            "core":{"name":"fsl-core","version":"4.12.0"},
            "solver":{"name":"z3","backend":"native-z3","version":"Z3 4.16.0.0"}}
    })
}

#[test]
fn telemetry_accepts_observed_memory_difference() {
    let original = telemetry_packet();
    let mut varied = original.clone();
    varied["cost"]["solver"]["memory_mb"] = json!(17.67);
    assert_eq!(stable(original.clone(), "bmc"), stable(varied, "bmc"));
    assert_eq!(original["cost"]["solver"]["memory_mb"], 17.68);
}

#[test]
fn telemetry_in_an_induction_proof_excludes_only_memory() {
    let mut proved = telemetry_packet();
    proved["engine"] = json!("induction");
    proved["result"] = json!("proved");
    proved["completeness"] = json!("unbounded");
    proved["depth"] = Value::Null;
    proved["checked_to_depth"] = json!(8);
    let mut varied = proved.clone();
    varied["cost"]["solver"]["memory_mb"] = json!(17.67);
    assert_eq!(
        stable(proved.clone(), "induction"),
        stable(varied.clone(), "induction")
    );
    // The same difference still counts outside the observed class.
    let mut violated = proved.clone();
    violated["result"] = json!("violated");
    let mut violated_varied = varied;
    violated_varied["result"] = json!("violated");
    assert_ne!(
        stable(violated, "induction"),
        stable(violated_varied, "induction")
    );
    // Other solver statistics stay compared, and memory must still be present.
    let mut other = proved.clone();
    other["cost"]["solver"]["conflicts"] = json!(3);
    assert_ne!(
        stable(proved.clone(), "induction"),
        stable(other, "induction")
    );
    let mut missing = proved;
    missing["cost"]["solver"]
        .as_object_mut()
        .unwrap()
        .remove("memory_mb");
    assert!(std::panic::catch_unwind(|| stable(missing, "induction")).is_err());
}

#[test]
fn telemetry_rejects_missing_memory_on_either_or_both_sides() {
    for (missing_left, missing_right) in [(true, false), (false, true), (true, true)] {
        let mut left = telemetry_packet();
        let mut right = left.clone();
        for (packet, missing) in [(&mut left, missing_left), (&mut right, missing_right)] {
            if missing {
                packet["cost"]["solver"]
                    .as_object_mut()
                    .unwrap()
                    .remove("memory_mb");
            }
        }
        assert!(
            std::panic::catch_unwind(|| {
                assert_eq!(stable(left, "bmc"), stable(right, "bmc"));
            })
            .is_err()
        );
    }
}

#[test]
fn telemetry_rejects_invalid_memory() {
    for invalid in [
        Value::Null,
        json!("17.68"),
        json!(true),
        json!([]),
        json!({}),
        json!(-1),
    ] {
        let mut packet = telemetry_packet();
        packet["cost"]["solver"]["memory_mb"] = invalid;
        assert!(std::panic::catch_unwind(|| stable(packet, "bmc")).is_err());
    }
    // JSON cannot encode nonfinite numbers; exercise the same numeric validator
    // directly rather than converting NaN/infinity into JSON null.
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(std::panic::catch_unwind(|| validate_memory(Some(invalid))).is_err());
    }
}

#[test]
fn telemetry_keeps_semantic_evidence_and_counters_exact() {
    let mut base = telemetry_packet();
    base["loc"] = json!({"line":7,"column":3});
    base["trace"] = json!([{"step":1,"state":{"r":[[false,true]],"memory_mb":7},
        "action":{"name":"link","params":{},"loc":{"line":6,"column":3}}}]);
    base["cti"] = json!({"states":[{"step":0,"state":{"r":[[true,false]]}}],"violated_at":1});
    for (path, replacement) in [
        ("/result", json!("violated")),
        ("/loc/line", json!(8)),
        ("/loc/column", json!(4)),
        ("/trace/0/state/r", json!([[true, false]])),
        ("/trace/0/state/memory_mb", json!(8)),
        ("/trace/0/action/loc/line", json!(9)),
        ("/cti/states/0/state/r", json!([])),
        ("/cti/violated_at", json!(2)),
        ("/depth", json!(3)),
        ("/checked_to_depth", json!(3)),
        ("/completeness", json!("unbounded")),
        ("/action_coverage/link", json!(false)),
        ("/cost/solver/checks", json!(18)),
        ("/cost/solver/conflicts", json!(3)),
        ("/cost/solver/decisions", json!(2)),
        ("/cost/solver/propagations", json!(11)),
        ("/cost/properties/0/checks", json!(2)),
        ("/versions/verifier/version", json!("changed")),
        ("/versions/core/version", json!("changed")),
        ("/versions/solver/version", json!("changed")),
    ] {
        let mut changed = base.clone();
        *changed.pointer_mut(path).unwrap() = replacement;
        assert_ne!(
            stable(base.clone(), "bmc"),
            stable(changed, "bmc"),
            "{path}"
        );
    }
    let mut extra = base.clone();
    extra["unexpected"] = json!(true);
    assert_ne!(stable(base.clone(), "bmc"), stable(extra, "bmc"));
    assert_ne!((0, stable(base.clone(), "bmc")), (1, stable(base, "bmc")));
}

#[test]
fn telemetry_keeps_unobserved_memory_exact() {
    for (mode, path, replacement) in [
        ("check", "/result", json!("ok")),
        ("explicit", "/result", json!("proved")),
        ("induction", "/result", json!("proved")),
        ("induction", "/result", json!("unknown_cti")),
        ("bmc", "/result", json!("unknown_cti")),
        ("bmc", "/result", json!("violated")),
        ("bmc", "/result", json!("unsupported")),
        ("bmc", "/versions/solver/backend", json!("wasm-z3")),
        ("bmc", "/versions/solver/backend", Value::Null),
        ("bmc", "/cost/solver/checks", json!(0)),
        ("bmc", "/completeness", json!("unbounded")),
        ("bmc", "/depth", json!(3)),
        ("bmc", "/checked_to_depth", json!(3)),
    ] {
        let mut left = telemetry_packet();
        *left.pointer_mut(path).unwrap() = replacement;
        let mut right = left.clone();
        right["cost"]["solver"]["memory_mb"] = json!(17.67);
        assert_ne!(stable(left, mode), stable(right, mode), "{mode} {path}");
    }
}

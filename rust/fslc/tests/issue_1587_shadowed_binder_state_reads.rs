// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1587: locally bound x is not a read of state x in any consumer.

use std::collections::BTreeSet;
use std::process::Command;

use serde_json::{Value, json};

#[allow(dead_code)]
#[path = "support/compose_scratch.rs"]
mod fixtures;

const SUM: &str = "sum(x in 0..1 of x) == 1";
const NESTED: &str = "sum(x in 0..1 of count(y in 0..x)) == 3";

fn source(state: &str, action: &str, property: &str) -> String {
    format!(
        "spec Reads {{\n state {{ {state}: Bool = false, y: Bool = false }}\n \
         action go() {{ {action} }}\n invariant Local {{ {property} }}\n}}"
    )
}

fn run(source: &str, args: &[&str]) -> (Value, i32) {
    let dir = fixtures::scratch("fslc-1587", &[("model.fsl", source)]);
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run native fslc");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).expect("CLI JSON");
    let exit = output.status.code().expect("exit code");
    let observed = json!({"result": value["result"], "kind": value["kind"],
        "message": value["message"], "warnings": value["warnings"],
        "undecided": value["undecided"]});
    eprintln!("OBS {args:?} exit={exit} output={observed}");
    (value, exit)
}

fn verify(source: &str, engine: &str, severity: &str) -> (Value, i32) {
    run(
        source,
        &[
            "verify",
            "model.fsl",
            "--engine",
            engine,
            "--depth",
            "2",
            "--no-cache",
            "--vacuity",
            severity,
            "--deadlock",
            "ignore",
        ],
    )
}

fn kinds(value: &Value) -> BTreeSet<&str> {
    value["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .map(|warning| warning["kind"].as_str().expect("warning kind"))
        .collect()
}

fn clean(actual: &(Value, i32), result: &str) {
    assert_eq!(actual.1, 0, "{actual:?}");
    assert_eq!(actual.0["result"], result, "{actual:?}");
}

#[test]
fn nested_check_and_explicit_match_renamed_control() {
    let forms = ["w", "x"].map(|state| source(state, "", NESTED));
    for form in &forms {
        clean(&run(form, &["check", "model.fsl"]), "ok");
    }
    for form in &forms {
        clean(&verify(form, "explicit", "warn"), "proved");
    }
}

#[test]
fn nested_symbolic_results_are_name_independent() {
    // #1480 still limits symbolic nested bounds; do not pin its error as correct.
    for engine in ["bmc", "induction"] {
        let [control, shadow] =
            ["w", "x"].map(|state| verify(&source(state, "", NESTED), engine, "warn"));
        let envelope = |(v, exit): &(Value, i32)| json!({"exit": exit, "kind": v["kind"], "message": v["message"]});
        assert_eq!(envelope(&shadow), envelope(&control));
    }
}

fn no_spurious_frozen(engine: &str, severity: &str) {
    let [control, shadow] =
        ["w", "x"].map(|state| verify(&source(state, "y = not y", SUM), engine, severity));
    assert_eq!(control.1, 0, "{control:?}");
    assert!(!kinds(&control.0).contains("tautology_over_frozen"));
    assert_eq!(shadow.1, 0, "{shadow:?}");
    assert_eq!(kinds(&shadow.0), kinds(&control.0));
    assert!(!kinds(&shadow.0).contains("tautology_over_frozen"));
}

macro_rules! engines {
    ($($engine:ident),+ $(,)?) => {$(
        mod $engine {
            use super::*;
            #[test]
            fn binder_has_no_frozen_warning() { no_spurious_frozen(stringify!($engine), "warn"); }
            #[test]
            fn binder_passes_vacuity_error() { no_spurious_frozen(stringify!($engine), "error"); }
            #[test]
            fn real_frozen_read_is_preserved() {
                for state in ["w", "x"] {
                    for frozen in [true, false] {
                        let action = if frozen { "y = not y".to_owned() }
                            else { format!("y = not y {state} = not {state}") };
                        let form = source(state, &action, &format!("{state} or not {state}"));
                        let actual = verify(&form, stringify!($engine), "warn");
                        assert_eq!(actual.1, 0, "{actual:?}");
                        assert_eq!(kinds(&actual.0).contains("tautology_over_frozen"), frozen);
                    }
                }
            }
        }
    )+};
}
engines!(bmc, induction, explicit);

#[test]
fn explain_trace_blames_only_the_free_state_read() {
    let [control, shadow] = ["w", "x"].map(|state| {
        // Removing the false guard retains both effects in the violating trace.
        let form = source(
            state,
            &format!("requires false {state} = true y = true"),
            &format!("({SUM}) and not y"),
        );
        let actual = run(&form, &["explain", "model.fsl", "--depth", "2"]);
        clean(&actual, "explained");
        let cases = actual.0["counterfactuals"]
            .as_array()
            .expect("counterfactuals");
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0]["weakening"]["op"], "requires-removal");
        assert_eq!(cases[0]["violation"]["violation_kind"], "invariant");
        assert_eq!(cases[0]["violation"]["invariant"], "Local");
        let trace = cases[0]["trace"].as_array().expect("violating trace");
        assert_eq!(trace.len(), 2);
        assert_eq!(trace[1]["state"]["y"], true);
        trace[1]["blame"].clone()
    });
    assert_eq!(control["effects"].as_array().expect("effects").len(), 1);
    assert_eq!(control["effects"][0]["target"], "y");
    // Compare complete blame, including guards, text and locations; no exclusions.
    assert_eq!(shadow, control);
}

#[test]
fn document_undecided_roots_match_renamed_control() {
    let [control, shadow] = ["w", "x"].map(|state| {
        // One requirement per root exposes root intersections as requirement_ids.
        let form = format!(
            "spec Roots {{\n state {{ {state}: Bool = false, y: Bool = false }}\n \
             action go() {{ y = not y }}\n \
             @requirement(\"REQ-X\", \"state root witness\")\n \
             invariant State {{ not {state} }}\n \
             @requirement(\"REQ-Y\", \"real state read\")\n \
             invariant Real {{ y or not y }}\n \
             @undecided(\"pending\")\n \
             invariant Local {{ ({SUM}) and (y or not y) }}\n}}"
        );
        let actual = run(&form, &["document", "claims", "model.fsl"]);
        clean(&actual, "requirement_claims");
        let records = actual.0["undecided"].as_array().expect("undecided records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["target"], "invariant:Local");
        actual.0["undecided"].clone()
    });
    assert_eq!(control[0]["requirement_ids"], json!(["REQ-Y"]));
    // Full public records, not digests or a projection that could hide an extra root.
    assert_eq!(shadow, control);
}

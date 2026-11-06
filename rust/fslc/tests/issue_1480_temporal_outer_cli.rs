// SPDX-License-Identifier: Apache-2.0
//! The 29 temporal-outer sources have executable four-route verdict contracts.
//! Cost packets are retained intact; this is not a telemetry-equivalence test.
#[path = "support/scratch_identity.rs"]
mod scratch_identity;
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};

const STATIC: &str = "invalid static integer expression";
// #1480: a wrapper bound no engine executes is refused at `check`, with its location.
const NOT_CONST: &str = "'n' is not an integer const";
const EXPLICIT: &str = "the explicit engine does not support leadsTo properties; use --engine bmc or exclude the leadsTo property";
const FILTER: &str = "leadsTo wrapper binders do not support where filters";
const COLLECTION: &str = "leadsTo wrapper binders do not support collection domains";
const TYPE_ERROR: &str =
    "invalid model expression: expression of type Bool is not assignable to Int";

#[derive(Clone, Copy)]
enum Outcome {
    Response,
    Witness(&'static str),
    Shared(&'static str, u32, u32, bool),
}
struct Case {
    id: &'static str,
    source: &'static str,
    property_line: u32,
    outcome: Outcome,
}
macro_rules! case {
    ($id:literal, $line:literal, $outcome:expr) => {
        Case {
            id: $id,
            source: include_str!(concat!("fixtures/issue_1480_temporal_outer/", $id, ".fsl")),
            property_line: $line,
            outcome: $outcome,
        }
    };
}
const CASES: &[Case] = &[
    case!("triangle-witness", 5, Outcome::Witness(r#"{"i":1,"j":1}"#)),
    case!("triangle-response", 5, Outcome::Response),
    case!(
        "bool-conditioned-witness",
        5,
        Outcome::Witness(r#"{"b":true,"i":1}"#)
    ),
    case!("bool-conditioned-response", 5, Outcome::Response),
    case!(
        "euclidean-witness",
        5,
        Outcome::Witness(r#"{"i":2,"j":-2}"#)
    ),
    case!("euclidean-response", 5, Outcome::Response),
    case!(
        "enum-bool-witness",
        6,
        Outcome::Witness(r#"{"tag":"Blue","b":true}"#)
    ),
    case!("enum-bool-response", 6, Outcome::Response),
    case!("constant-witness", 6, Outcome::Witness(r#"{"i":1}"#)),
    case!("constant-response", 6, Outcome::Response),
    case!("empty-dependent", 5, Outcome::Response),
    case!("empty-dependent-overflow", 5, Outcome::Response),
    case!(
        "empty-static-overflow",
        5,
        Outcome::Shared(STATIC, 5, 3, false)
    ),
    case!("overflow-low", 5, Outcome::Shared(STATIC, 5, 3, false)),
    case!("overflow-high", 5, Outcome::Shared(STATIC, 5, 3, false)),
    case!("filter-range", 5, Outcome::Shared(FILTER, 5, 3, false)),
    case!(
        "filter-empty-typed",
        5,
        Outcome::Shared(FILTER, 5, 3, false)
    ),
    case!(
        "collection-set",
        5,
        Outcome::Shared(COLLECTION, 5, 3, false)
    ),
    case!(
        "collection-empty-seq",
        5,
        Outcome::Shared(COLLECTION, 5, 3, false)
    ),
    case!("free-state", 5, Outcome::Shared(NOT_CONST, 5, 3, false)),
    case!(
        "const-state-collision",
        6,
        Outcome::Shared("binder bound is not a static integer", 6, 3, false)
    ),
    case!(
        "const-outer-collision",
        6,
        Outcome::Shared(
            "invalid model expression: local binding 'i' shadows a constant of the same name; rename the local binding",
            6,
            3,
            true
        )
    ),
    case!("shadow-own-source", 5, Outcome::Witness(r#"{"i":1}"#)),
    case!(
        "invalid-then-arm",
        5,
        // The base binary also reports the then-arm at the enclosing leadsTo.
        Outcome::Shared(TYPE_ERROR, 5, 3, true)
    ),
    case!(
        "invalid-else-arm",
        5,
        Outcome::Shared(TYPE_ERROR, 7, 43, true)
    ),
    case!(
        "error-priority-low-first",
        5,
        Outcome::Shared(TYPE_ERROR, 6, 65, true)
    ),
    case!(
        "error-priority-high-first",
        5,
        Outcome::Shared(TYPE_ERROR, 6, 38, true)
    ),
    case!(
        "unbounded-typed",
        5,
        Outcome::Shared("type 'Int' is not a finite scalar", 5, 3, false)
    ),
    case!(
        "struct-typed",
        6,
        Outcome::Shared(
            "invalid model expression: type 'Cell' is not a finite scalar",
            6,
            3,
            true
        )
    ),
];
fn versions() -> Value {
    json!({"verifier":{"name":"fslc-rust","version":env!("CARGO_PKG_VERSION")},
        "core":{"name":"fsl-core","version":env!("CARGO_PKG_VERSION")},
        "solver":{"name":"z3","backend":"native-z3","version":"Z3 4.16.0.0"}})
}
fn error_packet(message: &str) -> Value {
    json!({"fsl":"1.0","result":"error","kind":"semantics","message":message,"versions":versions()})
}
fn bounded_property(packet: &Value) -> bool {
    packet.get("leads_to") == Some(&json!({"Responds":{"checked_to_depth":3,"within":1}}))
}
fn witness(case: &Case, packet: &Value, bindings: &str) -> bool {
    let bindings: Value = serde_json::from_str(bindings).unwrap();
    packet["result"] == "violated"
        && packet["violation_kind"] == "leadsTo"
        && packet["invariant"] == "Responds"
        && packet["loc"] == json!({"line":case.property_line,"column":3})
        && packet["bindings"] == bindings
        && packet["pending_since"] == 0
        && packet["deadline"] == 1
        && packet["within"] == 1
        && packet["stutter"] == false
        && packet.get("loop_start").is_none()
        && packet["hint"]
            == "leadsTo deadline missed: P holds at step 0, but Q does not hold within 1 step(s)"
        && packet["trace_type"] == "leadsTo"
        && packet["completeness"] == "bounded"
        && packet["checked_to_depth"] == 3
        && packet["trace"]
            == json!([
                {"step":0,"state":{"tick":false}},
                {"step":1,"state":{"tick":false},"action":{"name":"idle","params":{},"loc":{"line":case.property_line-1,"column":3}},"changes":{}}
            ])
}
fn contract(case: &Case, route: &str, exit: Option<i32>, packet: &Value) -> bool {
    if packet["fsl"] != "1.0" || packet["versions"] != versions() {
        return false;
    }
    if let Outcome::Shared(message, line, column, origin) = case.outcome {
        let rendered = if origin {
            format!("{message} at {line}:{column}")
        } else {
            message.into()
        };
        let mut expected = error_packet(&rendered);
        expected["loc"] = json!({"line":line,"column":column});
        return exit == Some(2) && packet == &expected;
    }
    if route == "explicit" {
        return exit == Some(2) && packet == &error_packet(EXPLICIT);
    }
    if route == "check" {
        return exit == Some(0) && packet["result"] == "ok" && packet["spec"] == "TemporalOuter";
    }
    match case.outcome {
        Outcome::Witness(bindings) => {
            exit == Some(1) && packet["spec"] == "TemporalOuter" && witness(case, packet, bindings)
        }
        Outcome::Response => {
            exit == Some(0)
                && packet["spec"] == "TemporalOuter"
                && packet["result"]
                    == if route == "induction" {
                        "proved"
                    } else {
                        "verified"
                    }
                && packet["checked_to_depth"] == 3
                && bounded_property(packet)
                && packet["invariants_checked"] == json!([])
                && packet["transitions_checked"] == json!([])
                && packet["reachables"] == json!({})
                && packet["action_coverage"] == json!({"idle":true})
                && if route == "induction" {
                    packet["engine"] == "induction"
                        && packet["base_depth"] == 3
                        && packet["k_used"] == json!({})
                        && packet["completeness"] == "unbounded"
                        && packet["note"]
                            == "invariants proved for all depths; leadsTo checked to depth 3 only"
                } else {
                    packet["depth"] == 3
                        && packet["completeness"] == "bounded"
                        && packet["deadlock"] == json!({"found":false})
                        && packet["note"] == "bounded verification: no violation within depth 3"
                }
        }
        Outcome::Shared(..) => unreachable!(),
    }
}
fn observe(directory: &Path, route: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    if route == "check" {
        command.args(["check", "model.fsl"]);
    } else {
        command.args([
            "verify",
            "model.fsl",
            "--engine",
            route,
            "--depth",
            "3",
            "--no-cache",
        ]);
    }
    let output = command
        .current_dir(directory)
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .unwrap();
    std::fs::write(
        directory.join(format!("{route}.stdout.json")),
        &output.stdout,
    )
    .unwrap();
    std::fs::write(
        directory.join(format!("{route}.stderr.txt")),
        &output.stderr,
    )
    .unwrap();
    eprintln!(
        "TEMPORAL_RAW cwd={} route={route} exit={:?} stdout={} stderr={}",
        directory.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
#[test]
fn all_temporal_outer_fixtures_keep_four_route_contracts() {
    assert_eq!(CASES.len(), 29);
    let root =
        scratch_identity::unique(Path::new(env!("CARGO_TARGET_TMPDIR")).join("1480-temporal-cli"));
    std::fs::create_dir_all(&root).unwrap();
    // Collect every route before asserting: a failure cannot silently shorten the roster.
    let mut observations = vec![];
    for repeat in 0..2 {
        for case in CASES {
            let directory = root.join(format!("{repeat}-{}", case.id));
            std::fs::create_dir(&directory).unwrap();
            std::fs::write(directory.join("model.fsl"), case.source).unwrap();
            for route in ["check", "bmc", "induction", "explicit"] {
                observations.push((case, route, observe(&directory, route)));
            }
        }
    }
    assert_eq!(observations.len(), 232);
    let mut failures = vec![];
    for (case, route, output) in observations {
        let parsed = serde_json::from_slice::<Value>(&output.stdout);
        let good = parsed
            .as_ref()
            .is_ok_and(|packet| contract(case, route, output.status.code(), packet));
        if !good || !output.stderr.is_empty() {
            failures.push(format!(
                "{} {route}: exit={:?} packet={parsed:#?} stderr={:?}",
                case.id,
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        if good
            && route != "check"
            && route != "explicit"
            && matches!(case.outcome, Outcome::Witness(_))
        {
            let packet = parsed.as_ref().unwrap();
            let mut wrong_binding = packet.clone();
            wrong_binding["bindings"] = json!({"lost_candidate":0});
            assert!(!contract(case, route, output.status.code(), &wrong_binding));
            let mut wrong_deadline = packet.clone();
            wrong_deadline["deadline"] = json!(2);
            assert!(!contract(
                case,
                route,
                output.status.code(),
                &wrong_deadline
            ));
        }
        if let Ok(mut wrong) = parsed {
            // Wrong verdict must be rejected even when all actual payload fields remain.
            wrong["result"] = json!("not-the-contract-verdict");
            assert!(!contract(case, route, output.status.code(), &wrong));
        }
    }
    assert!(
        failures.is_empty(),
        "{}\nraw packets: {}",
        failures.join("\n"),
        root.display()
    );
}
#[test]
fn bounded_assurance_oracle_rejects_promotion_or_missing_deadline_metadata() {
    let mut packet = json!({"leads_to":{"Responds":{"checked_to_depth":3,"within":1}}});
    assert!(bounded_property(&packet));
    packet["leads_to"]["Responds"]["proved"] = json!(true);
    assert!(!bounded_property(&packet));
    packet = json!({"leads_to":{"Responds":{"checked_to_depth":3}}});
    assert!(!bounded_property(&packet));
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1451: init read order matches the calibrated concrete root-coverage oracle.

#[path = "support/compose_scratch.rs"]
#[allow(dead_code)]
mod compose_scratch;
#[path = "support/init_read_before_assign.rs"]
mod forms;

use forms::Expected;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

fn observe(dir: &Path, file: &str, consumer: &str) -> (Value, i32) {
    let binary = std::env::var_os("FSL_MATRIX_BINARY")
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_fslc")), PathBuf::from);
    let mut args = if consumer == "check" {
        vec!["check", file]
    } else {
        vec!["verify", file, "--depth", "4", "--no-cache"]
    };
    if !matches!(consumer, "check" | "default") {
        args.extend(["--engine", consumer]);
    }
    let output = Command::new(std::fs::canonicalize(binary).expect("selected native"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("native CLI");
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    (
        serde_json::from_slice(&output.stdout).expect("native JSON"),
        output.status.code().expect("exit"),
    )
}

fn root(name: &str, composed: bool) -> String {
    if composed {
        format!("part__{name}")
    } else {
        name.into()
    }
}

fn error(actual: &(Value, i32), message: Option<&str>) -> bool {
    actual.1 == 2
        && actual.0["result"] == "error"
        && actual.0["kind"] == "semantics"
        && message.is_none_or(|message| actual.0["message"] == message)
}

fn accepted(actual: &(Value, i32), consumer: &str) -> bool {
    // Independent verdict contracts: proof depth/assurance/cost legitimately
    // differ between engines. Error-envelope agreement below is full equality.
    let result = match consumer {
        "check" => "ok",
        "explicit" | "induction" => "proved",
        _ => "verified",
    };
    actual.1 == 0
        && actual.0["result"] == result
        && [
            "kind",
            "message",
            "violation_kind",
            "invariant",
            "action",
            "last_action",
            "violation",
        ]
        .iter()
        .all(|field| actual.0.get(*field).is_none())
}

fn point(case: &forms::Case) -> Value {
    let Some(marker) = &case.marker else {
        return Value::Null;
    };
    let before = &case.source[..case.source.find(marker).expect("authored read marker")];
    json!({"line":before.bytes().filter(|b|*b==b'\n').count()+1,"column":before.rsplit('\n').next().unwrap().chars().count()+1})
}

fn located(actual: &Value, expected: &Value) -> bool {
    actual.get("loc") == Some(expected)
        || actual["message"].as_str().is_some_and(|m| {
            location_suffixes(expected)
                .iter()
                .any(|suffix| m.ends_with(suffix))
        })
}

fn location_suffixes(expected: &Value) -> [String; 2] {
    // CoreError's existing Display forms, not arbitrary message substrings.
    [
        format!(" at {}:{}", expected["line"], expected["column"]),
        format!(" at a.fsl:{}:{}", expected["line"], expected["column"]),
    ]
}

fn matches_oracle(
    case: &forms::Case,
    actual: &(Value, i32),
    consumer: &str,
    composed: bool,
) -> bool {
    match case.expected {
        Expected::Before(name) => {
            let message = format!(
                "init references state variable '{}' before it is assigned",
                root(name, composed)
            );
            error(actual, Some(&message))
                || location_suffixes(&point(case))
                    .iter()
                    .any(|suffix| error(actual, Some(&format!("{message}{suffix}"))))
        }
        Expected::Free {
            root: name,
            partial,
        } if consumer == "explicit" => error(
            actual,
            Some(&format!(
                "init does not assign state variable(s): {}{}",
                root(name, composed),
                if partial {
                    " (partial component initialization is rejected by the explicit engine)"
                } else {
                    ""
                }
            )),
        ),
        Expected::Accepted | Expected::Free { .. } => accepted(actual, consumer),
        Expected::StaticRange => error(
            actual,
            Some(&format!(
                "init forall range bounds must be compile-time constants; state variable '{}' is not allowed",
                root("n", composed)
            )),
        ),
        Expected::ExistingKeyRefusal => error(actual, None),
    }
}

#[test]
#[allow(clippy::too_many_lines)] // Collect every calibrated consumer before the intended red assertion.
fn init_reads_preserve_the_explicit_assignment_contract() {
    let cases = forms::cases();
    assert_eq!(
        cases.iter().map(|c| &c.id).collect::<BTreeSet<_>>().len(),
        35
    );
    let report = compose_scratch::scratch("issue-1451-init-report", &[]);
    let mut rows = Vec::new();
    let mut inputs = Vec::new();
    let mut groups: BTreeMap<String, [usize; 2]> = BTreeMap::new();
    let mut oracle_failures = 0;
    let mut missing_check_refusals = 0;
    let mut detector_failures = 0;
    let mut control_failures = 0;
    for case in cases {
        let expected_point = point(&case);
        inputs.push(json!({"id":case.id,"family":case.family,"class":case.expected.name(),"source":case.source,"expected_location":expected_point}));
        let dir = compose_scratch::scratch(
            "issue-1451-init-case",
            &[
                ("a.fsl", &case.source),
                ("c.fsl", "compose C {\n  use A as part from \"a.fsl\"\n}\n"),
            ],
        );
        for (route, file, composed) in [("direct", "a.fsl", false), ("compose", "c.fsl", true)] {
            // Explicit is measured first but must match literal calibrated data;
            // it is never used to invent the expected rejection during a run.
            let explicit = observe(&dir, file, "explicit");
            let oracle_ok = matches_oracle(&case, &explicit, "explicit", composed);
            oracle_failures += usize::from(!oracle_ok);
            let consumers: &[&str] = match case.expected {
                // This is an existing EXPLICIT domain rule, not a #1451 source
                // acceptance/removal assertion. Source check is retained in calibration data.
                Expected::StaticRange => &["explicit"],
                Expected::ExistingKeyRefusal => &["explicit", "check"],
                _ => &["explicit", "check", "bmc", "induction", "default"],
            };
            let checked = if consumers.contains(&"check") {
                Some(observe(&dir, file, "check"))
            } else {
                None
            };
            for &consumer in consumers {
                let actual = match consumer {
                    "explicit" => explicit.clone(),
                    "check" => checked.clone().unwrap(),
                    _ => observe(&dir, file, consumer),
                };
                let before = matches!(case.expected, Expected::Before(_));
                let mut ok = matches_oracle(&case, &actual, consumer, composed);
                if before && consumer == "check" {
                    missing_check_refusals += usize::from(actual.1 == 0);
                    ok = ok && located(&actual.0, &expected_point);
                }
                // Once source check refuses, every verify entry must expose the
                // SAME complete error envelope, not a solver judgment or drift.
                let agreement = if before
                    && consumer != "check"
                    && checked.as_ref().is_some_and(|c| c.1 == 2)
                {
                    let same = checked.as_ref() == Some(&actual);
                    ok = ok && same;
                    Some(same)
                } else {
                    None
                };
                if !ok {
                    if before {
                        detector_failures += 1;
                    } else {
                        control_failures += 1;
                    }
                }
                let group = groups
                    .entry(format!(
                        "{}|{}|{route}|{consumer}",
                        case.family,
                        case.expected.name()
                    ))
                    .or_default();
                group[0] += 1;
                group[1] += usize::from(!ok);
                rows.push(json!({"id":case.id,"family":case.family,"class":case.expected.name(),"route":route,"consumer":consumer,"oracle_ok":oracle_ok,"ok":ok,"check_error_agreement":agreement,"expected_location":expected_point,"output":actual.0,"exit":actual.1}));
            }
        }
    }
    std::fs::write(report.join("report.json"),serde_json::to_vec(&json!({"inputs":inputs,"observations":rows,"groups":groups,"inapplicable":[],"oracle_failures":oracle_failures,"missing_check_refusals":missing_check_refusals,"detector_failures":detector_failures,"control_failures":control_failures})).unwrap()).unwrap();
    eprintln!(
        "INIT_READ_RESULT cases=35 N=0 observations={} oracle_failures={oracle_failures} missing_check_refusals={missing_check_refusals} detector_failures={detector_failures} control_failures={control_failures} report={}",
        rows.len(),
        report.display()
    );
    assert_eq!(
        oracle_failures, 0,
        "explicit calibration changed: stop before revising expectations"
    );
    assert_eq!(
        control_failures, 0,
        "accepted/free-init/existing-refusal control changed"
    );
    assert_eq!(
        detector_failures, 0,
        "check or verification disagrees with explicit init-read refusal"
    );
}

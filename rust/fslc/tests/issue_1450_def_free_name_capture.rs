// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1450: preserve def-body free globals rather than capture them at a call.

#[path = "support/compose_scratch.rs"]
#[allow(dead_code)]
mod compose_scratch;
#[path = "support/def_free_capture.rs"]
mod forms;

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn invoke(dir: &Path, args: &[&str]) -> (Value, i32) {
    let binary = std::env::var_os("FSL_MATRIX_BINARY")
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_fslc")), PathBuf::from);
    let output = Command::new(std::fs::canonicalize(binary).expect("selected native binary"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("native fslc");
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    (
        serde_json::from_slice(&output.stdout).expect("native JSON"),
        output.status.code().expect("exit"),
    )
}

fn point(source: &str, marker: &str) -> Value {
    let before = &source[..source.find(marker).expect("authored marker")];
    json!({"line":before.bytes().filter(|b| *b==b'\n').count()+1,"column":before.rsplit('\n').next().unwrap().chars().count()+1})
}

fn located_at(output: &Value, expected: &Value, component: bool) -> bool {
    // Existing source validation can preserve the original structured location
    // without wrapping; def-expansion CoreErrors also have a text-only form.
    if output.get("loc").is_some_and(|loc| loc == expected) {
        return true;
    }
    let embedded = component
        && output["message"].as_str().is_some_and(|m| {
            m.contains(&format!(
                "a.fsl:{}:{}",
                expected["line"], expected["column"]
            ))
        });
    embedded
        || output["message"].as_str().is_some_and(|m| {
            m.ends_with(&format!(" at {}:{}", expected["line"], expected["column"]))
        })
}

fn refused(observation: &(Value, i32)) -> bool {
    observation.1 == 2 && observation.0["result"] == "error" && observation.0["kind"] == "semantics"
}

#[test]
#[allow(clippy::too_many_lines)] // Preserve every observation before failing the calibrated oracle.
fn caller_bindings_do_not_capture_definition_free_names() {
    let (cases, excluded) = forms::cases();
    let intrinsic_cases = cases.iter().filter(|case| case.intrinsic()).count();
    assert_eq!(intrinsic_cases, 25);
    let mut inapplicable_counts = BTreeMap::new();
    for entry in &excluded {
        let reason = entry.split_once('\t').expect("applicability reason").1;
        *inapplicable_counts
            .entry(reason.to_owned())
            .or_insert(0usize) += 1;
    }
    let report = compose_scratch::scratch("issue-1450-free-report", &[]);
    let mut rows = Vec::new();
    let mut groups: BTreeMap<String, [usize; 4]> = BTreeMap::new();
    let mut controls = 0;
    let mut preservation = 0;
    let mut captures = 0;
    for case in cases {
        for fresh in [false, true] {
            let sources = case.render(fresh);
            let dir = compose_scratch::scratch(
                "issue-1450-free-case",
                &[
                    ("direct.fsl", &sources.direct),
                    ("a.fsl", &sources.component),
                    ("c.fsl", &sources.composed),
                ],
            );
            for (route, file, source) in [
                ("direct", "direct.fsl", &sources.direct),
                (
                    "compose",
                    "c.fsl",
                    if case.owner == forms::Owner::Component {
                        &sources.component
                    } else {
                        &sources.composed
                    },
                ),
            ] {
                let actual = invoke(&dir, &["check", file]);
                let wrapped = route == "compose" && case.owner == forms::Owner::Component;
                let call = point(source, "p(false)");
                let intrinsic = case.intrinsic() && !fresh;
                let expected: Vec<_> = if intrinsic {
                    case.intrinsic_markers()
                        .iter()
                        .map(|m| point(source, m))
                        .collect()
                } else {
                    vec![call]
                };
                let ok = if fresh {
                    actual.1 == 0 && actual.0["result"] == "ok"
                } else {
                    refused(&actual)
                        && expected.iter().any(|p| located_at(&actual.0, p, wrapped))
                        && (!intrinsic
                            || actual.0["message"].as_str().is_some_and(|m| {
                                m.contains(&format!("pattern binding '{}'", case.pattern_name()))
                                    && m.contains(case.pattern_rejection())
                            }))
                };
                let role = if fresh {
                    "renamed-control"
                } else if intrinsic {
                    "existing-pattern-refusal"
                } else {
                    "free-name-capture"
                };
                let group = groups.entry(case.id()).or_default();
                group[if fresh {
                    0
                } else if intrinsic {
                    1
                } else {
                    2
                }] += 1;
                group[3] += usize::from(!ok);
                if !ok {
                    if fresh {
                        controls += 1;
                    } else if intrinsic {
                        preservation += 1;
                    } else {
                        captures += 1;
                    }
                }
                rows.push(json!({"id":case.id(),"role":role,"route":route,"fresh":fresh,"expected_locations":expected,"accepted":actual.1==0,"ok":ok,"output":actual.0,"exit":actual.1,"directory":dir}));
            }
        }
    }
    std::fs::write(report.join("report.json"),serde_json::to_vec(&json!({"cases":rows,"groups":groups,"inapplicable":excluded,"inapplicable_counts":inapplicable_counts,"intrinsic_cases":intrinsic_cases,"control_failures":controls,"preservation_failures":preservation,"capture_failures":captures})).unwrap()).unwrap();
    eprintln!("DEF_FREE_N {inapplicable_counts:?}");
    eprintln!(
        "DEF_FREE_INVENTORY capture_cases={} existing_pattern_cases={intrinsic_cases} renamed_check_controls={} existing_pattern_routes={}",
        163 - intrinsic_cases,
        163 * 2,
        intrinsic_cases * 2
    );
    eprintln!(
        "DEF_FREE_RESULT cases=163 observations={} N={} control_failures={controls} preservation_failures={preservation} capture_failures={captures} report={}",
        rows.len(),
        excluded.len(),
        report.display()
    );
    assert_eq!(
        controls, 0,
        "renamed source controls invalid; stop before changing expectations"
    );
    assert_eq!(preservation, 0, "existing pattern refusal changed");
    assert_eq!(
        captures, 0,
        "definition free global captured by caller binding"
    );
}

#[test]
fn existing_argument_capture_guard_keeps_its_call_location() {
    let source = "spec S {\n state { flag: Bool } init { flag = false }\n def all(v: Bool) = forall i in 0..1 { v }\n action go() {}\n invariant P { forall i in 0..1 { all(i == 0) } }\n}\n";
    let compose = "compose C {\n  use S as part from \"a.fsl\"\n}\n";
    let plain_compose = source.replacen("spec S", "compose C", 1);
    let dir = compose_scratch::scratch(
        "issue-1450-existing",
        &[
            ("a.fsl", source),
            ("c.fsl", compose),
            ("own.fsl", &plain_compose),
        ],
    );
    let call = point(source, "all(i == 0)");
    let report = compose_scratch::scratch("issue-1450-existing-report", &[]);
    let mut records = Vec::new();
    let mut failures = 0;
    for file in ["a.fsl", "c.fsl", "own.fsl"] {
        let actual = invoke(&dir, &["check", file]);
        let ok = refused(&actual)
            && actual.0["message"]
                .as_str()
                .is_some_and(|m| m.contains("would capture variable 'i'"))
            && located_at(&actual.0, &call, file == "c.fsl");
        failures += usize::from(!ok);
        records.push(json!({"file":file,"ok":ok,"output":actual.0,"exit":actual.1}));
    }
    std::fs::write(
        report.join("report.json"),
        serde_json::to_vec(&records).unwrap(),
    )
    .unwrap();
    eprintln!(
        "DEF_EXISTING_RESULT observations={} failures={failures} report={}",
        records.len(),
        report.display()
    );
    assert_eq!(failures, 0, "existing argument-capture guard changed");
}

#[test]
fn exact_issue_control_is_violated_in_all_three_engines() {
    let fresh = forms::ISSUE.replace("let on = r.on", "let x = r.on");
    let compose = "compose C {\n  use D as part from \"a.fsl\"\n}\n";
    let bad_dir = compose_scratch::scratch(
        "issue-1450-issue-bad",
        &[("a.fsl", forms::ISSUE), ("c.fsl", compose)],
    );
    let dir = compose_scratch::scratch(
        "issue-1450-issue-control",
        &[("a.fsl", &fresh), ("c.fsl", compose)],
    );
    let capture = [
        invoke(&bad_dir, &["check", "a.fsl"]),
        invoke(&bad_dir, &["check", "c.fsl"]),
    ];
    let report = compose_scratch::scratch("issue-1450-issue-report", &[]);
    let mut outcomes = Vec::new();
    let mut control_failures = 0;
    for file in ["a.fsl", "c.fsl"] {
        let check = invoke(&dir, &["check", file]);
        control_failures += usize::from(check.1 != 0);
        outcomes.push(json!({"file":file,"engine":"check","output":check.0,"exit":check.1}));
        for engine in ["bmc", "explicit", "induction"] {
            let actual = invoke(
                &dir,
                &[
                    "verify",
                    file,
                    "--engine",
                    engine,
                    "--depth",
                    "4",
                    "--no-cache",
                ],
            );
            let ok = actual.1 == 1
                && actual.0["result"] == "violated"
                && actual.0["violation_kind"] == "invariant"
                && actual.0["violated_at_step"] == 1
                && actual.0["invariant"] == if file == "a.fsl" { "P" } else { "part.P" };
            control_failures += usize::from(!ok);
            outcomes.push(
                json!({"file":file,"engine":engine,"ok":ok,"output":actual.0,"exit":actual.1}),
            );
        }
    }
    std::fs::write(
        report.join("report.json"),
        serde_json::to_vec(
            &json!({"capture":capture,"controls":outcomes,"control_failures":control_failures}),
        )
        .unwrap(),
    )
    .unwrap();
    eprintln!(
        "DEF_ISSUE_RESULT control_failures={control_failures} capture_exits={:?} report={}",
        capture.each_ref().map(|c| c.1),
        report.display()
    );
    assert_eq!(
        control_failures, 0,
        "issue alpha control must violate in all engines"
    );
    for (index, actual) in capture.iter().enumerate() {
        assert!(
            refused(actual) && located_at(&actual.0, &point(forms::ISSUE, "p(false)"), index == 1),
            "{}",
            actual.0
        );
    }
}

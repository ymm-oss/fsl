// SPDX-License-Identifier: Apache-2.0

#[path = "support/issue_1463_forms.rs"]
mod forms;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use forms::{Outcome, ROUTES};
use serde_json::{Value, json};

fn run(path: &Path, route: &str, extra: &[&str]) -> (i32, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command.env("FSLC_CACHE", "off").env_remove("FSLC_UNSTABLE");
    if matches!(route, "check" | "replay") {
        command.arg(route).arg(path);
    } else {
        command
            .arg("verify")
            .arg(path)
            .args(["--engine", route, "--depth", "4", "--no-cache"]);
    }
    let output = command.args(extra).output().expect("fslc output");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        output.status.code().expect("process exit"),
        serde_json::from_slice(&output.stdout).expect("JSON"),
    )
}

fn agrees(route: &str, expected: Outcome, observed: &(i32, Value)) -> bool {
    let (exit, report) = observed;
    match expected {
        Outcome::Ok => *exit == 0 && report["result"] == "ok",
        Outcome::Safe => {
            *exit == 0
                && report["result"]
                    == if route == "bmc" || route == "auto" {
                        "verified"
                    } else {
                        "proved"
                    }
        }
        Outcome::Violated => *exit == 1 && report["result"] == "violated",
        Outcome::Error => {
            *exit == 2
                && report["result"] == "error"
                && report["message"].as_str().is_some_and(|s| !s.is_empty())
        }
    }
}

fn fixture(index: usize, source: &str, files: &[(String, String)]) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../target/issue-1463-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("worktree-local fixtures");
    let path = dir.join(format!("{index}.fsl"));
    std::fs::write(&path, source).expect("fixture source");
    for (name, source) in files {
        std::fs::write(dir.join(name), source).expect("resolved component");
    }
    path
}

#[test]
fn issue_1463_cli_supported_forms() {
    let mut failures = Vec::new();
    for (index, c) in forms::cases().iter().enumerate() {
        let path = fixture(index, &c.source, &c.files);
        let outputs = ROUTES.map(|route| run(&path, route, &[]));
        for ((route, expected), produced) in ROUTES.iter().zip(c.expected).zip(&outputs) {
            if !agrees(route, expected, produced) {
                failures.push(format!(
                    "{} {route}: expected {expected:?}; produced {} {}",
                    c.id, produced.0, produced.1
                ));
            }
        }
        if c.rejection.is_some() && outputs.iter().all(|o| o.0 == 2) {
            // No projection: all fields of source rejections must agree.
            for output in &outputs {
                forms::assert_rejection(c, &output.1);
                assert_eq!(output, &outputs[0], "{}: full CLI envelope", c.id);
            }
        }
        if let Some(expected) = c.auto {
            let produced = run(&path, "auto", &[]);
            if !agrees("auto", expected, &produced) {
                failures.push(format!(
                    "{} auto: expected {expected:?}; produced {} {}",
                    c.id, produced.0, produced.1
                ));
            } else if expected != Outcome::Error {
                assert_eq!(produced.1["engine"], "bmc", "{}: {}", c.id, produced.1);
                assert_eq!(
                    produced.1["engine_fallback"],
                    json!({"from":"explicit", "kind":"unsupported", "reason":outputs[3].1["message"]}),
                    "{}",
                    c.id
                );
            } else if c.rejection.is_some() && produced != outputs[0] {
                failures.push(format!(
                    "{} auto must fail before routing: expected {:?}; produced {produced:?}",
                    c.id, outputs[0]
                ));
            }
        }
        if c.selectors {
            for extra in [["--property", "Capture"], ["--exclude-property", "Capture"]] {
                let produced = run(&path, "explicit", &extra);
                if c.rejection.is_some() {
                    if produced != outputs[0] {
                        failures.push(format!(
                            "{} {extra:?}: source check bypassed: {}",
                            c.id, produced.1
                        ));
                    }
                } else {
                    let expected = if extra[0] == "--exclude-property" {
                        Outcome::Safe
                    } else {
                        Outcome::Error
                    };
                    assert!(
                        agrees("explicit", expected, &produced),
                        "{} {extra:?}: {produced:?}",
                        c.id
                    );
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failing cells:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn issue_1467_materialization_has_paired_negative_controls() {
    let cases = forms::cases();
    let positives: Vec<_> = cases
        .iter()
        .filter(|c| c.id.starts_with("X1/1467/"))
        .collect();
    assert_eq!(positives.len(), 4);
    let predicates = [
        "not done or Seq { 1 }[0] == 0",
        "not done or some(Set { 1 }) == some(Set { 0 })",
        "not done or Seq { 1 }.head() == 0",
        "not done or Set { 1 } == Set { 0 }",
    ];
    let expected_trace = json!([
        {"step":0,"state":{"done":false}},
        {"step":1,"state":{"done":true},
         "action":{"name":"go","params":{},"loc":{"line":7,"column":3}},
         "changes":{"done":{"from":false,"to":true}},
         "blame":{"guards":[],"effects":[]}}
    ]);
    for (index, (c, predicate)) in positives.iter().zip(predicates).enumerate() {
        assert_eq!(
            c.baseline,
            [Outcome::Ok, Outcome::Error, Outcome::Error, Outcome::Safe]
        );
        assert_eq!(
            c.expected,
            [Outcome::Ok, Outcome::Safe, Outcome::Safe, Outcome::Safe]
        );
        let positive = fixture(cases.len() + 2 * index, &c.source, &[]);
        for route in ["bmc", "induction", "explicit"] {
            let (exit, report) = run(&positive, route, &[]);
            assert_eq!(exit, 0, "{route}: {report}");
            assert_eq!(
                report["result"],
                if route == "bmc" { "verified" } else { "proved" }
            );
            assert_eq!(
                report["completeness"],
                if route == "bmc" {
                    "bounded"
                } else {
                    "unbounded"
                }
            );
        }
        // These are new controls, not rows with invented historical baselines.
        let original = c
            .source
            .lines()
            .find(|line| line.contains("invariant Capture"))
            .unwrap();
        let source = c
            .source
            .replace(original, &format!("  invariant Capture {{ {predicate} }}"));
        let negative = fixture(cases.len() + 2 * index + 1, &source, &[]);
        let (exit, report) = run(&negative, "check", &[]);
        assert_eq!(exit, 0, "{report}");
        assert_eq!(report["result"], "ok");
        for route in ["bmc", "induction", "explicit"] {
            let (exit, report) = run(&negative, route, &[]);
            assert_eq!(exit, 1, "{route}: {report}");
            assert_eq!(report["result"], "violated");
            assert_eq!(report["violation_kind"], "invariant");
            assert_eq!(report["invariant"], "Capture");
            assert_eq!(report["violated_at_step"], 1);
            assert_eq!(report["loc"], json!({"line":8,"column":3}));
            // Full trace agreement only, not equality of engine envelopes/costs.
            assert_eq!(report["trace"], expected_trace, "{route}: {report}");
        }
        let trace = negative.with_extension("trace.json");
        std::fs::write(&trace, r#"[{"action":"go","params":{}}]"#).unwrap();
        let (exit, report) = run(&negative, "replay", &["--trace", trace.to_str().unwrap()]);
        assert_eq!(exit, 1, "{report}");
        assert_eq!(report["result"], "nonconformant");
        assert_eq!(report["violation"]["kind"], "invariant");
        assert_eq!(report["violation"]["name"], "Capture");
        assert_eq!(report["failed_at_event"], 0);
        assert_eq!(report["state_before"], expected_trace[0]["state"]);
        assert_eq!(
            report["violation"]["action"],
            expected_trace[1]["action"]["name"]
        );
    }
}

#[test]
fn issue_1463_inventory_covers_all_contract_rows() {
    let cases = forms::cases();
    let ids: BTreeSet<_> = cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), cases.len(), "duplicate fixture ID");
    let groups: BTreeSet<_> = ids.iter().map(|id| id.split('/').next().unwrap()).collect();
    assert_eq!(
        groups,
        BTreeSet::from([
            "L1", "L2", "L3", "L4", "I1", "I2", "S1", "S2", "S3", "S4", "R1", "R2", "E1", "E2",
            "X1"
        ])
    );
    for c in &cases {
        if c.rejection.is_some() {
            assert_eq!(
                c.baseline[0],
                Outcome::Ok,
                "{}: new rejection must check on base",
                c.id
            );
            assert_eq!(c.expected, [Outcome::Error; 4], "{}", c.id);
            assert!(
                forms::location(c).0 > if c.files.is_empty() { 3 } else { 2 },
                "{}: points at state declaration",
                c.id
            );
        }
    }
}

/// Detector for the newly documented E1 exclusion, not a claim that baseline
/// auto routing was broken. Removing the product documentation makes this red.
#[test]
fn issue_1463_nested_init_exclusion_is_documented() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut missing = Vec::new();
    for (path, words) in [
        (
            "docs/manual/LANGUAGE.md",
            ["nested", "init", "explicit", "auto"],
        ),
        (
            "docs/manual/LANGUAGE.ja.md",
            ["入れ子", "init", "explicit", "auto"],
        ),
        (
            "skills/fsl/references/syntax.md",
            ["nested", "init", "explicit", "auto"],
        ),
    ] {
        let text = std::fs::read_to_string(root.join(path)).expect("language reference");
        if !text
            .split("\n\n")
            .any(|paragraph| words.iter().all(|word| paragraph.contains(word)))
        {
            missing.push(path);
        }
    }
    assert!(
        missing.is_empty(),
        "document nested init as an explicit limitation routed by auto in one paragraph: {missing:?}"
    );
}

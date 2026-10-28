// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_cases.rs"]
pub(crate) mod cases;

use cases::{Results, Scratch, value, without_versions};
use serde_json::{Value, json};
use std::process::Output;

fn run(args: &[String], gate: bool) -> Output {
    cases::invoke(env!("CARGO_BIN_EXE_fslc"), args, &cases::root(), gate)
}

fn lsp_row(results: &mut Results, label: &str) {
    let source = cases::rejected_domain();
    let (diagnostics, model) = fslc_rust::source_diagnostic::diagnostics_with_model(
        &source,
        "source.fsl",
        &fsl_core::FsResolver::new(cases::root()),
        &cases::gated(),
    );
    let codes: Vec<_> = diagnostics.iter().map(|d| d.code.as_str()).collect();
    results.check(
        label,
        (&codes, model.is_none()),
        (["deprecated_domain_enum_union"], true),
        codes == ["deprecated_domain_enum_union"] && model.is_none(),
    );
}

fn repair_accepts(verb: &str, actual: &Value, exit: Option<i32>, expected: &Value) -> bool {
    match verb {
        "lint" => {
            exit == Some(1)
                && actual["result"] == "lint"
                && actual["files"][0]["findings"] == expected["findings"]
        }
        "migrate" => {
            exit == Some(0)
                && actual["result"] == "migrated"
                && actual["files"][0]["findings"] == expected["findings"]
                && actual["files"][0]["edits"]
                    .as_array()
                    .is_some_and(|e| e.len() == 1)
        }
        "fmt" => exit == Some(1) && actual["result"] == "format_check",
        _ => unreachable!(),
    }
}

#[test]
pub(crate) fn every_registered_root_has_the_same_admission_before_dialect_or_sidecars() {
    let mut results = Results::default();
    let mut executed = Vec::new();
    for row in cases::rows().into_iter().filter(|r| r.owner == "native") {
        executed.push(row.key.to_owned());
        if row.role == "lsp" {
            lsp_row(&mut results, row.key);
            continue;
        }
        let scratch = Scratch::new();
        let canonical = format!("fsl 5\n{}", cases::DOMAIN);
        let args = scratch.args(row.argv, &canonical);
        let output = run(&args, true);
        if row.key == "version" {
            // The nonreader is executed; cli_contract_conformance owns its
            // metadata/alias assertions, not this source-admission matrix.
            continue;
        }
        let actual_control = json!([
            output.status.code(),
            value(&output)["result"],
            value(&output)["kind"],
            value(&output)["warnings"],
            scratch.0.join("out").exists()
        ]);
        let expected_control = &row.preserved;
        results.check(
            &format!("preserved/{}", row.key),
            &actual_control,
            expected_control,
            &actual_control == expected_control,
        );
        if row.role == "nonreader" {
            continue;
        }
        let rejected = cases::rejected_domain();
        let scratch = Scratch::new();
        let args = scratch.args(row.argv, &rejected);
        let expected = cases::refusal(
            env!("CARGO_BIN_EXE_fslc"),
            &cases::root(),
            &scratch.0.join("source.fsl"),
        );
        if row.key == "check" {
            continue;
        } // Already exercised as the oracle.
        let output = run(&args, true);
        if row.role == "repair" {
            let actual = value(&output);
            results.check(
                row.key,
                (output.status.code(), &actual),
                &expected,
                repair_accepts(row.key, &actual, output.status.code(), &expected),
            );
        } else {
            results.error_envelope(row.key, &output, &expected, !scratch.0.join("out").exists());
        }
        if row.role == "reader" && row.argv.contains("{out}") {
            let scratch = Scratch::new();
            let mut args = scratch.args(row.argv, &rejected);
            let out = scratch.0.join("out");
            let missing = scratch.0.join("missing-parent/out");
            for arg in &mut args {
                if arg == out.to_str().unwrap() {
                    *arg = missing.to_str().unwrap().to_owned();
                }
            }
            assert!(!missing.parent().unwrap().exists());
            let expected = cases::refusal(
                env!("CARGO_BIN_EXE_fslc"),
                &cases::root(),
                &scratch.0.join("source.fsl"),
            );
            let output = run(&args, true);
            results.error_envelope(
                &format!("priority/output-parent/{}", row.key),
                &output,
                &expected,
                !missing.exists(),
            );
        }
    }
    cases::assignment("native", &executed);
    results.finish();
}

const VERIFY_ROUTES: &[&str] = &[
    "sweep", "chain", "ledger", "html", "mutate", "db check", "ai check",
];

fn chain_expected(scratch: &Scratch, source: &std::path::Path, detail: &Value) -> Value {
    json!({"fsl":"1.0","result":"error","manifest":scratch.0.join("chain.toml"),"keep_going":false,
        "layers":[{"layer":"design","kind":"verify","file":source,"status":"failed","result":detail["result"],"exit_code":2,"detail":detail,"depth":2}],
        "failed":["design"],"kind":"chain","message":"one or more chain layers returned an error"})
}

#[test]
pub(crate) fn four_warning_classes_and_refusals_finish_once_on_all_eight_verification_routes() {
    let mut results = Results::default();
    for &(code, token, body) in cases::WARNING_CASES {
        let legacy = cases::legacy(code, body);
        // Model error (not a syntax error) is deliberate: whole envelopes are
        // deterministic and expose report baseline/warning ownership without
        // adding a second solver-timing normalization framework.
        let broken = cases::model_error(&legacy);
        for header in ["", "fsl 4\n", "fsl 5\n"] {
            let source = format!("{header}{broken}");
            let next = header == "fsl 5\n";
            let edition = if next {
                fsl_syntax::Edition::Fsl5
            } else {
                fsl_syntax::Edition::Fsl4
            };
            // Occurrence identity is checked independently of route-local paths.
            let findings = cases::findings(&source, "source.fsl", edition);
            let finding = findings
                .iter()
                .find(|f| f["code"] == code)
                .expect("requested finding");
            let offset = source.find(token).unwrap();
            let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
            assert_eq!(
                finding["loc"]["line"],
                source[..offset].matches('\n').count() + 1
            );
            assert_eq!(
                finding["loc"]["column"],
                source[line_start..offset].chars().count() + 1
            );
            assert_eq!(
                finding["loc"]["end_column"],
                source[line_start..offset].chars().count() + token.chars().count() + 1
            );
            assert_eq!(finding["severity"], if next { "error" } else { "warning" });
            assert!(finding["machine_applicable"].is_boolean());
            assert!(finding["edits"].is_array());
            for key in VERIFY_ROUTES {
                let scratch = Scratch::new();
                let oracle = run(
                    &scratch.args("verify {src} --depth 2 --no-cache", &source),
                    true,
                );
                assert_eq!(oracle.status.code(), Some(2));
                let expected = without_versions(value(&oracle));
                let path = scratch.0.join("source.fsl");
                let findings = cases::findings(&source, path.to_str().unwrap(), edition);
                // The oracle pins the complete finding once, not by rerunning
                // verify with its own identical argv as a separate route cell.
                assert_eq!(
                    if next {
                        &expected["findings"]
                    } else {
                        &expected["warnings"]
                    },
                    &json!(findings)
                );
                let row = cases::rows().into_iter().find(|r| r.key == *key).unwrap();
                let output = run(&scratch.args(row.argv, &source), true);
                let wanted = if next {
                    expected.clone()
                } else if *key == "chain" {
                    chain_expected(&scratch, &path, &expected)
                } else if matches!(*key, "db check" | "ai check") {
                    let mut wanted = json!({"fsl":"1.0","result":"error","kind":"semantics","message":if *key == "db check" {"expected a dbsystem document"} else {"expected an ai_component or agent document"},"warnings":findings});
                    if !header.is_empty() {
                        wanted["fsl_edition"] = json!(4);
                    }
                    wanted
                } else {
                    expected.clone()
                };
                results.error_envelope(
                    &format!("route/{key}/{code}/{header:?}"),
                    &output,
                    &wanted,
                    !scratch.0.join("out").exists(),
                );
            }
        }
    }
    results.finish();
}

#[test]
pub(crate) fn argv_validation_remains_ahead_of_source_admission() {
    let mut results = Results::default();
    for row in cases::rows()
        .into_iter()
        .filter(|row| row.owner == "native" && row.role != "lsp")
    {
        let scratch = Scratch::new();
        let mut args = scratch.args(row.argv, &format!("fsl 5\n{}", cases::DOMAIN));
        args.push("--edition-inventory-unknown".into());
        let first = run(&args, true);
        scratch.write("source.fsl", &cases::rejected_domain());
        let second = run(&args, true);
        let actual = value(&second);
        results.check(
            &format!("preserved/argv/{}", row.key),
            (second.status.code(), &actual),
            value(&first),
            first.status.code() == Some(2)
                && second.status.code() == Some(2)
                && actual == value(&first)
                && actual["kind"] == "usage",
        );
    }
    results.finish();
}

#[test]
pub(crate) fn report_baselines_receive_one_finished_warning_before_rendering() {
    let scratch = Scratch::new();
    let source = cases::WARNING_CASES[1].2;
    let path = scratch.write("source.fsl", source);
    let mut results = Results::default();
    for repeat in 0..2 {
        let oracle = run(
            &scratch.args("verify {src} --depth 2 --no-cache", source),
            false,
        );
        assert_eq!(oracle.status.code(), Some(0));
        let mut expected = without_versions(value(&oracle));
        assert_eq!(
            expected["warnings"],
            json!(cases::findings(
                source,
                path.to_str().unwrap(),
                fsl_syntax::Edition::Fsl4
            ))
        );
        let ledger = run(&scratch.args("ledger {src} --depth 2", source), false);
        let content = String::from_utf8_lossy(&ledger.stdout);
        let body = content.split("## 付録:").next().unwrap();
        let finding = &expected["warnings"][0];
        let code = finding["code"].as_str().unwrap();
        let message = finding["message"].as_str().unwrap();
        let migration_rows = body
            .lines()
            .filter(|line| {
                line.starts_with('|') && line.split('|').any(|cell| cell.trim() == "migration")
            })
            .count();
        results.check(
            &format!("report/ledger/{repeat}"),
            body,
            json!({"trace_type":"migration","summary_contains":["移行候補",code,message],"excludes":["vacuity","mutate"]}),
            ledger.status.code() == Some(0) && migration_rows == 1 && body.lines().any(|line| ["移行候補", code, message].iter().all(|part| line.contains(*part))) && !body.contains("vacuity") && !body.contains("mutate"),
        );
        // --no-timing is the public report projection (#1469), not a blanket
        // comparison exclusion. Every removed key is live on the CLI oracle.
        let cost = expected["cost"].as_object_mut().unwrap();
        assert!(cost.remove("elapsed_s").is_some());
        let solver = cost["solver"].as_object_mut().unwrap();
        for key in [
            "check_elapsed_s",
            "memory_mb",
            "conflicts",
            "decisions",
            "propagations",
        ] {
            assert!(solver.remove(key).is_some());
        }
        assert_eq!(cost["properties"].as_array().unwrap().len(), 8);
        for property in cost["properties"].as_array_mut().unwrap() {
            assert!(
                property
                    .as_object_mut()
                    .unwrap()
                    .remove("elapsed_s")
                    .is_some()
            );
        }
        let html = run(
            &scratch.args("html {src} --depth 2 --no-timing", source),
            false,
        );
        let content = String::from_utf8(html.stdout).unwrap();
        let raw = content
            .split_once("<summary>verify JSON</summary>")
            .unwrap()
            .1
            .split_once("<pre>")
            .unwrap()
            .1
            .split_once("</pre>")
            .unwrap()
            .0;
        let unescaped = raw
            .replace("&quot;", "\"")
            .replace("&#x27;", "'")
            .replace("&#39;", "'")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&");
        let actual: Value = serde_json::from_str(&unescaped).unwrap();
        results.check(
            &format!("report/html-full-baseline/{repeat}"),
            &actual,
            &expected,
            html.status.code() == Some(0) && actual == expected,
        );
    }
    results.finish();
}

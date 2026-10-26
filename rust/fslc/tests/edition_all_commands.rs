// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_cases.rs"]
mod cases;

use cases::{Results, Scratch, value, without_versions};
use serde_json::{Value, json};
use std::process::Output;

fn run(args: &[String], gate: bool) -> Output {
    cases::invoke(env!("CARGO_BIN_EXE_fslc"), args, &cases::root(), gate)
}

fn lsp_row(results: &mut Results, label: &str) {
    let source = format!(
        "fsl 5\n{}",
        cases::legacy("deprecated_domain_enum_union", cases::DOMAIN)
    );
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

#[test]
fn every_registered_root_has_the_same_admission_before_dialect_or_sidecars() {
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
            let expected = format!("fslc {}\n", env!("CARGO_PKG_VERSION"));
            results.check(
                "preserved/version",
                (&output.status.code(), &output.stdout),
                &expected,
                output.status.success()
                    && output.stdout == expected.as_bytes()
                    && output.stderr.is_empty(),
            );
            continue;
        }
        let actual_control = json!([
            output.status.code(),
            value(&output)["result"],
            value(&output)["kind"],
            value(&output)["warnings"],
            scratch.0.join("out").exists()
        ]);
        let expected_control = accepted(row.key);
        results.check(
            &format!("preserved/{}", row.key),
            &actual_control,
            &expected_control,
            actual_control == expected_control,
        );
        if row.role == "nonreader" {
            continue;
        }
        let rejected = format!(
            "fsl 5\n{}",
            cases::legacy("deprecated_domain_enum_union", cases::DOMAIN)
        );
        let scratch = Scratch::new();
        let args = scratch.args(row.argv, &rejected);
        let expected_output = run(&scratch.args("check {src}", &rejected), true);
        assert_eq!(expected_output.status.code(), Some(2));
        let expected = without_versions(value(&expected_output));
        assert_eq!(expected["kind"], "deprecated_domain_enum_union");
        let output = run(&args, true);
        let actual = without_versions(value(&output));
        let agrees = if row.role == "repair" {
            match row.key {
                "lint" => {
                    output.status.code() == Some(1)
                        && actual["result"] == "lint"
                        && actual["files"][0]["findings"] == expected["findings"]
                }
                "migrate" => {
                    output.status.code() == Some(0)
                        && actual["result"] == "migrated"
                        && actual["files"][0]["findings"] == expected["findings"]
                        && actual["files"][0]["edits"]
                            .as_array()
                            .is_some_and(|e| e.len() == 1)
                }
                "fmt" => output.status.code() == Some(1) && actual["result"] == "format_check",
                _ => unreachable!(),
            }
        } else {
            output.status.code() == Some(2) && actual == expected && !scratch.0.join("out").exists()
        };
        results.check(
            row.key,
            (
                output.status.code(),
                &actual,
                String::from_utf8_lossy(&output.stderr),
            ),
            &expected,
            agrees,
        );
    }
    cases::assignment("native", &executed);
    results.finish();
}

// PARENT observations for the canonical half of the SAME domain pair. Wrong
// dialect and sidecar errors remain preservation controls, not admission detectors.
fn accepted(key: &str) -> Value {
    let (exit, result, kind, warnings, artifact) = match key {
        "ai check"
        | "ai compat"
        | "ai replay"
        | "compat check"
        | "db check"
        | "db observe"
        | "counterexample export" => (2, "error", Some("semantics"), Value::Null, false),
        "ai drift"
        | "ai eval"
        | "ai regress"
        | "causal analyze"
        | "causal check"
        | "causal diff"
        | "causal ledger"
        | "causal observe-expectations"
        | "causal verify-expectations" => (2, "error", Some("parse"), Value::Null, false),
        "approval check" | "approval create" | "approval diff" | "replay" => {
            (2, "error", Some("io"), Value::Null, false)
        }
        "document check" | "document claims" | "document generate" => {
            (2, "error", Some("document"), Value::Null, false)
        }
        "refine" => (2, "error", Some("type"), Value::Null, false),
        "ai compare" => (0, "compared", None, Value::Null, false),
        "analyze" | "domain analyze" => (0, "analyzed", None, Value::Null, false),
        "chain" => (0, "verified", None, Value::Null, false),
        "check" => (0, "ok", None, json!([]), false),
        "conformance" => (0, "conformance", None, Value::Null, false),
        "db import" => (0, "imported", None, json!([]), true),
        "diff" => (0, "semantic_diff", None, Value::Null, false),
        "domain check" => (0, "verified_under_assumptions", None, Value::Null, false),
        "domain expand" => (0, "expanded", None, Value::Null, true),
        "domain generate" | "testgen" => (0, "generated", None, Value::Null, true),
        "domain replay" => (1, "nonconformant", None, Value::Null, false),
        "domain testgen" => (0, "generated", Some("domain_testgen"), json!([]), true),
        "explain" => (0, "explained", None, Value::Null, false),
        "fmt" => (1, "format_check", None, Value::Null, false),
        "html" => (0, "generated", Some("html_report"), Value::Null, true),
        "kernel" => (0, "kernel", None, Value::Null, false),
        "ledger" => (0, "generated", Some("audit_ledger"), Value::Null, true),
        "lint" => (0, "lint", None, Value::Null, false),
        "migrate" => (0, "migrated", None, Value::Null, false),
        "mutate" => (0, "mutated", None, Value::Null, false),
        "scenarios" => (0, "scenarios", None, json!([]), false),
        "sweep" => (0, "sweep_passed", None, Value::Null, false),
        "testplan" => (0, "testplan", None, Value::Null, false),
        "typestate" => (0, "typestate", None, Value::Null, false),
        "verify" => (0, "verified", None, json!([]), false),
        "version" => return json!([0, null, null, null, false]),
        _ => panic!("canonical preservation expectation missing for {key}"),
    };
    json!([exit, result, kind, warnings, artifact])
}

const VERIFY_ROUTES: &[&str] = &[
    "verify", "sweep", "chain", "ledger", "html", "mutate", "db check", "ai check",
];

fn chain_expected(
    scratch: &Scratch,
    source: &std::path::Path,
    detail: &Value,
    status: i32,
) -> Value {
    let mut expected = json!({"fsl":"1.0","result":if status == 0 {"verified"} else if status == 2 {"error"} else {"violated"},"manifest":scratch.0.join("chain.toml"),"keep_going":false,
        "layers":[{"layer":"design","kind":"verify","file":source,"status":if status == 0 {"passed"} else {"failed"},"result":detail["result"],"exit_code":status,"detail":detail,"depth":2}]});
    if status != 0 {
        expected["failed"] = json!(["design"]);
    }
    if status == 2 {
        expected["kind"] = json!("chain");
        expected["message"] = json!("one or more chain layers returned an error");
    }
    expected
}

#[test]
fn four_warning_classes_and_refusals_finish_once_on_all_eight_verification_routes() {
    let mut results = Results::default();
    for &(code, token, body) in cases::WARNING_CASES {
        let legacy = cases::legacy(code, body);
        // Model error (not a syntax error) is deliberate: whole envelopes are
        // deterministic and expose report baseline/warning ownership without
        // adding a second solver-timing normalization framework.
        let broken = legacy
            .replace("status == Open or status == Closed", "missing_name")
            .replace("done == true or done == false", "missing_name")
            .replace(
                "invariant Typed { true }",
                "invariant Typed { missing_name }",
            );
        assert_ne!(broken, legacy);
        for header in ["", "fsl 4\n", "fsl 5\n"] {
            let source = format!("{header}{broken}");
            let scratch = Scratch::new();
            let args = scratch.args("verify {src} --depth 2 --no-cache", &source);
            let oracle = run(&args, true);
            assert_eq!(oracle.status.code(), Some(2));
            let expected = without_versions(value(&oracle));
            let next = header == "fsl 5\n";
            let edition = if next {
                fsl_syntax::Edition::Fsl5
            } else {
                fsl_syntax::Edition::Fsl4
            };
            let path = scratch.0.join("source.fsl");
            let findings = cases::findings(&source, path.to_str().unwrap(), edition);
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
            let emitted = if next {
                &expected["findings"]
            } else {
                &expected["warnings"]
            };
            results.check(
                &format!("oracle/{code}/{header:?}"),
                emitted,
                &findings,
                emitted == &json!(findings),
            );
            for key in VERIFY_ROUTES {
                let row = cases::rows().into_iter().find(|r| r.key == *key).unwrap();
                let output = run(&scratch.args(row.argv, &source), true);
                let actual = without_versions(value(&output));
                let wanted = if next {
                    expected.clone()
                } else if *key == "chain" {
                    chain_expected(&scratch, &path, &expected, 2)
                } else if matches!(*key, "db check" | "ai check") {
                    let mut wanted = json!({"fsl":"1.0","result":"error","kind":"semantics","message":if *key == "db check" {"expected a dbsystem document"} else {"expected an ai_component or agent document"},"warnings":findings});
                    if !header.is_empty() {
                        wanted["fsl_edition"] = json!(4);
                    }
                    wanted
                } else {
                    expected.clone()
                };
                results.check(
                    &format!("route/{key}/{code}/{header:?}"),
                    (output.status.code(), &actual),
                    &wanted,
                    output.status.code() == Some(2)
                        && actual == wanted
                        && !scratch.0.join("out").exists(),
                );
            }
        }
    }
    results.finish();
}

#[test]
fn nested_success_or_failure_verifications_keep_the_whole_finished_envelope() {
    fn normalize(value: &mut Value) {
        // These exact 12 numeric elapsed leaves are observed in the existing
        // #1417 control_seam_fail; no warning/location/assurance is excluded.
        let cost = value.get_mut("cost").expect("live cost");
        assert_eq!(cost["properties"].as_array().unwrap().len(), 10);
        for pointer in ["/elapsed_s", "/solver/check_elapsed_s"] {
            let leaf = cost.pointer_mut(pointer).unwrap();
            assert!(leaf.as_f64().is_some_and(|v| v.is_finite() && v >= 0.0));
            *leaf = json!(0);
        }
        for property in cost["properties"].as_array_mut().unwrap() {
            let leaf = property.get_mut("elapsed_s").unwrap();
            assert!(leaf.as_f64().is_some_and(|v| v.is_finite() && v >= 0.0));
            *leaf = json!(0);
        }
    }
    let scratch = Scratch::new();
    let path =
        cases::root().join("rust/fslc/tests/fixtures/issue_1008_not_evaluated/impl_broken.fsl");
    scratch.write(
        "chain.toml",
        &format!("[design]\nfile = {}\ndepth = 2\n", json!(path)),
    );
    let invoke = |key: &str| {
        let mut args = vec![
            key.to_owned(),
            path.to_str().unwrap().to_owned(),
            "--depth".into(),
            if key == "sweep" {
                "2..2".into()
            } else {
                "2".into()
            },
        ];
        if key == "verify" {
            args.push("--no-cache".into());
        }
        if key == "chain" {
            args = vec![
                "chain".into(),
                scratch.0.join("chain.toml").to_str().unwrap().to_owned(),
            ];
        }
        run(&args, false)
    };
    let mut results = Results::default();
    for repeat in 0..2 {
        let oracle = invoke("verify");
        assert_eq!(oracle.status.code(), Some(1));
        let mut expected = without_versions(value(&oracle));
        assert_eq!(
            expected["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|w| w["code"] == "inline_action_maps")
                .count(),
            1
        );
        normalize(&mut expected);
        for route in ["sweep", "chain", "mutate"] {
            let output = invoke(route);
            let mut actual = value(&output);
            let wanted = if route == "chain" {
                normalize(actual.pointer_mut("/layers/0/detail").unwrap());
                chain_expected(&scratch, &path, &expected, 1)
            } else if route == "sweep" {
                normalize(actual.pointer_mut("/sweep/results/0/verification").unwrap());
                normalize(
                    actual
                        .pointer_mut("/sweep/minimal_counterexample/verification")
                        .unwrap(),
                );
                let mut detail = expected.clone();
                detail["bounds_overrides"] = json!({"instances":{},"values":{}});
                let cell = json!({"scope":{"instances":{},"values":{},"depth":2},"summary":{"result":detail["result"],"checked_to_depth":2},"verification":detail});
                json!({"fsl":"1.0","result":"sweep_failed","spec":detail["spec"],"sweep":{"minimality_order":["instances","values","depth"],"ranges":{"instances":{},"values":{},"depth":[2,2]},"results":[cell],"minimal_counterexample":cell,"inconclusive_scopes":[]}})
            } else {
                normalize(&mut actual);
                expected.clone()
            };
            results.check(
                &format!("nested/{route}/{repeat}"),
                (output.status.code(), &actual),
                &wanted,
                output.status.code() == Some(1) && actual == wanted,
            );
        }
    }
    results.finish();
}

#[test]
fn metadata_aliases_and_argv_validation_remain_ahead_of_source_admission() {
    let mut results = Results::default();
    for alias in ["version", "-V", "--version"] {
        let output = run(&[alias.to_owned()], true);
        let expected = format!("fslc {}\n", env!("CARGO_PKG_VERSION"));
        results.check(
            &format!("preserved/{alias}"),
            (output.status.code(), &output.stdout),
            &expected,
            output.status.success() && output.stdout == expected.as_bytes(),
        );
    }
    let output = run(&["--cli-contract".into()], true);
    let expected: Value = serde_json::from_str(include_str!("../cli-contract.json")).unwrap();
    results.check(
        "preserved/cli-contract",
        value(&output),
        &expected,
        output.status.success() && value(&output) == expected,
    );
    for row in cases::rows()
        .into_iter()
        .filter(|row| row.owner == "native" && row.role != "lsp")
    {
        let mut help: Vec<_> = row.key.split_whitespace().map(str::to_owned).collect();
        help.push("--help".into());
        let first = run(&help, false);
        *help.last_mut().unwrap() = "-h".into();
        let second = run(&help, true);
        results.check(
            &format!("preserved/help/{}", row.key),
            (&first.status.code(), &second.status.code()),
            "same complete help bytes",
            first.status.success()
                && second.status.success()
                && !first.stdout.is_empty()
                && first.stdout == second.stdout
                && first.stderr == second.stderr,
        );
        let scratch = Scratch::new();
        let mut args = scratch.args(row.argv, &format!("fsl 5\n{}", cases::DOMAIN));
        args.push("--edition-inventory-unknown".into());
        let first = run(&args, true);
        scratch.write(
            "source.fsl",
            &format!(
                "fsl 5\n{}",
                cases::legacy("deprecated_domain_enum_union", cases::DOMAIN)
            ),
        );
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
fn report_baselines_receive_one_finished_warning_before_rendering() {
    let scratch = Scratch::new();
    let source = cases::WARNING_CASES[1].2;
    let path = scratch.write("source.fsl", source);
    let resolver = fsl_core::FsResolver::new(&scratch.0);
    let kernel =
        fsl_core::parse_kernel_source_with_file(source, &resolver, path.to_str().unwrap()).unwrap();
    let model = fsl_core::build_model(kernel).unwrap();
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
        let scenarios = run(&scratch.args("scenarios {src} --depth 2", source), false);
        let wanted_ledger = fsl_tools::render_ledger(
            path.to_str().unwrap(),
            &model,
            &expected,
            &value(&scenarios),
            None,
            &[],
        );
        let ledger = run(&scratch.args("ledger {src} --depth 2", source), false);
        results.check(
            &format!("report/ledger/{repeat}"),
            String::from_utf8_lossy(&ledger.stdout),
            &wanted_ledger,
            ledger.status.code() == Some(0) && ledger.stdout == wanted_ledger.as_bytes(),
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

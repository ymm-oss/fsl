// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_cases.rs"]
pub(crate) mod cases;

use cases::{Results, Scratch, value, without_versions};
use serde_json::{Value, json};

fn run(scratch: &Scratch, template: &str) -> std::process::Output {
    cases::command(env!("CARGO_BIN_EXE_fslc"), &scratch.0, template, true)
}

// Count complete findings across the envelope, not only its outer warnings.
fn warnings(output: &Value) -> Vec<Value> {
    match output {
        Value::Object(fields) => fields
            .iter()
            .flat_map(|(key, value)| {
                if key == "warnings" {
                    value.as_array().expect("warnings array").clone()
                } else {
                    warnings(value)
                }
            })
            .collect(),
        Value::Array(items) => items.iter().flat_map(warnings).collect(),
        _ => Vec::new(),
    }
}

fn summary(output: &std::process::Output) -> Value {
    let actual = value(output);
    json!([
        output.status.code(),
        actual["result"],
        actual["kind"],
        warnings(&actual)
    ])
}

fn finding(source: &str, code: &str, line: usize, column: usize, end_column: usize) -> Value {
    let findings = cases::findings(source, "source.fsl", fsl_syntax::Edition::Fsl4);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["code"], code);
    assert_eq!(
        findings[0]["loc"],
        json!({"file":"source.fsl", "line":line, "column":column,
            "end_line":line, "end_column":end_column})
    );
    findings[0].clone()
}

#[test]
pub(crate) fn domain_check_finishes_once_for_definitive_and_non_definitive_verdicts() {
    let mut results = Results::default();
    let legacy = cases::legacy("deprecated_domain_enum_union", cases::DOMAIN);
    for (label, source, exit, result, kind) in [
        (
            "definitive",
            legacy.clone(),
            0,
            "verified_under_assumptions",
            Value::Null,
        ),
        (
            "non-definitive",
            cases::model_error(&legacy),
            2,
            "error",
            json!("semantics"),
        ),
    ] {
        let scratch = Scratch::new();
        scratch.write("source.fsl", &source);
        let expected = vec![finding(&source, "deprecated_domain_enum_union", 1, 12, 39)];
        let output = run(&scratch, "domain check source.fsl --depth 2");
        let actual = value(&output);
        let produced = summary(&output);
        let wanted = json!([exit, result, kind, expected]);
        results.check(label, &produced, &wanted, produced == wanted);
        if label == "definitive" {
            assert_eq!(actual["kernel"]["result"], "verified");
        } else {
            assert!(actual.get("kernel").is_none());
            assert_eq!(actual["loc"], json!({"line":1,"column":368}));
        }
    }
    results.finish();
}

#[test]
pub(crate) fn repeated_file_slots_each_contribute_one_warning() {
    let mut results = Results::default();
    let source = cases::WARNING_CASES[1].2;
    let expected = finding(source, "legacy_string_metadata", 1, 55, 76);
    for (command, result) in [
        ("diff source.fsl source.fsl --depth 2", "semantic_diff"),
        (
            "refine source.fsl source.fsl mapping.fsl --depth 2",
            "refines",
        ),
    ] {
        let scratch = Scratch::new();
        scratch.write("source.fsl", source);
        let output = run(&scratch, command);
        let produced = summary(&output);
        let wanted = json!([0, result, null, [expected, expected]]);
        results.check(command, &produced, &wanted, produced == wanted);
    }
    results.finish();
}

#[test]
pub(crate) fn chain_owns_warnings_in_layer_detail_and_refuses_before_effects() {
    let mut results = Results::default();
    for (mode, depth) in [("check", ""), ("verify", "depth = 2\n")] {
        let scratch = Scratch::new();
        let source = cases::WARNING_CASES[1].2;
        let path = scratch.write("source.fsl", source);
        let mut expected = finding(source, "legacy_string_metadata", 1, 55, 76);
        expected["loc"]["file"] = json!(path);
        let manifest = format!("[design]\nfile = {}\n{depth}", json!(path));
        scratch.write("chain.toml", &manifest);
        let output = run(&scratch, "chain chain.toml");
        let actual = value(&output);
        let detail = &actual["layers"][0]["detail"];
        let produced = json!([
            output.status.code(),
            actual["result"],
            actual["kind"],
            actual["layers"][0]["kind"],
            detail["warnings"],
            warnings(&actual)
        ]);
        let wanted = json!([0, "verified", null, mode, [expected], [expected]]);
        results.check(
            &format!("chain/{mode}/warning"),
            &produced,
            &wanted,
            produced == wanted,
        );
        // keep-going cannot turn edition refusal into an implementation run.
        scratch.write("source.fsl", &cases::rejected_metadata());
        scratch.write(
            "chain.toml",
            &format!("{manifest}[impl]\ncommand = \"touch marker\"\nevidence = \"exit_code\"\n"),
        );
        let expected = cases::refusal(env!("CARGO_BIN_EXE_fslc"), &scratch.0, &path);
        let output = run(&scratch, "chain chain.toml --keep-going");
        results.error_envelope(
            &format!("chain/{mode}/refusal"),
            &output,
            &expected,
            !scratch.0.join("marker").exists(),
        );
    }
    results.finish();
}

#[test]
pub(crate) fn causal_root_keeps_one_owned_warning_and_refuses_before_output() {
    let mut results = Results::default();
    let scratch = Scratch::new();
    // A genuine causal document keeps its specialized warning unchanged.
    scratch.write(
        "source.fsl",
        "causal C { timebase day horizon 30 variable outcome { role outcome } }\n",
    );
    let output = run(&scratch, "causal check source.fsl");
    let expected = json!([{"kind":"causal_outcome_unmeasured", "severity":"warning",
        "message":"outcome 'outcome' has no observes or proxy measurement",
        "loc":{"line":1,"column":45}, "involved_nodes":["variable:outcome"]}]);
    let produced = summary(&output);
    let wanted = json!([0, "causal_model_checked", null, expected]);
    results.check(
        "causal/specialized-warning",
        &produced,
        &wanted,
        produced == wanted,
    );
    // Causal is editionless. An edition-admitted legacy root still owes its
    // migration warning when the inner consumer rejects the wrong dialect.
    let source = cases::WARNING_CASES[1].2;
    let expected = finding(source, "legacy_string_metadata", 1, 55, 76);
    for (command, message) in [
        ("causal check source.fsl", "expected 'causal'"),
        (
            "causal observe-expectations source.fsl --from-log missing.jsonl --mapping mapping.fsl --scope missing.json --period-start 2026-01-01 --period-end 2026-03-31 --out out.json",
            "expected 'causal' at 1:1",
        ),
    ] {
        scratch.write("source.fsl", source);
        let output = run(&scratch, command);
        let actual = without_versions(value(&output));
        let wanted = json!({"fsl":"1.0","result":"error","kind":"parse",
            "message":message,"diagnostic_code":"FSL-PARSE",
            "loc":{"line":1,"column":1},"warnings":[expected]});
        results.error_envelope(
            &format!("{command}/warning"),
            &output,
            &wanted,
            !scratch.0.join("out.json").exists(),
        );
        assert_eq!(actual["loc"], json!({"line":1,"column":1}));
        scratch.write("source.fsl", &cases::rejected_metadata());
        let wanted = cases::refusal(
            env!("CARGO_BIN_EXE_fslc"),
            &scratch.0,
            std::path::Path::new("source.fsl"),
        );
        let output = run(&scratch, command);
        results.error_envelope(
            &format!("{command}/refusal"),
            &output,
            &wanted,
            !scratch.0.join("out.json").exists(),
        );
    }
    results.finish();
}

#[test]
pub(crate) fn unreadable_roots_keep_the_command_specific_error_kind() {
    let mut results = Results::default();
    let scratch = Scratch::new();
    assert!(!scratch.0.join("missing.fsl").exists());
    for (command, kind) in [
        ("diff missing.fsl missing.fsl --depth 2", "io"),
        ("ai check missing.fsl --depth 2", "io"),
        ("ai replay missing.fsl --logs records.json", "io"),
        ("ai compat missing.fsl", "io"),
        ("domain check missing.fsl --depth 2", "semantics"),
    ] {
        let output = run(&scratch, command);
        let actual = without_versions(value(&output));
        let wanted = json!([2, "error", kind, null, null, null]);
        let produced = json!([
            output.status.code(),
            actual["result"],
            actual["kind"],
            actual["loc"],
            actual["findings"],
            actual["warnings"]
        ]);
        results.check(command, &produced, &wanted, produced == wanted);
    }
    results.finish();
}

#[test]
pub(crate) fn domain_check_forwards_requested_edition_without_changing_conflicts() {
    let mut results = Results::default();
    let legacy = cases::legacy("deprecated_domain_enum_union", cases::DOMAIN);
    for (header, flag, conflict) in [
        ("", "next", None),
        ("fsl 4\n", "next", Some((4, 5))),
        ("fsl 5\n", "4", Some((5, 4))),
        ("fsl 5\n", "current", None),
    ] {
        let scratch = Scratch::new();
        let source = format!("{header}{legacy}");
        scratch.write("source.fsl", &source);
        let expected = if let Some((declared, requested)) = conflict {
            json!({"fsl":"1.0","result":"error","kind":"parse",
                "message":format!("the document declares 'fsl {declared}' but --edition {requested} was requested at 1:1"),
                "diagnostic_code":"edition_conflict","loc":{"line":1,"column":1}})
        } else {
            let mut expected = json!({"fsl":"1.0","result":"error",
                "kind":"deprecated_domain_enum_union","edition":"next",
                "findings":cases::findings(&source, "source.fsl", fsl_syntax::Edition::Fsl5)});
            assert_eq!(
                expected["findings"][0]["code"],
                "deprecated_domain_enum_union"
            );
            assert_eq!(
                expected["findings"][0]["loc"],
                json!({"file":"source.fsl",
                "line":if header.is_empty() {1} else {2}, "column":12,
                "end_line":if header.is_empty() {1} else {2}, "end_column":39})
            );
            if !header.is_empty() {
                expected["fsl_edition"] = json!(5);
            }
            expected
        };
        let command = format!("domain check source.fsl --depth 2 --edition {flag}");
        let output = run(&scratch, &command);
        results.error_envelope(&format!("{header:?}/{flag}"), &output, &expected, true);
    }
    results.finish();
}

#[test]
pub(crate) fn counterexample_refuses_before_model_and_source_preflight() {
    let mut results = Results::default();
    for (mode, source, engine) in [
        (
            "model",
            cases::headed(&cases::model_error(cases::WARNING_CASES[1].2)),
            "bmc",
        ),
        ("preflight", cases::rejected_metadata(), "induction"),
    ] {
        let scratch = Scratch::new();
        let path = scratch.write("source.fsl", &source);
        let expected = cases::refusal(env!("CARGO_BIN_EXE_fslc"), &scratch.0, &path);
        let output = run(
            &scratch,
            &format!(
                "counterexample export {} --depth 2 --engine {engine} -o out.json",
                path.display()
            ),
        );
        results.error_envelope(
            &format!("control/counterexample export/{mode}"),
            &output,
            &expected,
            !scratch.0.join("out.json").exists(),
        );
    }
    results.finish();
}

#[cfg(unix)]
#[test]
pub(crate) fn counterexample_refuses_before_digest_rereads_the_root() {
    let scratch = Scratch::new();
    let source = cases::rejected_metadata();
    let expected = cases::refusal_envelope(
        &source,
        "/dev/stdin",
        serde_json::from_value(json!({"fsl":"1.0"})).unwrap(),
    );
    let output = cases::invoke_input(
        env!("CARGO_BIN_EXE_fslc"),
        &cases::split("counterexample export /dev/stdin --depth 2 -o out.json"),
        &scratch.0,
        &source,
    );
    let mut results = Results::default();
    results.error_envelope(
        "control/counterexample export/digest",
        &output,
        &expected,
        !scratch.0.join("out.json").exists(),
    );
    results.finish();
}

#[test]
pub(crate) fn mutate_success_keeps_one_complete_legacy_warning() {
    let mut results = Results::default();
    let scratch = Scratch::new();
    let source = cases::WARNING_CASES[1].2;
    scratch.write("source.fsl", source);
    let expected = finding(source, "legacy_string_metadata", 1, 55, 76);
    let output = run(&scratch, "mutate source.fsl --depth 2 --max-mutants 0");
    let actual = value(&output);
    let produced = json!([summary(&output), actual["baseline"], actual["mutants"]]);
    let wanted = json!([[0, "mutated", null, [expected]], "verified", []]);
    results.check(
        "control/mutate/success-warning",
        &produced,
        &wanted,
        produced == wanted,
    );
    results.finish();
}

#[cfg(unix)]
#[test]
pub(crate) fn sweep_reuses_one_captured_root_across_all_depth_cells() {
    let scratch = Scratch::new();
    let source = cases::WARNING_CASES[1].2;
    scratch.write("source.fsl", source);
    let mut results = Results::default();
    // The pipe is supplied exactly once and then closed. Each subsequent open
    // sees EOF, not another copy of the source. No sleeps or file-change race.
    for (label, command, path, depths) in [
        (
            "preserved/single-capture/verify",
            "verify /dev/stdin --depth 2 --no-cache",
            "/dev/stdin",
            vec![],
        ),
        (
            "preserved/single-capture/regular-sweep",
            "sweep source.fsl --depth 2..3",
            "source.fsl",
            vec![2, 3],
        ),
        (
            "control/sweep/capture",
            "sweep /dev/stdin --depth 2..3",
            "/dev/stdin",
            vec![2, 3],
        ),
    ] {
        let expected = cases::findings(source, path, fsl_syntax::Edition::Fsl4);
        let output = cases::invoke_input(
            env!("CARGO_BIN_EXE_fslc"),
            &cases::split(command),
            &scratch.0,
            source,
        );
        let actual = value(&output);
        let agrees = if depths.is_empty() {
            actual["result"] == "verified" && actual["warnings"] == json!(expected)
        } else {
            actual["result"] == "sweep_passed"
                && actual["sweep"]["results"].as_array().is_some_and(|cells| {
                    cells.len() == depths.len()
                        && cells.iter().zip(&depths).all(|(cell, depth)| {
                            cell["scope"]["depth"] == *depth
                                && cell["verification"]["result"] == "verified"
                                && cell["verification"]["warnings"] == json!(expected)
                        })
                })
        };
        results.check(
            label,
            (output.status.code(), &actual),
            (0, &depths, &expected),
            output.status.code() == Some(0) && agrees,
        );
    }
    results.finish();
}

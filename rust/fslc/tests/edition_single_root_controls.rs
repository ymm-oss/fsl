// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_cases.rs"]
mod cases;

use cases::{Results, Scratch, value, without_versions};
use serde_json::json;

fn run(scratch: &Scratch, command: &str) -> std::process::Output {
    cases::command(env!("CARGO_BIN_EXE_fslc"), &scratch.0, command, true)
}

#[test]
fn replay_mapping_io_precedes_an_ordinary_root_parse_error() {
    let scratch = Scratch::new();
    scratch.write("broken.fsl", "spec Broken {\n");
    scratch.write("log.jsonl", "");
    let missing = scratch.0.join("missing.fsl");
    let io = std::fs::read_to_string(&missing).unwrap_err();
    let expected = json!({"fsl":"1.0", "result":"error", "kind":"io", "message":io.to_string()});
    let output = run(
        &scratch,
        "replay broken.fsl --from-log log.jsonl --mapping missing.fsl",
    );
    let mut results = Results::default();
    results.error_envelope("ordering/replay/mapping-io", &output, &expected, true);
    results.finish();
}

#[test]
fn document_admission_precedes_artifact_io_and_frontmatter_errors() {
    let scratch = Scratch::new();
    let source = cases::headed(cases::DOCUMENT_LEGACY);
    scratch.write("source.fsl", &source);
    scratch.write("broken.md", "---\nnot-a-frontmatter-field\n---\n");
    let expected = cases::refusal_envelope(
        &source,
        "source.fsl",
        serde_json::from_value(json!({"fsl":"1.0"})).unwrap(),
    );
    let mut results = Results::default();
    for artifact in ["missing.md", "broken.md"] {
        let output = run(&scratch, &format!("document check source.fsl {artifact}"));
        results.error_envelope(
            &format!("ordering/document/{artifact}"),
            &output,
            &expected,
            true,
        );
    }
    results.finish();
}

fn markdown(source: &str, label: &str, base: &std::path::Path) -> String {
    let claims = fsl_tools::project_requirement_claims_from_source_with_options(
        source,
        Some(label),
        base,
        &cases::gated(),
    )
    .unwrap();
    let kernel = fsl_core::parse_kernel_source_with_options(
        source,
        &fsl_core::FsResolver::new(base),
        &cases::gated(),
    )
    .unwrap();
    let model = fsl_core::build_model(kernel.clone()).unwrap();
    fsl_tools::render_requirements_document_with_options(
        &claims,
        &kernel,
        &model,
        source,
        fsl_tools::Locale::Ja,
        None,
        None,
        None,
        &cases::gated(),
    )
    .unwrap()
    .markdown
}

#[test]
fn document_check_uses_the_accepted_root_not_the_recorded_label_file() {
    let scratch = Scratch::new();
    let source = cases::DOCUMENT_LEGACY.replace(
        "action finish() \"REQ-PROBE-001: 日本語😀\"",
        "@requirement(\"REQ-PROBE-001\", \"日本語😀\") action finish()",
    );
    scratch.write("recorded.fsl", &source);
    scratch.write("accepted.fsl", &source);
    let generated = run(&scratch, "document generate recorded.fsl -o document.md");
    assert_eq!(generated.status.code(), Some(0));
    let expected = markdown(&source, "recorded.fsl", &scratch.0);
    let written = std::fs::read_to_string(scratch.0.join("document.md")).unwrap();
    let mut results = Results::default();
    results.check(
        "document/generate/recorded-label",
        &written,
        &expected,
        written == expected,
    );
    let reference = run(&scratch, "document check recorded.fsl document.md");
    assert_eq!(reference.status.code(), Some(0));
    assert_eq!(value(&reference)["result"], "document_conformant");
    scratch.write(
        "recorded.fsl",
        &source.replace("\"Raw\"", "\"Different requirement\""),
    );
    let accepted = run(&scratch, "document check accepted.fsl document.md");
    let actual = without_versions(value(&accepted));
    let expected = without_versions(value(&reference));
    results.check(
        "document/check/recorded-label-is-not-input",
        (accepted.status.code(), &actual, &accepted.stderr),
        (Some(0), &expected, Vec::<u8>::new()),
        accepted.status.code() == Some(0) && actual == expected && accepted.stderr.is_empty(),
    );
    let decoy = run(&scratch, "document check recorded.fsl document.md");
    results.check(
        "document/check/label-file-negative-control",
        (decoy.status.code(), value(&decoy)),
        "1/document_drifted",
        decoy.status.code() == Some(1) && value(&decoy)["result"] == "document_drifted",
    );
    results.finish();
}

#[cfg(unix)]
#[test]
fn document_generate_projects_the_captured_stream_without_reopening_its_label() {
    let scratch = Scratch::new();
    let source = cases::DOCUMENT_LEGACY;
    // A repository-relative label is required by the public Kernel contract.
    std::os::unix::fs::symlink("/dev/stdin", scratch.0.join("stream.fsl")).unwrap();
    let expected = markdown(source, "stream.fsl", &scratch.0);
    let output = cases::invoke_input(
        env!("CARGO_BIN_EXE_fslc"),
        &cases::split("document generate stream.fsl"),
        &scratch.0,
        source,
    );
    let actual = String::from_utf8(output.stdout.clone()).unwrap();
    let mut results = Results::default();
    // The display label names a stream already at EOF. Reopening that label
    // cannot reproduce the source supplied once to this invocation.
    results.check(
        "document/generate/captured-source",
        (output.status.code(), &actual),
        (Some(0), &expected),
        output.status.code() == Some(0) && actual == expected,
    );
    results.finish();
}

#[test]
fn report_output_file_envelopes_stay_closed_and_baselines_warn_once() {
    let scratch = Scratch::new();
    let source = cases::WARNING_CASES[1].2;
    scratch.write("source.fsl", source);
    let expected_warning = cases::findings(source, "source.fsl", fsl_syntax::Edition::Fsl4);
    let mut results = Results::default();
    for (verb, kind, options) in [
        ("html", "html_report", "--no-timing"),
        ("ledger", "audit_ledger", ""),
    ] {
        let output = run(
            &scratch,
            &format!("{verb} source.fsl --depth 2 {options} -o report.out"),
        );
        let actual = without_versions(value(&output));
        let expected = json!({"fsl":"1.0", "result":"generated", "kind":kind, "spec":"S", "output":"report.out"});
        let content = std::fs::read_to_string(scratch.0.join("report.out")).unwrap();
        // Both formats embed JSON. Count the serialized diagnostic identity,
        // not its summary row or copies in the rendered source text.
        let diagnostic = if verb == "html" {
            "&quot;code&quot;: &quot;legacy_string_metadata&quot;"
        } else {
            "\"code\": \"legacy_string_metadata\""
        };
        let actual_warning_count = content.matches(diagnostic).count();
        results.check(
            &format!("report/{verb}/output-envelope"),
            (
                output.status.code(),
                &actual,
                &output.stderr,
                actual_warning_count,
            ),
            (Some(0), &expected, Vec::<u8>::new(), expected_warning.len()),
            output.status.code() == Some(0)
                && actual == expected
                && output.stderr.is_empty()
                && actual_warning_count == expected_warning.len()
                && !expected_warning.is_empty(),
        );
    }
    results.finish();
}

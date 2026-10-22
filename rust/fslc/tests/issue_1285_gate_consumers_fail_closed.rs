// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1285 stage 2 (#1242, #1243): every consumer that reports a result about a
//! spec's acceptance / forbidden traces runs the requirement-trace gate and
//! fails closed on its refusal, reporting the gate's own error with exit 2 —
//! the envelope `check` / `scenarios` report for the same spec. Before, explain
//! exited 0 with no witnesses, html wrote a report without them, ledger rendered
//! a ledger, the document commands rendered a document, and the
//! `requirements_document` / `ledger` approvals recorded or compared it.
//! The analyze graph is structure, not a verdict: it still projects the cases,
//! marked `trace_gate: "unchecked"`.
//!
//! Fixtures: `examples/gallery/errors/forbidden_op_accepted.fsl` (a forbidden
//! whose last step is accepted) and
//! `examples/gallery/errors/error_acceptance_false_expect.fsl` (an acceptance
//! whose expectation does not hold), plus `guarded()`, the first with the
//! guard that makes its forbidden hold.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

const FORBIDDEN_ACCEPTED: &str = "examples/gallery/errors/forbidden_op_accepted.fsl";
const ACCEPTANCE_FALSE: &str = "examples/gallery/errors/error_acceptance_false_expect.fsl";
const REFUSED: [&str; 2] = [FORBIDDEN_ACCEPTED, ACCEPTANCE_FALSE];

static NEXT: AtomicU64 = AtomicU64::new(0);

fn repository_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "fslc-1285s2-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).expect("remove stale scratch directory");
    }
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn run(cwd: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(cwd)
        .env_remove("FSLC_UNSTABLE")
        .output()
        .expect("run native fslc")
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not a JSON envelope ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// `scenarios`' output for `spec`: the gate's refusal, which every consumer
/// below must report verbatim.
fn gate_refusal(cwd: &Path, spec: &str) -> Value {
    let output = run(cwd, &["scenarios", spec]);
    assert_eq!(output.status.code(), Some(2), "{spec}: scenarios exit");
    let refusal = json(&output);
    assert_eq!(refusal["result"], "error", "{refusal}");
    assert!(
        matches!(refusal["kind"].as_str(), Some("forbidden" | "acceptance")),
        "{spec}: the gate refuses a trace: {refusal}"
    );
    refusal
}

fn assert_refused(cwd: &Path, spec: &str, arguments: &[&str]) {
    let output = run(cwd, arguments);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{arguments:?}: exit; stdout {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        json(&output),
        gate_refusal(cwd, spec),
        "{arguments:?} reports the gate's error"
    );
}

/// `forbidden_op_accepted.fsl` with the guard its forbidden asks for.
fn guarded() -> String {
    let source =
        std::fs::read_to_string(repository_file(FORBIDDEN_ACCEPTED)).expect("read fixture");
    let guarded = source.replace(
        "requires order[o] == Paid or order[o] == Shipped",
        "requires order[o] == Paid",
    );
    assert_ne!(guarded, source, "the fixture's weakened guard moved");
    guarded
}

/// The `requirements` document `document generate` renders for `spec.fsl` in
/// `directory`, rendered through the library, which does not run the gate.
fn ungated_document(directory: &Path) -> String {
    let source = std::fs::read_to_string(directory.join("spec.fsl")).expect("read spec");
    let claims =
        fsl_tools::project_requirement_claims_from_source(&source, Some("spec.fsl"), directory)
            .expect("project claims");
    let resolver = fsl_core::FsResolver::new(directory);
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("lower spec");
    let model = fsl_core::build_model(kernel.clone()).expect("build model");
    fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        &source,
        fsl_tools::Locale::parse("ja").expect("ja locale"),
        None,
        None,
        None,
    )
    .expect("render document")
    .markdown
}

fn git(cwd: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A Git repository holding `source` as a committed `spec.fsl`.
fn approval_repository(name: &str, source: &str) -> PathBuf {
    let root = scratch(name);
    std::fs::write(root.join("spec.fsl"), source).expect("write spec");
    git(&root, &["init", "-q"]);
    git(
        &root,
        &["config", "user.email", "approval-test@example.com"],
    );
    git(&root, &["config", "user.name", "Approval Test"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["add", "spec.fsl"]);
    git(&root, &["commit", "-qm", "baseline"]);
    root
}

fn copy_fixture(directory: &Path, fixture: &str) {
    std::fs::copy(repository_file(fixture), directory.join("spec.fsl")).expect("copy fixture");
}

/// #1242: explain (and its readable form) reported `explained`, exit 0, with
/// `witnesses: []` for a spec whose traces the gate refuses.
#[test]
fn explain_reports_the_gate_error_instead_of_an_explanation() {
    let root = repository_file(".");
    for spec in REFUSED {
        assert_refused(&root, spec, &["explain", spec]);
        assert_refused(&root, spec, &["explain", spec, "--readable"]);
    }
}

/// html already exited 2 (its verification failed), but wrote a report whose
/// witness section was silently empty. It now stops at explain's gate error
/// and writes nothing.
#[test]
fn html_writes_no_report_for_a_refused_spec() {
    let root = repository_file(".");
    for spec in REFUSED {
        let directory = scratch("html");
        let report = directory.join("report.html");
        let report_arg = report.to_str().expect("UTF-8 path");
        assert_refused(&root, spec, &["html", spec, "-o", report_arg]);
        assert!(!report.exists(), "{spec}: html wrote {}", report.display());
        std::fs::remove_dir_all(directory).expect("remove scratch directory");
    }
}

/// #1243: ledger rendered a ledger (exit 2 from its verification) with a row
/// per case, including the refused one.
#[test]
fn ledger_reports_the_gate_error_instead_of_a_ledger() {
    let root = repository_file(".");
    for spec in REFUSED {
        assert_refused(&root, spec, &["ledger", spec]);
    }
}

/// #1243: `document generate` / `document claims` / `document check` rendered,
/// projected, and conformance-checked a document stating the refused cases
/// as claims, exit 0.
#[test]
fn document_commands_report_the_gate_error() {
    for fixture in REFUSED {
        let directory = scratch("document");
        copy_fixture(&directory, fixture);
        std::fs::write(
            directory.join("requirements.md"),
            ungated_document(&directory),
        )
        .expect("write document");
        assert_refused(
            &directory,
            "spec.fsl",
            &["document", "generate", "spec.fsl"],
        );
        assert_refused(&directory, "spec.fsl", &["document", "claims", "spec.fsl"]);
        assert_refused(
            &directory,
            "spec.fsl",
            &["document", "check", "spec.fsl", "requirements.md"],
        );
        std::fs::remove_dir_all(directory).expect("remove scratch directory");
    }
}

/// The same document commands on the guarded spec still render: the gate
/// passes and nothing is refused.
#[test]
fn document_commands_render_a_spec_the_gate_passes() {
    let directory = scratch("document-guarded");
    std::fs::write(directory.join("spec.fsl"), guarded()).expect("write spec");
    let generated = run(&directory, &["document", "generate", "spec.fsl"]);
    assert_eq!(generated.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(generated.stdout).expect("UTF-8 document"),
        ungated_document(&directory)
    );
    std::fs::remove_dir_all(directory).expect("remove scratch directory");
}

/// #1243: `approval create --kind requirements_document` recorded an approval
/// of a document whose fresh rendering states the refused cases (exit 0).
#[test]
fn requirements_document_approval_reports_the_gate_error() {
    let source =
        std::fs::read_to_string(repository_file(FORBIDDEN_ACCEPTED)).expect("read fixture");
    let root = approval_repository("approval-document", &source);
    std::fs::write(root.join("requirements.md"), ungated_document(&root)).expect("write document");
    assert_refused(
        &root,
        "spec.fsl",
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "requirements_document",
            "--artifact",
            "requirements.md",
            "--approver",
            "alice",
            "-o",
            "requirements.approval.json",
        ],
    );
    assert!(!root.join("requirements.approval.json").exists());
    std::fs::remove_dir_all(root).expect("remove scratch repository");
}

/// #1243 (the case reported on the issue): `approval create --kind ledger`
/// recorded an approval of a ledger listing the gate's error (exit 0).
#[test]
fn ledger_approval_reports_the_gate_error() {
    let source =
        std::fs::read_to_string(repository_file(FORBIDDEN_ACCEPTED)).expect("read fixture");
    let root = approval_repository("approval-ledger", &source);
    // Before the fix this wrote the ledger (exiting 2 for its verification);
    // now it writes nothing, and the approval refuses either way.
    let _ = run(
        &root,
        &["ledger", "spec.fsl", "--depth", "2", "-o", "review.md"],
    );
    assert_refused(
        &root,
        "spec.fsl",
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "ledger",
            "--artifact",
            "review.md",
            "--approver",
            "alice",
            "--depth",
            "2",
            "-o",
            "ledger.approval.json",
        ],
    );
    assert!(!root.join("ledger.approval.json").exists());
    std::fs::remove_dir_all(root).expect("remove scratch repository");
}

/// `approval check` of a ledger record reproduces the ledger: once the spec's
/// traces are refused it reports the gate's error instead of `drifted`, exit 0.
#[test]
fn ledger_approval_check_reports_the_gate_error_after_the_spec_breaks() {
    let root = approval_repository("approval-check", &guarded());
    let ledger = run(
        &root,
        &["ledger", "spec.fsl", "--depth", "2", "-o", "review.md"],
    );
    assert_eq!(
        ledger.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ledger.stdout)
    );
    let created = run(
        &root,
        &[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "ledger",
            "--artifact",
            "review.md",
            "--approver",
            "alice",
            "--depth",
            "2",
            "-o",
            "ledger.approval.json",
        ],
    );
    assert_eq!(
        created.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&created.stdout)
    );
    copy_fixture(&root, FORBIDDEN_ACCEPTED);
    git(&root, &["commit", "-qam", "weaken the cancel guard"]);
    assert_refused(
        &root,
        "spec.fsl",
        &[
            "approval",
            "check",
            "spec.fsl",
            "--record",
            "ledger.approval.json",
        ],
    );
    std::fs::remove_dir_all(root).expect("remove scratch repository");
}

fn trace_nodes(analysis: &Value) -> Vec<&Value> {
    analysis["nodes"]
        .as_array()
        .expect("analysis nodes")
        .iter()
        .filter(|node| matches!(node["kind"].as_str(), Some("acceptance" | "forbidden")))
        .collect()
}

/// D4: the analyze graph does not fail on a refused contract; it projects the
/// cases and marks each `trace_gate: "unchecked"`. Before, they were projected
/// like checked cases.
#[test]
fn analyze_marks_the_cases_of_a_refused_contract_unchecked() {
    let root = repository_file(".");
    for (spec, case) in [
        (FORBIDDEN_ACCEPTED, "forbidden:FB-1"),
        (ACCEPTANCE_FALSE, "acceptance:AC-1"),
    ] {
        let output = run(&root, &["analyze", spec]);
        assert_eq!(output.status.code(), Some(0), "{spec}: analyze exit");
        let analysis = json(&output);
        let nodes = trace_nodes(&analysis);
        assert_eq!(
            nodes
                .iter()
                .map(|node| node["id"].as_str())
                .collect::<Vec<_>>(),
            [Some(case)],
            "{spec}: the case is still projected"
        );
        assert_eq!(nodes[0]["trace_gate"], "unchecked", "{spec}: {}", nodes[0]);
    }
    let directory = scratch("analyze-guarded");
    std::fs::write(directory.join("spec.fsl"), guarded()).expect("write spec");
    let analysis = json(&run(&directory, &["analyze", "spec.fsl"]));
    let nodes = trace_nodes(&analysis);
    assert_eq!(nodes.len(), 1, "{analysis}");
    assert!(
        nodes[0].get("trace_gate").is_none(),
        "a checked case carries no mark: {}",
        nodes[0]
    );
    std::fs::remove_dir_all(directory).expect("remove scratch directory");
}

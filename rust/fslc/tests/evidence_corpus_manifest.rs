// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Command-owned manifest for the corpus's evidence-only documents: `causal`
//! sidecars and the `fsl-ai` agent/component/project surface (issue #645,
//! #537 C4 residual).
//!
//! These documents are not `check`-shaped: `causal` bypasses dialect
//! dispatch entirely (`docs/DESIGN-causal.md`), and `agent`/`ai_component`
//! evaluate under `fslc ai check`, not `fslc check`, for their declared
//! result. Before this manifest, nothing walked the corpus and failed on an
//! unregistered evidence-only document -- only fixed-path issue tests
//! (`causal_cli.rs`, `issue_468_recursive_agent.rs`,
//! `issue_509_ai_declared_policy_execution.rs`, `issue_542_...rs`,
//! `issue_562_...rs`, `issue_563_...rs`) exercised the 6 files that exist
//! today, by name. A new `examples/causal/*.fsl` or `examples/ai/*.fsl` file
//! would be checked only by `corpus_check_sweep.rs`'s bare `check` -- which
//! proves the file parses, never that its actual evaluating command
//! (`fslc causal check`, `fslc ai check`) was ever run.
//!
//! # Classification is native, not string-sniffed
//!
//! `rust/fsl-wasm`'s Worker-parity harness (`test-browser.mjs`) already
//! classifies this exact corpus the same way `fslc ai check`'s own dispatch
//! does, reusing native functions rather than re-deriving the sniff:
//! `fsl_syntax::is_causal_source` (causal bypasses dialect dispatch, so the
//! FRONTENDS-restricted `dialect_keyword` cannot see it -- `is_causal_source`
//! is the pre-dispatch sniff `causal.rs`'s own doc comment names for exactly
//! this purpose) and `fsl_syntax::dialect_keyword` for the registered
//! `agent`/`ai_component` frontends. `fslc_rust::frontend_output::is_ai_project`
//! is the same predicate `run_ai_check` (`rust/fslc/src/main.rs`) itself
//! calls first to route to `run_ai_project_check` instead of the plain
//! component/agent path. `classify` below is that same three-function
//! decision tree, not a fourth reimplementation of it.
//!
//! # Roster
//!
//! `causal` (3): `examples/causal/{incident_response,marketing_funnel,
//! subscription_retention}.fsl`. `funnel.fsl`, `incident_system.fsl`, and
//! `subscription_business.fsl` are `spec`-dialect design/business companions
//! the causal files `uses ... from` -- ordinary specs, already owned by
//! `corpus_check_sweep.rs`, not causal documents themselves.
//!
//! `agent`/`ai_component`/`ai project` (3): `examples/ai/
//! recursive_support_agent.fsl` (`agent`), `examples/ai/
//! refund_agent_tool_safety.fsl` (plain `ai_component`), `examples/ai/
//! support_answer_quality.fsl` (`ai_component` carrying `dataset`/
//! `evaluator`/`statistical_property` blocks, i.e. an fsl-ai project
//! declaration). `examples/annotations/annotated_ai_component.fsl` is a
//! fourth `ai_component` file in the corpus, but its own doc comment
//! declares `fslc check`, not `fslc ai check`, as its evaluating command
//! (issue #281's annotation-syntax sample); it is a registered
//! `EvidenceExclusion` below, not a seventh roster row.

use std::process::Command;

use serde_json::Value;

#[path = "support/mod.rs"]
mod support;
use support::{corpus_files, repo_relative, root};

/// What `classify` answers for one corpus document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Construct {
    Causal,
    AiAgent,
    /// A plain `ai_component` file (the hard-contract path, `verified_under_assumptions`).
    AiComponent,
    /// An `ai_component` file whose `is_ai_project` predicate is true (the
    /// `dataset`/`evaluator`/... project path, `ai_project_analyzed`).
    AiProject,
}

/// The native classification `fslc ai check`'s own dispatch uses, applied
/// read-only for roster purposes (issue #645 step 1: "native, not a string
/// sniff reinvented").
fn classify(source: &str) -> Option<Construct> {
    if fsl_syntax::is_causal_source(source) {
        return Some(Construct::Causal);
    }
    if fslc_rust::frontend_output::is_ai_project(source) {
        return Some(Construct::AiProject);
    }
    match fsl_syntax::dialect_keyword(source) {
        Ok("agent") => Some(Construct::AiAgent),
        Ok("ai_component") => Some(Construct::AiComponent),
        _ => None,
    }
}

/// Where a row's `declared_by` citation points, re-checked by
/// `manifest_rows_agree_with_their_citations`: some line of `path` must
/// still contain `anchor`. Unlike `refine_corpus_parity.rs`'s same-named
/// type, the check here is whole-file, not same-line -- these are prose
/// citations from README/design-doc paragraphs, and the command line and the
/// stated result routinely wrap across two lines of Markdown.
struct Declaration {
    path: &'static str,
    anchor: &'static str,
}

/// One live manifest row.
struct Row {
    path: &'static str,
    construct: Construct,
    /// The command tokens before the file's own path argument, e.g.
    /// `["causal", "check"]` or `["ai", "check"]`.
    command: &'static [&'static str],
    expected_result: &'static str,
    declared_by: &'static str,
    declaration: Declaration,
}

const ROWS: &[Row] = &[
    Row {
        path: "examples/causal/incident_response.fsl",
        construct: Construct::Causal,
        command: &["causal", "check"],
        expected_result: "causal_model_checked",
        declared_by: "docs/LANGUAGE.md:2726-2730 (`causal <Name> { ... }` is evaluated by \
                      `fslc causal check model.fsl`) and docs/DESIGN-causal.md:475 (the \
                      command's success envelope: `\"result\": \"causal_model_checked\"`)",
        declaration: Declaration {
            path: "docs/DESIGN-causal.md",
            anchor: "\"result\": \"causal_model_checked\"",
        },
    },
    Row {
        path: "examples/causal/marketing_funnel.fsl",
        construct: Construct::Causal,
        command: &["causal", "check"],
        expected_result: "causal_model_checked",
        declared_by: "docs/LANGUAGE.md:2726-2730 and docs/DESIGN-causal.md:475, as for \
                      incident_response.fsl above -- one command contract, three fixtures",
        declaration: Declaration {
            path: "docs/DESIGN-causal.md",
            anchor: "\"result\": \"causal_model_checked\"",
        },
    },
    Row {
        path: "examples/causal/subscription_retention.fsl",
        construct: Construct::Causal,
        command: &["causal", "check"],
        expected_result: "causal_model_checked",
        declared_by: "docs/LANGUAGE.md:2726-2730 and docs/DESIGN-causal.md:475, as above",
        declaration: Declaration {
            path: "docs/DESIGN-causal.md",
            anchor: "\"result\": \"causal_model_checked\"",
        },
    },
    Row {
        path: "examples/ai/recursive_support_agent.fsl",
        construct: Construct::AiAgent,
        command: &["ai", "check"],
        expected_result: "agent_analyzed",
        declared_by: "examples/ai/README.md:11 (`fslc ai check \
                      examples/ai/recursive_support_agent.fsl`) and :33-34 (`fslc ai check` \
                      returns `agent_analyzed`, deterministic `agent_ir`, ...)",
        declaration: Declaration {
            path: "examples/ai/README.md",
            anchor: "agent_analyzed",
        },
    },
    Row {
        path: "examples/ai/refund_agent_tool_safety.fsl",
        construct: Construct::AiComponent,
        command: &["ai", "check"],
        expected_result: "verified_under_assumptions",
        declared_by: "examples/ai/README.md:10 (`fslc ai check \
                      examples/ai/refund_agent_tool_safety.fsl`) and :28-30 (`fslc ai check` \
                      lowers the hard-contract authority model ... and returns \
                      `verified_under_assumptions` when the finite hard-contract expansion \
                      verifies)",
        declaration: Declaration {
            path: "examples/ai/README.md",
            anchor: "verified_under_assumptions",
        },
    },
    Row {
        path: "examples/ai/support_answer_quality.fsl",
        construct: Construct::AiProject,
        command: &["ai", "check"],
        expected_result: "ai_project_analyzed",
        declared_by: "docs/LANGUAGE.md:2459-2462 (`fslc ai check` parses a project-level \
                      fsl-ai evidence declaration -- combining `ai_component`, `dataset`, \
                      `evaluator`, `failure_mode`, `statistical_property`, `ai_migration`, \
                      `observed_property` -- with the same parser the evidence commands run, \
                      and returns `ai_project_analyzed`) and examples/ai/README.md:43 \
                      (`support_answer_quality.fsl` declares `dataset`, `evaluator`, ... \
                      blocks, i.e. is that shape)",
        declaration: Declaration {
            path: "docs/LANGUAGE.md",
            anchor: "ai_project_analyzed",
        },
    },
];

/// A classified document that carries no manifest row, and the re-checked
/// fact that keeps its exclusion honest. Mirrors
/// `corpus_expectation_manifest.rs::StructuralExclusion`.
struct EvidenceExclusion {
    path: &'static str,
    reason: &'static str,
    /// Substring that must still appear verbatim in `path`'s own source: the
    /// file's own doc comment states its evaluating command.
    premise_needle: &'static str,
}

const EXCLUSIONS: &[EvidenceExclusion] = &[EvidenceExclusion {
    path: "examples/annotations/annotated_ai_component.fsl",
    reason: "issue #281 annotation-syntax sample (nested `@requirement(...)` on ai_component \
             declarations), not fsl-ai evidence; its own doc comment declares `fslc check`, not \
             `fslc ai check`, and rust/fsl-tools/tests/document.rs already exercises it. Bare \
             `check` already owns it via corpus_check_sweep.rs's >150-file sweep",
    premise_needle: "fslc check examples/annotations/annotated_ai_component.fsl",
}];

/// Every `causal`/`agent`/`ai_component`(project-or-not) document under
/// `specs/` + `examples/` must carry a manifest row or a reasoned exclusion,
/// in both directions: an unclassified-but-registered path is stale, an
/// unregistered classified path is the gap this manifest exists to close.
#[test]
fn every_evidence_only_document_is_registered() {
    let root = root();
    let mut found: Vec<(String, Construct)> = Vec::new();
    for path in corpus_files(&root) {
        let source = std::fs::read_to_string(&path).expect("read corpus source");
        if let Some(construct) = classify(&source) {
            found.push((repo_relative(&root, &path), construct));
        }
    }
    let found_count = found.len();
    assert_eq!(
        found_count, 7,
        "expected exactly 7 evidence-only documents in the corpus today (3 causal + 3 fsl-ai \
         rows + 1 annotation-sample exclusion), found {found_count}: {found:?}. If this is an \
         intentional new document, register a Row or an EvidenceExclusion; if it is a \
         regression in `classify`, fix that instead",
    );

    let registered: std::collections::BTreeMap<&str, Construct> = ROWS
        .iter()
        .map(|row| (row.path, row.construct))
        .chain(
            EXCLUSIONS
                .iter()
                .map(|exclusion| (exclusion.path, Construct::AiComponent)),
        )
        .collect();
    assert_eq!(
        registered.len(),
        ROWS.len() + EXCLUSIONS.len(),
        "a path is registered twice across ROWS and EXCLUSIONS"
    );

    let mut failures = Vec::new();
    for (path, construct) in &found {
        match registered.get(path.as_str()) {
            None => failures.push(format!(
                "{path}: classified as {construct:?} but registered in neither ROWS nor \
                 EXCLUSIONS in evidence_corpus_manifest.rs. Add a row citing the primary \
                 declaration of its evaluating command, or a reasoned exclusion."
            )),
            Some(registered_construct) if registered_construct != construct => {
                failures.push(format!(
                    "{path}: classified as {construct:?} but registered as \
                     {registered_construct:?}. `classify`'s decision changed; update the row."
                ));
            }
            Some(_) => {}
        }
    }
    let found_paths: std::collections::BTreeSet<&str> =
        found.iter().map(|(path, _)| path.as_str()).collect();
    for path in registered.keys() {
        if !found_paths.contains(path) {
            failures.push(format!(
                "{path}: registered here but `classify` no longer recognizes it as \
                 causal/agent/ai_component (deleted, moved, or changed shape). Remove the stale \
                 entry."
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Re-checks every `EvidenceExclusion`'s premise: the excluded file's own
/// source must still contain the command it claims to declare. Self-retiring
/// (#568) -- the day `annotated_ai_component.fsl` starts declaring
/// `fslc ai check` instead, this fails and the file must join `ROWS`.
#[test]
fn every_evidence_exclusion_premise_still_holds() {
    let root = root();
    let mut failures = Vec::new();
    for exclusion in EXCLUSIONS {
        let source = std::fs::read_to_string(root.join(exclusion.path))
            .unwrap_or_else(|error| panic!("read {}: {error}", exclusion.path));
        if !source.contains(exclusion.premise_needle) {
            failures.push(format!(
                "{}: STALE. The file no longer contains {:?}, so the reason ({}) no longer has \
                 anything backing it.",
                exclusion.path, exclusion.premise_needle, exclusion.reason
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every row's `declared_by` citation must still be true: some line of the
/// cited file must still contain the row's `declaration.anchor`. Checked
/// whole-file (not same-line, unlike `refine_corpus_parity.rs`'s
/// `Declaration`) because these are prose citations, and every anchor here
/// already includes the row's own `expected_result` string, so a match also
/// confirms the file still states that verdict.
#[test]
fn manifest_rows_agree_with_their_citations() {
    let root = root();
    let mut failures = Vec::new();
    for row in ROWS {
        let citing = std::fs::read_to_string(root.join(row.declaration.path))
            .unwrap_or_else(|error| panic!("read {}: {error}", row.declaration.path));
        if !citing.contains(row.declaration.anchor) {
            failures.push(format!(
                "{}: no line of {} contains {:?} any more. `declared_by` is stale: {}",
                row.path, row.declaration.path, row.declaration.anchor, row.declared_by
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn run_fslc(args: &[String]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc {}`: {error}; stderr={}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status = output.status.code().unwrap_or_else(|| {
        panic!(
            "`fslc {}` terminated by signal, no exit code",
            args.join(" ")
        )
    });
    (value, status)
}

/// Compares one row's declaration against an observed envelope and exit
/// status; extracted so the real run and the negative control below share
/// one comparator (same rationale as
/// `corpus_expectation_manifest.rs::compare_case`).
fn compare_row(
    path: &str,
    expected_result: &str,
    envelope: &Value,
    exit_status: i32,
) -> Option<String> {
    let result = envelope
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{path}: envelope has no string `result` field: {envelope}"));
    if result != expected_result {
        return Some(format!(
            "{path}: declared expected-result={expected_result:?}, got result={result:?} \
             ({envelope})"
        ));
    }
    let expected_exit =
        fslc_rust::outcome::exit_status(&serde_json::json!({"result": expected_result}), 2);
    if exit_status != expected_exit {
        return Some(format!(
            "{path}: result={result:?} binds exit={expected_exit} \
             (fslc_rust::outcome::exit_status), got exit={exit_status}"
        ));
    }
    None
}

/// Runs every row's declared command and compares `result` and exit status
/// (#537 C4: both channels). All 6 declared results are success-class in
/// `fslc_rust::outcome::outcome_class`, so exit 0 in every row; the
/// comparator still derives it from production code rather than asserting
/// `0` directly, the same discipline `corpus_expectation_manifest.rs` uses.
#[test]
fn native_matches_the_declared_expectation_for_every_registered_row() {
    let root = root();
    let mut failures = Vec::new();
    for row in ROWS {
        let abs_path = root.join(row.path).to_str().expect("utf8 path").to_owned();
        let args: Vec<String> = row
            .command
            .iter()
            .map(|token| (*token).to_owned())
            .chain(std::iter::once(abs_path))
            .collect();
        let (envelope, exit_status) = run_fslc(&args);
        if let Some(failure) = compare_row(row.path, row.expected_result, &envelope, exit_status) {
            failures.push(failure);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Negative control (AGENTS.md; issue #645 step 6): proves `compare_row` can
/// detect a wrong expectation, matching
/// `corpus_expectation_manifest.rs::compare_case_reports_a_wrong_expectation_as_a_mismatch`.
#[test]
fn compare_row_reports_a_wrong_expectation_as_a_mismatch() {
    let observed = serde_json::json!({"result": "causal_model_checked"});

    let wrong_result = compare_row(
        "examples/causal/does_not_matter.fsl",
        "agent_analyzed",
        &observed,
        0,
    );
    assert!(
        wrong_result.is_some(),
        "declared `agent_analyzed` vs observed `causal_model_checked` must be reported"
    );

    let wrong_exit = compare_row(
        "examples/causal/does_not_matter.fsl",
        "causal_model_checked",
        &observed,
        1,
    );
    assert!(
        wrong_exit.is_some(),
        "declared `causal_model_checked` (exit 0) vs observed exit 1 must be reported"
    );

    let right = compare_row(
        "examples/causal/does_not_matter.fsl",
        "causal_model_checked",
        &observed,
        0,
    );
    assert!(
        right.is_none(),
        "a correct expectation must not be reported as a mismatch: {right:?}"
    );
}

/// `classify`'s own negative control: a corpus document from a dialect none
/// of the four evidence-only paths recognize must classify as `None`, not
/// silently join one of the buckets. Uses a real corpus file
/// (`specs/cart_v1.fsl`, an ordinary kernel spec) rather than a synthetic
/// string, so the control exercises the same `fsl_syntax`/`fslc_rust`
/// functions the real classification does.
#[test]
fn classify_does_not_misclassify_an_ordinary_spec() {
    let root = root();
    let source =
        std::fs::read_to_string(root.join("specs/cart_v1.fsl")).expect("read specs/cart_v1.fsl");
    assert_eq!(
        classify(&source),
        None,
        "an ordinary kernel spec must not classify as an evidence-only construct"
    );
}

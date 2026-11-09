// SPDX-License-Identifier: Apache-2.0

//! Exhaustive native `check` sweep over `specs/` + `examples/` (issue #485
//! item 5). `rust/fsl-lsp/tests/corpus.rs` only parses and indexes every
//! file; it never builds a checked model, so a file that fails `fslc check`
//! can rot silently forever — this is exactly how the 18 files issue #485
//! repaired (a `requirement` annotation text conflict) went undetected on
//! `main`. This test closes that hole: every `.fsl` file must either
//! succeed under `fslc check`, declare `// expected-result: error` somewhere
//! in its header, or be on the explicit, reasoned exclusion below.
//! `refinement`-dialect files are excluded structurally, not because
//! coverage is known to exist elsewhere: a mapping file has no `state` block
//! to build a checked model from, so `fslc check` always reports
//! `semantics`/"spec has no state block" for one regardless of whether the
//! mapping itself is sound. Whether a given mapping is actually exercised by
//! `fslc refine` is a separate, narrower claim this sweep does not make.
//! That claim is owned by `refine_corpus_parity.rs`, whose manifest binds
//! every one of these files to a `refine` run or to a self-retiring
//! exclusion (issue #593, #537 C4), and which walks the same corpus rather
//! than listing paths, so a mapping added here cannot escape both tests at
//! once. `examples/gallery/injected/` (its own primary/blind detector matrix
//! in `injection_detector_matrix.rs`) is a genuine verified-elsewhere
//! category, not a silent skip.
//!
//! This sweep used to also assert the *positive* half of the
//! `examples/gallery/{errors,adversarial}` header convention: that a file
//! declaring `expected-result: error` **for `check` specifically** actually
//! fails under `check`. That assertion now belongs to
//! `corpus_expectation_manifest.rs`, which runs every header's declared
//! command verbatim (not just `check`) and compares `result`/`kind`/exit
//! against the declaration (issue #645, #537 C4 residual). This file's
//! per-file rule remains deliberately narrower than an oracle: a file that
//! declares no error anywhere must not fail `check` unexpectedly. The
//! independent `check` and `verify` Verdict Conservation Laws below bind
//! their envelopes to process exits without declaring a per-file result; the
//! verify owner checks the exact public 0/1/2/3 mapping, not only zero versus
//! non-zero.
//! Splitting these properties prevents a gap, not a hole:
//! `corpus_expectation_manifest.rs`'s roster is derived from the same header
//! convention this sweep reads, so a `check`-targeted declared fixture cannot
//! lose its owner by falling between the two files.
//!
//! `check_result_and_exit_status_never_contradict` and
//! `verify_result_and_exit_status_never_contradict` are independent
//! properties over the same corpus sweep (issue #537 C2 and issue #761 F1,
//! Verdict Conservation Law). They hold no per-file oracle and need none of
//! the exclusions above: unlike "does this file check clean", "does the exit
//! status agree with the result class" is a closed law that every corpus file
//! -- including deliberately-failing fixtures and the
//! `refinement`/`examples/gallery/injected/` categories excluded above --
//! must satisfy. Their class definition is production code
//! (`fslc_rust::outcome::outcome_class`), not a list in this file.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[path = "support/mod.rs"]
mod support;
use support::{corpus_files, repo_relative, root, top_level_keyword};

/// Repo-relative path (forward slashes) -> reason a bare
/// check-must-pass-or-declare-error rule does not apply. Each reason names
/// the test or issue that actually owns this file's expected behavior, so a
/// stale entry is reviewable.
const GOVERNANCE_FIXTURE_EXCLUSIONS: &[(&str, &str)] = &[
    (
        "examples/gallery/errors/governance_malformed_business.fsl",
        "malformed `business` fixture consumed by governance_malformed_dependency.fsl's \
         `delegates ... from`; its own parse failure is the fixture's purpose, validated \
         indirectly by cli_regression.rs::native_check_locates_a_malformed_dependency_at_the_governance_reference",
    ),
    (
        "examples/gallery/errors/governance_malformed_dependency.fsl",
        "validated by cli_regression.rs::native_check_locates_a_malformed_dependency_at_the_governance_reference",
    ),
    (
        "examples/gallery/errors/governance_missing_before.fsl",
        "validated by cli_regression.rs::native_check_rejects_an_incomplete_governance_contract",
    ),
    (
        "examples/gallery/adversarial/governance_semantic_after.fsl",
        "fixture referenced by governance_semantic_dependency.fsl's `after ... from`; its own \
         undefined-type failure is the fixture's purpose, validated indirectly by \
         cli_regression.rs::native_check_locates_a_semantic_dependency_error_at_the_preservation",
    ),
    (
        "examples/gallery/adversarial/governance_semantic_dependency.fsl",
        "validated by cli_regression.rs::native_check_locates_a_semantic_dependency_error_at_the_preservation",
    ),
];

/// Runs `fslc check` on `path` and returns the parsed stdout envelope
/// together with the process exit status, so a single sweep of the corpus
/// can serve both the per-file oracle below and the oracle-free
/// `result`/exit conservation law in
/// `check_result_and_exit_status_never_contradict`.
fn run_check(path: &Path) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", path.to_str().expect("utf8 path")])
        .current_dir(root())
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc check {}`: {error}; stderr={}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status = output.status.code().unwrap_or_else(|| {
        panic!(
            "`fslc check {}` terminated by signal, no exit code",
            path.display()
        )
    });
    (value, status)
}

/// Runs the bounded BMC baseline used by the corpus-wide verify/exit
/// conservation law. These arguments also match the verify side of the
/// ledger comparison below.
fn run_verify(path: &Path) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "verify",
            path.to_str().expect("utf8 path"),
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "--engine",
            "bmc",
        ])
        .current_dir(root())
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc verify {}`: {error}; stderr={}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status = output.status.code().unwrap_or_else(|| {
        panic!(
            "`fslc verify {}` terminated by signal, no exit code",
            path.display()
        )
    });
    (value, status)
}

/// Exact public exit contract for a top-level `fslc verify` envelope.
/// Keep this independent from the CLI's final process-status normalization so
/// a production mapping mutation cannot change both produced and expected.
fn expected_verify_exit(envelope: &Value) -> i32 {
    match envelope.get("result").and_then(Value::as_str) {
        Some("verified" | "proved") => 0,
        // `refinement_failed` / `impl_violated`: a failed inline `implements`
        // seam or governance preservation folded into the verdict
        // (`docs/manual/LANGUAGE.md` exit table; issue #1303 made
        // `examples/refinement_liveness/governance_detects_safety_loss.fsl`
        // the corpus's first such `verify`).
        Some(
            "violated" | "reachable_failed" | "unknown_cti" | "unknown_budget"
            | "refinement_failed" | "impl_violated",
        ) => 1,
        Some("error") if envelope.get("kind").and_then(Value::as_str) == Some("internal") => 3,
        Some("error") => 2,
        result => panic!(
            "unregistered `fslc verify` result {result:?}; update the public exact-exit contract"
        ),
    }
}

#[test]
fn every_corpus_spec_checks_ok_or_declares_its_error() {
    let root = root();
    let files = corpus_files(&root);
    assert!(
        files.len() > 150,
        "corpus scan floor: found only {} .fsl files under specs/+examples/, expected 150+ \
         (the directory walk may be broken)",
        files.len()
    );

    let governance_exclusions = GOVERNANCE_FIXTURE_EXCLUSIONS
        .iter()
        .map(|&(path, _)| path)
        .collect::<BTreeSet<_>>();
    let mut failures = Vec::new();

    for path in &files {
        let rel = repo_relative(&root, path);

        if rel.starts_with("examples/gallery/injected/") {
            continue; // own primary/blind matrix: injection_detector_matrix.rs
        }

        let source = std::fs::read_to_string(path).expect("read corpus source");
        if top_level_keyword(&source) == Some("refinement") {
            continue; // no `state` block to `check`; `refine` coverage is owned by refine_corpus_parity.rs
        }

        if governance_exclusions.contains(rel.as_str()) {
            continue;
        }

        let (result, _exit) = run_check(path);

        // Whether a *declared* `check`-targeted error actually reproduces is
        // `corpus_expectation_manifest.rs`'s claim, not this sweep's: it runs
        // every header's `expected-command` verbatim and compares
        // `result`/`kind`/exit against the declaration. This sweep keeps only
        // the complementary half -- a file that declares no error anywhere
        // must not fail `check` unexpectedly -- which needs no oracle beyond
        // "does the header say `expected-result: error`".
        let declares_error = support::headers(&source)
            .get("expected-result")
            .is_some_and(|r| r == "error");
        let is_error = result["result"] == "error";
        if !declares_error && is_error {
            failures.push(format!(
                "{rel}: `fslc check` unexpectedly failed and the file does not declare \
                 `expected-result: error` anywhere (add the header, register a reasoned \
                 exclusion, or fix the regression): {result}"
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// `fslc lint` (current edition) sweep over the same corpus (issue #608).
// Agents and readers copy the shipped examples, and the 5.0 migration guide
// sends users to `lint`, so a corpus file must lint clean (exit 0, zero
// findings) unless it is listed below with the exit and the finding codes
// (with counts) it is expected to keep. Each entry says why the findings cannot
// be fixed in the file itself. An entry is exact: a listed file that gains a
// finding, loses one, or changes exit fails this test, so a new finding is
// caught and a stale entry has to be updated.
const INJECTED_VARIANT: &str = "injected-defect variant: its IDs and legacy tags are the signals \
     `examples/gallery/injected/MATRIX.json` and its `expect-signal` header pin for \
     injection_detector_matrix.rs";
const FIXTURE_FAILS_TO_LOAD: &str = "error/adversarial fixture that fails to load by design, so \
     `lint` exits 2; corpus_expectation_manifest.rs or cli_regression.rs owns the failure";
const GALLERY_FAILURE_FIXTURE: &str = "error/adversarial gallery fixture: its subject is a \
     `check`/`verify` failure whose documented output and tests pin these IDs";
const LINT_UNREADABLE_DIALECT: &str = "`lint` cannot read this dialect yet (#609)";
const POLICY_NAMED_BY_PRESERVE_PROGRESS: &str = "the policy IDs are the names the \
     requirements->business refinement's `preserve progress { respond <name> by ... }` targets, \
     and `respond` takes an identifier (#1782), so a canonical hyphenated POL ID would drop \
     that liveness check";
const GOVERNANCE_SATISFIED_BY_SCOPE_RESCAN: &str = "lint false positive (#1788): on a \
     `CTRL-EXPENSE-001 is satisfied_by policy ...` line the canonical control ID is accepted, \
     but its scope segment is rescanned as a second control ID `EXPENSE-001`";
const PYTHON_CONFORMANCE_HARNESS: &str = "keeps legacy string tags because \
     `examples/e2e/impl/test_conformance.py` loads it with the frozen Python Monitor, whose \
     parser rejects `@requirement` (see the file header)";
const FROZEN_PYTHON_TESTS_LOAD_IT: &str = "keeps its one legacy string tag because frozen-Python \
     reference tests (`tests/test_*.py`) load it with the Python parser, which rejects \
     `@requirement`";
const BYTE_PINNED_FIXTURE: &str = "byte-identity captures pin this file's IDs: \
     `fixtures/issue_1425/guarded_cancel.*.stdout.json` (do not regenerate) and the \
     Python-parity pytest digest in testgen_contract.rs";
const LEGACY_METADATA_FIXTURE: &str = "deliberate legacy-metadata fixture: \
     `rust/fsl-wasm/test-browser.mjs` (#1449) requires exactly one `legacy_string_metadata` \
     warning and `support/edition_dependency_cases.rs` E01 rewrites its legacy line";

/// Repo-relative path, the `fslc lint` exit it keeps, the finding codes with
/// their counts it keeps (empty when lint cannot read the file), and why.
type LintAllowance = (
    &'static str,
    i32,
    &'static [(&'static str, usize)],
    &'static str,
);
const LINT_ALLOWLIST: &[LintAllowance] = &[
    (
        "examples/agentic_rag/agentic_rag_business.fsl",
        1,
        &[("non_canonical_id", 4)],
        POLICY_NAMED_BY_PRESERVE_PROGRESS,
    ),
    (
        "examples/ai/support_answer_quality.fsl",
        2,
        &[],
        LINT_UNREADABLE_DIALECT,
    ),
    (
        "examples/causal/incident_response.fsl",
        2,
        &[],
        LINT_UNREADABLE_DIALECT,
    ),
    (
        "examples/causal/marketing_funnel.fsl",
        2,
        &[],
        LINT_UNREADABLE_DIALECT,
    ),
    (
        "examples/causal/subscription_retention.fsl",
        2,
        &[],
        LINT_UNREADABLE_DIALECT,
    ),
    (
        "examples/consulting/governance_controls.fsl",
        1,
        &[("non_canonical_id", 4)],
        GOVERNANCE_SATISFIED_BY_SCOPE_RESCAN,
    ),
    (
        "examples/e2e/3_design.fsl",
        1,
        &[("legacy_string_metadata", 10)],
        PYTHON_CONFORMANCE_HARNESS,
    ),
    (
        "examples/gallery/adversarial/business_precedence_bypass.fsl",
        1,
        &[("non_canonical_id", 1)],
        GALLERY_FAILURE_FIXTURE,
    ),
    (
        "examples/gallery/adversarial/clever_double_assignment_placement.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/adversarial/governance_semantic_after.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/adversarial/governance_semantic_dependency.fsl",
        1,
        &[("non_canonical_id", 2)],
        GALLERY_FAILURE_FIXTURE,
    ),
    (
        "examples/gallery/errors/error_acceptance_false_expect.fsl",
        1,
        &[("non_canonical_id", 2)],
        GALLERY_FAILURE_FIXTURE,
    ),
    (
        "examples/gallery/errors/forbidden_op_accepted.fsl",
        1,
        &[("non_canonical_id", 2)],
        GALLERY_FAILURE_FIXTURE,
    ),
    (
        "examples/gallery/errors/governance_malformed_business.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/errors/governance_malformed_dependency.fsl",
        1,
        &[("non_canonical_id", 2)],
        GALLERY_FAILURE_FIXTURE,
    ),
    (
        "examples/gallery/errors/name_duplicate_state_variable.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/errors/name_reserved_declaration.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/errors/parse_missing_expression.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/errors/semantics_compose_broken_component.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/errors/semantics_compose_component_parse_failure.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/errors/semantics_duplicate_assignment.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/errors/type_struct_set_field.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/errors/type_undeclared_type.fsl",
        2,
        &[],
        FIXTURE_FAILS_TO_LOAD,
    ),
    (
        "examples/gallery/errors/vacuous_implication_warning.fsl",
        1,
        &[("legacy_string_metadata", 1), ("non_canonical_id", 1)],
        GALLERY_FAILURE_FIXTURE,
    ),
    (
        "examples/gallery/injected/bank__boundary_flip.fsl",
        1,
        &[("legacy_string_metadata", 2), ("non_canonical_id", 5)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/bank__fabricated_constraint.fsl",
        1,
        &[("legacy_string_metadata", 3), ("non_canonical_id", 5)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/bank__guard_weakening.fsl",
        1,
        &[("legacy_string_metadata", 2), ("non_canonical_id", 5)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/bank__invariant_weakening.fsl",
        1,
        &[("legacy_string_metadata", 4), ("non_canonical_id", 6)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/bank__omission.fsl",
        1,
        &[("legacy_string_metadata", 3), ("non_canonical_id", 4)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/bank__over_strengthened_guard.fsl",
        1,
        &[("legacy_string_metadata", 3), ("non_canonical_id", 5)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/bank__unreachable_antecedent.fsl",
        1,
        &[("legacy_string_metadata", 4), ("non_canonical_id", 6)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/order_workflow__boundary_flip.fsl",
        1,
        &[("legacy_string_metadata", 2), ("non_canonical_id", 5)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/order_workflow__fabricated_constraint.fsl",
        1,
        &[("legacy_string_metadata", 1), ("non_canonical_id", 3)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/order_workflow__guard_weakening.fsl",
        1,
        &[("legacy_string_metadata", 3), ("non_canonical_id", 8)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/order_workflow__invariant_weakening.fsl",
        1,
        &[("legacy_string_metadata", 5), ("non_canonical_id", 9)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/order_workflow__omission.fsl",
        1,
        &[("legacy_string_metadata", 4), ("non_canonical_id", 7)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/order_workflow__over_strengthened_guard.fsl",
        1,
        &[("legacy_string_metadata", 3), ("non_canonical_id", 6)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/order_workflow__unreachable_antecedent.fsl",
        1,
        &[("legacy_string_metadata", 3), ("non_canonical_id", 6)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/return_system__boundary_flip.fsl",
        1,
        &[("legacy_string_metadata", 1), ("non_canonical_id", 4)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/return_system__fabricated_constraint.fsl",
        1,
        &[("non_canonical_id", 1)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/return_system__guard_weakening.fsl",
        1,
        &[("legacy_string_metadata", 1), ("non_canonical_id", 4)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/return_system__invariant_weakening.fsl",
        1,
        &[("legacy_string_metadata", 1), ("non_canonical_id", 4)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/return_system__omission.fsl",
        1,
        &[("non_canonical_id", 2)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/return_system__over_strengthened_guard.fsl",
        1,
        &[("legacy_string_metadata", 2), ("non_canonical_id", 4)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/injected/return_system__unreachable_antecedent.fsl",
        1,
        &[("legacy_string_metadata", 1), ("non_canonical_id", 3)],
        INJECTED_VARIANT,
    ),
    (
        "examples/gallery/valid/small_forbidden_guarded_cancel.fsl",
        1,
        &[("non_canonical_id", 2)],
        BYTE_PINNED_FIXTURE,
    ),
    (
        "examples/multi_agent_system/multi_agent_business.fsl",
        1,
        &[("non_canonical_id", 6)],
        POLICY_NAMED_BY_PRESERVE_PROGRESS,
    ),
    (
        "examples/named_predicate.fsl",
        1,
        &[("legacy_string_metadata", 1), ("non_canonical_id", 1)],
        LEGACY_METADATA_FIXTURE,
    ),
    (
        "examples/nfr/sla_worker_design.fsl",
        1,
        &[("legacy_string_metadata", 1)],
        FROZEN_PYTHON_TESTS_LOAD_IT,
    ),
    (
        "examples/nfr/sla_worker_kernel.fsl",
        1,
        &[("legacy_string_metadata", 1)],
        FROZEN_PYTHON_TESTS_LOAD_IT,
    ),
    (
        "examples/nfr/support_sla.fsl",
        1,
        &[("legacy_string_metadata", 1)],
        FROZEN_PYTHON_TESTS_LOAD_IT,
    ),
    (
        "examples/ui_spike/return_ui.fsl",
        1,
        &[("legacy_string_metadata", 1)],
        FROZEN_PYTHON_TESTS_LOAD_IT,
    ),
];

fn run_lint(path: &Path) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["lint", path.to_str().expect("utf8 path")])
        .current_dir(root())
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc lint {}`: {error}; stderr={}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status = output.status.code().unwrap_or_else(|| {
        panic!(
            "`fslc lint {}` terminated by signal, no exit code",
            path.display()
        )
    });
    (value, status)
}

#[test]
fn every_corpus_spec_lints_clean_or_is_allowlisted() {
    let root = root();
    let files = corpus_files(&root);
    assert!(
        files.len() > 150,
        "corpus scan floor: found only {} .fsl files under specs/+examples/, expected 150+ \
         (the directory walk may be broken)",
        files.len()
    );
    let corpus = files
        .iter()
        .map(|path| repo_relative(&root, path))
        .collect::<BTreeSet<_>>();
    let mut failures = Vec::new();
    let mut listed = BTreeSet::new();
    for &(path, _, _, _) in LINT_ALLOWLIST {
        if !listed.insert(path) {
            failures.push(format!("{path}: listed twice in LINT_ALLOWLIST"));
        }
        if !corpus.contains(path) {
            failures.push(format!("{path}: LINT_ALLOWLIST entry is not a corpus file"));
        }
    }

    for path in &files {
        let rel = repo_relative(&root, path);
        let (envelope, exit) = run_lint(path);
        let mut codes = std::collections::BTreeMap::<String, usize>::new();
        for finding in envelope["files"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|file| file["findings"].as_array().into_iter().flatten())
        {
            *codes
                .entry(finding["code"].as_str().unwrap_or("?").to_owned())
                .or_default() += 1;
        }
        match LINT_ALLOWLIST
            .iter()
            .find(|&&(listed, _, _, _)| listed == rel)
        {
            Some(&(_, expected_exit, expected_codes, reason)) => {
                let expected_codes = expected_codes
                    .iter()
                    .map(|&(code, count)| (code.to_owned(), count))
                    .collect::<std::collections::BTreeMap<_, _>>();
                if exit != expected_exit || codes != expected_codes {
                    failures.push(format!(
                        "{rel}: allowlisted with lint exit {expected_exit} and findings \
                         {expected_codes:?} ({reason}) but now exits {exit} with findings \
                         {codes:?}; fix the new finding, or remove or update the stale entry"
                    ));
                }
            }
            None => {
                if exit != 0 || !codes.is_empty() {
                    failures.push(format!(
                        "{rel}: `fslc lint` exits {exit} with findings {codes:?}; rewrite the \
                         file in the canonical form lint asks for, or add a reasoned \
                         LINT_ALLOWLIST entry"
                    ));
                }
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Verdict Conservation Law for `fslc check` (issue #537 C2), checked
/// without any per-file oracle: a success-class `result` must exit 0, and
/// every failure-class `result` must exit non-zero. The envelope itself must
/// also be a JSON object carrying a string `result` field.
///
/// The class definition is `fslc_rust::outcome::outcome_class`, in production
/// code. This test used to carry its own `CHECK_SUCCESS_RESULTS` allowlist,
/// because `check`'s family arms each set their exit code at their own return
/// point and there was no production enumeration to defer to. That is the
/// shape #577 retired 28 instances of: a conservation check whose class
/// definition lives in the test can only ever agree with itself, and the
/// production code it is supposed to constrain is free to drift. The
/// allowlist is gone. An unregistered value now falls to the failure class in
/// `outcome.rs` -- never here -- so a new false-green shape fails loudly at
/// the definition instead of being quietly re-declared in a test fixture.
#[test]
fn check_result_and_exit_status_never_contradict() {
    let root = root();
    let files = corpus_files(&root);
    assert!(
        files.len() > 150,
        "corpus scan floor: found only {} .fsl files under specs/+examples/, expected 150+ \
         (the directory walk may be broken)",
        files.len()
    );

    let mut failures = Vec::new();
    let mut observed_results = BTreeSet::new();

    for path in &files {
        let rel = repo_relative(&root, path);
        let (envelope, exit) = run_check(path);

        let Some(object) = envelope.as_object() else {
            failures.push(format!(
                "{rel}: `fslc check` stdout is not a JSON object: {envelope}"
            ));
            continue;
        };
        let Some(result) = object.get("result").and_then(Value::as_str) else {
            failures.push(format!(
                "{rel}: `fslc check` envelope has no string `result` field: {envelope}"
            ));
            continue;
        };

        observed_results.insert(result.to_owned());
        let is_success = fslc_rust::outcome::outcome_class(&envelope).is_success();
        if is_success && exit != 0 {
            failures.push(format!(
                "{rel}: result={result:?} is success-class but exit={exit} (false red)"
            ));
        } else if !is_success && exit == 0 {
            failures.push(format!(
                "{rel}: result={result:?} is failure-class per \
                 `fslc_rust::outcome::outcome_class` but exit=0 (false green). If this is a \
                 new success-class result, register it there -- not here"
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Not a conservation check: a corpus that happened to contain zero
    // check-failing files would make the `!is_success && exit == 0` arm
    // above vacuously true for every file, so this law's negative side
    // would never actually fire. Confirm the corpus still exercises it.
    assert!(
        observed_results.contains("error"),
        "expected at least one corpus file to produce result=\"error\"; \
         got {observed_results:?} -- the failure-class arm of this law is untested"
    );
}

/// Exact Verdict Conservation Law for `fslc verify` over the complete corpus.
/// Like the check-side owner above, this has no per-file expected verdict:
/// it derives the exact expected exit from each envelope's public result/kind
/// class and binds it to the process status for every accepted, deliberately
/// failing, and structurally inapplicable corpus file without exclusions.
#[test]
fn verify_result_and_exit_status_never_contradict() {
    let root = root();
    let files = corpus_files(&root);
    assert!(
        files.len() > 150,
        "corpus scan floor: found only {} .fsl files under specs/+examples/, expected 150+ \
         (the directory walk may be broken)",
        files.len()
    );

    let mut failures = Vec::new();
    let mut observed_results = BTreeSet::new();

    for path in &files {
        let rel = repo_relative(&root, path);
        let (envelope, exit) = run_verify(path);

        let Some(object) = envelope.as_object() else {
            failures.push(format!(
                "{rel}: `fslc verify` stdout is not a JSON object: {envelope}"
            ));
            continue;
        };
        let Some(result) = object.get("result").and_then(Value::as_str) else {
            failures.push(format!(
                "{rel}: `fslc verify` envelope has no string `result` field: {envelope}"
            ));
            continue;
        };

        observed_results.insert(result.to_owned());
        let expected_exit = expected_verify_exit(&envelope);
        if exit != expected_exit {
            failures.push(format!(
                "{rel}: verify result={result:?}; produced exit={exit}, expected \
                 exit={expected_exit} from the public exact-exit contract"
            ));
        }

        let produced_success_class = fslc_rust::outcome::outcome_class(&envelope).is_success();
        let expected_success_class = expected_exit == 0;
        if produced_success_class != expected_success_class {
            failures.push(format!(
                "{rel}: verify result={result:?}; production outcome_class produced \
                 success={produced_success_class}, expected success={expected_success_class} \
                 from exact exit={expected_exit}"
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    for required in ["verified", "violated", "reachable_failed", "error"] {
        assert!(
            observed_results.contains(required),
            "expected corpus verify to exercise result={required:?}; got {observed_results:?}"
        );
    }
}

#[test]
fn verify_result_classes_map_to_exact_public_exit_codes() {
    for result in ["verified", "proved"] {
        assert_eq!(
            expected_verify_exit(&serde_json::json!({"result": result})),
            0
        );
    }
    for result in [
        "violated",
        "reachable_failed",
        "unknown_cti",
        "unknown_budget",
    ] {
        assert_eq!(
            expected_verify_exit(&serde_json::json!({"result": result})),
            1
        );
    }
    assert_eq!(
        expected_verify_exit(&serde_json::json!({"result": "error", "kind": "semantics"})),
        2
    );
    assert_eq!(
        expected_verify_exit(&serde_json::json!({"result": "error", "kind": "internal"})),
        3
    );
}

/// Issue #901: the production table (`fslc_rust::outcome::Verdict`) and the
/// independent public contract above agree on every verification verdict.
/// The two are written separately on purpose, so a mutation of one table
/// entry turns this test red as well as the product's corpus sweep.
#[test]
fn production_verdict_table_matches_the_public_exit_contract() {
    use fslc_rust::outcome::Verdict;

    for verdict in Verdict::ALL {
        if !verdict.is_verification_verdict() {
            continue;
        }
        let envelope = match verdict {
            Verdict::InternalError => serde_json::json!({"result": "error", "kind": "internal"}),
            Verdict::SpecError => serde_json::json!({"result": "error", "kind": "semantics"}),
            _ => serde_json::json!({"result": verdict.result()}),
        };
        assert_eq!(
            verdict.exit_code(),
            expected_verify_exit(&envelope),
            "{verdict:?}: production exit table disagrees with the public exact-exit contract"
        );
    }
}

/// Runs `fslc` with `args` and returns only its exit status -- `ledger`
/// prints rendered Markdown (not JSON) to stdout when `-o` is omitted, so
/// unlike `run_check` there is no envelope here to parse.
fn run_exit_status(args: &[&str]) -> i32 {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native CLI");
    output
        .status
        .code()
        .unwrap_or_else(|| panic!("`fslc {args:?}` terminated by signal, no exit code"))
}

/// Verdict Conservation Law for `fslc ledger` (issue #592), checked without
/// any per-file oracle -- structurally different from
/// `check_result_and_exit_status_never_contradict` above because it cannot
/// be: `ledger`'s JSON envelope reports `result:"generated"` unconditionally,
/// whether the verification baseline it embeds is clean or violated, so
/// there is no top-level `result` string whose vocabulary a
/// `CHECK_SUCCESS_RESULTS`-style allowlist could key off. `ledger` renders
/// that baseline by calling `run_verify` with the same
/// `--depth`/`--deadlock ignore`/`--engine bmc` `ledger` itself defaults to
/// (`prepare_ledger_report` in `rust/fslc/src/main.rs`), so this law instead
/// runs `fslc verify` at matching arguments as an independent process and
/// requires the two commands' exit-code *class* (0 success vs non-zero
/// failure) to agree for every corpus file: a `ledger` that exits 0 over a
/// spec its own `verify` pass found violated is hiding that verdict.
#[test]
fn ledger_exit_status_agrees_with_its_verify_baseline() {
    let root = root();
    let files = corpus_files(&root);
    assert!(
        files.len() > 150,
        "corpus scan floor: found only {} .fsl files under specs/+examples/, expected 150+ \
         (the directory walk may be broken)",
        files.len()
    );

    let mut failures = Vec::new();
    let mut saw_failure_class = false;

    for path in &files {
        let rel = repo_relative(&root, path);
        let path_str = path.to_str().expect("utf8 path");
        let (_verify_envelope, verify_exit) = run_verify(path);
        let ledger_exit = run_exit_status(&["ledger", path_str, "--depth", "2"]);

        if verify_exit != 0 {
            saw_failure_class = true;
        }
        if (verify_exit == 0) != (ledger_exit == 0) {
            failures.push(format!(
                "{rel}: verify exit={verify_exit} but ledger exit={ledger_exit} at the same \
                 depth/deadlock/engine -- ledger must not hide a verify verdict (issue #592)"
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Not a conservation check: see the identical rationale above.
    assert!(
        saw_failure_class,
        "expected at least one corpus file to fail `fslc verify` at depth 2; \
         the failure-class arm of this law is untested"
    );
}

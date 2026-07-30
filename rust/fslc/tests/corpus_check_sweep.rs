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
//! against the declaration (issue #645, #537 C4 residual). This file keeps
//! exactly two properties, deliberately narrower than a per-file oracle:
//! (i) a file that declares no error anywhere must not fail `check`
//! unexpectedly, and (ii) the Verdict Conservation Law below. Splitting the
//! two prevents a gap, not a hole: `corpus_expectation_manifest.rs`'s roster
//! is derived from the same header convention this sweep reads, so a
//! `check`-targeted declared fixture cannot lose its owner by falling
//! between the two files.
//!
//! `check_result_and_exit_status_never_contradict` below is a second,
//! independent property over the same corpus sweep (issue #537 C2, Verdict
//! Conservation Law). It holds no per-file oracle and needs none of the
//! exclusions above: unlike "does this file check clean", "does the exit
//! status agree with the result class" is a closed law that every corpus
//! file -- including deliberately-failing fixtures and the
//! `refinement`/`examples/gallery/injected/` categories excluded above --
//! must satisfy. Its class definition is production code
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
        let verify_exit = run_exit_status(&[
            "verify",
            path_str,
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "--engine",
            "bmc",
        ]);
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

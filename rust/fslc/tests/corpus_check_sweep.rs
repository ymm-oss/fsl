// SPDX-License-Identifier: Apache-2.0

//! Exhaustive native `check` sweep over `specs/` + `examples/` (issue #485
//! item 5). `rust/fsl-lsp/tests/corpus.rs` only parses and indexes every
//! file; it never builds a checked model, so a file that fails `fslc check`
//! can rot silently forever — this is exactly how the 18 files issue #485
//! repaired (a `requirement` annotation text conflict) went undetected on
//! `main`. This test closes that hole: every `.fsl` file must either
//! succeed under `fslc check`, declare `// expected-result: error` for a
//! header-less (or explicitly `check`-targeted) invocation — the
//! `examples/gallery/{errors,adversarial}` convention — or be on the
//! explicit, reasoned exclusion below. `refinement`-dialect files are
//! excluded structurally, not because coverage is known to exist elsewhere:
//! a mapping file has no `state` block to build a checked model from, so
//! `fslc check` always reports `semantics`/"spec has no state block" for one
//! regardless of whether the mapping itself is sound. Whether a given
//! mapping is actually exercised by `fslc refine` is a separate, narrower
//! claim this sweep does not make. Of the 28 such files, only 6 are
//! exercised by any native test — `refine_corpus_parity.rs` (issue #593):
//! `specs/cart_refines.fsl`, `specs/seat_refines.fsl`,
//! `examples/gallery/errors/refinement_failed_map.fsl`,
//! `examples/gallery/adversarial/refine_mapping_boundary_map.fsl`, and 2 of
//! the 5 `examples/refinement_liveness/*_refines.fsl` files (the
//! `*_progress_refines` pair; the other 3 and the remaining 22 mappings
//! across `agentic_rag`, `multi_agent_system`, `refinement_chain`, and
//! elsewhere are refined by nothing — see #593, tracked separately from
//! this sweep).
//! `examples/gallery/injected/` (its own primary/blind detector matrix in
//! `injection_detector_matrix.rs`) is a genuine verified-elsewhere category,
//! not a silent skip.
//!
//! `check_result_and_exit_status_never_contradict` below is a second,
//! independent property over the same corpus sweep (issue #537 C2, Verdict
//! Conservation Law). It holds no per-file oracle and needs none of the
//! exclusions above: unlike "does this file check clean", "does the exit
//! status agree with the result class" is a closed law that every corpus
//! file -- including deliberately-failing fixtures and the
//! `refinement`/`examples/gallery/injected/` categories excluded above --
//! must satisfy.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

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

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn collect_fsl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read corpus directory") {
        let path = entry.expect("read corpus entry").path();
        if path.is_dir() {
            collect_fsl_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "fsl") {
            out.push(path);
        }
    }
}

fn repo_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("path under workspace root")
        .to_string_lossy()
        .replace('\\', "/")
}

/// The file's top-level dialect keyword (`spec`, `requirements`,
/// `refinement`, `governance`, ...): the first token on the first
/// non-blank, non-`//`-comment line.
fn top_level_keyword(source: &str) -> Option<&str> {
    source
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("//"))
        .and_then(|line| line.split_whitespace().next())
}

/// Parse the `// key: value` header comments in the first 10 lines, the
/// `expected-command`/`expected-result` convention
/// `examples/gallery/{valid,errors,adversarial}` use.
fn headers(source: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for line in source.lines().take(10) {
        let Some(body) = line.trim().strip_prefix("//") else {
            continue;
        };
        if let Some((key, value)) = body.trim().split_once(':') {
            out.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    out
}

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
    let mut files = Vec::new();
    collect_fsl_files(&root.join("specs"), &mut files);
    collect_fsl_files(&root.join("examples"), &mut files);
    files.sort();
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
            continue; // no `state` block to `check`; `fslc refine` coverage tracked by #483, not asserted here
        }

        if governance_exclusions.contains(rel.as_str()) {
            continue;
        }

        let (result, _exit) = run_check(path);

        let file_headers = headers(&source);
        // `expected-command`/`expected-result` may target a *stricter* verb
        // than `check` (`verify`, `refine`): a declared error there does not
        // guarantee `check` itself fails (e.g. vacuity only surfaces under
        // `verify --vacuity error`), so it only pins the "must fail" edge
        // when the header targets `check` specifically. Any declared error
        // (for any command) also makes a `check`-time failure unsurprising
        // — a semantics-class defect (e.g. double assignment) often
        // surfaces at both. Only a *completely undeclared* `check` failure
        // is the structural hole this test exists to catch.
        let declares_error = file_headers
            .get("expected-result")
            .is_some_and(|r| r == "error");
        let targets_check = file_headers
            .get("expected-command")
            .is_none_or(|command| command == "check" || command.starts_with("check "));

        let is_error = result["result"] == "error";
        if targets_check && declares_error && !is_error {
            failures.push(format!(
                "{rel}: declares `expected-result: error` for `check` but got {result}"
            ));
        } else if !declares_error && is_error {
            failures.push(format!(
                "{rel}: `fslc check` unexpectedly failed and the file does not declare \
                 `expected-result: error` anywhere (add the header, register a reasoned \
                 exclusion, or fix the regression): {result}"
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `result` values `fslc check` emits that belong to the success class
/// (exit status must be 0).
///
/// `check`'s own family arms (`run_check_with_tags`, `agent_check_output`,
/// `run_causal_check` in `rust/fslc/src/main.rs`/`causal.rs`) each set their
/// exit code directly at the return site rather than deriving it from
/// `result` through one shared function -- unlike, say, `mutate_exit_status`
/// in `main.rs`, there is no existing single production-code enumeration of
/// `check`'s result vocabulary this test could defer to instead. This is
/// therefore deliberately an allowlist, not a denylist: issue #537 C2
/// requires that aggregation "never default an unknown variant to
/// success", so a `check` result value that is not (yet) listed here falls
/// through to the failure class below and the test fails loudly instead of
/// silently accepting a new false-green shape.
const CHECK_SUCCESS_RESULTS: &[&str] = &[
    "ok",                   // rust/fslc/src/main.rs: run_check_with_tags, agent_check_output
    "causal_model_checked", // rust/fsl-tools/src/causal_analysis.rs
];

/// Verdict Conservation Law for `fslc check` (issue #537 C2), checked
/// without any per-file oracle: a success-class `result` must exit 0, and
/// every other `result` -- including a value not yet in
/// `CHECK_SUCCESS_RESULTS` -- must exit non-zero. The envelope itself must
/// also be a JSON object carrying a string `result` field.
#[test]
fn check_result_and_exit_status_never_contradict() {
    let root = root();
    let mut files = Vec::new();
    collect_fsl_files(&root.join("specs"), &mut files);
    collect_fsl_files(&root.join("examples"), &mut files);
    files.sort();
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
        let is_success = CHECK_SUCCESS_RESULTS.contains(&result);
        if is_success && exit != 0 {
            failures.push(format!(
                "{rel}: result={result:?} is success-class but exit={exit} (false red)"
            ));
        } else if !is_success && exit == 0 {
            failures.push(format!(
                "{rel}: result={result:?} is not a registered success-class result but \
                 exit=0 (false green)"
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

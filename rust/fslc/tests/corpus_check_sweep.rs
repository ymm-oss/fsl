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
//! claim this sweep does not make (of the 28 such files, issue #483 found
//! only the gallery fixtures and `examples/refinement_liveness/*` are
//! refined by any test; the `agentic_rag`/`multi_agent_system` mappings are
//! refined by nothing — see #483, not re-asserted as closed here).
//! `examples/gallery/injected/` (its own primary/blind detector matrix in
//! `injection_detector_matrix.rs`) is a genuine verified-elsewhere category,
//! not a silent skip.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Repo-relative path (forward slashes) -> reason a bare
/// check-must-pass-or-declare-error rule does not apply. Each reason names
/// the test or issue that actually owns this file's expected behavior, so a
/// stale entry is reviewable (and, for issue #475, this test additionally
/// pins the current defect so a fix forces the entry's removal instead of
/// silently going unnoticed).
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

/// Known, currently-broken files pending a fix tracked by a filed issue, not
/// this test's scope. Pins the exact current failure so the entry must be
/// removed (loudly, via a failing assertion) once the fix lands, instead of
/// this sweep silently going green either way.
const KNOWN_DB_DOUBLE_ASSIGNMENT_GAP: &[&str] = &[
    "examples/db/safe_rename_preservation.fsl",
    "examples/db/unsafe_lossy_merge_preservation.fsl",
    "examples/db/unsafe_lossy_split_preservation.fsl",
    "examples/db/unsafe_split_without_annotation.fsl",
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

fn run_check(path: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", path.to_str().expect("utf8 path")])
        .current_dir(root())
        .output()
        .expect("run native CLI");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc check {}`: {error}; stderr={}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        )
    })
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
    let db_gap = KNOWN_DB_DOUBLE_ASSIGNMENT_GAP
        .iter()
        .copied()
        .collect::<BTreeSet<&str>>();

    let mut failures = Vec::new();
    let mut db_gap_regressions = Vec::new();
    let mut seen_db_gap = BTreeSet::new();

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

        let result = run_check(path);

        if db_gap.contains(rel.as_str()) {
            seen_db_gap.insert(rel.clone());
            let is_expected_double_assignment = result["result"] == "error"
                && result["kind"] == "semantics"
                && result["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("assign the same state location more than once"));
            if !is_expected_double_assignment {
                db_gap_regressions.push(format!(
                    "{rel}: no longer the known double-assignment error (remove from \
                     KNOWN_DB_DOUBLE_ASSIGNMENT_GAP and close issue #475): {result}"
                ));
            }
            continue;
        }

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

    let seen_db_gap = seen_db_gap
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<&str>>();
    let missing_db_gap = db_gap.difference(&seen_db_gap).copied().collect::<Vec<_>>();
    assert!(
        missing_db_gap.is_empty(),
        "KNOWN_DB_DOUBLE_ASSIGNMENT_GAP entries no longer found in the corpus scan: {missing_db_gap:?}"
    );
    assert!(
        db_gap_regressions.is_empty(),
        "{}",
        db_gap_regressions.join("\n")
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

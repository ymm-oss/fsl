// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Command-owned runner for every corpus fixture that declares its own
//! `expected-command`/`expected-result`/`expected-kind` header (issue #645,
//! #537 C4 residual).
//!
//! `examples/gallery/{valid,errors,adversarial}` carries ~38 of these
//! headers, and `tests/test_gallery.py` is the only oracle that has ever run
//! them: it is frozen-Python, and `tools/check-native-integration.sh` does
//! not execute Python, so no required native gate ran any of them. 12 of the
//! 38 had never been executed by *any* native test at all -- three of those
//! (`vacuous_contradictory_init`, `vacuous_implication_warning`,
//! `violated_deadlock_terminal`) declare verdicts that only reproduce under a
//! non-default flag (`--vacuity error` / `--deadlock error`), so nothing
//! native had ever exercised the flag that makes the fixture's own claim
//! true.
//!
//! This runner walks `specs/` + `examples/` (never a hard-coded list, the
//! shape #577 retired 28 stale instances of) and reads each header directly
//! at test time rather than transcribing it into a static manifest: the
//! header comment **is** the declaration here, unlike
//! `refine_corpus_parity.rs`'s refinement mappings, most of which are
//! declared in a README rather than the mapping file itself. Reading the
//! header at test time means there is nothing to keep in sync -- the
//! roster's declared command and this runner's executed command are, by
//! construction, the same string.
//!
//! Two commands are structurally out of scope and left to their existing
//! owners:
//!
//! - A header whose `expected-command` starts with `refine` is
//!   `refine_corpus_parity.rs`'s row (`refinement_failed_map.fsl`,
//!   `refine_mapping_boundary_map.fsl`); running it again here would give it
//!   two owners.
//! - `examples/gallery/injected/` keeps its own primary/blind detector
//!   matrix in `injection_detector_matrix.rs`. `every_injected_fixture_declares_its_detector_premise`
//!   below only re-checks the self-retiring premise that lets this runner
//!   skip the directory: every file there names its own `inject`/
//!   `expect-detector` header.

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

#[path = "support/mod.rs"]
mod support;
use support::{corpus_files, headers, repo_relative, root};

/// A file under `examples/gallery/{valid,errors,adversarial}` that carries no
/// `expected-command`/`expected-result` header of its own, and why: each is
/// an operand or companion of another file's declared expectation, or is
/// already owned by a different registry. `every_structural_exclusion_premise_still_holds`
/// re-measures `cited_in` for every entry, so an exclusion cannot outlive its
/// reason (#568) -- the day a citing registry drops the path, this test fails
/// and names the row that must gain its own header or a new reason.
struct StructuralExclusion {
    path: &'static str,
    reason: &'static str,
    /// A repo-relative file whose source must still contain the literal
    /// string `path`. Re-checked, not trusted.
    cited_in: &'static str,
}

const STRUCTURAL_EXCLUSIONS: &[StructuralExclusion] = &[
    StructuralExclusion {
        path: "examples/gallery/adversarial/governance_semantic_after.fsl",
        reason: "governance fixture whose own undefined-type failure is the fixture's purpose; \
                 corpus_check_sweep.rs::GOVERNANCE_FIXTURE_EXCLUSIONS already owns it \
                 (validated indirectly by cli_regression.rs), and refine_corpus_parity.rs \
                 registers it as the `impl` operand of governance_semantic_mapping.fsl",
        cited_in: "rust/fslc/tests/corpus_check_sweep.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/adversarial/governance_semantic_before.fsl",
        reason: "carries no gallery header; it is the `abs` operand of \
                 governance_semantic_mapping.fsl, registered in refine_corpus_parity.rs's \
                 CASES (its bare `fslc check` obligation is still owned by corpus_check_sweep.rs, \
                 which does not exclude it)",
        cited_in: "rust/fslc/tests/refine_corpus_parity.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/adversarial/governance_semantic_dependency.fsl",
        reason: "governance-dialect file validated indirectly by cli_regression.rs, per \
                 corpus_check_sweep.rs::GOVERNANCE_FIXTURE_EXCLUSIONS",
        cited_in: "rust/fslc/tests/corpus_check_sweep.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/adversarial/governance_semantic_mapping.fsl",
        reason: "refinement-dialect mapping; its expectation is owned by refine_corpus_parity.rs's \
                 manifest (CASES entry keyed on this path), not the gallery header convention",
        cited_in: "rust/fslc/tests/refine_corpus_parity.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/adversarial/refine_mapping_boundary_abs.fsl",
        reason: "refine operand (`// expected-helper: used by refine_mapping_boundary_map.fsl`); \
                 registered as the `abstraction` operand in refine_corpus_parity.rs's CASES",
        cited_in: "rust/fslc/tests/refine_corpus_parity.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/adversarial/refine_mapping_boundary_impl.fsl",
        reason: "refine operand (`// expected-helper: used by refine_mapping_boundary_map.fsl`); \
                 registered as the `implementation` operand in refine_corpus_parity.rs's CASES",
        cited_in: "rust/fslc/tests/refine_corpus_parity.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/errors/governance_malformed_business.fsl",
        reason: "malformed `business` fixture; corpus_check_sweep.rs::GOVERNANCE_FIXTURE_EXCLUSIONS \
                 already owns it",
        cited_in: "rust/fslc/tests/corpus_check_sweep.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/errors/governance_malformed_dependency.fsl",
        reason: "governance-dialect file; corpus_check_sweep.rs::GOVERNANCE_FIXTURE_EXCLUSIONS \
                 already owns it",
        cited_in: "rust/fslc/tests/corpus_check_sweep.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/errors/governance_missing_before.fsl",
        reason: "governance-dialect file; corpus_check_sweep.rs::GOVERNANCE_FIXTURE_EXCLUSIONS \
                 already owns it",
        cited_in: "rust/fslc/tests/corpus_check_sweep.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/errors/refinement_failed_abs.fsl",
        reason: "refine operand (`// expected-helper: used by refinement_failed_map.fsl`); \
                 registered as the `abstraction` operand in refine_corpus_parity.rs's CASES",
        cited_in: "rust/fslc/tests/refine_corpus_parity.rs",
    },
    StructuralExclusion {
        path: "examples/gallery/errors/refinement_failed_impl.fsl",
        reason: "refine operand (`// expected-helper: used by refinement_failed_map.fsl`); \
                 registered as the `implementation` operand in refine_corpus_parity.rs's CASES",
        cited_in: "rust/fslc/tests/refine_corpus_parity.rs",
    },
];

/// One roster row read from a corpus header. `expected_kind` is `None` when
/// the file declares no `expected-kind` (the success rows: `proved`/`verified`).
struct RosterEntry {
    path: String,
    command: String,
    expected_result: String,
    expected_kind: Option<String>,
}

/// Results this manifest's roster must never see. Each carries its verdict in
/// a sibling JSON field rather than `result` (`fslc_rust::outcome::outcome_class`'s
/// doc comment), so a bare `expected-result: <value>` header could not state
/// their outcome even if one appeared. The gallery convention has never used
/// them; this list documents that as a checked fact instead of an assumption
/// (issue #645 step 5).
const SIBLING_FIELD_RESULTS: &[&str] = &[
    "approval_check",
    "format_check",
    "lint",
    "semantic_diff",
    "semantic_diff_batch",
];

/// Walk `specs/` + `examples/` and read every `expected-command` header
/// directly, splitting the roster in three: fixtures this runner owns and
/// executes, refine-targeted fixtures deferred to `refine_corpus_parity.rs`,
/// and the set of `examples/gallery/{valid,errors,adversarial}` paths seen
/// (for the structural-coverage check below).
fn read_roster(root: &Path) -> (Vec<RosterEntry>, Vec<String>) {
    let mut owned = Vec::new();
    let mut deferred_to_refine = Vec::new();

    for path in corpus_files(root) {
        let rel = repo_relative(root, &path);
        let source = std::fs::read_to_string(&path).expect("read corpus source");
        let file_headers = headers(&source);
        let Some(command) = file_headers.get("expected-command") else {
            continue;
        };
        if command == "refine" || command.starts_with("refine ") {
            deferred_to_refine.push(rel);
            continue;
        }
        // `examples/named_predicate.fsl` is the one file outside the gallery
        // convention: it declares `// expected: verified` rather than
        // `// expected-result: verified`. That is still a declaration, just
        // under a different key, so it is read rather than treated as
        // missing -- the alternative is editing the file's own header to
        // match this runner's naming, which the no-.fsl-rewrite rule
        // forbids.
        let expected_result = file_headers
            .get("expected-result")
            .or_else(|| file_headers.get("expected"))
            .unwrap_or_else(|| {
                panic!("{rel}: has `expected-command` but no `expected-result`/`expected` header")
            })
            .clone();
        assert!(
            !SIBLING_FIELD_RESULTS.contains(&expected_result.as_str()),
            "{rel}: declares expected-result={expected_result:?}, a sibling-field verdict \
             (fslc_rust::outcome::outcome_class); this runner only compares the top-level \
             `result` string and cannot express that contract -- write a dedicated test instead \
             of a header",
        );
        owned.push(RosterEntry {
            path: rel,
            command: command.clone(),
            expected_result,
            expected_kind: file_headers.get("expected-kind").cloned(),
        });
    }

    (owned, deferred_to_refine)
}

/// Repo-relative paths under `examples/gallery/{valid,errors,adversarial}`.
fn gallery_declared_fixture_paths(root: &Path) -> Vec<String> {
    corpus_files(root)
        .into_iter()
        .map(|path| repo_relative(root, &path))
        .filter(|rel| {
            rel.starts_with("examples/gallery/valid/")
                || rel.starts_with("examples/gallery/errors/")
                || rel.starts_with("examples/gallery/adversarial/")
        })
        .collect()
}

/// Every `examples/gallery/{valid,errors,adversarial}` file must carry its
/// own `expected-command`/`expected-result` header or a reasoned, re-checked
/// exclusion: an unregistered corpus fixture must fail loudly, not sit
/// unexercised the way 12 of the 38 declared fixtures did before this runner.
#[test]
fn every_declared_gallery_fixture_is_registered_or_structurally_excluded() {
    let root = root();
    let declared = gallery_declared_fixture_paths(&root);
    assert!(
        declared.len() >= 45,
        "corpus scan floor: found only {} files under \
         examples/gallery/{{valid,errors,adversarial}}, expected 45+ (the directory walk may be \
         broken)",
        declared.len()
    );

    let (owned, deferred_to_refine) = read_roster(&root);
    let has_header: std::collections::BTreeSet<&str> = owned
        .iter()
        .map(|entry| entry.path.as_str())
        .chain(deferred_to_refine.iter().map(String::as_str))
        .collect();
    let excluded: std::collections::BTreeSet<&str> =
        STRUCTURAL_EXCLUSIONS.iter().map(|e| e.path).collect();

    let mut failures = Vec::new();
    for path in &declared {
        if !has_header.contains(path.as_str()) && !excluded.contains(path.as_str()) {
            failures.push(format!(
                "{path}: no `expected-command`/`expected-result` header and no \
                 StructuralExclusion entry in corpus_expectation_manifest.rs. Add the header \
                 (the gallery convention), or register a reasoned, re-checkable exclusion."
            ));
        }
    }
    for exclusion in STRUCTURAL_EXCLUSIONS {
        if !declared.contains(&exclusion.path.to_owned()) {
            failures.push(format!(
                "{}: registered as a StructuralExclusion but is no longer a gallery file \
                 (deleted, moved, or gained a header and duplicated its own exclusion). Remove \
                 the stale entry.",
                exclusion.path
            ));
        }
        if has_header.contains(exclusion.path) {
            failures.push(format!(
                "{}: registered as a StructuralExclusion but also carries a header now -- \
                 remove the exclusion, the file has claimed its own row.",
                exclusion.path
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Re-measures every `StructuralExclusion`'s premise: the citing file must
/// still contain the excluded path verbatim. A citing registry that drops the
/// path (a rename, a retired governance exclusion, a mapping removed from
/// `refine_corpus_parity.rs`) leaves the excluded gallery file with no owner
/// at all, and this is what catches that (#568 self-retiring shape).
#[test]
fn every_structural_exclusion_premise_still_holds() {
    let root = root();
    let mut failures = Vec::new();
    for exclusion in STRUCTURAL_EXCLUSIONS {
        let citing = std::fs::read_to_string(root.join(exclusion.cited_in))
            .unwrap_or_else(|error| panic!("read {}: {error}", exclusion.cited_in));
        if !citing.contains(exclusion.path) {
            failures.push(format!(
                "{}: STALE. {} no longer mentions this path, so the reason \
                 (\"{}\") no longer has anything backing it. Give this file its own header, or \
                 re-cite wherever it is now owned.",
                exclusion.path, exclusion.cited_in, exclusion.reason
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Re-measures the premise `read_roster`'s `refine`-deferral rests on: every
/// path it hands to `refine_corpus_parity.rs` must still appear, verbatim, in
/// that file's source. Without this, the deferral is an unchecked assumption
/// with no counterpart to `StructuralExclusion`'s `cited_in` re-measurement:
/// if `refine_corpus_parity.rs` ever dropped `refinement_failed_map.fsl` or
/// `refine_mapping_boundary_map.fsl` from its `CASES`/`EXCLUSIONS` (a rename,
/// a merge conflict, a future edit), that file's `expected-command: refine`
/// header would go silently unexecuted by every native test -- deferred here,
/// dropped there, owned nowhere.
#[test]
fn every_refine_deferred_fixture_is_still_named_in_refine_corpus_parity() {
    let root = root();
    let (_owned, deferred_to_refine) = read_roster(&root);
    assert!(
        !deferred_to_refine.is_empty(),
        "expected at least one `expected-command: refine` header deferred to \
         refine_corpus_parity.rs (refinement_failed_map.fsl, refine_mapping_boundary_map.fsl); \
         found none -- the deferral path itself may be broken"
    );

    let citing = std::fs::read_to_string(root.join("rust/fslc/tests/refine_corpus_parity.rs"))
        .expect("read rust/fslc/tests/refine_corpus_parity.rs");
    let mut failures = Vec::new();
    for path in &deferred_to_refine {
        if !citing.contains(path.as_str()) {
            failures.push(format!(
                "{path}: declares `expected-command: refine`, deferred to \
                 refine_corpus_parity.rs, but that file's source no longer contains this path. \
                 It has no owner any more -- add it back to refine_corpus_parity.rs's \
                 CASES/EXCLUSIONS, or give it a non-refine header and let \
                 corpus_expectation_manifest.rs execute it directly."
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every file under `examples/gallery/injected/` must declare its own
/// `inject:`/`expect-detector:` header -- the premise that lets this runner
/// (and `corpus_check_sweep.rs`) skip the directory in favor of
/// `injection_detector_matrix.rs`'s primary/blind matrix. A file that lands
/// there without either header would be silently unexercised by everything.
#[test]
fn every_injected_fixture_declares_its_detector_premise() {
    let root = root();
    let mut files = Vec::new();
    support::collect_fsl_files(&root.join("examples/gallery/injected"), &mut files);
    assert!(
        files.len() >= 14,
        "corpus scan floor: found only {} files under examples/gallery/injected/, expected 14+ \
         (the directory walk may be broken)",
        files.len()
    );

    let mut failures = Vec::new();
    for path in &files {
        let rel = repo_relative(&root, path);
        let source = std::fs::read_to_string(path).expect("read corpus source");
        let file_headers = headers(&source);
        if !file_headers.contains_key("inject") && !file_headers.contains_key("expect-detector") {
            failures.push(format!(
                "{rel}: declares neither `// inject:` nor `// expect-detector:`; register it in \
                 injection_detector_matrix.rs (or give it a gallery header and move it out of \
                 examples/gallery/injected/)."
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

/// The declared command line, with the fixture's own absolute path inserted
/// right after the verb -- the same slot `tests/test_gallery.py::_argv` uses.
fn command_args(command: &str, abs_path: &Path) -> Vec<String> {
    let mut tokens = command.split_whitespace();
    let verb = tokens
        .next()
        .unwrap_or_else(|| panic!("empty expected-command"));
    let mut args = vec![
        verb.to_owned(),
        abs_path.to_str().expect("utf8 path").to_owned(),
    ];
    args.extend(tokens.map(str::to_owned));
    args
}

/// The failure-class `kind` a JSON envelope carries for `entry`'s declared
/// verdict, or `None` for a success-class verdict (no kind to compare).
///
/// The field name is not uniform: `result: "error"` envelopes (`check`, and
/// `verify` for a static or vacuity error) carry `kind`; `result: "violated"`
/// envelopes carry `violation_kind` instead. This mirrors
/// `tests/test_gallery.py::_actual_kind` (`out.get("kind") or
/// out.get("violation_kind")`), the frozen reference's own resolution of the
/// same two-field vocabulary -- read here, not re-derived from observed
/// output, per the no-observation-transcription rule.
fn actual_kind(envelope: &Value) -> Option<&str> {
    envelope
        .get("kind")
        .and_then(Value::as_str)
        .or_else(|| envelope.get("violation_kind").and_then(Value::as_str))
}

/// Compares one roster entry's declaration against an observed envelope and
/// exit status. Returns `Some(failure message)` on any mismatch, `None` on
/// full agreement. Extracted from the execution loop so both the real run
/// and the negative control below share one comparator -- the property this
/// manifest exists to prove is that *this function* can tell a correct
/// declaration from a wrong one, not merely that today's fixtures happen to
/// pass.
fn compare_case(
    path: &str,
    expected_result: &str,
    expected_kind: Option<&str>,
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
    if let Some(expected_kind) = expected_kind {
        let kind = actual_kind(envelope);
        if kind != Some(expected_kind) {
            return Some(format!(
                "{path}: declared expected-kind={expected_kind:?}, got kind={kind:?} \
                 ({envelope})"
            ));
        }
    }
    // Every gallery `error` result is a spec-level error (parse / type /
    // name / semantics / vacuous / vacuous_implication / acceptance /
    // forbidden), never an internal one, so `error_status` is always 2 --
    // `docs/LANGUAGE.md`'s exit-code table's own boundary between the two.
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

/// Runs each roster row's declared command line verbatim and compares
/// `result`, the declared `kind` (when the header states one), and the exit
/// code the production outcome module binds to that result (#537 C4: all
/// three, never just the JSON envelope). Rows run concurrently: several of
/// these fixtures invoke `--engine induction`, which is the slower engine.
#[test]
fn native_matches_the_declared_expectation_for_every_registered_fixture() {
    let root = root();
    let (owned, _deferred_to_refine) = read_roster(&root);
    assert!(
        owned.len() >= 30,
        "corpus scan floor: found only {} roster rows (expected-command headers minus refine-\
         targeted ones), expected 30+ (the directory walk, or the header parse, may be broken)",
        owned.len()
    );

    let failures = for_each_parallel(owned.len(), |index| {
        let entry = &owned[index];
        let abs_path = root.join(&entry.path);
        let args = command_args(&entry.command, &abs_path);
        let (envelope, exit_status) = run_fslc(&args);
        compare_case(
            &entry.path,
            &entry.expected_result,
            entry.expected_kind.as_deref(),
            &envelope,
            exit_status,
        )
    });

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Negative control (AGENTS.md; issue #645 step 7): proves `compare_case` can
/// actually detect a wrong expectation, not merely that today's fixtures
/// happen to agree with it. A comparator that always returns `None` would
/// make the test above pass for any corpus, declared or not.
#[test]
fn compare_case_reports_a_wrong_expectation_as_a_mismatch() {
    let observed = serde_json::json!({"result": "violated", "violation_kind": "invariant"});

    let wrong_result = compare_case("specs/does_not_matter.fsl", "proved", None, &observed, 1);
    assert!(
        wrong_result.is_some(),
        "declared `proved` vs observed `violated` must be reported as a mismatch"
    );

    let wrong_kind = compare_case(
        "specs/does_not_matter.fsl",
        "violated",
        Some("type_bound"),
        &observed,
        1,
    );
    assert!(
        wrong_kind.is_some(),
        "declared kind `type_bound` vs observed `invariant` must be reported as a mismatch"
    );

    let wrong_exit = compare_case(
        "specs/does_not_matter.fsl",
        "violated",
        Some("invariant"),
        &observed,
        0,
    );
    assert!(
        wrong_exit.is_some(),
        "declared `violated` (exit 1) vs observed exit 0 must be reported as a mismatch"
    );

    let right = compare_case(
        "specs/does_not_matter.fsl",
        "violated",
        Some("invariant"),
        &observed,
        1,
    );
    assert!(
        right.is_none(),
        "a correct expectation must not be reported as a mismatch: {right:?}"
    );
}

/// Runs `job` over `0..count` on a small worker pool and collects the failure
/// strings, matching `refine_corpus_parity.rs`'s parallel harness: several
/// rows here run `--engine induction`, and running them concurrently keeps
/// this manifest's wall-clock cost near the slowest row rather than the sum.
fn for_each_parallel(count: usize, job: impl Fn(usize) -> Option<String> + Sync) -> Vec<String> {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .min(count.max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= count {
                        break;
                    }
                    if let Some(failure) = job(index) {
                        failures.lock().expect("failure list").push(failure);
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().expect("failure list");
    failures.sort();
    failures
}

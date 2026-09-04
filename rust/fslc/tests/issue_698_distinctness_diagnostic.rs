// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::Path;
use std::process::Command;

use fsl_core::WRITE_DISTINCTNESS_UNPROVED_CODE;
use serde_json::{Value, json};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn run_cli(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run fslc");
    let status = output.status.code().unwrap_or(-1);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout must be JSON: status={status} stderr={} error={error}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, status)
}

fn shared_diagnostic(source: &str, path: &str) -> fslc_rust::source_diagnostic::SourceDiagnostic {
    let resolver = fsl_core::FsResolver::new(Path::new("."));
    fslc_rust::source_diagnostic::diagnostics(source, path, &resolver)
        .into_iter()
        .find(|diagnostic| diagnostic.kind != "migration")
        .expect("expected a semantic diagnostic")
}

#[test]
fn genuine_literal_duplicate_keeps_legacy_duplicate_contract() {
    let fixture = format!("{FIXTURE_DIR}/issue_698_genuine_literal.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read fixture");
    for command in ["check", "verify"] {
        let args = if command == "check" {
            vec!["check", &fixture]
        } else {
            vec!["verify", &fixture, "--no-cache"]
        };
        let (value, status) = run_cli(&args);
        assert_eq!(status, 2, "{command}: {value}");
        assert_eq!(value["result"], "error", "{command}");
        assert_eq!(value["kind"], "semantics", "{command}");
        assert_eq!(
            value["message"], "an action may not assign the same state location more than once",
            "{command}"
        );
        assert!(value.get("diagnostic_code").is_none(), "{command}: {value}");
        assert!(value.get("hint").is_none(), "{command}: {value}");
        assert_eq!(value["loc"], json!({"line": 7, "column": 5}), "{command}");
    }
    let shared = shared_diagnostic(&source, &fixture);
    assert_eq!(shared.code, "FSL-SEMANTIC");
    assert_eq!(
        shared.message,
        "an action may not assign the same state location more than once"
    );
    assert_eq!(shared.span.start.line, 7);
    assert_eq!(shared.span.start.column, 5);
    assert!(shared.hint.is_none());
    assert!(shared.quick_fix.is_none());
}

#[test]
fn forall_constant_duplicate_keeps_legacy_duplicate_contract() {
    let fixture = format!("{FIXTURE_DIR}/issue_698_forall_constant.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read fixture");
    let (value, status) = run_cli(&["check", &fixture]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(
        value["message"],
        "an action may not assign the same state location more than once"
    );
    assert!(value.get("diagnostic_code").is_none(), "{value}");
    assert_eq!(value["loc"], json!({"line": 6, "column": 21}));
    let shared = shared_diagnostic(&source, &fixture);
    assert_eq!(shared.code, "FSL-SEMANTIC");
    assert_eq!(shared.span.start.line, 6);
    assert_eq!(shared.span.start.column, 21);
}

#[test]
fn affine_index_reports_distinctness_unproved_contract() {
    let fixture = format!("{FIXTURE_DIR}/issue_698_affine_index.fsl");
    let source = std::fs::read_to_string(&fixture).expect("read fixture");
    let expected_hint = "forall k: Cell { if k >= BASE and k < BASE + 4 { m[k] = true } }";
    for command in ["check", "verify"] {
        let args = if command == "check" {
            vec!["check", &fixture]
        } else {
            vec!["verify", &fixture, "--no-cache"]
        };
        let (value, status) = run_cli(&args);
        assert_eq!(status, 2, "{command}: {value}");
        assert_eq!(value["result"], "error", "{command}");
        assert_eq!(value["kind"], "semantics", "{command}");
        assert_eq!(
            value["message"], "cannot prove write-index distinctness across forall iterations",
            "{command}"
        );
        assert_eq!(
            value["diagnostic_code"], WRITE_DISTINCTNESS_UNPROVED_CODE,
            "{command}"
        );
        assert_eq!(value["hint"], expected_hint, "{command}");
        assert_eq!(value["loc"], json!({"line": 8, "column": 21}), "{command}");
    }
    let shared = shared_diagnostic(&source, &fixture);
    assert_eq!(shared.code, WRITE_DISTINCTNESS_UNPROVED_CODE);
    assert_eq!(shared.span.start.line, 8);
    assert_eq!(shared.span.start.column, 21);
    assert_eq!(shared.hint.as_deref(), Some(expected_hint));
    assert_eq!(
        shared
            .quick_fix
            .as_ref()
            .map(|edit| edit.replacement.as_str()),
        Some(expected_hint)
    );
}

#[test]
fn hint_positive_control_checks_cleanly() {
    let fixed = format!("{FIXTURE_DIR}/issue_698_affine_index_fixed.fsl");
    let (value, status) = run_cli(&["check", &fixed]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "ok");
}

fn build_model_from_source(source: &str) -> Result<fsl_core::KernelModel, fsl_core::ModelError> {
    let kernel =
        fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new(".")).expect("parse");
    fsl_core::build_model(kernel)
}

#[test]
fn non_affine_index_stays_rejected_without_actionable_hint() {
    let source = r"spec NonAffine {
  type Cell = 0..7
  type Off = 0..3
  state { m: Map<Cell, Bool> }
  init { forall i: Cell { m[i] = false } }
  action shift() {
    forall c: Off { m[c + c] = true }
  }
}";
    let error = build_model_from_source(source).expect_err("non-affine index must stay rejected");
    assert_eq!(
        error.message,
        "cannot prove write-index distinctness across forall iterations"
    );
    assert_eq!(
        error.diagnostic_code,
        Some(WRITE_DISTINCTNESS_UNPROVED_CODE)
    );
    assert!(error.hint.is_none());
    assert!(error.quick_fix.is_none());
}

#[test]
fn rhs_binder_use_blocks_machine_hint() {
    let source = r"spec RhsBinder {
  type Cell = 0..7
  type Off = 0..3
  const BASE = 2
  state { m: Map<Cell, Bool> }
  init { forall i: Cell { m[i] = false } }
  action shift() {
    forall c: Off { m[BASE + c] = c }
  }
}";
    let error = build_model_from_source(source)
        .expect_err("binder-dependent RHS must not get a machine hint");
    assert_eq!(
        error.diagnostic_code,
        Some(WRITE_DISTINCTNESS_UNPROVED_CODE)
    );
    assert!(error.hint.is_none());
    assert!(error.quick_fix.is_none());
}

#[test]
fn docs_distinguish_proven_duplicate_from_conservative_rejection() {
    let english =
        std::fs::read_to_string(format!("{REPO_ROOT}/docs/LANGUAGE.md")).expect("read LANGUAGE.md");
    let japanese = std::fs::read_to_string(format!("{REPO_ROOT}/docs/LANGUAGE.ja.md"))
        .expect("read LANGUAGE.ja.md");
    let syntax = std::fs::read_to_string(format!("{REPO_ROOT}/skills/fsl/references/syntax.md"))
        .expect("read skills/fsl/references/syntax.md");
    for doc in [&english, &japanese, &syntax] {
        assert!(doc.contains("FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED"));
        assert!(doc.contains("cannot prove write-index distinctness across forall iterations"));
        assert!(
            doc.contains("same state location more than once")
                || doc.contains("legacy duplicate-write")
        );
    }
}

/// REJECTING CONTROL for the independent review's counterexample: a binder
/// whose domain does not start at zero. Emitting `offset .. offset + width`
/// silently shifts the whole interval -- `Off = 2..5` written as `BASE + c`
/// covers `BASE+2 ..= BASE+5`, not `BASE ..= BASE+3`. The hint must name the
/// set the original statement writes, or it is a wrong repair dressed as an
/// actionable one.
#[test]
fn nonzero_lower_bound_hint_names_the_written_interval() {
    let path = format!("{FIXTURE_DIR}/issue_698_nonzero_lower_bound.fsl");
    let (output, status) = run_cli(&["check", &path]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(
        output["diagnostic_code"], "FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED",
        "{output:#}"
    );
    let hint = output["hint"].as_str().expect("hint");
    // Off = 2..5 with BASE + c writes BASE+2 ..= BASE+5, so the exclusive upper
    // bound is BASE + 6.
    assert!(
        hint.contains("k >= BASE + 2"),
        "lower bound must carry the binder's own lower bound: {hint}"
    );
    assert!(
        hint.contains("k < BASE + 6"),
        "exclusive upper bound must be offset + hi + 1: {hint}"
    );
}

/// REJECTING CONTROL: the replacement introduces `k`, so an RHS that already
/// refers to a `k` from an enclosing scope would be captured by it. The
/// classification and code must still arrive; only the machine-applicable
/// repair is withheld.
#[test]
fn an_rhs_that_names_k_withholds_the_hint() {
    let source = r"
spec CapturesK {
  type Cell = 0..7
  type Off  = 0..3
  const BASE = 2
  state { m: Map<Cell, Int> }
  init { forall i: Cell { m[i] = 0 } }
  action shift(k: Cell) {
    forall c: Off { m[BASE + c] = k }
  }
}
";
    let error = build_model_from_source(source).expect_err("distinctness is unproved here");
    assert_eq!(
        error.diagnostic_code,
        Some("FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED"),
        "{error:?}"
    );
    assert!(
        error.hint.is_none(),
        "a hint that rebinds a name the RHS already uses must be withheld: {error:?}"
    );
}

/// REJECTING CONTROL for the independent review's second counterexample: the
/// state map itself is named `k`. The replacement binder shadows it, so the
/// rewritten `k[k] = ...` resolves its root to the scalar binder and fails to
/// check. A repair that does not check is worse than no repair, so the hint is
/// withheld while the classification, code and location still arrive.
#[test]
fn a_state_map_named_k_withholds_the_hint() {
    let path = format!("{FIXTURE_DIR}/issue_698_map_named_k.fsl");
    let (output, status) = run_cli(&["check", &path]);
    assert_eq!(status, 2, "{output:#}");
    assert_eq!(
        output["diagnostic_code"], "FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED",
        "{output:#}"
    );
    assert!(
        output.get("hint").is_none() || output["hint"].is_null(),
        "a rewrite whose binder shadows the written map must not be offered: {output:#}"
    );
}

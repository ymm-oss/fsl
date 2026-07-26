// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native port of `tools/check_rust_refinement_parity.py` (issue #593).
//!
//! The Python script exercised the same 6 corpus refinement mappings and
//! checked both the stdout JSON envelope and the process exit code against
//! the Python reference implementation, but it was never wired into any CI
//! workflow or `tools/check-native-integration.sh` -- `grep -rn
//! "check_rust_refinement_parity" .github/ tools/` has zero hits. Of the 28
//! `refinement`-dialect files in the corpus (`corpus_check_sweep.rs`
//! excludes all of them from its `check` sweep because a mapping file has
//! no `state` block), these are the only 6 ever exercised by any test, and
//! that exercise ran nowhere. This test closes that hole natively: it
//! compares each case's `result` field (and, for `refinement_failed` cases,
//! its `kind`) plus the exit code against a fixed expectation, so a broken
//! mapping fails a named test instead of silently rotting. It does not
//! attempt AGENTS.md-forbidden Python parity (the required product gate
//! must not execute Python); it pins the native envelope directly.
//!
//! This is a minimal-scope fix (issue #593 fix sketch B), not the broader
//! C: it does not extend coverage to the remaining 22 refinement mappings
//! in the corpus (`specs/bank_refines.fsl`, `examples/agentic_rag/*`,
//! `examples/multi_agent_system/*`, `examples/refinement_chain/*`, and
//! others) -- that needs the manifest issue #537 C4 describes
//! (impl/abs/mapping/depth/expected-result/kind) and is tracked separately.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// One corpus refinement mapping this test exercises, mirroring a row of
/// `CASES` in `tools/check_rust_refinement_parity.py`.
struct Case {
    implementation: &'static str,
    abstraction: &'static str,
    mapping: &'static str,
    depth: u32,
    /// Expected `result` field.
    expected_result: &'static str,
    /// Expected `kind` field for a `refinement_failed` result; `None` when
    /// `expected_result` is `"refines"`.
    expected_kind: Option<&'static str>,
}

const CASES: &[Case] = &[
    Case {
        implementation: "specs/cart_impl.fsl",
        abstraction: "specs/cart_v1.fsl",
        mapping: "specs/cart_refines.fsl",
        depth: 6,
        expected_result: "refines",
        expected_kind: None,
    },
    Case {
        implementation: "specs/seat_booking_impl.fsl",
        abstraction: "specs/seat_booking.fsl",
        mapping: "specs/seat_refines.fsl",
        depth: 6,
        expected_result: "refines",
        expected_kind: None,
    },
    Case {
        implementation: "examples/gallery/errors/refinement_failed_impl.fsl",
        abstraction: "examples/gallery/errors/refinement_failed_abs.fsl",
        mapping: "examples/gallery/errors/refinement_failed_map.fsl",
        depth: 4,
        expected_result: "refinement_failed",
        expected_kind: Some("abs_requires_failed"),
    },
    Case {
        implementation: "examples/gallery/adversarial/refine_mapping_boundary_impl.fsl",
        abstraction: "examples/gallery/adversarial/refine_mapping_boundary_abs.fsl",
        mapping: "examples/gallery/adversarial/refine_mapping_boundary_map.fsl",
        depth: 2,
        expected_result: "refinement_failed",
        expected_kind: Some("abs_state_mismatch"),
    },
    Case {
        implementation: "examples/refinement_liveness/design_drops_liveness.fsl",
        abstraction: "examples/refinement_liveness/policy.fsl",
        mapping: "examples/refinement_liveness/design_drops_liveness_progress_refines.fsl",
        depth: 8,
        expected_result: "refinement_failed",
        expected_kind: Some("progress_lost"),
    },
    Case {
        implementation: "examples/refinement_liveness/design_keeps_liveness.fsl",
        abstraction: "examples/refinement_liveness/policy.fsl",
        mapping: "examples/refinement_liveness/design_keeps_liveness_progress_refines.fsl",
        depth: 8,
        expected_result: "refines",
        expected_kind: None,
    },
];

fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run_refine(case: &Case) -> (Value, i32) {
    let root = root();
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args([
            "refine",
            case.implementation,
            case.abstraction,
            case.mapping,
            "--depth",
            &case.depth.to_string(),
        ])
        .current_dir(&root)
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc refine {} {} {}`: {error}; stderr={}",
            case.implementation,
            case.abstraction,
            case.mapping,
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status = output.status.code().unwrap_or_else(|| {
        panic!(
            "`fslc refine {} {} {}` terminated by signal, no exit code",
            case.implementation, case.abstraction, case.mapping
        )
    });
    (value, status)
}

/// `tools/check_rust_refinement_parity.py::_expected_status`: `error` exits
/// 2, `refinement_failed` exits 1, everything else (`refines`) exits 0.
fn expected_status(result: &str) -> i32 {
    match result {
        "error" => 2,
        "refinement_failed" => 1,
        _ => 0,
    }
}

/// Each of the 6 corpus refinement mappings `tools/check_rust_refinement_parity.py`
/// covered must keep reporting its expected `result`/`kind` and exit code
/// under native `fslc refine`. A mapping edit that breaks the correspondence
/// must fail its named case here, not go unnoticed the way #466, #483,
/// #493, #494, and #512 did.
#[test]
fn native_refine_matches_expected_result_and_exit_for_the_corpus_mapping_regressions() {
    let mut failures = Vec::new();

    for case in CASES {
        let (output, status) = run_refine(case);
        let result = output["result"].as_str().unwrap_or_else(|| {
            panic!(
                "{}: `fslc refine` envelope has no string `result` field: {output}",
                case.mapping
            )
        });

        if result != case.expected_result {
            failures.push(format!(
                "{}: expected result={:?}, got result={result:?} ({output})",
                case.mapping, case.expected_result
            ));
            continue;
        }

        if let Some(expected_kind) = case.expected_kind {
            let kind = output["kind"].as_str();
            if kind != Some(expected_kind) {
                failures.push(format!(
                    "{}: expected kind={expected_kind:?}, got kind={kind:?} ({output})",
                    case.mapping
                ));
            }
        }

        let expected_exit = expected_status(case.expected_result);
        if status != expected_exit {
            failures.push(format!(
                "{}: result={result:?} expects exit={expected_exit}, got exit={status}",
                case.mapping
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `outcome_kind` axis: the 7 `fsl_runtime::Monitor` `outcome.kind` values
//! (issue #537 C3 slice 1). Rows are *referenced*, not re-owned: this module
//! reads `fslc_rust::coverage::OUTCOME_FEATURE_KEYS` directly rather than
//! copying its 7-entry list, so the two can never drift.
//!
//! Declared columns: `Monitor` (the concrete Monitor/BFS surface that
//! produces `outcome.kind`, already registered and corpus-observed by
//! `tests/conformance_coverage.rs`) and `CLI` (the `fslc conformance`
//! command, which serializes the same field over the process boundary --
//! exercised here directly, since no existing test decomposes the CLI's
//! golden-diff test by kind).

use std::collections::BTreeSet;
use std::process::Command;

use serde_json::Value;

use crate::claim::{Axis, Citation, Claim, workspace_root};

/// Every `OUTCOME_FEATURE_KEYS` kind has real evidence on both declared
/// columns: `Monitor` from the existing bidirectional coverage test, `CLI`
/// from [`cli_conformance_command_emits_every_registered_outcome_kind`]
/// below (no existing test decomposes the CLI's golden-diff assertion by
/// kind, so this axis owns a small targeted one, per the brief's "write a
/// small targeted test when no cell can be cited" rule).
#[must_use]
pub fn axis() -> Axis {
    let rows = fslc_rust::coverage::OUTCOME_FEATURE_KEYS
        .iter()
        .map(|&(kind, _)| kind)
        .collect::<Vec<_>>();
    let columns = vec!["Monitor", "CLI"];
    let mut cells = std::collections::BTreeMap::new();
    for &row in &rows {
        cells.insert(
            (row, "Monitor"),
            Claim::Exercised {
                by: Citation {
                    path: "rust/fslc/tests/conformance_coverage.rs",
                    anchor: "fn every_outcome_kind_the_corpus_emits_is_registered_and_exercised()",
                },
            },
        );
        cells.insert(
            (row, "CLI"),
            Claim::Exercised {
                by: Citation {
                    path: "rust/fslc/tests/assurance/outcome_kind.rs",
                    anchor: "fn cli_conformance_command_emits_every_registered_outcome_kind()",
                },
            },
        );
    }
    Axis {
        name: "outcome_kind",
        rows,
        columns,
        cells,
    }
}

/// Runs the real `fslc conformance` subcommand (not the library-level
/// `conformance_vectors` call `conformance_coverage.rs` uses) against the
/// same fixed fixture manifest `coverage.rs` scans, and confirms every
/// `OUTCOME_FEATURE_KEYS` kind appears in the CLI's own stdout JSON. This is
/// the axis's `CLI`-column exercising evidence.
#[test]
fn cli_conformance_command_emits_every_registered_outcome_kind() {
    let mut observed = BTreeSet::new();
    for (file, depth) in [("kernel_contract.fsl", 2), ("conformance_failures.fsl", 1)] {
        let path = workspace_root().join("rust/fslc/tests/fixtures").join(file);
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args([
                "conformance",
                path.to_str().expect("UTF-8 path"),
                "--depth",
                &depth.to_string(),
            ])
            .current_dir(workspace_root())
            .output()
            .expect("run native CLI");
        assert!(
            output.status.success(),
            "fslc conformance {file} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).expect("conformance JSON");
        for vector in value["vectors"].as_array().expect("vectors") {
            if let Some(kind) = vector["outcome"]["kind"].as_str() {
                observed.insert(kind.to_owned());
            }
        }
    }

    for &(kind, _) in fslc_rust::coverage::OUTCOME_FEATURE_KEYS {
        assert!(
            observed.contains(kind),
            "CLI `fslc conformance` never emitted outcome.kind == \"{kind}\" for the fixed fixture manifest; observed={observed:?}"
        );
    }
}

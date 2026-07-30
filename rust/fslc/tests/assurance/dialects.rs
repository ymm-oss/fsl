// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `dialects` axis: every keyword emitted by
//! `fsl_syntax::dispatch::frontends!` (issue #537 C3 slice 3).
//!
//! Rows reference `DIALECT_KEYWORDS` directly. Explicit posture matches and
//! the corpus-representation test make a new frontend fail this slice until
//! its CLI, Worker, and C4 ownership posture is reviewed.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde_json::Value;

use crate::claim::{Axis, Citation, Claim};
use crate::support::{corpus_files, root};

const BARE_CHECK_SWEEP: Citation = Citation {
    path: "rust/fslc/tests/corpus_check_sweep.rs",
    anchor: "fn every_corpus_spec_checks_ok_or_declares_its_error()",
};
const REFINEMENT_CHECK_CONTROL: Citation = Citation {
    path: "rust/fslc/tests/assurance/dialects.rs",
    anchor: "fn refinement_bare_check_fails_closed_and_stays_owned_by_refine()",
};
const AGENT_CHECK: Citation = Citation {
    path: "rust/fslc/tests/issue_468_recursive_agent.rs",
    anchor: "fn check_is_parseable_for_corpus_sweeps_and_stays_lenient_on_the_top_level_result()",
};
const WORKER_PARITY: Citation = Citation {
    path: "rust/fsl-wasm/test-browser.mjs",
    anchor: "const envelopeDifferences = differences(",
};
const WORKER_AGENT_EXCLUSION: Citation = Citation {
    path: "rust/fsl-wasm/test-browser.mjs",
    anchor: "function assertAgentWorkerProbeFailsClosed(probe, envelope) {",
};
const WORKER_REFINEMENT_CONTROL: Citation = Citation {
    path: "rust/fsl-wasm/test-browser.mjs",
    anchor: "const retiredRefinementCase = \"specs/cart_refines.fsl\";",
};
const REFINEMENT_OWNER: Citation = Citation {
    path: "rust/fslc/tests/refine_corpus_parity.rs",
    anchor: "fn native_refine_matches_the_declared_result_and_exit_for_every_registered_mapping()",
};
const EVIDENCE_OWNER: Citation = Citation {
    path: "rust/fslc/tests/evidence_corpus_manifest.rs",
    anchor: "fn native_matches_the_declared_expectation_for_every_registered_row()",
};
const EXPECTATION_OWNER: Citation = Citation {
    path: "rust/fslc/tests/corpus_expectation_manifest.rs",
    anchor: "fn native_matches_the_declared_expectation_for_every_registered_fixture()",
};

fn cli_claim(row: &'static str) -> Claim {
    match row {
        "refinement" => Claim::UnsupportedFailClosed {
            by: REFINEMENT_CHECK_CONTROL,
        },
        "agent" => Claim::Exercised { by: AGENT_CHECK },
        "spec" | "compose" | "business" | "governance" | "requirements" | "domain" | "dbsystem"
        | "ai_component" => Claim::Exercised {
            by: BARE_CHECK_SWEEP,
        },
        _ => panic!("unreviewed dialect '{row}' has no CLI-check posture"),
    }
}

fn worker_claim(row: &'static str) -> Claim {
    match row {
        "agent" => Claim::UnsupportedFailClosed {
            by: WORKER_AGENT_EXCLUSION,
        },
        "refinement" => Claim::UnsupportedFailClosed {
            by: WORKER_REFINEMENT_CONTROL,
        },
        "spec" | "compose" | "business" | "governance" | "requirements" | "domain" | "dbsystem"
        | "ai_component" => Claim::Exercised { by: WORKER_PARITY },
        _ => panic!("unreviewed dialect '{row}' has no Worker posture"),
    }
}

fn corpus_claim(row: &'static str) -> Claim {
    match row {
        "refinement" => Claim::Exercised {
            by: REFINEMENT_OWNER,
        },
        "agent" | "ai_component" => Claim::Exercised { by: EVIDENCE_OWNER },
        "spec" => Claim::Exercised {
            by: EXPECTATION_OWNER,
        },
        "compose" | "business" | "governance" | "requirements" | "domain" | "dbsystem" => {
            Claim::Exercised {
                by: BARE_CHECK_SWEEP,
            }
        }
        _ => panic!("unreviewed dialect '{row}' has no C4 corpus owner"),
    }
}

#[must_use]
pub fn axis() -> Axis {
    fsl_syntax::validate_frontend_registry().expect("valid frontend registry");
    let rows = fsl_syntax::DIALECT_KEYWORDS.to_vec();
    let columns = vec!["CLI check", "Worker", "corpus"];
    let mut cells: BTreeMap<(&'static str, &'static str), Claim> = BTreeMap::new();

    for &row in &rows {
        cells.insert((row, "CLI check"), cli_claim(row));
        cells.insert((row, "Worker"), worker_claim(row));
        cells.insert((row, "corpus"), corpus_claim(row));
    }

    Axis {
        name: "dialects",
        rows,
        columns,
        cells,
    }
}

/// Registry-derived representation gate. The global corpus floors in the C4
/// owners do not by themselves prove that every frontend still has an
/// artifact, so this test derives the observed set with `dialect_keyword`
/// and requires every `frontends!` keyword to remain represented.
#[test]
fn every_registered_dialect_has_a_corpus_representative_and_reviewed_posture() {
    fsl_syntax::validate_frontend_registry().expect("valid frontend registry");
    let reviewed_axis = axis();
    assert_eq!(reviewed_axis.rows, fsl_syntax::DIALECT_KEYWORDS);
    let repository = root();
    let mut observed = BTreeSet::new();
    for path in corpus_files(&repository) {
        let source = std::fs::read_to_string(&path).expect("read corpus source");
        if let Ok(keyword) = fsl_syntax::dialect_keyword(&source) {
            observed.insert(keyword);
        }
    }
    let missing = fsl_syntax::DIALECT_KEYWORDS
        .iter()
        .copied()
        .filter(|keyword| !observed.contains(keyword))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "registered dialects without a specs/examples corpus representative: {missing:?}"
    );
}

/// `refinement` is a registered parser frontend, but a mapping is evaluated
/// by `fslc refine`, not as a standalone Kernel spec. Bare `check` must
/// refuse it instead of reporting a misleading success. C4's refinement
/// manifest separately exercises the owning command for every mapping.
#[test]
fn refinement_bare_check_fails_closed_and_stays_owned_by_refine() {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "specs/cart_refines.fsl"])
        .current_dir(root())
        .output()
        .expect("run native CLI");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("native check stdout JSON");
    assert_eq!(output.status.code(), Some(2), "{envelope}");
    assert_eq!(envelope["result"], "error");
    assert!(
        envelope["message"]
            .as_str()
            .is_some_and(|message| message.contains("no state block")),
        "{envelope}"
    );
}

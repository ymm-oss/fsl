// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Evidence/assurance overlay for `fslc document generate`/`check` (issue
//! #332): overlays saved external verification evidence onto a generated
//! requirements document, at requirement granularity, using the exact
//! assurance vocabulary and classifier `fslc ledger` already established
//! (issue #171, `docs/DESIGN-assurance-classes.md`,
//! `rust/fsl-tools/src/ledger.rs`) — this module adds no new classification
//! logic of its own. See `docs/DESIGN-document-evidence-overlay.md`.
//!
//! v1 is evidence-only: there is no live verify pass here (that stays
//! `fslc ledger`'s job; the document's own determinism contract — the same
//! spec always renders byte-identical Markdown — is incompatible with
//! embedding a live solver run's outcome). Every requirement's assurance is
//! computed purely from `--evidence PATH` envelopes matched by requirement
//! ID, exactly the same matching contract `fslc ledger --evidence` already
//! uses (`ledger::evidence_requirement_ids`), defaulting to `not_run` when
//! nothing matches — never a live judgment this crate invents itself.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::ledger::{assurance_label, assurance_token, evidence_requirement_ids};

/// A glossary-file-shaped bundle: the loaded evidence envelopes plus a
/// digest of the raw file set (issue #330's `AppliedGlossary` is the
/// pattern this mirrors). `fsl-tools` never touches a filesystem itself;
/// the CLI reads/hashes the files and passes both in.
pub struct AppliedEvidence<'a> {
    pub files: &'a [(String, Value)],
    pub digest: &'a str,
}

#[derive(Clone, Debug, Default)]
pub struct EvidenceEntry {
    pub label: String,
    pub result: Option<String>,
    pub source: String,
}

/// One requirement's assurance, split into the three fixed dimensions the
/// rendered document always shows explicitly (never omitted, per acceptance
/// criterion 1): `formal` (`proved`/`bounded` evidence), `conformance`
/// (`replay-observed` — implementation/runtime evidence), `statistical`
/// (`statistical` — Wilson-interval evidence). Each is empty, not merely
/// absent, when nothing matched — rendering must show `not_run` for an
/// empty dimension, not skip the line.
#[derive(Clone, Debug, Default)]
pub struct RequirementAssurance {
    pub formal: Vec<EvidenceEntry>,
    pub conformance: Vec<EvidenceEntry>,
    pub statistical: Vec<EvidenceEntry>,
}

impl RequirementAssurance {
    #[must_use]
    pub fn is_not_run(&self) -> bool {
        self.formal.is_empty() && self.conformance.is_empty() && self.statistical.is_empty()
    }
}

/// Classify every evidence envelope attached to `requirement_id` (by the
/// exact same requirement-id matching `fslc ledger --evidence` already
/// uses), routing each into its fixed dimension by `assurance_token`. An
/// envelope classified `not_run` contributes nothing (an unclassifiable
/// envelope is not evidence of anything) — `RequirementAssurance` is only
/// ever empty-shows-`not_run`, never populated with a `not_run` entry
/// itself.
///
/// This function performs no classification of its own: `assurance_token`/
/// `assurance_label` (`ledger.rs`) are the sole source of truth, so a
/// `bounded` envelope can never render as `proved` here any more than it
/// already can in `fslc ledger` (acceptance criterion 3).
#[must_use]
pub fn requirement_assurance(
    requirement_id: &str,
    evidence: &[(String, Value)],
) -> RequirementAssurance {
    let mut out = RequirementAssurance::default();
    for (path, item) in evidence {
        if !evidence_requirement_ids(item).contains(&requirement_id) {
            continue;
        }
        let token = assurance_token(item);
        if token == "not_run" {
            continue;
        }
        let depth = item
            .get("checked_to_depth")
            .or_else(|| item.get("depth"))
            .and_then(Value::as_u64);
        let entry = EvidenceEntry {
            label: assurance_label(token, depth),
            result: item
                .get("result")
                .and_then(Value::as_str)
                .map(str::to_owned),
            source: path.clone(),
        };
        match token {
            "proved" | "bounded" => out.formal.push(entry),
            "replay-observed" => out.conformance.push(entry),
            "statistical" => out.statistical.push(entry),
            _ => {}
        }
    }
    let rank = |entry: &EvidenceEntry| -> (u8, String) {
        let strength = u8::from(!entry.label.starts_with("proved"));
        (strength, entry.label.clone())
    };
    out.formal.sort_by_key(rank);
    out.conformance.sort_by_key(rank);
    out.statistical.sort_by_key(rank);
    out
}

/// Every evidence file that names at least one requirement ID but matches
/// none of `requirement_ids` — a plausible typo, or a file meant for a
/// different spec. An evidence file naming *no* requirement IDs at all is
/// legitimate whole-spec evidence (the same convention `fslc ledger`
/// already renders as "（仕様全体）"), not an unmatched file, so it is never
/// included here.
#[must_use]
pub fn unmatched_evidence_paths(
    requirement_ids: &BTreeSet<&str>,
    evidence: &[(String, Value)],
) -> Vec<String> {
    evidence
        .iter()
        .filter(|(_, item)| {
            let ids = evidence_requirement_ids(item);
            !ids.is_empty() && !ids.iter().any(|id| requirement_ids.contains(id))
        })
        .map(|(path, _)| path.clone())
        .collect()
}

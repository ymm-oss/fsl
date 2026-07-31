// SPDX-License-Identifier: Apache-2.0

//! CI-internal Triangulated Assurance claim and registry contracts (#670).

use std::collections::{BTreeMap, BTreeSet};

use crate::matrix_claim::Citation;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceState {
    Executable,
    Skipped,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct EvidenceRef {
    pub by: Citation,
    pub state: EvidenceState,
}

impl EvidenceRef {
    fn check(&self, field: &str) -> Result<(), String> {
        if self.state != EvidenceState::Executable {
            return Err(format!(
                "{field}: evidence is {:?}, not executable",
                self.state
            ));
        }
        if self.by.path.trim().is_empty() || self.by.anchor.trim().is_empty() {
            return Err(format!(
                "{field}: citation path and anchor must be non-empty"
            ));
        }
        self.by
            .recheck()
            .map_err(|error| format!("{field}: {error}"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservationKind {
    RawSource,
    RawProcess,
    RawTrace,
    PreclassifiedVerdict,
}

#[derive(Clone, Copy, Debug)]
pub struct ObservationEvidence {
    pub observed_by: EvidenceRef,
    pub kind: ObservationKind,
    pub fields: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
pub struct ObserverEvidence {
    pub observed_by: EvidenceRef,
    pub semantic_owner: &'static str,
    pub semantic_lineage: &'static [&'static str],
}

#[derive(Clone, Copy, Debug)]
pub struct AgreementEdges {
    pub model_world: EvidenceRef,
    pub oracle_world: EvidenceRef,
    pub model_oracle: EvidenceRef,
}

#[derive(Clone, Copy, Debug)]
pub struct Calibration {
    pub accepting: EvidenceRef,
    pub rejecting: &'static [EvidenceRef],
    pub common_mode: Option<EvidenceRef>,
}

#[derive(Clone, Copy, Debug)]
pub struct ScopeEvidence {
    pub declared_by: Citation,
    pub commands: &'static [&'static str],
    pub feature: &'static str,
    pub domain: &'static str,
    pub backend: &'static str,
    pub platform: &'static str,
    pub corpus_revision: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct TriangulatedClaim {
    pub id: &'static str,
    pub contract: Citation,
    pub common_observation: ObservationEvidence,
    pub model_observer: ObserverEvidence,
    pub independent_observer: ObserverEvidence,
    pub edges: AgreementEdges,
    pub calibration: Calibration,
    pub scope: ScopeEvidence,
}

impl TriangulatedClaim {
    pub fn check(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push("id must be non-empty".to_owned());
        }
        check_citation(self.contract, "contract", &mut errors);
        self.check_observation(&mut errors);
        self.check_observers(&mut errors);
        for (field, evidence) in [
            ("edges.model_world", self.edges.model_world),
            ("edges.oracle_world", self.edges.oracle_world),
            ("edges.model_oracle", self.edges.model_oracle),
            ("calibration.accepting", self.calibration.accepting),
        ] {
            if let Err(error) = evidence.check(field) {
                errors.push(error);
            }
        }
        if self.calibration.rejecting.is_empty() {
            errors.push("calibration.rejecting: at least one control is required".to_owned());
        }
        for (index, rejecting) in self.calibration.rejecting.iter().enumerate() {
            if let Err(error) = rejecting.check(&format!("calibration.rejecting[{index}]")) {
                errors.push(error);
            }
        }
        if let Some(common_mode) = self.calibration.common_mode
            && let Err(error) = common_mode.check("calibration.common_mode")
        {
            errors.push(error);
        }
        self.check_scope(&mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("{}: {}", self.id, errors.join("; ")))
        }
    }

    fn check_observation(&self, errors: &mut Vec<String>) {
        if let Err(error) = self
            .common_observation
            .observed_by
            .check("common_observation")
        {
            errors.push(error);
        }
        if self.common_observation.kind == ObservationKind::PreclassifiedVerdict {
            errors.push(
                "common_observation: preclassified production verdict is not raw evidence"
                    .to_owned(),
            );
            return;
        }
        let required: &[&str] = match self.common_observation.kind {
            ObservationKind::RawSource => &["source_bytes", "source_revision"],
            ObservationKind::RawProcess => &[
                "stdout_bytes",
                "stderr_bytes",
                "process_exit",
                "binary_revision",
            ],
            ObservationKind::RawTrace => &[
                "trace",
                "step",
                "state",
                "violation_kind",
                "failed_location",
            ],
            ObservationKind::PreclassifiedVerdict => &[],
        };
        let fields = self
            .common_observation
            .fields
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        for field in required {
            if !fields.contains(field) {
                errors.push(format!("common_observation: missing raw field '{field}'"));
            }
        }
        if fields.len() != self.common_observation.fields.len() {
            errors.push("common_observation: duplicate field name".to_owned());
        }
    }

    fn check_observers(&self, errors: &mut Vec<String>) {
        for (field, observer) in [
            ("model_observer", self.model_observer),
            ("independent_observer", self.independent_observer),
        ] {
            if let Err(error) = observer.observed_by.check(field) {
                errors.push(error);
            }
            if observer.semantic_owner.trim().is_empty() {
                errors.push(format!("{field}: semantic_owner must be non-empty"));
            }
            if observer.semantic_lineage.is_empty()
                || observer
                    .semantic_lineage
                    .iter()
                    .any(|entry| entry.trim().is_empty())
            {
                errors.push(format!("{field}: semantic_lineage must be non-empty"));
            }
        }
        if self.model_observer.semantic_owner == self.independent_observer.semantic_owner {
            errors.push(format!(
                "observer independence: shared semantic owner '{}'",
                self.model_observer.semantic_owner
            ));
        }
        let model = self
            .model_observer
            .semantic_lineage
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let shared = self
            .independent_observer
            .semantic_lineage
            .iter()
            .copied()
            .filter(|entry| model.contains(entry))
            .collect::<Vec<_>>();
        if !shared.is_empty() {
            errors.push(format!(
                "observer independence: shared semantic decision lineage {shared:?}"
            ));
        }
    }

    fn check_scope(&self, errors: &mut Vec<String>) {
        check_citation(self.scope.declared_by, "scope.declared_by", errors);
        if self.scope.commands.is_empty()
            || self
                .scope
                .commands
                .iter()
                .any(|value| value.trim().is_empty())
        {
            errors.push("scope.commands must be non-empty".to_owned());
        }
        for (field, value) in [
            ("scope.feature", self.scope.feature),
            ("scope.domain", self.scope.domain),
            ("scope.backend", self.scope.backend),
            ("scope.platform", self.scope.platform),
            ("scope.corpus_revision", self.scope.corpus_revision),
        ] {
            if value.trim().is_empty() {
                errors.push(format!("{field} must be non-empty"));
            }
        }
    }
}

fn check_citation(citation: Citation, field: &str, errors: &mut Vec<String>) {
    if citation.path.trim().is_empty() || citation.anchor.trim().is_empty() {
        errors.push(format!(
            "{field}: citation path and anchor must be non-empty"
        ));
    } else if let Err(error) = citation.recheck() {
        errors.push(format!("{field}: {error}"));
    }
}

pub struct Registry {
    pub required_ids: &'static [&'static str],
    pub claims: Vec<TriangulatedClaim>,
}

impl Registry {
    pub fn check_complete(&self) -> Result<(), String> {
        let mut counts = BTreeMap::new();
        for claim in &self.claims {
            *counts.entry(claim.id).or_insert(0_usize) += 1;
        }
        let required = self.required_ids.iter().copied().collect::<BTreeSet<_>>();
        let mut errors = self
            .required_ids
            .iter()
            .filter(|id| counts.get(**id).copied().unwrap_or(0) != 1)
            .map(|id| {
                format!(
                    "required claim '{id}' has {} registrations (expected exactly one)",
                    counts.get(id).copied().unwrap_or(0)
                )
            })
            .collect::<Vec<_>>();
        errors.extend(
            counts
                .keys()
                .filter(|id| !required.contains(**id))
                .map(|id| format!("stale/unrequired claim '{id}' is registered")),
        );
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub fn check_claims(&self) -> Result<(), String> {
        let errors = self
            .claims
            .iter()
            .filter_map(|claim| claim.check().err())
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    pub fn check_common_mode_calibration(&self) -> Result<(), String> {
        if self
            .claims
            .iter()
            .any(|claim| claim.calibration.common_mode.is_some())
        {
            Ok(())
        } else {
            Err("registry has no calibrated common-mode fault control".to_owned())
        }
    }
}

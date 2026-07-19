// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Requirement Claim IR (RCIR) v1 (issue #325).
//!
//! `project_requirement_claims_from_source` compiles a checked `requirements`
//! or direct `spec` dialect model into the versioned RCIR contract
//! (`schemas/fslc/document/requirement-claims.v1.schema.json`). RCIR is not a
//! second semantics: it embeds the validated Public Kernel v2 contract and
//! attaches document roles and traceability to stable semantic targets.
//! See `docs/DESIGN-document-requirement-claim-ir.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const RCIR_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/document/requirement-claims.v1.schema.json";
pub const RCIR_SCHEMA_VERSION: &str = "1.0.0";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequirementClaimSet {
    #[serde(rename = "$schema")]
    pub schema: String,
    pub schema_version: String,
    pub result: String,
    pub spec: SpecInfo,
    pub public_kernel: Value,
    pub semantics: SemanticsInfo,
    pub requirements: Vec<Requirement>,
    pub claims: Vec<Claim>,
    pub trace_cases: Vec<TraceCase>,
    pub undecided: Vec<UndecidedItem>,
    pub analysis_scope: AnalysisScope,
    pub coverage: Coverage,
    pub provenance: ProvenanceSummary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpecInfo {
    pub name: String,
    pub dialect: String,
    pub source: Option<String>,
    pub spec_digest: String,
    pub spec_digest_algorithm: String,
    pub claim_set_digest: String,
    pub claim_set_digest_algorithm: String,
    pub claim_digest_algorithm: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticsInfo {
    pub updates: String,
    pub reads: String,
    pub failed_step: String,
    pub fairness: String,
}

impl Default for SemanticsInfo {
    fn default() -> Self {
        Self {
            updates: "simultaneous".to_owned(),
            reads: "pre_state".to_owned(),
            failed_step: "rollback".to_owned(),
            fairness: "weak".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub statements: Vec<RequirementStatement>,
    pub claim_ids: Vec<String>,
    pub kinds: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequirementStatement {
    pub text: Option<String>,
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SourceRef {
    pub path: Option<String>,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    Operation,
    StateRule,
    TransitionRule,
    ProgressRule,
    ReachabilityGoal,
    AcceptanceTrace,
    ForbiddenTrace,
    DeadlineRule,
    TerminalRule,
}

impl ClaimKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Operation => "operation",
            Self::StateRule => "state_rule",
            Self::TransitionRule => "transition_rule",
            Self::ProgressRule => "progress_rule",
            Self::ReachabilityGoal => "reachability_goal",
            Self::AcceptanceTrace => "acceptance_trace",
            Self::ForbiddenTrace => "forbidden_trace",
            Self::DeadlineRule => "deadline_rule",
            Self::TerminalRule => "terminal_rule",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub kind: ClaimKind,
    pub requirements: Vec<String>,
    pub subject: Value,
    #[serde(skip)]
    pub enablement: Option<Value>,
    #[serde(skip)]
    pub effects: Option<Value>,
    #[serde(skip)]
    pub postconditions: Option<Value>,
    #[serde(skip)]
    pub condition: Option<Value>,
    #[serde(skip)]
    pub progress: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fairness: Option<String>,
    pub semantic_targets: Vec<String>,
    pub source: Option<SourceRef>,
    pub provenance: ClaimProvenance,
    pub claim_digest: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceAssurance {
    SourceBacked,
    GeneratedFromSource,
    GeneratedOnly,
    Unknown,
}

impl ProvenanceAssurance {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceBacked => "source_backed",
            Self::GeneratedFromSource => "generated_from_source",
            Self::GeneratedOnly => "generated_only",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClaimProvenance {
    pub assurance: ProvenanceAssurance,
    pub sources: Vec<SourceRef>,
    pub origin_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceCaseKind {
    Acceptance,
    Forbidden,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceCase {
    pub id: String,
    pub kind: TraceCaseKind,
    pub text: String,
    pub steps: Vec<Value>,
    pub expectation: Option<Value>,
    pub source: SourceRef,
    pub requirements: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UndecidedItem {
    pub target: String,
    pub declaration: String,
    pub reason: String,
    pub requirement_ids: Vec<String>,
    pub source: Option<SourceRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AnalysisScope {
    pub instances: Vec<Value>,
    pub values: Vec<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    pub authored: Vec<String>,
    pub rendered: Vec<String>,
    pub unattributed: Vec<String>,
    pub unsupported: Vec<UnsupportedEntry>,
    pub counts: CoverageCounts,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnsupportedEntry {
    pub target: String,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CoverageCounts {
    pub authored: usize,
    pub rendered: usize,
    pub unattributed: usize,
    pub unsupported: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceSummary {
    pub completeness: Completeness,
    pub identity_stability: String,
    pub counts: AssuranceCounts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AssuranceCounts {
    pub source_backed: usize,
    pub generated_from_source: usize,
    pub generated_only: usize,
    pub unknown: usize,
}

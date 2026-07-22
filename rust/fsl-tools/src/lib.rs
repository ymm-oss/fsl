// SPDX-License-Identifier: Apache-2.0

//! Native report generators and specialized FSL dialect engines.

mod ai;
mod analysis;
mod analysis_export;
mod analysis_graph;
mod causal;
mod causal_analysis;
mod causal_evidence;
mod causal_expectation;
mod causal_ledger_projection;
mod causal_plan;
mod causal_projection;
mod db;
mod db_import;
mod document;
mod document_check;
mod document_coverage;
mod document_digest;
mod document_evidence;
mod document_glossary;
mod document_markers;
mod document_project;
mod document_render;
mod document_render_expr;
mod domain;
mod domain_codegen;
mod domain_naming;
mod html;
mod ledger;
mod mutate;
mod public_kernel;
mod refinement_analysis;
mod testgen;
mod typestate;
mod undecided;

pub use ai::{check_ai, replay_ai};
pub use analysis::{analyze_model, build_tsg, review_finding, structural_review_findings};
pub use analysis_export::export_analysis_graph;
pub use causal::{
    Binding, BindingKind, CausalError, CausalModel, CausalWarning, Claim as CausalClaim,
    ClaimStatus, Clock, Feedback, ImportedSpec, Interval, Lag, Persistence, Polarity,
    ScopeVocabulary, Variable, VariableRole, active_adjacency, build_causal_model, reachable_from,
    strongly_connected_components,
};
pub use causal_analysis::{
    causal_check_json, causal_review_findings, earliest_from, feedback_loop_classes,
    indicator_classes, latest_bound_from, polarity_reach,
};
pub use causal_evidence::{
    Applicability, CAUSAL_SUPPORT_UNTESTED, EVIDENCE_SCHEMA_VERSION, EvidenceArtifact,
    EvidenceError, FORMAL_ASSURANCE_NOT_RUN, LIFECYCLE_SCHEMA_VERSION, LifecycleStatus,
    ScopeApplication, SupportOverlay, aggregate_support, artifact_digest, canonical_json,
    causal_evidence_graph, lifecycle_record_digest, parse_artifact, validate_lifecycle_chain,
};
pub use causal_expectation::{CompiledExpectation, compile_expectations};
pub use causal_ledger_projection::build_ledger;
pub use causal_plan::{ObservationWindow, PLAN_SCHEMA_VERSION, PlanArtifact, parse_plan};
pub use causal_projection::{
    causal_diff_json, causal_dot, causal_graph_projection, causal_mermaid, causal_review_json,
    causal_timeline_projection, causal_traceability_projection,
};
pub use db::{DbToolError, check_db, observe_db, validate_db};
pub use db_import::{DbImport, import_db};
pub use document::{
    AnalysisScope, AssuranceCounts, Claim, ClaimKind, ClaimProvenance, Completeness, Coverage,
    CoverageCounts, ProvenanceAssurance, ProvenanceSummary, RCIR_SCHEMA_ID, RCIR_SCHEMA_VERSION,
    Requirement, RequirementClaimSet, RequirementStatement, SemanticsInfo, SourceRef, SpecInfo,
    TraceCase, TraceCaseKind, UndecidedItem, UnsupportedEntry,
};
pub use document_check::{
    CheckError, DocumentCheckReport, DriftReason, check_requirements_document,
};
pub use document_coverage::{
    RCIR_TARGET_KIND_REGISTRY, TargetKindRow, TargetTreatment, target_kind,
};
pub use document_digest::{CLAIM_BLOCK_DIGEST_ALGORITHM, framed_text_digest};
pub use document_evidence::{
    AppliedEvidence, EvidenceEntry, RequirementAssurance, requirement_assurance,
    unmatched_evidence_paths,
};
pub use document_glossary::{
    AppliedGlossary, GLOSSARY_SCHEMA, Glossary, GlossaryIssue, UnknownTarget, parse_glossary,
    unknown_targets,
};
pub use document_markers::{
    DOCUMENT_RENDERER, DOCUMENT_RENDERER_VERSION, DOCUMENT_SCHEMA, Frontmatter, MarkerIssue,
    NORMATIVE_SCOPE, ParsedDocument, SLOT_NAMES, Segment, parse_frontmatter_only,
    parse_generated_document,
};
pub use document_project::{
    DocumentDialect, DocumentInput, DocumentProjectionError, RCIR_SUPPORTED_DIALECTS,
    project_requirement_claims, project_requirement_claims_from_source,
};
pub use document_render::{
    AppliedApproval, AppliedApprovals, Locale, RenderedDocument, render_requirements_document,
};
pub use domain::{
    analyze_domain, check_domain, domain_adapter_files, domain_kernel_source, domain_scaffold,
    domain_scaffold_metadata,
};
pub use html::render_html_report;
pub use ledger::{render_ledger, render_ledger_with_approvals};
pub use mutate::{BuiltinMutant, enumerate_builtin_mutants};
pub use refinement_analysis::analyze_refinement;
pub use testgen::{
    TestgenInput, compose_testgen_input, generate_testgen, public_kernel_testgen_input,
};
pub use typestate::analyze_typestate;
pub use undecided::{UndecidedRecord, undecided_declarations, undecided_records};

/// Complete a deterministic graph envelope from already normalized nodes and edges.
#[must_use]
pub fn complete_analysis_graph(
    projection: &str,
    nodes: &[serde_json::Value],
    edges: &[serde_json::Value],
) -> serde_json::Value {
    analysis_graph::graph_envelope(projection, nodes, edges)
}

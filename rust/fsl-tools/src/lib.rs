// SPDX-License-Identifier: Apache-2.0

//! Native report generators and specialized FSL dialect engines.

mod ai;
mod analysis;
mod analysis_export;
mod analysis_graph;
mod db;
mod db_import;
mod document;
mod document_check;
mod document_digest;
mod document_markers;
mod document_project;
mod document_render;
mod document_render_expr;
mod domain;
mod domain_codegen;
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
pub use document_digest::{CLAIM_BLOCK_DIGEST_ALGORITHM, framed_text_digest};
pub use document_markers::{
    DOCUMENT_RENDERER, DOCUMENT_RENDERER_VERSION, DOCUMENT_SCHEMA, Frontmatter, MarkerIssue,
    NORMATIVE_SCOPE, ParsedDocument, SLOT_NAMES, Segment, parse_frontmatter_only,
    parse_generated_document,
};
pub use document_project::{
    DocumentDialect, DocumentInput, project_requirement_claims,
    project_requirement_claims_from_source,
};
pub use document_render::{Locale, RenderedDocument, render_requirements_document};
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

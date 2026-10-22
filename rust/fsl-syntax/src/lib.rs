// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Syntax layer for the Rust fslc port.
//!
//! Phase 0 starts with the expression grammar because it is shared by every
//! declaration dialect.  The public `kernel_ast_v1` projection deliberately emits
//! the tuple/list JSON shape of the Python reference implementation, allowing
//! differential tests without making that legacy representation the Rust AST.

mod ai;
mod ai_project;
mod annotation;
mod annotation_parse;
mod ast;
mod causal;
mod db;
mod dispatch;
mod domain;
mod edition;
mod glossary;
mod identifier;
pub mod keywords;
mod lexer;
mod literate;
mod lossless;
mod parser;
pub mod recursion;
mod respell;
mod source_spellings;
mod surface;
mod syntax_expr;
mod unstable;

pub use ai::{
    AiAgentContract, AiAgentGrant, AiAgentOutput, AiAuthority, AiAuthorityRule, AiCheckRule,
    AiComponent, AiDelegationEdge, AiFailurePolicy, AiFallback, AiHardCheck, AiLoc, AiTool,
    parse_ai_component,
};
pub use ai_project::{
    AiDataset, AiMetricRequirement, AiMigration, AiObservedProperty, AiObservedRequirement,
    AiProject, AiRawBlock, AiRegressionRequirement, AiStatisticalProperty, AiUnparsedClause,
    parse_ai_project,
};
pub use annotation::{
    Annotation, AnnotationError, AnnotationRegistry, AnnotationValue, Annotations, RequirementLink,
    SymbolPath,
};
pub use ast::{
    AggregateKind, Binder, ConditionalSpans, Expr, Pattern, QualifiedName, SourcePos, Span,
};
pub use causal::{
    CausalClaimDecl, CausalClock, CausalEvidenceDecl, CausalExpectationDecl, CausalFeedbackDecl,
    CausalInterval, CausalLag, CausalPersistence, CausalRef, CausalScopeDimension, CausalSource,
    CausalUse, CausalVariableDecl, ExpectationTrigger, MeasurementKind, MeasurementRef,
    ScopeRelation, ScopeRelationKind, ScopeSelection, is_causal_source, parse_causal,
};
pub use db::{
    DbArtifact, DbCheck, DbCheckRule, DbColumn, DbColumnRef, DbDatabase, DbEnvironment,
    DbEnvironmentArtifact, DbFlag, DbFlagCondition, DbMigration, DbMigrationOp, DbSystem, DbTable,
    parse_db_system,
};
pub use dispatch::{
    DIALECT_KEYWORDS, ParsedDocument, SourceFile, declaration_keyword, dialect_keyword,
    dialect_keyword_with, parse_document, parse_document_with, validate_frontend_registry,
};
pub use domain::{
    DomainAggregate, DomainAssignment, DomainAwait, DomainCommand, DomainDecide, DomainEffect,
    DomainError, DomainEvent, DomainEvolve, DomainField, DomainInvariant, DomainLoc,
    DomainProjection, DomainReject, DomainRetry, DomainSaga, DomainSagaCompensation,
    DomainSagaStep, DomainSpec, DomainStalePolicy, DomainType, DomainTypeSourceForm, parse_domain,
};
pub use edition::{
    EDITION_CONFLICT, EDITION_HEADER_NOT_ACCEPTED, EDITIONLESS_DECLARATIONS, Edition,
    EditionHeader, MISSING_EDITION_HEADER, UNSUPPORTED_EDITION, document_edition, edition_header,
    first_significant_offset,
};
pub use glossary::{
    ALIAS_COLLISION, ALIAS_IN_IMPLEMENTS, ALIAS_SHADOWS_KEYWORD, ALIAS_SHADOWS_NAME,
    DUPLICATE_ALIAS, GLOSSARY_DIAGNOSTICS, Glossary, GlossaryEntry, GlossaryName,
    KEYWORD_SPELLINGS, MISPLACED_GLOSSARY, MIXED_NAMES, NameUse, alias_at, document_glossary,
    is_keyword_spelling,
};
pub use identifier::{
    CANONICAL_NAME_NOT_ASCII, FULLWIDTH_ASCII_IDENTIFIER, FULLWIDTH_SPACE, FULLWIDTH_SYMBOL,
    IDENTIFIER_DIAGNOSTICS, IdentifierRule, IdentifierSeverity, MIDDLE_DOT_IDENTIFIER,
    MIXED_SCRIPT_IDENTIFIER, NON_ASCII_REQUIREMENT_ID, NON_NFC_IDENTIFIER,
    RESERVED_IDENTIFIER_CODES,
};
pub use lexer::{LexError, LexWarning, Token, TokenKind, lex, lex_with, lexical_warnings};
pub use literate::extract_literate_fsl;
pub use lossless::{
    CanonicalRewrite, CanonicalRewriteKind, FormatError, LosslessDocument, LosslessKind,
    LosslessNode, SourceEdit, UnsafeFormatKind, apply_source_edits, canonical_rewrites,
    canonical_rewrites_with, format_source, format_source_respelled, format_source_with,
    lossless_document, lossless_document_with, source_position, source_span,
};
pub use parser::{
    ParseError, parse_expr, parse_surface_document, parse_surface_document_with, parse_surface_spec,
};
pub use respell::{
    KeywordLanguage, NameForm, RESPELL_NOT_APPLICABLE, Respelling, respell_source, space_particles,
};
pub use source_spellings::{
    CheckSpelling, ExpressionSpelling, KeywordSpelling, PhraseKind, PhraseSpelling, SourceSpellings,
};
pub use surface::{
    AcceptanceExpectation, AcceptanceStep, ActionItem, ActionTarget, BusinessGoalBody,
    BusinessItem, BusinessPolicyBody, ComposeItem, ConceptDeclaration, ControlAttribute,
    CorrespondenceOrigin, DecisionRow, DecisionTable, EditionWarning, EventForm,
    GovernanceArtifactRef, GovernanceDelegateItem, GovernanceItem, HelpfulAction, LValue,
    MapsClause, MetaTag, NextEditionRequirements, Param, PreservationItem, ProcessCover,
    ProcessField, ProcessFields, ProcessItem, ProcessTransition, RefinementItem, RefinementParam,
    RequirementAction, RequirementActionItem, RequirementBlockItem, RequirementBranch,
    RequirementsItem, RuleGuard, RuleProperty, SpecItem, StateField, Statement, SurfaceAgent,
    SurfaceBusiness, SurfaceCompose, SurfaceDocument, SurfaceGovernance, SurfaceRefinement,
    SurfaceRequirements, SurfaceSpec, SyncAction, SyncRef, TimeItem, TypeExpr, VerifyItem,
    stage_reference,
};
pub use syntax_expr::{
    SyntaxBinder, SyntaxExpr, SyntaxExprKind, SyntaxIdent, SyntaxLValue, SyntaxOperator,
    SyntaxPattern, SyntaxQualifiedName, SyntaxTypeExpr, SyntaxTypeExprKind,
};
pub use unstable::{ParseOptions, UnstableFeature, UnstableFeatures};

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Syntax layer for the Rust fslc port.
//!
//! Phase 0 starts with the expression grammar because it is shared by every
//! declaration dialect.  The public `python_ast` projection deliberately emits
//! the tuple/list JSON shape of the Python reference implementation, allowing
//! differential tests without making that legacy representation the Rust AST.

mod ai;
mod annotation;
mod annotation_parse;
mod ast;
mod db;
mod dispatch;
mod domain;
mod lexer;
mod lossless;
mod parser;
mod surface;
mod syntax_expr;

pub use ai::{
    AiAuthority, AiAuthorityRule, AiCheckRule, AiComponent, AiFallback, AiHardCheck, AiLoc, AiTool,
    parse_ai_component,
};
pub use annotation::{
    Annotation, AnnotationError, AnnotationRegistry, AnnotationValue, Annotations, RequirementLink,
    SymbolPath,
};
pub use ast::{
    AggregateKind, Binder, ConditionalSpans, Expr, Pattern, QualifiedName, SourcePos, Span,
};
pub use db::{
    DbArtifact, DbCheck, DbCheckRule, DbColumn, DbColumnRef, DbDatabase, DbEnvironment,
    DbEnvironmentArtifact, DbFlag, DbFlagCondition, DbMigration, DbMigrationOp, DbSystem, DbTable,
    parse_db_system,
};
pub use dispatch::{
    DIALECT_KEYWORDS, ParsedDocument, SourceFile, declaration_keyword, dialect_keyword,
    parse_document, validate_frontend_registry,
};
pub use domain::{
    DomainAggregate, DomainAssignment, DomainAwait, DomainCommand, DomainDecide, DomainEffect,
    DomainError, DomainEvent, DomainEvolve, DomainField, DomainInvariant, DomainLoc,
    DomainProjection, DomainReject, DomainRetry, DomainSaga, DomainSagaCompensation,
    DomainSagaStep, DomainSpec, DomainStalePolicy, DomainType, DomainTypeSourceForm, parse_domain,
};
pub use lexer::{LexError, Token, TokenKind, lex};
pub use lossless::{
    CanonicalRewrite, CanonicalRewriteKind, FormatEdition, FormatError, LosslessDocument,
    LosslessKind, LosslessNode, SourceEdit, apply_source_edits, canonical_rewrites, format_source,
    lossless_document, source_position, source_span,
};
pub use parser::{ParseError, parse_expr, parse_surface_document, parse_surface_spec};
pub use surface::{
    AcceptanceExpectation, AcceptanceStep, ActionItem, ActionTarget, BusinessGoalBody,
    BusinessItem, BusinessPolicyBody, ComposeItem, ControlAttribute, CorrespondenceOrigin,
    GovernanceArtifactRef, GovernanceDelegateItem, GovernanceItem, HelpfulAction, LValue,
    MapsClause, MetaTag, Param, PreservationItem, ProcessCover, ProcessField, ProcessFields,
    ProcessItem, ProcessTransition, RefinementItem, RefinementParam, RequirementAction,
    RequirementActionItem, RequirementBlockItem, RequirementBranch, RequirementsItem, SpecItem,
    StateField, Statement, SurfaceAgent, SurfaceBusiness, SurfaceCompose, SurfaceDocument,
    SurfaceGovernance, SurfaceRefinement, SurfaceRequirements, SurfaceSpec, SyncAction, SyncRef,
    TimeItem, TypeExpr, VerifyItem,
};
pub use syntax_expr::{
    SyntaxBinder, SyntaxExpr, SyntaxExprKind, SyntaxIdent, SyntaxLValue, SyntaxOperator,
    SyntaxPattern, SyntaxQualifiedName, SyntaxTypeExpr, SyntaxTypeExprKind,
};

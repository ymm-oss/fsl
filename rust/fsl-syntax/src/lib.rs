// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Syntax layer for the Rust fslc port.
//!
//! Phase 0 starts with the expression grammar because it is shared by every
//! declaration dialect.  The public `python_ast` projection deliberately emits
//! the tuple/list JSON shape of the Python reference implementation, allowing
//! differential tests without making that legacy representation the Rust AST.

mod ai;
mod ast;
mod db;
mod domain;
mod lexer;
mod parser;
mod surface;

pub use ai::{
    AiAuthority, AiComponent, AiFallback, AiHardCheck, AiLoc, AiTool, parse_ai_component,
};
pub use ast::{Binder, Expr, Pattern, QualifiedName, SourcePos, Span};
pub use db::{
    DbArtifact, DbCheck, DbColumn, DbColumnRef, DbDatabase, DbEnvironment, DbEnvironmentArtifact,
    DbFlag, DbFlagCondition, DbMigration, DbMigrationOp, DbSystem, DbTable, parse_db_system,
};
pub use domain::{
    DomainAggregate, DomainAssignment, DomainAwait, DomainCommand, DomainDecide, DomainEffect,
    DomainError, DomainEvent, DomainEvolve, DomainField, DomainInvariant, DomainLoc,
    DomainProjection, DomainReject, DomainRetry, DomainSaga, DomainSagaCompensation,
    DomainSagaStep, DomainSpec, DomainStalePolicy, DomainType, parse_domain,
};
pub use lexer::{LexError, Token, TokenKind, lex};
pub use parser::{ParseError, parse_expr, parse_surface_document, parse_surface_spec};
pub use surface::{
    AcceptanceExpectation, AcceptanceStep, ActionItem, ActionTarget, BusinessGoalBody,
    BusinessItem, BusinessPolicyBody, ComposeItem, ControlAttribute, GovernanceArtifactRef,
    GovernanceDelegateItem, GovernanceItem, HelpfulAction, LValue, MapsClause, MetaTag, Param,
    PreservationItem, ProcessField, ProcessFields, ProcessItem, ProcessTransition, RefinementItem,
    RefinementParam, RequirementAction, RequirementActionItem, RequirementBlockItem,
    RequirementBranch, RequirementsItem, SpecItem, Statement, SurfaceBusiness, SurfaceCompose,
    SurfaceDocument, SurfaceGovernance, SurfaceRefinement, SurfaceRequirements, SurfaceSpec,
    SyncAction, SyncRef, TimeItem, TypeExpr, VerifyItem,
};

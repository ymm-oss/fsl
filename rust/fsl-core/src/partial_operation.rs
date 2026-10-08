// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The one inventory of partial operations (issue #1166).
//!
//! `docs/manual/LANGUAGE.md` names six operations whose evaluation can fail
//! (`head`/`pop`/`at`, a `Seq` index read, `/`, `%`). Before #1166 that set was
//! written out four times -- the Public Kernel's `partial_operations`, the
//! verifier's implicit-`partial_op` candidate check, the runtime's error
//! classification, and `fslc explain`'s `auto_checks` -- and the copies had
//! drifted (`explain` counted no index read and walked no binder). The
//! evaluators stay independent (`docs/design/DESIGN-rust-components.md`); what
//! lives here is only the static classification they all consume:
//!
//! - [`PartialOperation`] is the set itself. Every consumer matches on it
//!   exhaustively, so a seventh kind cannot be added without each of them
//!   deciding what it means.
//! - [`PartialOperation::candidate`] is the node-local syntactic classifier.
//! - [`expression_has_partial_operation_candidate`] and friends walk every
//!   operand, binder range/collection/`where` parts included, and answer the
//!   verifier's question "can this fail at all?".
//! - [`action_partial_operations`] is the typed per-site enumeration (an index
//!   read counts only on a `Seq`) that `explain` lists.

use std::collections::HashMap;

use fsl_syntax::{Binder, Expr, LValue, Span, Statement};

use crate::typecheck::{
    TypeEnv, base_env, binder_type, extend_pattern_binding, infer_type, resolve,
};
use crate::{ActionDef, ActionGuard, KernelModel, ParamDef, TypeRef, recursion};

/// A kernel operation that is undefined on part of its domain.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PartialOperation {
    /// `s.head()` on an empty sequence.
    Head,
    /// `s.pop()` on an empty sequence.
    Pop,
    /// `s.at(i)` outside the live prefix.
    At,
    /// `s[i]` on a `Seq` outside the live prefix. A `Map` index is total over
    /// its finite key domain and is not a partial operation.
    Index,
    /// `a / b` with `b == 0` in action context.
    Divide,
    /// `a % b` with `b == 0` in action context.
    Remainder,
}

impl PartialOperation {
    /// Every partial operation, in declaration order.
    pub const ALL: [Self; 6] = [
        Self::Head,
        Self::Pop,
        Self::At,
        Self::Index,
        Self::Divide,
        Self::Remainder,
    ];

    /// The Public Kernel `partial_operations[].operation` spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::Pop => "pop",
            Self::At => "at",
            Self::Index => "index",
            Self::Divide => "divide",
            Self::Remainder => "remainder",
        }
    }

    /// Classify `expr`'s own node, ignoring its operands.
    ///
    /// Every `Expr::Index` is returned as [`PartialOperation::Index`]: without
    /// types a `Seq` read cannot be told from a total `Map` read, so this is a
    /// candidate. Typed consumers confirm the collection is a `Seq`.
    #[must_use]
    pub fn candidate(expr: &Expr) -> Option<Self> {
        match expr {
            Expr::Method { name, .. } => match name.as_str() {
                "head" => Some(Self::Head),
                "pop" => Some(Self::Pop),
                "at" => Some(Self::At),
                _ => None,
            },
            Expr::Index(_, _) => Some(Self::Index),
            Expr::Binary { op, .. } => match op.as_str() {
                "/" => Some(Self::Divide),
                "%" => Some(Self::Remainder),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Whether evaluating `expr` can reach a partial operation, binder parts
/// included. Syntactic: an index read counts whatever its collection type.
#[must_use]
pub fn expression_has_partial_operation_candidate(expr: &Expr) -> bool {
    let mut found = false;
    walk_expr(expr, &mut Untyped, &mut |_| found = true);
    found
}

/// Visit every untyped candidate of `expr`, binder parts included, in the walk
/// [`expression_has_partial_operation_candidate`] answers from, for a caller
/// that tells the kinds apart.
pub(crate) fn for_each_partial_operation_candidate(
    expr: &Expr,
    visit: &mut impl FnMut(PartialOperation),
) {
    walk_expr(expr, &mut Untyped, visit);
}

/// [`expression_has_partial_operation_candidate`] for a binder's range,
/// collection and `where` parts.
#[must_use]
pub fn binder_has_partial_operation_candidate(binder: &Binder) -> bool {
    let mut found = false;
    walk_binder(binder, &mut Untyped, &mut |_| found = true);
    found
}

/// [`expression_has_partial_operation_candidate`] for an assignment target:
/// the index expressions an indexed update evaluates. An indexed target itself
/// counts as an index candidate, as the untyped classifier does for reads.
#[must_use]
pub fn lvalue_has_partial_operation_candidate(target: &LValue) -> bool {
    match target {
        LValue::Var(_) => false,
        LValue::Index(_, _) => true,
        LValue::Field(base, _) => lvalue_has_partial_operation_candidate(base),
    }
}

/// Whether any `requires`/`let`, statement, or `ensures` of `action` can
/// reach a partial operation. The verifier skips its implicit `partial_op`
/// queries when no action of the model has a candidate.
#[must_use]
pub fn action_has_partial_operation_candidate(action: &ActionDef) -> bool {
    action.guards.iter().any(|guard| match guard {
        ActionGuard::Let(_, expr) | ActionGuard::Requires(expr) => {
            expression_has_partial_operation_candidate(expr)
        }
    }) || action
        .statements
        .iter()
        .any(statement_has_partial_operation_candidate)
        || action
            .ensures
            .iter()
            .any(expression_has_partial_operation_candidate)
}

pub(crate) fn statement_has_partial_operation_candidate(statement: &Statement) -> bool {
    match statement {
        Statement::Assign { target, value, .. } => {
            expression_has_partial_operation_candidate(value)
                || lvalue_has_partial_operation_candidate(target)
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            expression_has_partial_operation_candidate(condition)
                || then_statements
                    .iter()
                    .chain(else_statements)
                    .any(statement_has_partial_operation_candidate)
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            binder_has_partial_operation_candidate(binder)
                || statements
                    .iter()
                    .any(statement_has_partial_operation_candidate)
        }
    }
}

/// One partial-operation site inside an action, attributed to the clause that
/// contains it.
#[derive(Clone, Copy, Debug)]
pub struct ActionPartialOperation<'a> {
    pub operation: PartialOperation,
    /// The containing `requires`/`ensures` clause or statement; an action's
    /// own span for a `let`, which carries none of its own.
    pub span: Span,
    /// What `span` points at, for rendering the site.
    pub clause: PartialOperationClause<'a>,
}

/// The authored clause a partial-operation site is reported under.
#[derive(Clone, Copy, Debug)]
pub enum PartialOperationClause<'a> {
    /// A `requires`/`let`/`ensures` expression, an assignment's value, or an
    /// `if`'s condition.
    Expr(&'a Expr),
    /// A statement-level `forall`'s binder (range, collection, `where`).
    Binder(&'a Binder),
}

/// Every partial-operation site of `action` -- `requires`, then `let`s, then
/// statements (assignment targets and `forall` binders included), then
/// `ensures` -- one entry per occurrence. An index read counts only when its
/// collection is a `Seq`; when the collection's type cannot be inferred the
/// read is kept, so an inference gap lists too much rather than too little.
#[must_use]
pub fn action_partial_operations<'a>(
    model: &KernelModel,
    action: &'a ActionDef,
) -> Vec<ActionPartialOperation<'a>> {
    let mut env = base_env(model);
    for param in &action.params {
        let ty = match param {
            ParamDef::Typed { ty, .. } => ty.clone(),
            ParamDef::Range { lo, hi, .. } => TypeRef::Range(*lo, *hi),
        };
        env.insert(param.name().to_owned(), ty);
    }
    let mut scope = Typed { env, model };
    let mut requires = Vec::new();
    let mut lets = Vec::new();
    let mut require_spans = action.require_spans.iter();
    for guard in &action.guards {
        match guard {
            ActionGuard::Requires(expr) => {
                let span = require_spans.next().copied().unwrap_or(action.span);
                collect(expr, span, &mut scope, &mut requires);
                scope.bind_pattern(expr);
            }
            ActionGuard::Let(name, expr) => {
                collect(expr, action.span, &mut scope, &mut lets);
                match infer_type(expr, &scope.env, model, None) {
                    Ok(ty) => scope.env.insert(name.clone(), ty),
                    Err(_) => scope.env.remove(name),
                };
            }
        }
    }
    let mut sites = requires;
    sites.extend(lets);
    for statement in &action.statements {
        collect_statement(statement, &mut scope, &mut sites);
    }
    for (expr, span) in action.ensures.iter().zip(&action.ensure_spans) {
        collect(expr, *span, &mut scope, &mut sites);
    }
    sites
}

/// Whether `statement` -- binder parts, `where` and body included -- contains a
/// partial operation, classified like [`action_partial_operations`] (an index
/// read counts only on a `Seq`, an uninferable one counts). `env` is the
/// statement's enclosing scope; a name it cannot type keeps a read, so the
/// answer errs toward `true`.
pub(crate) fn statement_has_partial_operation(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
) -> bool {
    let mut scope = Typed {
        env: env.clone(),
        model,
    };
    let mut sites = Vec::new();
    collect_statement(statement, &mut scope, &mut sites);
    !sites.is_empty()
}

fn collect<'a, S: Scope>(
    clause: &'a Expr,
    span: Span,
    scope: &mut S,
    sites: &mut Vec<ActionPartialOperation<'a>>,
) {
    walk_expr(clause, scope, &mut |operation| {
        sites.push(ActionPartialOperation {
            operation,
            span,
            clause: PartialOperationClause::Expr(clause),
        });
    });
}

fn collect_statement<'a, S: Scope>(
    statement: &'a Statement,
    scope: &mut S,
    sites: &mut Vec<ActionPartialOperation<'a>>,
) {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => {
            let mut push = |operation| {
                sites.push(ActionPartialOperation {
                    operation,
                    span: *span,
                    clause: PartialOperationClause::Expr(value),
                });
            };
            walk_lvalue(target, scope, &mut push);
            walk_expr(value, scope, &mut push);
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            collect(condition, *span, scope, sites);
            for item in then_statements.iter().chain(else_statements) {
                collect_statement(item, scope, sites);
            }
        }
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            walk_binder(binder, scope, &mut |operation| {
                sites.push(ActionPartialOperation {
                    operation,
                    span: *span,
                    clause: PartialOperationClause::Binder(binder),
                });
            });
            let saved = scope.enter(binder);
            for item in statements {
                collect_statement(item, scope, sites);
            }
            scope.leave(saved);
        }
    }
}

/// How a walk resolves the one type-dependent classification and tracks the
/// bindings that resolution needs.
trait Scope {
    type Saved;
    fn is_seq(&self, collection: &Expr) -> bool;
    /// Whether an indexed assignment target names a `Seq`. Unreachable on a
    /// checked model today -- `check` accepts indexed targets on `Map` and
    /// `Relation` only -- so the typed walk never counts one; it stays so a
    /// future `Seq` indexed update is classified like a `Seq` index read.
    fn is_seq_target(&self, name: &str) -> bool;
    /// Bring `binder`'s variable into scope, returning what it shadowed.
    fn enter(&mut self, binder: &Binder) -> Self::Saved;
    fn leave(&mut self, saved: Self::Saved);
    /// `left and right` evaluates `right` with `left`'s `is some(x)` bindings.
    fn pattern_scope(&self, left: &Expr) -> Self;
}

struct Untyped;

impl Scope for Untyped {
    type Saved = ();
    fn is_seq(&self, _: &Expr) -> bool {
        true
    }
    fn is_seq_target(&self, _: &str) -> bool {
        true
    }
    fn enter(&mut self, _: &Binder) {}
    fn leave(&mut self, (): ()) {}
    fn pattern_scope(&self, _: &Expr) -> Self {
        Self
    }
}

struct Typed<'m> {
    env: TypeEnv,
    model: &'m KernelModel,
}

impl Typed<'_> {
    fn bind_pattern(&mut self, expr: &Expr) {
        let mut env = self.env.clone();
        if extend_pattern_binding(expr, &mut env, self.model).is_ok() {
            self.env = env;
        }
    }

    fn resolves_to_seq(&self, ty: Option<TypeRef>) -> bool {
        ty.is_none_or(|ty| {
            resolve(self.model, &ty).map_or(true, |ty| matches!(ty, TypeRef::Seq(_, _)))
        })
    }
}

impl Scope for Typed<'_> {
    type Saved = HashMap<String, Option<TypeRef>>;
    fn is_seq(&self, collection: &Expr) -> bool {
        self.resolves_to_seq(infer_type(collection, &self.env, self.model, None).ok())
    }
    fn is_seq_target(&self, name: &str) -> bool {
        self.resolves_to_seq(self.env.get(name).cloned())
    }
    fn enter(&mut self, binder: &Binder) -> Self::Saved {
        let name = binder_name(binder);
        let ty = binder_type(binder, &self.env, self.model).ok();
        let previous = match ty {
            Some(ty) => self.env.insert(name.to_owned(), ty),
            None => self.env.remove(name),
        };
        HashMap::from([(name.to_owned(), previous)])
    }
    fn leave(&mut self, saved: Self::Saved) {
        for (name, previous) in saved {
            match previous {
                Some(ty) => self.env.insert(name, ty),
                None => self.env.remove(&name),
            };
        }
    }
    fn pattern_scope(&self, left: &Expr) -> Self {
        let mut scope = Typed {
            env: self.env.clone(),
            model: self.model,
        };
        scope.bind_pattern(left);
        scope
    }
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}

/// The one traversal every consumer shares: `visit` is called once per
/// partial-operation occurrence, in evaluation order, binder parts included.
fn walk_expr<S: Scope>(expr: &Expr, scope: &mut S, visit: &mut impl FnMut(PartialOperation)) {
    recursion::guard(|| walk_expr_inner(expr, scope, visit));
}

fn walk_expr_inner<S: Scope>(expr: &Expr, scope: &mut S, visit: &mut impl FnMut(PartialOperation)) {
    if let Some(operation) = PartialOperation::candidate(expr) {
        let counts = match (operation, expr) {
            (PartialOperation::Index, Expr::Index(collection, _)) => scope.is_seq(collection),
            _ => true,
        };
        if counts {
            visit(operation);
        }
    }
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => {}
        Expr::Some(item)
        | Expr::Neg(item)
        | Expr::Not(item)
        | Expr::Field(item, _)
        | Expr::Stage { entity: item, .. }
        | Expr::UnaryNamed { expr: item, .. }
        | Expr::Is { expr: item, .. } => walk_expr(item, scope, visit),
        Expr::Set(items) | Expr::Seq(items) | Expr::Call { args: items, .. } => {
            for item in items {
                walk_expr(item, scope, visit);
            }
        }
        Expr::Struct { fields, .. } => {
            for (_, item) in fields {
                walk_expr(item, scope, visit);
            }
        }
        Expr::Index(left, right) | Expr::BinaryNamed { left, right, .. } => {
            walk_expr(left, scope, visit);
            walk_expr(right, scope, visit);
        }
        Expr::Binary { left, right, .. } => {
            walk_expr(left, scope, visit);
            let mut right_scope = scope.pattern_scope(left);
            walk_expr(right, &mut right_scope, visit);
        }
        Expr::Method { receiver, args, .. } => {
            walk_expr(receiver, scope, visit);
            for arg in args {
                walk_expr(arg, scope, visit);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            walk_expr(condition, scope, visit);
            walk_expr(then_expr, scope, visit);
            walk_expr(else_expr, scope, visit);
        }
        Expr::Quantified { binder, body, .. } => {
            walk_binder(binder, scope, visit);
            let saved = scope.enter(binder);
            walk_expr(body, scope, visit);
            scope.leave(saved);
        }
        Expr::Aggregate { binder, value, .. } => {
            walk_binder(binder, scope, visit);
            if let Some(value) = value {
                let saved = scope.enter(binder);
                walk_expr(value, scope, visit);
                scope.leave(saved);
            }
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            walk_expr(first, scope, visit);
            walk_expr(second, scope, visit);
            walk_expr(third, scope, visit);
        }
    }
}

/// A binder's range bounds and collection are evaluated in the outer scope;
/// its `where` filter sees the bound variable.
fn walk_binder<S: Scope>(binder: &Binder, scope: &mut S, visit: &mut impl FnMut(PartialOperation)) {
    let where_expr = match binder {
        Binder::Typed { where_expr, .. } => where_expr,
        Binder::Range {
            lo, hi, where_expr, ..
        } => {
            walk_expr(lo, scope, visit);
            walk_expr(hi, scope, visit);
            where_expr
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            walk_expr(collection, scope, visit);
            where_expr
        }
    };
    if let Some(where_expr) = where_expr {
        let saved = scope.enter(binder);
        walk_expr(where_expr, scope, visit);
        scope.leave(saved);
    }
}

fn walk_lvalue<S: Scope>(target: &LValue, scope: &mut S, visit: &mut impl FnMut(PartialOperation)) {
    match target {
        LValue::Var(_) => {}
        LValue::Index(name, index) => {
            if scope.is_seq_target(name) {
                visit(PartialOperation::Index);
            }
            walk_expr(index, scope, visit);
        }
        LValue::Field(base, _) => walk_lvalue(base, scope, visit),
    }
}

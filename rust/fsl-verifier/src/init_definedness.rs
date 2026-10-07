// SPDX-License-Identifier: Apache-2.0

//! Definedness of `init` (#1258).
//!
//! `init` is evaluated with the action-context rules (LANGUAGE.md §6:
//! division and remainder by zero, the `Seq` partial operations, a finite
//! `Map` key outside its domain, and checked i64 overflow), and the concrete
//! engines stop with a semantics error when it fails. The symbolic init
//! encoding (`transition::init_constraints`) totalizes every one of them, so
//! BMC and induction used to accept an init no concrete engine can run.
//!
//! [`init_failure_sites`] enumerates, in evaluation order, every place an
//! init evaluation can fail, each with the condition under which that
//! evaluation reaches it and fails there and the message the concrete
//! evaluator gives for the same failure, located at the init statement. The
//! sites only name the failure: whether init is defined is decided by
//! `transition::init_evaluation_status`, the same `evaluation_status` the
//! action checks use. An empty site list means no init statement contains an
//! operation that can fail, so the definedness query is not asked at all.

use fsl_core::{
    KernelAggregateKind as AggregateKind, KernelBinder as Binder, KernelExpr as Expr,
    KernelLValue as LValue, KernelModel, KernelStatement as Statement, Span, recursion,
};
use fsl_solver::SmtSolver;

use crate::VerifyError;
use crate::eval::{
    binder_values, binder_where, eval, evaluation_status, i64_term_is_in_range, index_accessible,
    method_definedness,
};
use crate::value::{Bindings, SymbolicState, SymbolicValue, bool_term, int_term};

/// One place an init evaluation can fail.
pub(crate) struct InitFailureSite<T> {
    /// The evaluation reaches this operation, its operands are defined, and
    /// the operation itself fails.
    pub condition: T,
    /// The concrete evaluator's message for the failure, located at the init
    /// statement that contains it.
    pub message: String,
}

/// Whether any init statement contains an operation that can fail, decided
/// on the syntax alone, before a single solver term is built.
///
/// The backend's term table is shared by every later solver session on the
/// thread, so even building terms the search never built moves the
/// under-determined models later sessions return (the induction step's CTI
/// is one). An init with nothing that can fail must therefore build nothing.
/// `false` is exact only in that direction: every construct
/// `evaluation_status` can report undefined answers `true` here (arithmetic,
/// negation, `abs`, `sum`, an index read, a `Seq` method, an indexed
/// assignment target), except the one total form `forall k: K { m[k] = .. }`
/// with `m: Map<K, _>`, whose key is the binder itself.
pub(crate) fn init_may_fail(model: &KernelModel) -> bool {
    let mut binders = Vec::new();
    model
        .init
        .iter()
        .any(|statement| statement_may_fail(model, statement, &mut binders))
}

fn statement_may_fail<'a>(
    model: &KernelModel,
    statement: &'a Statement,
    binders: &mut Vec<(&'a str, Option<&'a str>)>,
) -> bool {
    match statement {
        Statement::Assign { target, value, .. } => {
            expression_may_fail(value) || lvalue_may_fail(model, target, binders)
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            expression_may_fail(condition)
                || then_statements
                    .iter()
                    .chain(else_statements)
                    .any(|statement| statement_may_fail(model, statement, binders))
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            if binder_may_fail(binder) {
                return true;
            }
            let entry = match binder {
                Binder::Typed {
                    name, type_name, ..
                } if type_name.namespace.is_none() => {
                    (name.as_str(), Some(type_name.name.as_str()))
                }
                Binder::Typed { name, .. }
                | Binder::Range { name, .. }
                | Binder::Collection { name, .. } => (name.as_str(), None),
            };
            binders.push(entry);
            let may_fail = statements
                .iter()
                .any(|statement| statement_may_fail(model, statement, binders));
            binders.pop();
            may_fail
        }
    }
}

fn lvalue_may_fail(model: &KernelModel, target: &LValue, binders: &[(&str, Option<&str>)]) -> bool {
    match target {
        LValue::Var(_) => false,
        LValue::Field(base, _) => lvalue_may_fail(model, base, binders),
        LValue::Index(name, index) => {
            let Expr::Var(variable) = index else {
                return true;
            };
            let binder_type = binders
                .iter()
                .rev()
                .find(|(binder, _)| binder == variable)
                .and_then(|(_, ty)| *ty);
            let key_type = model.state.iter().find_map(|(state, ty)| match ty {
                fsl_core::TypeRef::Map(key, _) if state == name => Some(key.as_ref()),
                _ => None,
            });
            !matches!(
                (binder_type, key_type),
                (Some(binder), Some(fsl_core::TypeRef::Named(key))) if binder == key
            )
        }
    }
}

fn binder_may_fail(binder: &Binder) -> bool {
    match binder {
        Binder::Typed { where_expr, .. } => where_expr.as_deref().is_some_and(expression_may_fail),
        Binder::Range {
            lo, hi, where_expr, ..
        } => {
            expression_may_fail(lo)
                || expression_may_fail(hi)
                || where_expr.as_deref().is_some_and(expression_may_fail)
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            expression_may_fail(collection)
                || where_expr.as_deref().is_some_and(expression_may_fail)
        }
    }
}

fn expression_may_fail(expr: &Expr) -> bool {
    recursion::guard(|| match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => false,
        Expr::Neg(_)
        | Expr::Index(_, _)
        | Expr::Method { .. }
        | Expr::Aggregate {
            kind: AggregateKind::Sum,
            ..
        } => true,
        Expr::UnaryNamed { name, .. } if name == "abs" => true,
        Expr::Binary { op, .. } if matches!(op.as_str(), "/" | "%" | "+" | "-" | "*") => true,
        Expr::Some(inner)
        | Expr::Not(inner)
        | Expr::Field(inner, _)
        | Expr::Is { expr: inner, .. }
        | Expr::Stage { entity: inner, .. }
        | Expr::UnaryNamed { expr: inner, .. } => expression_may_fail(inner),
        Expr::Set(items) | Expr::Seq(items) | Expr::Call { args: items, .. } => {
            items.iter().any(expression_may_fail)
        }
        Expr::Struct { fields, .. } => fields.iter().any(|(_, item)| expression_may_fail(item)),
        Expr::Binary { left, right, .. } | Expr::BinaryNamed { left, right, .. } => {
            expression_may_fail(left) || expression_may_fail(right)
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            expression_may_fail(first) || expression_may_fail(second) || expression_may_fail(third)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            expression_may_fail(condition)
                || expression_may_fail(then_expr)
                || expression_may_fail(else_expr)
        }
        Expr::Quantified { binder, body, .. } => {
            binder_may_fail(binder) || expression_may_fail(body)
        }
        Expr::Aggregate { binder, value, .. } => {
            binder_may_fail(binder) || value.as_deref().is_some_and(expression_may_fail)
        }
    })
}

/// Every init failure site, in evaluation order.
pub(crate) fn init_failure_sites<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    state: &SymbolicState<S::Term>,
) -> Result<Vec<InitFailureSite<S::Term>>, VerifyError> {
    let mut sites = Vec::new();
    let reached = solver.bool_value(true);
    for statement in &model.init {
        statement_sites(
            solver,
            model,
            statement,
            state,
            &Bindings::new(),
            &reached,
            &mut sites,
        )?;
    }
    Ok(sites)
}

fn located(message: &str, span: Span) -> String {
    format!(
        "{message} in init at {}:{}",
        span.start.line, span.start.column
    )
}

#[allow(clippy::too_many_lines)]
fn statement_sites<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    statement: &Statement,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    reached: &S::Term,
    sites: &mut Vec<InitFailureSite<S::Term>>,
) -> Result<(), VerifyError> {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => {
            let mut local = Vec::new();
            expression_sites(solver, model, value, state, bindings, reached, &mut local)?;
            let value_defined =
                evaluation_status(solver, model, value, state, bindings, None)?.fully_defined;
            let target_reached = solver.and(&[reached.clone(), value_defined])?;
            lvalue_sites(
                solver,
                model,
                target,
                state,
                bindings,
                &target_reached,
                &mut local,
            )?;
            sites.extend(
                local
                    .into_iter()
                    .map(|(condition, message)| InitFailureSite {
                        condition,
                        message: located(message, *span),
                    }),
            );
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            let mut local = Vec::new();
            expression_sites(
                solver, model, condition, state, bindings, reached, &mut local,
            )?;
            sites.extend(
                local
                    .into_iter()
                    .map(|(condition, message)| InitFailureSite {
                        condition,
                        message: located(message, *span),
                    }),
            );
            let condition_defined =
                evaluation_status(solver, model, condition, state, bindings, None)?.fully_defined;
            let value = eval(solver, model, condition, state, &mut bindings.clone(), None)?;
            let value = bool_term(&value)?.clone();
            let then_reached =
                solver.and(&[reached.clone(), condition_defined.clone(), value.clone()])?;
            for statement in then_statements {
                statement_sites(
                    solver,
                    model,
                    statement,
                    state,
                    bindings,
                    &then_reached,
                    sites,
                )?;
            }
            let else_reached =
                solver.and(&[reached.clone(), condition_defined, solver.not(&value)?])?;
            for statement in else_statements {
                statement_sites(
                    solver,
                    model,
                    statement,
                    state,
                    bindings,
                    &else_reached,
                    sites,
                )?;
            }
        }
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let where_expr = match binder {
                Binder::Typed { where_expr, .. }
                | Binder::Range { where_expr, .. }
                | Binder::Collection { where_expr, .. } => where_expr.as_deref(),
            };
            for (name, value) in binder_values(solver, model, binder)? {
                let mut local = bindings.clone();
                local.insert(name, value);
                let mut body_reached = reached.clone();
                if let Some(where_expr) = where_expr {
                    let mut where_sites = Vec::new();
                    expression_sites(
                        solver,
                        model,
                        where_expr,
                        state,
                        &local,
                        reached,
                        &mut where_sites,
                    )?;
                    sites.extend(where_sites.into_iter().map(|(condition, message)| {
                        InitFailureSite {
                            condition,
                            message: located(message, *span),
                        }
                    }));
                    let where_defined =
                        evaluation_status(solver, model, where_expr, state, &local, None)?
                            .fully_defined;
                    let where_term =
                        binder_where(solver, model, binder, state, &mut local.clone(), None)?
                            .unwrap_or_else(|| solver.bool_value(true));
                    body_reached = solver.and(&[body_reached, where_defined, where_term])?;
                }
                for statement in statements {
                    statement_sites(
                        solver,
                        model,
                        statement,
                        state,
                        &local,
                        &body_reached,
                        sites,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn lvalue_sites<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    target: &LValue,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    reached: &S::Term,
    sites: &mut Vec<(S::Term, &'static str)>,
) -> Result<(), VerifyError> {
    match target {
        LValue::Var(_) => Ok(()),
        LValue::Field(base, _) => {
            lvalue_sites(solver, model, base, state, bindings, reached, sites)
        }
        LValue::Index(name, index) => {
            expression_sites(solver, model, index, state, bindings, reached, sites)?;
            let index_defined =
                evaluation_status(solver, model, index, state, bindings, None)?.fully_defined;
            let index_value = eval(solver, model, index, state, &mut bindings.clone(), None)?;
            let root = state
                .get(name)
                .ok_or_else(|| VerifyError::new(format!("unknown state variable '{name}'")))?;
            index_site(
                solver,
                model,
                root,
                &index_value,
                &solver.and(&[reached.clone(), index_defined])?,
                IndexUse::Assignment,
                sites,
            )
        }
    }
}

/// How an index is used: the concrete evaluator words a read and an
/// assignment target differently.
#[derive(Clone, Copy)]
enum IndexUse {
    Read,
    Assignment,
}

fn index_site<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    base: &SymbolicValue<S::Term>,
    index: &SymbolicValue<S::Term>,
    reached: &S::Term,
    usage: IndexUse,
    sites: &mut Vec<(S::Term, &'static str)>,
) -> Result<(), VerifyError> {
    let message = match (base, usage) {
        (SymbolicValue::Map { .. }, IndexUse::Read) => "map index outside finite key domain",
        (SymbolicValue::Seq { .. }, IndexUse::Read) => "sequence index out of range",
        (SymbolicValue::Map { .. }, IndexUse::Assignment) => {
            "map assignment index outside key domain"
        }
        (SymbolicValue::Seq { .. }, IndexUse::Assignment) => {
            "sequence assignment index out of range"
        }
        _ => return Err(VerifyError::new("indexing requires a map or sequence")),
    };
    let accessible = index_accessible(solver, model, base, index)?;
    sites.push((
        solver.and(&[reached.clone(), solver.not(&accessible)?])?,
        message,
    ));
    Ok(())
}

fn defined<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
) -> Result<S::Term, VerifyError> {
    Ok(evaluation_status(solver, model, expr, state, bindings, None)?.fully_defined)
}

/// The failure sites of one expression, mirroring `evaluation_status`'s
/// reachability: `and`/`or`/`=>` and `if` short-circuit, operands are
/// evaluated left to right, and a binder's pattern bindings flow exactly as
/// the evaluator threads them.
fn expression_sites<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    reached: &S::Term,
    sites: &mut Vec<(S::Term, &'static str)>,
) -> Result<(), VerifyError> {
    recursion::guard(|| {
        expression_sites_inner(solver, model, expr, state, bindings, reached, sites)
    })
}

#[allow(clippy::too_many_lines)]
fn expression_sites_inner<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    expr: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    reached: &S::Term,
    sites: &mut Vec<(S::Term, &'static str)>,
) -> Result<(), VerifyError> {
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => {
            Ok(())
        }
        Expr::Neg(inner) => negation_sites(
            solver,
            model,
            inner,
            state,
            bindings,
            reached,
            sites,
            "integer overflow in negation",
        ),
        Expr::UnaryNamed {
            name, expr: inner, ..
        } if name == "abs" => negation_sites(
            solver,
            model,
            inner,
            state,
            bindings,
            reached,
            sites,
            "integer overflow in abs",
        ),
        Expr::Some(inner)
        | Expr::Not(inner)
        | Expr::Field(inner, _)
        | Expr::Is { expr: inner, .. }
        | Expr::Stage { entity: inner, .. }
        | Expr::UnaryNamed { expr: inner, .. } => {
            expression_sites(solver, model, inner, state, bindings, reached, sites)
        }
        Expr::Set(items) | Expr::Seq(items) | Expr::Call { args: items, .. } => {
            ordered_sites(solver, model, items.iter(), state, bindings, reached, sites)
        }
        Expr::Struct { fields, .. } => ordered_sites(
            solver,
            model,
            fields.iter().map(|(_, item)| item),
            state,
            bindings,
            reached,
            sites,
        ),
        Expr::BinaryNamed { left, right, .. } => ordered_sites(
            solver,
            model,
            [left.as_ref(), right.as_ref()].into_iter(),
            state,
            bindings,
            reached,
            sites,
        ),
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => ordered_sites(
            solver,
            model,
            [first.as_ref(), second.as_ref(), third.as_ref()].into_iter(),
            state,
            bindings,
            reached,
            sites,
        ),
        Expr::Index(base, index) => {
            let mut local = bindings.clone();
            expression_sites(solver, model, base, state, &local, reached, sites)?;
            let base_defined = defined(solver, model, base, state, &local)?;
            let base_value = eval(solver, model, base, state, &mut local, None)?;
            let index_reached = solver.and(&[reached.clone(), base_defined])?;
            expression_sites(solver, model, index, state, &local, &index_reached, sites)?;
            let index_defined = defined(solver, model, index, state, &local)?;
            let index_value = eval(solver, model, index, state, &mut local, None)?;
            index_site(
                solver,
                model,
                &base_value,
                &index_value,
                &solver.and(&[index_reached, index_defined])?,
                IndexUse::Read,
                sites,
            )
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            let mut local = bindings.clone();
            expression_sites(solver, model, receiver, state, &local, reached, sites)?;
            let receiver_defined = defined(solver, model, receiver, state, &local)?;
            let receiver_value = eval(solver, model, receiver, state, &mut local, None)?;
            let mut operation_reached = solver.and(&[reached.clone(), receiver_defined])?;
            for argument in args {
                expression_sites(
                    solver,
                    model,
                    argument,
                    state,
                    &local,
                    &operation_reached,
                    sites,
                )?;
                operation_reached = solver.and(&[
                    operation_reached,
                    defined(solver, model, argument, state, &local)?,
                ])?;
            }
            let message = match (&receiver_value, name.as_str(), args.len()) {
                (SymbolicValue::Seq { .. }, "head", 0) => "head() on empty sequence",
                (SymbolicValue::Seq { .. }, "pop", 0) => "pop() on empty sequence",
                (SymbolicValue::Seq { .. }, "at", 1) => "at() index out of range",
                _ => return Ok(()),
            };
            let argument_values = args
                .iter()
                .map(|argument| eval(solver, model, argument, state, &mut local, None))
                .collect::<Result<Vec<_>, _>>()?;
            let operation_defined =
                method_definedness(solver, name, &receiver_value, &argument_values)?;
            sites.push((
                solver.and(&[operation_reached, solver.not(&operation_defined)?])?,
                message,
            ));
            Ok(())
        }
        Expr::Binary { op, left, right } => {
            let mut local = bindings.clone();
            expression_sites(solver, model, left, state, &local, reached, sites)?;
            let left_defined = defined(solver, model, left, state, &local)?;
            let right_reached = match op.as_str() {
                "and" | "=>" => {
                    let left_value = eval(solver, model, left, state, &mut local, None)?;
                    bool_term(&left_value)?.clone()
                }
                "or" => {
                    let left_value = eval(solver, model, left, state, &mut local, None)?;
                    solver.not(bool_term(&left_value)?)?
                }
                _ => solver.bool_value(true),
            };
            let right_reached =
                solver.and(&[reached.clone(), left_defined.clone(), right_reached])?;
            expression_sites(solver, model, right, state, &local, &right_reached, sites)?;
            let operation = match op.as_str() {
                "/" => ("division by zero", "integer overflow in division"),
                "%" => ("remainder by zero", "integer overflow in remainder"),
                "+" => ("", "integer overflow in addition"),
                "-" => ("", "integer overflow in subtraction"),
                "*" => ("", "integer overflow in multiplication"),
                _ => return Ok(()),
            };
            let right_defined = defined(solver, model, right, state, &local)?;
            let operands_reached = solver.and(&[reached.clone(), left_defined, right_defined])?;
            let left_value = eval(solver, model, left, state, &mut local, None)?;
            let right_value = eval(solver, model, right, state, &mut local, None)?;
            let left_term = int_term(&left_value)?;
            let right_term = int_term(&right_value)?;
            if matches!(op.as_str(), "/" | "%") {
                let zero = solver.equal(right_term, &solver.int_value(0))?;
                sites.push((solver.and(&[operands_reached.clone(), zero])?, operation.0));
                let overflow = solver.and(&[
                    solver.equal(left_term, &solver.int_value(i64::MIN))?,
                    solver.equal(right_term, &solver.int_value(-1))?,
                ])?;
                sites.push((solver.and(&[operands_reached, overflow])?, operation.1));
            } else {
                let result = match op.as_str() {
                    "+" => solver.add(left_term, right_term)?,
                    "-" => solver.sub(left_term, right_term)?,
                    _ => solver.mul(left_term, right_term)?,
                };
                sites.push((
                    solver.and(&[
                        operands_reached,
                        solver.not(&i64_term_is_in_range(solver, &result)?)?,
                    ])?,
                    operation.1,
                ));
            }
            Ok(())
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            let mut local = bindings.clone();
            expression_sites(solver, model, condition, state, &local, reached, sites)?;
            let condition_defined = defined(solver, model, condition, state, &local)?;
            let value = eval(solver, model, condition, state, &mut local, None)?;
            let value = bool_term(&value)?.clone();
            let then_reached =
                solver.and(&[reached.clone(), condition_defined.clone(), value.clone()])?;
            expression_sites(
                solver,
                model,
                then_expr,
                state,
                &local,
                &then_reached,
                sites,
            )?;
            let else_reached =
                solver.and(&[reached.clone(), condition_defined, solver.not(&value)?])?;
            expression_sites(
                solver,
                model,
                else_expr,
                state,
                &local,
                &else_reached,
                sites,
            )
        }
        // A binder expression is one site: its range, `where`, body, and (for
        // `sum`) accumulation are not named separately.
        Expr::Quantified { .. } | Expr::Aggregate { .. } => {
            let message = match expr {
                Expr::Aggregate {
                    kind: AggregateKind::Sum,
                    ..
                } => "integer overflow in sum or undefined summand",
                _ => "undefined evaluation",
            };
            let undefined = solver.not(&defined(solver, model, expr, state, bindings)?)?;
            sites.push((solver.and(&[reached.clone(), undefined])?, message));
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn negation_sites<S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    inner: &Expr,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    reached: &S::Term,
    sites: &mut Vec<(S::Term, &'static str)>,
    message: &'static str,
) -> Result<(), VerifyError> {
    expression_sites(solver, model, inner, state, bindings, reached, sites)?;
    let inner_defined = defined(solver, model, inner, state, bindings)?;
    let value = eval(solver, model, inner, state, &mut bindings.clone(), None)?;
    let overflow = solver.equal(int_term(&value)?, &solver.int_value(i64::MIN))?;
    sites.push((
        solver.and(&[reached.clone(), inner_defined, overflow])?,
        message,
    ));
    Ok(())
}

fn ordered_sites<'a, S: SmtSolver>(
    solver: &S,
    model: &KernelModel,
    items: impl Iterator<Item = &'a Expr>,
    state: &SymbolicState<S::Term>,
    bindings: &Bindings<S::Term>,
    reached: &S::Term,
    sites: &mut Vec<(S::Term, &'static str)>,
) -> Result<(), VerifyError> {
    let mut item_reached = reached.clone();
    for item in items {
        expression_sites(solver, model, item, state, bindings, &item_reached, sites)?;
        item_reached =
            solver.and(&[item_reached, defined(solver, model, item, state, bindings)?])?;
    }
    Ok(())
}

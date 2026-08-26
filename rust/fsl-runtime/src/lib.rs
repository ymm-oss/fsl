// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Solver-independent concrete evaluator, monitor, replay, and BFS oracle.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use fsl_core::{
    ActionCorrespondenceTarget, ActionDef, ActionGuard, FslValue as Value,
    KernelAggregateKind as AggregateKind, KernelBinder as Binder, KernelExpr as Expr,
    KernelLValue as LValue, KernelModel, KernelStatement as Statement, ModelError, ParamDef,
    Refinement, TraceAction, TraceChange, TraceStep, TypeDef, TypeRef, display_name,
    insert_requirement_metadata, internal_origin_json, model_warnings, origin_display_name,
    state_summary, static_leadsto_bindings,
};
use serde_json::{Value as JsonValue, json};

mod explicit;
mod trace;

pub use explicit::{
    ExplicitReachableWitness, ExplicitResult, ExplicitViolation, deterministic_initial_state,
    explicit_unsupported_reason, verify_explicit, verify_explicit_selected,
};

pub type State = BTreeMap<String, Value>;
pub type Bindings = BTreeMap<String, Value>;

std::thread_local! {
    /// DESIGN-divmod.md §2.1/§2.3: while set, `/` and `%` by a zero divisor
    /// evaluate to the totally-defined value `0` instead of raising the
    /// §2.2 action-context unguarded-operation error. Property-context
    /// evaluation entry points (invariant, trans, reachable, leadsTo, and
    /// refinement state mapping) scope this with [`with_total_division`];
    /// action guard/statement/ensures evaluation leaves it unset so an
    /// unguarded `/`/`%` there is still classified `partial_op`.
    static TOTAL_DIVISION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct TotalDivisionScope {
    previous: bool,
}

impl Drop for TotalDivisionScope {
    fn drop(&mut self) {
        TOTAL_DIVISION.with(|flag| flag.set(self.previous));
    }
}

/// Evaluate property-context expressions with `/` and `%` by zero totally
/// defined as `0` (DESIGN-divmod.md §2.1, §2.3) for the duration of `body`.
fn with_total_division<T>(body: impl FnOnce() -> T) -> T {
    let _scope = TotalDivisionScope {
        previous: TOTAL_DIVISION.with(|flag| flag.replace(true)),
    };
    body()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeError {
    pub message: String,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RuntimeError {}

impl From<ModelError> for RuntimeError {
    fn from(error: ModelError) -> Self {
        Self {
            message: error.message,
        }
    }
}

/// Evaluate one expression in a concrete logical state.
///
/// Pattern matches intentionally mutate `bindings`, matching FSL's
/// `value is some(name)` binding semantics for the remainder of a
/// short-circuited Boolean expression.
///
/// # Errors
///
/// Returns [`RuntimeError`] for unknown names, type mismatches, invalid
/// indexing/method calls, partial operations, or checked integer overflow.
#[allow(clippy::too_many_lines)]
pub fn eval(
    expr: &Expr,
    state: &State,
    bindings: &mut Bindings,
    model: &KernelModel,
    old_state: Option<&State>,
) -> Result<Value, RuntimeError> {
    match expr {
        Expr::Num(value) => Ok(Value::Int(*value)),
        Expr::Bool(value) => Ok(Value::Bool(*value)),
        Expr::None => Ok(Value::None),
        Expr::Some(expr) => Ok(Value::Some(Box::new(eval(
            expr, state, bindings, model, old_state,
        )?))),
        Expr::Set(items) => Ok(Value::Set(
            items
                .iter()
                .map(|item| eval(item, state, bindings, model, old_state))
                .collect::<Result<_, _>>()?,
        )),
        Expr::Seq(items) => Ok(Value::Seq(
            items
                .iter()
                .map(|item| eval(item, state, bindings, model, old_state))
                .collect::<Result<_, _>>()?,
        )),
        Expr::Struct { name, fields } => Ok(Value::Struct {
            type_name: name.clone(),
            fields: fields
                .iter()
                .map(|(field, expr)| {
                    Ok((
                        field.clone(),
                        eval(expr, state, bindings, model, old_state)?,
                    ))
                })
                .collect::<Result<_, RuntimeError>>()?,
        }),
        Expr::Var(name) => bindings
            .get(name)
            .or_else(|| state.get(name))
            .or_else(|| model.consts.get(name))
            .or_else(|| model.enum_members.get(name))
            .cloned()
            .ok_or_else(|| runtime_error(format!("unknown identifier '{name}'"))),
        Expr::EnumMember { type_name, member } => {
            let Some(TypeDef::Enum { members, .. }) = model.types.get(type_name) else {
                return Err(runtime_error(format!("unknown enum type '{type_name}'")));
            };
            if !members.contains(member) {
                return Err(runtime_error(format!(
                    "unknown enum member '{type_name}.{member}'"
                )));
            }
            Ok(Value::Enum {
                type_name: type_name.clone(),
                member: member.clone(),
            })
        }
        Expr::Call { name, .. } => {
            Err(runtime_error(format!("unexpanded predicate call '{name}'")))
        }
        Expr::Stage { .. } => Err(runtime_error("unlowered stage access")),
        Expr::Index(base, index) => {
            let base = eval(base, state, bindings, model, old_state)?;
            let index = eval(index, state, bindings, model, old_state)?;
            match base {
                Value::Map(values) => values
                    .get(&index)
                    .cloned()
                    .ok_or_else(|| runtime_error("map index outside finite key domain")),
                Value::Seq(values) => values
                    .get(as_usize(index, "sequence index out of range")?)
                    .cloned()
                    .ok_or_else(|| runtime_error("sequence index out of range")),
                _ => Err(runtime_error("indexing requires a map or sequence")),
            }
        }
        Expr::Field(base, field) => {
            let base = eval(base, state, bindings, model, old_state)?;
            match base {
                Value::Struct { fields, .. } => fields
                    .get(field)
                    .cloned()
                    .ok_or_else(|| runtime_error(format!("unknown struct field '{field}'"))),
                _ => Err(runtime_error("field access requires a struct")),
            }
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            let receiver = eval(receiver, state, bindings, model, old_state)?;
            eval_method(receiver, name, args, state, bindings, model, old_state)
        }
        Expr::Binary { op, left, right } => {
            eval_binary(op, left, right, state, bindings, model, old_state)
        }
        Expr::Neg(expr) => {
            let value = as_int(eval(expr, state, bindings, model, old_state)?)?;
            Ok(Value::Int(value.checked_neg().ok_or_else(|| {
                runtime_error("integer overflow in negation")
            })?))
        }
        Expr::Not(expr) => Ok(Value::Bool(!as_bool(eval(
            expr, state, bindings, model, old_state,
        )?)?)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            if as_bool(eval(condition, state, bindings, model, old_state)?)? {
                eval(then_expr, state, bindings, model, old_state)
            } else {
                eval(else_expr, state, bindings, model, old_state)
            }
        }
        Expr::Is { expr, pattern } => {
            let value = eval(expr, state, bindings, model, old_state)?;
            match (value, pattern) {
                (Value::None, fsl_core::Pattern::None) => Ok(Value::Bool(true)),
                (Value::Some(_), fsl_core::Pattern::None)
                | (Value::None, fsl_core::Pattern::Some(_)) => Ok(Value::Bool(false)),
                (Value::Some(value), fsl_core::Pattern::Some(name)) => {
                    bindings.insert(name.clone(), *value);
                    Ok(Value::Bool(true))
                }
                _ => Err(runtime_error("is pattern requires an Option value")),
            }
        }
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => {
            let candidates = binder_values(binder, state, bindings, model, old_state)?;
            if quantifier == "forall" {
                for (value, mut local) in candidates {
                    local.insert(binder_name(binder).to_owned(), value);
                    if !binder_where_holds(binder, state, &mut local, model, old_state)? {
                        continue;
                    }
                    if !as_bool(eval(body, state, &mut local, model, old_state)?)? {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            } else {
                for (value, mut local) in candidates {
                    local.insert(binder_name(binder).to_owned(), value);
                    if !binder_where_holds(binder, state, &mut local, model, old_state)? {
                        continue;
                    }
                    if as_bool(eval(body, state, &mut local, model, old_state)?)? {
                        bindings.extend(local);
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
        }
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => {
            let mut matches = 0_i64;
            let mut sum = 0_i64;
            for (candidate, mut local) in binder_values(binder, state, bindings, model, old_state)?
            {
                local.insert(binder_name(binder).to_owned(), candidate);
                if !binder_where_holds(binder, state, &mut local, model, old_state)? {
                    continue;
                }
                matches += 1;
                if let Some(value) = value {
                    sum = sum
                        .checked_add(as_int(eval(value, state, &mut local, model, old_state)?)?)
                        .ok_or_else(|| runtime_error("integer overflow in sum"))?;
                }
            }
            Ok(match kind {
                AggregateKind::Count => Value::Int(matches),
                AggregateKind::Sum => Value::Int(sum),
                AggregateKind::Unique => Value::Bool(matches <= 1),
                AggregateKind::ExactlyOne => Value::Bool(matches == 1),
            })
        }
        Expr::UnaryNamed { name, expr, .. } => match name.as_str() {
            "old" => eval(
                expr,
                old_state.ok_or_else(|| runtime_error("old() used without old state"))?,
                bindings,
                model,
                None,
            ),
            "abs" => {
                let value = as_int(eval(expr, state, bindings, model, old_state)?)?;
                Ok(Value::Int(
                    value
                        .checked_abs()
                        .ok_or_else(|| runtime_error("integer overflow in abs"))?,
                ))
            }
            "rel_acyclic" | "rel_functional" | "rel_injective" | "rel_domain" | "rel_range" => {
                eval_relation_unary(name, eval(expr, state, bindings, model, old_state)?, model)
            }
            _ => Err(runtime_error(format!(
                "unsupported unary expression '{name}'"
            ))),
        },
        Expr::BinaryNamed { name, left, right } => {
            let left = as_int(eval(left, state, bindings, model, old_state)?)?;
            let right = as_int(eval(right, state, bindings, model, old_state)?)?;
            Ok(Value::Int(if name == "min" {
                left.min(right)
            } else if name == "max" {
                left.max(right)
            } else {
                return Err(runtime_error(format!(
                    "unsupported binary function '{name}'"
                )));
            }))
        }
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } if name == "rel_reachable" => relation_reachable(
            eval(first, state, bindings, model, old_state)?,
            &eval(second, state, bindings, model, old_state)?,
            &eval(third, state, bindings, model, old_state)?,
        ),
        Expr::TernaryNamed { name, .. } => Err(runtime_error(format!(
            "unsupported ternary function '{name}'"
        ))),
    }
}

/// Return the concrete binder assignments that make a failed property false.
///
/// This mirrors the diagnostic projection used by the symbolic verifier: it
/// walks quantified properties and reports all failing values for the first
/// false top-level conjunct.
///
/// # Errors
///
/// Returns [`RuntimeError`] when the expression cannot be evaluated.
pub fn violating_bindings(
    expr: &Expr,
    state: &State,
    model: &KernelModel,
) -> Result<Option<Vec<Bindings>>, RuntimeError> {
    fn search(
        expr: &Expr,
        state: &State,
        bindings: &Bindings,
        model: &KernelModel,
    ) -> Result<Option<Vec<Bindings>>, RuntimeError> {
        if let Expr::Quantified { binder, body, .. } = expr {
            let mut bad = Vec::new();
            for (value, mut local) in binder_values(binder, state, bindings, model, None)? {
                local.insert(binder_name(binder).to_owned(), value);
                if !binder_where_holds(binder, state, &mut local, model, None)? {
                    continue;
                }
                if !as_bool(eval(body, state, &mut local, model, None)?)? {
                    bad.push(local);
                }
            }
            return Ok((!bad.is_empty()).then_some(bad));
        }
        if let Expr::Binary { op, left, right } = expr
            && op == "and"
        {
            if let Some(bad) = search(left, state, bindings, model)? {
                return Ok(Some(bad));
            }
            return search(right, state, bindings, model);
        }
        let mut local = bindings.clone();
        if as_bool(eval(expr, state, &mut local, model, None)?)? {
            Ok(None)
        } else {
            Ok(Some(vec![local]))
        }
    }

    with_total_division(|| search(expr, state, &Bindings::new(), model))
}

#[allow(clippy::too_many_arguments)]
fn eval_method(
    receiver: Value,
    name: &str,
    args: &[Expr],
    state: &State,
    bindings: &mut Bindings,
    model: &KernelModel,
    old_state: Option<&State>,
) -> Result<Value, RuntimeError> {
    let values = args
        .iter()
        .map(|arg| eval(arg, state, bindings, model, old_state))
        .collect::<Result<Vec<_>, _>>()?;
    match receiver {
        Value::Set(mut set) => match (name, values.as_slice()) {
            ("contains", [value]) => Ok(Value::Bool(set.contains(value))),
            ("add", [value]) => {
                set.insert(value.clone());
                Ok(Value::Set(set))
            }
            ("remove", [value]) => {
                set.remove(value);
                Ok(Value::Set(set))
            }
            ("size", []) => Ok(Value::Int(i64_len(set.len())?)),
            _ => Err(runtime_error(format!("invalid Set method '{name}'"))),
        },
        Value::Seq(mut sequence) => match (name, values.as_slice()) {
            ("contains", [value]) => Ok(Value::Bool(sequence.contains(value))),
            ("push", [value]) => {
                sequence.push(value.clone());
                Ok(Value::Seq(sequence))
            }
            ("pop", []) => {
                if sequence.is_empty() {
                    Err(runtime_error("pop() on empty sequence"))
                } else {
                    sequence.remove(0);
                    Ok(Value::Seq(sequence))
                }
            }
            ("head", []) => sequence
                .first()
                .cloned()
                .ok_or_else(|| runtime_error("head() on empty sequence")),
            ("at", [index]) => sequence
                .get(as_usize(index.clone(), "at() index out of range")?)
                .cloned()
                .ok_or_else(|| runtime_error("at() index out of range")),
            ("size", []) => Ok(Value::Int(i64_len(sequence.len())?)),
            _ => Err(runtime_error(format!("invalid Seq method '{name}'"))),
        },
        Value::Relation(mut relation) => match (name, values.as_slice()) {
            ("contains", [source, target]) => Ok(Value::Bool(
                relation.contains(&(source.clone(), target.clone())),
            )),
            ("add", [source, target]) => {
                relation.insert((source.clone(), target.clone()));
                Ok(Value::Relation(relation))
            }
            ("remove", [source, target]) => {
                relation.remove(&(source.clone(), target.clone()));
                Ok(Value::Relation(relation))
            }
            _ => Err(runtime_error(format!("invalid relation method '{name}'"))),
        },
        _ => Err(runtime_error("method receiver has no collection methods")),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_binary(
    op: &str,
    left: &Expr,
    right: &Expr,
    state: &State,
    bindings: &mut Bindings,
    model: &KernelModel,
    old_state: Option<&State>,
) -> Result<Value, RuntimeError> {
    if op == "and" {
        if !as_bool(eval(left, state, bindings, model, old_state)?)? {
            return Ok(Value::Bool(false));
        }
        return Ok(Value::Bool(as_bool(eval(
            right, state, bindings, model, old_state,
        )?)?));
    }
    if op == "or" {
        if as_bool(eval(left, state, bindings, model, old_state)?)? {
            return Ok(Value::Bool(true));
        }
        return Ok(Value::Bool(as_bool(eval(
            right, state, bindings, model, old_state,
        )?)?));
    }
    if op == "=>" {
        if !as_bool(eval(left, state, bindings, model, old_state)?)? {
            return Ok(Value::Bool(true));
        }
        return Ok(Value::Bool(as_bool(eval(
            right, state, bindings, model, old_state,
        )?)?));
    }
    let left = eval(left, state, bindings, model, old_state)?;
    let right = eval(right, state, bindings, model, old_state)?;
    match op {
        "+" => checked_binary(left, right, i64::checked_add, "addition"),
        "-" => checked_binary(left, right, i64::checked_sub, "subtraction"),
        "*" => checked_binary(left, right, i64::checked_mul, "multiplication"),
        "/" => {
            let left = as_int(left)?;
            let right = as_int(right)?;
            if right == 0 {
                if TOTAL_DIVISION.with(std::cell::Cell::get) {
                    Ok(Value::Int(0))
                } else {
                    Err(runtime_error("division by zero"))
                }
            } else {
                Ok(Value::Int(left.div_euclid(right)))
            }
        }
        "%" => {
            let left = as_int(left)?;
            let right = as_int(right)?;
            if right == 0 {
                if TOTAL_DIVISION.with(std::cell::Cell::get) {
                    Ok(Value::Int(0))
                } else {
                    Err(runtime_error("remainder by zero"))
                }
            } else {
                Ok(Value::Int(left.rem_euclid(right)))
            }
        }
        "==" => Ok(Value::Bool(left == right)),
        "!=" => Ok(Value::Bool(left != right)),
        "<" | "<=" | ">" | ">=" => {
            let ordering = left.cmp(&right);
            Ok(Value::Bool(match op {
                "<" => ordering.is_lt(),
                "<=" => ordering.is_le(),
                ">" => ordering.is_gt(),
                ">=" => ordering.is_ge(),
                _ => unreachable!(),
            }))
        }
        _ => Err(runtime_error(format!("unknown operator '{op}'"))),
    }
}

fn binder_values(
    binder: &Binder,
    state: &State,
    bindings: &Bindings,
    model: &KernelModel,
    old_state: Option<&State>,
) -> Result<Vec<(Value, Bindings)>, RuntimeError> {
    let values = match binder {
        Binder::Typed { type_name, .. } => {
            model.domain_values(&TypeRef::Named(qualified_type(type_name)?))?
        }
        Binder::Range { lo, hi, .. } => {
            let mut local = bindings.clone();
            let lo = as_int(eval(lo, state, &mut local, model, old_state)?)?;
            let hi = as_int(eval(hi, state, &mut local, model, old_state)?)?;
            (lo..=hi).map(Value::Int).collect()
        }
        Binder::Collection { collection, .. } => {
            let mut local = bindings.clone();
            match eval(collection, state, &mut local, model, old_state)? {
                Value::Set(values) => values.into_iter().collect(),
                Value::Seq(values) => values,
                _ => return Err(runtime_error("collection binder requires Set or Seq")),
            }
        }
    };
    Ok(values
        .into_iter()
        .map(|value| (value, bindings.clone()))
        .collect())
}

fn binder_where_holds(
    binder: &Binder,
    state: &State,
    bindings: &mut Bindings,
    model: &KernelModel,
    old_state: Option<&State>,
) -> Result<bool, RuntimeError> {
    let condition = match binder {
        Binder::Typed { where_expr, .. }
        | Binder::Range { where_expr, .. }
        | Binder::Collection { where_expr, .. } => where_expr.as_deref(),
    };
    condition.map_or(Ok(true), |condition| {
        as_bool(eval(condition, state, bindings, model, old_state)?)
    })
}

fn relation_reachable(
    relation: Value,
    source: &Value,
    target: &Value,
) -> Result<Value, RuntimeError> {
    let Value::Relation(edges) = relation else {
        return Err(runtime_error("reachable() requires a relation"));
    };
    // Non-reflexive: `reachable(r, a, a)` is true only via a real path of
    // one or more edges back to `a`, never a free zero-hop `a == a` step
    // (`docs/LANGUAGE.md`'s relation section; matches the frozen Python
    // reference's `_relation_reachable` in `src/fslc/runtime.py`, and this
    // crate's own symbolic evaluator). The frontier starts at `source`'s
    // *direct successors*, not `source` itself, so an empty or acyclic
    // relation never reports self-reachability by construction. `source`
    // itself is deliberately left out of `seen` here, so a cycle that
    // leads back to `source` still re-enqueues it -- needed to detect
    // `reachable(r, a, a)` via a multi-hop cycle through `a`.
    let mut seen = BTreeSet::new();
    let mut frontier = Vec::new();
    for (_, next) in edges.iter().filter(|(from, _)| from == source) {
        if seen.insert(next.clone()) {
            frontier.push(next.clone());
        }
    }
    while let Some(current) = frontier.pop() {
        if &current == target {
            return Ok(Value::Bool(true));
        }
        for (_, next) in edges.iter().filter(|(from, _)| from == &current) {
            if seen.insert(next.clone()) {
                frontier.push(next.clone());
            }
        }
    }
    Ok(Value::Bool(false))
}

fn eval_relation_unary(
    name: &str,
    relation: Value,
    _model: &KernelModel,
) -> Result<Value, RuntimeError> {
    let Value::Relation(edges) = relation else {
        return Err(runtime_error("relation function requires a relation"));
    };
    match name {
        "rel_functional" => {
            let mut sources = BTreeMap::<Value, usize>::new();
            for (source, _) in &edges {
                *sources.entry(source.clone()).or_default() += 1;
            }
            Ok(Value::Bool(sources.values().all(|count| *count <= 1)))
        }
        "rel_injective" => {
            let mut targets = BTreeMap::<Value, usize>::new();
            for (_, target) in &edges {
                *targets.entry(target.clone()).or_default() += 1;
            }
            Ok(Value::Bool(targets.values().all(|count| *count <= 1)))
        }
        "rel_domain" => Ok(Value::Set(
            edges.iter().map(|(source, _)| source.clone()).collect(),
        )),
        "rel_range" => Ok(Value::Set(
            edges.iter().map(|(_, target)| target.clone()).collect(),
        )),
        "rel_acyclic" => {
            for node in edges
                .iter()
                .flat_map(|(source, target)| [source.clone(), target.clone()])
                .collect::<BTreeSet<_>>()
            {
                for (_, next) in edges.iter().filter(|(source, _)| source == &node) {
                    if as_bool(relation_reachable(
                        Value::Relation(edges.clone()),
                        next,
                        &node,
                    )?)? {
                        return Ok(Value::Bool(false));
                    }
                }
            }
            Ok(Value::Bool(true))
        }
        _ => Err(runtime_error(format!("unknown relation function '{name}'"))),
    }
}

fn qualified_type(name: &fsl_core::QualifiedName) -> Result<String, RuntimeError> {
    if name.namespace.is_some() {
        Err(runtime_error(
            "qualified type remained after kernel lowering",
        ))
    } else {
        Ok(name.name.clone())
    }
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}

fn checked_binary(
    left: Value,
    right: Value,
    operation: fn(i64, i64) -> Option<i64>,
    name: &str,
) -> Result<Value, RuntimeError> {
    operation(as_int(left)?, as_int(right)?)
        .map(Value::Int)
        .ok_or_else(|| runtime_error(format!("integer overflow in {name}")))
}

#[allow(clippy::needless_pass_by_value)]
fn as_bool(value: Value) -> Result<bool, RuntimeError> {
    match value {
        Value::Bool(value) => Ok(value),
        _ => Err(runtime_error("expected Boolean value")),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn as_int(value: Value) -> Result<i64, RuntimeError> {
    match value {
        Value::Int(value) => Ok(value),
        _ => Err(runtime_error("expected integer value")),
    }
}

fn as_usize(value: Value, message: &str) -> Result<usize, RuntimeError> {
    usize::try_from(as_int(value)?).map_err(|_| runtime_error(message))
}

fn i64_len(value: usize) -> Result<i64, RuntimeError> {
    i64::try_from(value).map_err(|_| runtime_error("collection size exceeds i64"))
}

fn runtime_error(message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        message: message.into(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnabledAction {
    pub action: String,
    pub params: BTreeMap<String, Value>,
    bindings: Bindings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct Violation {
    pub kind: String,
    pub name: String,
    pub step: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use = "a StepResult carries `violation`: dropping it discards the Monitor's verdict for this step"]
pub struct StepResult {
    pub action: String,
    pub params: BTreeMap<String, Value>,
    pub state: State,
    pub attempted_state: Option<State>,
    pub violation: Option<Violation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
// NOTE: inert on `BoundedLivenessMonitor::observe`, which returns
// `Result<Option<Self>, _>` -- `#[must_use]` does not propagate through
// `Option` and `Option` is not itself `#[must_use]` (issue #868). Annotated
// anyway: it is an outcome, and this catches any future bare return.
#[must_use]
pub struct BoundedLivenessViolation {
    pub property: String,
    pub bindings: Bindings,
    pub pending_since: usize,
    pub deadline: usize,
    pub within: usize,
    pub step: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedLivenessPending {
    pub property: String,
    pub bindings: Bindings,
    pub pending_since: usize,
    pub deadline: usize,
    pub within: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedLivenessStatus {
    pub checked_properties: Vec<String>,
    pub unbounded_properties: Vec<String>,
    pub pending: Vec<BoundedLivenessPending>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BoundedLivenessProperty {
    definition: fsl_core::LeadsToDef,
    within: usize,
    bindings: Vec<Bindings>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedLivenessMonitor {
    model: KernelModel,
    properties: Vec<BoundedLivenessProperty>,
    unbounded_properties: Vec<String>,
    pending: BTreeMap<(usize, Bindings), (usize, usize)>,
    next_step: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Monitor {
    pub model: KernelModel,
    pub state: State,
    step: usize,
}

impl Monitor {
    fn failed_step(
        &mut self,
        action: &str,
        params: &BTreeMap<String, Value>,
        kind: &str,
        attempted_state: Option<State>,
    ) -> StepResult {
        self.step += 1;
        StepResult {
            action: action.to_owned(),
            params: params.clone(),
            state: self.state.clone(),
            attempted_state,
            violation: Some(Violation {
                kind: kind.to_owned(),
                name: format!("_{kind}_{action}"),
                step: self.step,
            }),
        }
    }

    /// Build a monitor whose initial state is exactly `state`, without
    /// running `model`'s init at all.
    ///
    /// For a caller that already has a complete concrete initial state —
    /// an observed replay trace's own step 0, an explicit
    /// `--initial-state` snapshot, or a BMC witness's first state — there
    /// is nothing left for init to compute. `init` may legitimately leave
    /// some state free (a symbolic engine explores every admissible
    /// value), so building through [`Monitor::new`] here would wrongly
    /// demand a determinism [`Monitor::new`] does not need to provide
    /// (#519).
    #[must_use]
    pub fn from_state(model: KernelModel, state: State) -> Self {
        Self {
            model,
            state,
            step: 0,
        }
    }

    /// Initialize a solver-independent concrete monitor.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when init does not deterministically assign
    /// every state variable (component-wise; see
    /// `docs/DESIGN-bridge.md` "Determinism of init") or sequential init
    /// execution fails. A model whose init leaves some state free is
    /// admissible to `verify`/BMC, which explores every admissible value —
    /// concrete execution has no such freedom to explore, so construction
    /// fails closed instead of picking one arbitrary default value and
    /// silently treating it as the specification's initial state.
    pub fn new(model: KernelModel) -> Result<Self, RuntimeError> {
        explicit::check_deterministic_init(&model)?;
        let mut state = model
            .state
            .iter()
            .map(|(name, ty)| Ok((name.clone(), model.default_value(ty)?)))
            .collect::<Result<State, RuntimeError>>()?;
        let mut bindings = Bindings::new();
        let mut written = BTreeMap::new();
        for statement in &model.init {
            execute_init_statement(statement, &mut state, &mut bindings, &model, &mut written)?;
        }
        Ok(Self {
            model,
            state,
            step: 0,
        })
    }

    /// Enumerate every enabled bounded action instance in the current state.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when parameter domains or guards cannot be
    /// evaluated concretely.
    pub fn enabled(&self) -> Result<Vec<EnabledAction>, RuntimeError> {
        let mut enabled = Vec::new();
        for action in &self.model.actions {
            for params in action_parameter_bindings(action, &self.model)? {
                if let Some(bindings) =
                    evaluate_action_guards(action, &params, &self.state, &self.model)?
                {
                    enabled.push(EnabledAction {
                        action: action.name.clone(),
                        params,
                        bindings,
                    });
                }
            }
        }
        Ok(enabled)
    }

    /// Evaluate and execute one bounded action call, including disabled and
    /// partial guard outcomes. This is the concrete conformance entry point.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] for an unknown action, invalid parameters, or
    /// a non-partial evaluation failure.
    pub fn attempt(
        &mut self,
        action_name: &str,
        params: &BTreeMap<String, Value>,
    ) -> Result<StepResult, RuntimeError> {
        let action = self
            .model
            .actions
            .iter()
            .find(|action| action.name == action_name)
            .cloned()
            .ok_or_else(|| runtime_error(format!("unknown action '{action_name}'")))?;
        let bindings = match evaluate_action_guards(&action, params, &self.state, &self.model) {
            Ok(Some(bindings)) => bindings,
            Ok(None) => {
                return Ok(self.failed_step(action_name, params, "requires_failed", None));
            }
            Err(error) if is_partial_operation_error(&error.message) => {
                return Ok(self.failed_step(action_name, params, "partial_op", None));
            }
            Err(error) => return Err(error),
        };
        self.execute_selected(
            &EnabledAction {
                action: action_name.to_owned(),
                params: params.clone(),
                bindings,
            },
            None,
        )
    }

    /// Execute one previously enumerated enabled instance.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] for stale/unknown instances, update errors, or
    /// expression/type failures.
    pub fn step(&mut self, enabled: &EnabledAction) -> Result<StepResult, RuntimeError> {
        self.step_selected(enabled, None)
    }

    /// Execute one enabled instance while checking an optional selection of
    /// implicit state-bound properties.
    ///
    /// `None` checks every implicit bound. `Some` is used by the explicit
    /// verifier when `--property` or `--exclude-property` narrows verification.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] for stale/unknown instances, update errors, or
    /// expression/type failures.
    pub fn step_selected(
        &mut self,
        enabled: &EnabledAction,
        checked_bounds: Option<&BTreeSet<String>>,
    ) -> Result<StepResult, RuntimeError> {
        let action = self
            .model
            .actions
            .iter()
            .find(|action| action.name == enabled.action)
            .cloned()
            .ok_or_else(|| runtime_error(format!("unknown action '{}'", enabled.action)))?;
        let bindings = evaluate_action_guards(&action, &enabled.params, &self.state, &self.model)?
            .ok_or_else(|| runtime_error(format!("stale enabled action '{}'", enabled.action)))?;
        self.execute_selected(
            &EnabledAction {
                action: enabled.action.clone(),
                params: enabled.params.clone(),
                bindings,
            },
            checked_bounds,
        )
    }

    #[allow(clippy::too_many_lines)]
    fn execute_selected(
        &mut self,
        enabled: &EnabledAction,
        checked_bounds: Option<&BTreeSet<String>>,
    ) -> Result<StepResult, RuntimeError> {
        let action = self
            .model
            .actions
            .iter()
            .find(|action| action.name == enabled.action)
            .ok_or_else(|| runtime_error(format!("unknown action '{}'", enabled.action)))?;
        let old_state = self.state.clone();
        let mut pending = State::new();
        let mut writes = BTreeSet::new();
        let mut bindings = enabled.bindings.clone();
        for statement in &action.statements {
            if let Err(error) = compute_statement_update(
                statement,
                &old_state,
                &mut pending,
                &mut writes,
                &mut bindings,
                &self.model,
            ) {
                if is_partial_operation_error(&error.message) {
                    self.step += 1;
                    return Ok(StepResult {
                        action: enabled.action.clone(),
                        params: enabled.params.clone(),
                        state: old_state,
                        attempted_state: None,
                        violation: Some(Violation {
                            kind: "partial_op".to_owned(),
                            name: format!("_partial_{}", action.name),
                            step: self.step,
                        }),
                    });
                }
                return Err(error);
            }
        }
        let mut next = old_state.clone();
        next.extend(pending);
        self.step += 1;
        let violation = match check_state_selected(
            &next,
            Some(&old_state),
            &self.model,
            self.step,
            checked_bounds,
        ) {
            Ok(violation) => violation,
            Err(error) if is_partial_operation_error(&error.message) => {
                return Ok(StepResult {
                    action: enabled.action.clone(),
                    params: enabled.params.clone(),
                    state: old_state,
                    attempted_state: Some(next),
                    violation: Some(Violation {
                        kind: "partial_op".to_owned(),
                        name: format!("_partial_{}", action.name),
                        step: self.step,
                    }),
                });
            }
            Err(error) => return Err(error),
        };
        if let Some(violation) = violation {
            return Ok(StepResult {
                action: enabled.action.clone(),
                params: enabled.params.clone(),
                state: old_state,
                attempted_state: Some(next),
                violation: Some(violation),
            });
        }
        for ensure in &action.ensures {
            let evaluated = eval(ensure, &next, &mut bindings, &self.model, Some(&old_state));
            let value = match evaluated {
                Ok(value) => value,
                Err(error) if is_partial_operation_error(&error.message) => {
                    return Ok(StepResult {
                        action: enabled.action.clone(),
                        params: enabled.params.clone(),
                        state: old_state,
                        attempted_state: Some(next),
                        violation: Some(Violation {
                            kind: "partial_op".to_owned(),
                            name: format!("_partial_{}", action.name),
                            step: self.step,
                        }),
                    });
                }
                Err(error) => return Err(error),
            };
            if !as_bool(value)? {
                return Ok(StepResult {
                    action: enabled.action.clone(),
                    params: enabled.params.clone(),
                    state: old_state,
                    attempted_state: Some(next),
                    violation: Some(Violation {
                        kind: "ensures".to_owned(),
                        name: action.name.clone(),
                        step: self.step,
                    }),
                });
            }
        }
        self.state = next.clone();
        Ok(StepResult {
            action: enabled.action.clone(),
            params: enabled.params.clone(),
            state: next,
            attempted_state: None,
            violation: None,
        })
    }

    /// Check type bounds and declared invariants at the current state.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when a property cannot be evaluated.
    pub fn current_violation(&self) -> Result<Option<Violation>, RuntimeError> {
        check_state(&self.state, None, &self.model, self.step)
    }

    /// Check the current state while honoring an optional selection of
    /// implicit state-bound properties.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when a property cannot be evaluated.
    pub fn current_violation_selected(
        &self,
        checked_bounds: Option<&BTreeSet<String>>,
    ) -> Result<Option<Violation>, RuntimeError> {
        check_state_selected(&self.state, None, &self.model, self.step, checked_bounds)
    }
}

impl BoundedLivenessMonitor {
    /// Build a solver-free monitor for every `leadsTo ... within K` property.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] for negative deadlines, overflow, dynamic range
    /// bounds, `where` filters, or collection binders unsupported by the
    /// symbolic deadline checker.
    pub fn new(model: KernelModel) -> Result<Self, RuntimeError> {
        let mut properties = Vec::new();
        let mut unbounded_properties = Vec::new();
        for property in &model.leadstos {
            let Some(within) = property.within else {
                unbounded_properties.push(property.name.clone());
                continue;
            };
            let within = usize::try_from(within)
                .map_err(|_| runtime_error("leadsTo within must be non-negative"))?;
            properties.push(BoundedLivenessProperty {
                definition: property.clone(),
                within,
                bindings: static_leadsto_bindings(&model, property)?,
            });
        }
        Ok(Self {
            model,
            properties,
            unbounded_properties,
            pending: BTreeMap::new(),
            next_step: 0,
        })
    }

    /// Observe one consecutive logical trace state.
    ///
    /// `step` counts every action and stutter observation. A response is valid
    /// on its deadline state; failure is reported only when Q is still false.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] for non-consecutive steps or concrete expression
    /// evaluation failures.
    pub fn observe(
        &mut self,
        state: &State,
        step: usize,
    ) -> Result<Option<BoundedLivenessViolation>, RuntimeError> {
        with_total_division(|| self.observe_inner(state, step))
    }

    fn observe_inner(
        &mut self,
        state: &State,
        step: usize,
    ) -> Result<Option<BoundedLivenessViolation>, RuntimeError> {
        if step != self.next_step {
            return Err(runtime_error(format!(
                "bounded liveness expected step {}, got {step}",
                self.next_step
            )));
        }
        for (property_index, property) in self.properties.iter().enumerate() {
            for binding in &property.bindings {
                let key = (property_index, binding.clone());
                let mut after_binding = binding.clone();
                let after = as_bool(eval(
                    &property.definition.after,
                    state,
                    &mut after_binding,
                    &self.model,
                    None,
                )?)?;
                if after {
                    self.pending.remove(&key);
                    continue;
                }
                if let Some((pending_since, deadline)) = self.pending.get(&key).copied() {
                    if step >= deadline {
                        return Ok(Some(BoundedLivenessViolation {
                            property: property.definition.name.clone(),
                            bindings: binding.clone(),
                            pending_since,
                            deadline,
                            within: property.within,
                            step,
                        }));
                    }
                    continue;
                }
                let mut before_binding = binding.clone();
                let before = as_bool(eval(
                    &property.definition.before,
                    state,
                    &mut before_binding,
                    &self.model,
                    None,
                )?)?;
                if before {
                    let deadline = step
                        .checked_add(property.within)
                        .ok_or_else(|| runtime_error("bounded liveness deadline exceeds usize"))?;
                    self.pending.insert(key, (step, deadline));
                    if property.within == 0 {
                        return Ok(Some(BoundedLivenessViolation {
                            property: property.definition.name.clone(),
                            bindings: binding.clone(),
                            pending_since: step,
                            deadline: step,
                            within: 0,
                            step,
                        }));
                    }
                }
            }
        }
        self.next_step = step
            .checked_add(1)
            .ok_or_else(|| runtime_error("bounded liveness step exceeds usize"))?;
        Ok(None)
    }

    #[must_use]
    pub fn status(&self) -> BoundedLivenessStatus {
        let pending = self
            .pending
            .iter()
            .map(|((property_index, bindings), (pending_since, deadline))| {
                let property = &self.properties[*property_index];
                BoundedLivenessPending {
                    property: property.definition.name.clone(),
                    bindings: bindings.clone(),
                    pending_since: *pending_since,
                    deadline: *deadline,
                    within: property.within,
                }
            })
            .collect();
        BoundedLivenessStatus {
            checked_properties: self
                .properties
                .iter()
                .map(|property| property.definition.name.clone())
                .collect(),
            unbounded_properties: self.unbounded_properties.clone(),
            pending,
        }
    }
}

fn is_partial_operation_error(message: &str) -> bool {
    matches!(
        message,
        "pop() on empty sequence"
            | "head() on empty sequence"
            | "at() index out of range"
            | "sequence index out of range"
            | "division by zero"
            | "remainder by zero"
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct ReachableWitness {
    pub step: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct BfsResult {
    pub spec: String,
    pub depth: usize,
    pub states_explored: usize,
    pub violation: Option<Violation>,
    pub reachables: BTreeMap<String, Option<ReachableWitness>>,
    pub deadlock_step: Option<usize>,
    pub action_coverage: BTreeMap<String, bool>,
}

fn terminal_holds(monitor: &Monitor) -> Result<bool, RuntimeError> {
    let Some(terminal) = &monitor.model.terminal else {
        return Ok(false);
    };
    with_total_division(|| {
        match eval(
            terminal,
            &monitor.state,
            &mut Bindings::new(),
            &monitor.model,
            None,
        )? {
            Value::Bool(value) => Ok(value),
            _ => Err(runtime_error("terminal expression must be Boolean")),
        }
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefinementFailure {
    pub kind: String,
    pub at: Option<String>,
    pub step: usize,
    pub impl_action: Option<TraceAction>,
    pub alpha_before: Option<State>,
    pub alpha_after_expected: Option<State>,
    pub alpha_after_actual: Option<State>,
    pub impl_trace: Vec<TraceStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct RefinementCheck {
    pub implementation: String,
    pub abstraction: String,
    pub depth: usize,
    pub action_map: BTreeMap<String, String>,
    pub abs_has_ensures: bool,
    pub failure: Option<RefinementFailure>,
    /// Set instead of `failure` when the implementation violates its own
    /// semantics (a type bound, invariant, `trans`, `ensures`, or
    /// `partial_op`) within `depth`, independent of the refinement mapping.
    /// This is a property of the refinement *input* (the impl spec is
    /// broken on its own), not a refinement fidelity verdict, so it must
    /// never be reported as `refines` or folded into `refinement_failed`.
    pub impl_violation: Option<(Violation, Vec<TraceStep>)>,
}

fn merged_refinement_model(
    implementation: &KernelModel,
    abstraction: &KernelModel,
) -> Result<KernelModel, RuntimeError> {
    let mut merged = implementation.clone();
    for (name, definition) in &abstraction.types {
        if let Some(existing) = implementation.types.get(name) {
            let conflicts = matches!(
                (existing, definition),
                (TypeDef::Enum { .. }, TypeDef::Enum { .. })
                    | (TypeDef::Struct { .. }, TypeDef::Struct { .. })
            ) && existing != definition;
            if conflicts {
                return Err(runtime_error(format!(
                    "type '{name}' is declared differently in the impl and abs specs"
                )));
            }
        }
        merged.types.insert(name.clone(), definition.clone());
    }
    for (name, value) in &abstraction.enum_members {
        merged.enum_members.insert(name.clone(), value.clone());
    }
    Ok(merged)
}

fn alpha_state(
    implementation_state: &State,
    implementation: &KernelModel,
    abstraction: &KernelModel,
    mapping: &Refinement,
    eval_model: &KernelModel,
) -> Result<State, RuntimeError> {
    with_total_division(|| {
        alpha_state_inner(
            implementation_state,
            implementation,
            abstraction,
            mapping,
            eval_model,
        )
    })
}

fn alpha_state_inner(
    implementation_state: &State,
    implementation: &KernelModel,
    abstraction: &KernelModel,
    mapping: &Refinement,
    eval_model: &KernelModel,
) -> Result<State, RuntimeError> {
    let mut alpha = State::new();
    for (name, abs_ty) in &abstraction.state {
        let state_map = mapping
            .state_maps
            .get(name)
            .ok_or_else(|| runtime_error(format!("missing map for '{name}'")))?;
        let value = if let Some(binder) = &state_map.binder {
            let key_ty = match abs_ty {
                TypeRef::Map(key, _) => key.as_ref(),
                _ => {
                    return Err(runtime_error(format!(
                        "indexed map for '{name}' requires abstract Map state"
                    )));
                }
            };
            let mut entries = BTreeMap::new();
            for key in abstraction.map_key_values(key_ty)? {
                let mut bindings = Bindings::new();
                bindings.insert(binder_name(binder).to_owned(), key.clone());
                let mapped = eval(
                    &state_map.expr,
                    implementation_state,
                    &mut bindings,
                    eval_model,
                    None,
                )?;
                entries.insert(key, mapped);
            }
            Value::Map(entries)
        } else {
            eval(
                &state_map.expr,
                implementation_state,
                &mut Bindings::new(),
                eval_model,
                None,
            )?
        };
        alpha.insert(name.clone(), value);
    }
    let _ = implementation;
    Ok(alpha)
}

fn refinement_failure(
    kind: &str,
    at: Option<&str>,
    step: usize,
    trace: &[TraceStep],
    before: Option<State>,
    expected: Option<State>,
    actual: Option<State>,
) -> RefinementFailure {
    RefinementFailure {
        kind: kind.to_owned(),
        at: at.map(str::to_owned),
        step,
        impl_action: trace.last().and_then(|entry| entry.action.clone()),
        alpha_before: before,
        alpha_after_expected: expected,
        alpha_after_actual: actual,
        impl_trace: trace.to_vec(),
    }
}

fn refinement_action_instance(
    monitor: &Monitor,
    action: &ActionDef,
    params: BTreeMap<String, Value>,
) -> Result<Option<EnabledAction>, RuntimeError> {
    let mut bindings = params.clone();
    for guard in &action.guards {
        match guard {
            ActionGuard::Let(name, expr) => {
                let value = eval(expr, &monitor.state, &mut bindings, &monitor.model, None)?;
                bindings.insert(name.clone(), value);
            }
            ActionGuard::Requires(expr) => {
                if !as_bool(eval(
                    expr,
                    &monitor.state,
                    &mut bindings,
                    &monitor.model,
                    None,
                )?)? {
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(EnabledAction {
        action: action.name.clone(),
        params,
        bindings,
    }))
}

fn abstract_action_state(
    state: &State,
    abstraction: &KernelModel,
    action: &ActionDef,
    params: &BTreeMap<String, Value>,
) -> Result<State, RuntimeError> {
    let mut extended = state.clone();
    for (state_name, state_ty) in &abstraction.state {
        let TypeRef::Map(key_ty, value_ty) = state_ty else {
            continue;
        };
        let Some(Value::Map(entries)) = extended.get_mut(state_name) else {
            continue;
        };
        for param in &action.params {
            let param_ty = match param {
                ParamDef::Typed { ty, .. } => ty,
                ParamDef::Range { .. } => continue,
            };
            if param_ty == key_ty.as_ref() {
                let value = params[param.name()].clone();
                if let std::collections::btree_map::Entry::Vacant(entry) = entries.entry(value) {
                    entry.insert(abstraction.default_value(value_ty)?);
                }
            }
        }
    }
    Ok(extended)
}

fn project_abstract_state(state: &State, abstraction: &KernelModel) -> Result<State, RuntimeError> {
    let mut projected = state.clone();
    for (state_name, state_ty) in &abstraction.state {
        let TypeRef::Map(key_ty, _) = state_ty else {
            continue;
        };
        let Some(Value::Map(entries)) = projected.get_mut(state_name) else {
            continue;
        };
        let keys = abstraction
            .map_key_values(key_ty)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        entries.retain(|key, _| keys.contains(key));
    }
    Ok(projected)
}

/// Every concrete initial state consistent with `model`'s init.
///
/// A model whose init assigns every state variable on every path returns
/// exactly the single state [`Monitor::new`] would build — no behavior
/// change for the common deterministic case. A model with a state variable
/// init never assigns on any path (nondeterministic init, DESIGN-init-if.md)
/// is domain-enumerated instead: refinement's step-0 self-consistency
/// precondition and init correspondence must reason about every reachable
/// initial valuation, not one arbitrarily materialized default (issue
/// #493). This does not touch [`Monitor::new`] itself or any other caller —
/// the general Monitor-construction gate for partial/nondeterministic init
/// is a different surface (issue #519).
///
/// # Errors
///
/// Returns [`RuntimeError`] when a free variable's type is not a finite
/// scalar domain, or a combination cannot be evaluated concretely.
fn concrete_initial_states(model: &KernelModel) -> Result<Vec<State>, RuntimeError> {
    let free = explicit::unassigned_init_state_vars(model);
    if free.is_empty() {
        return Ok(vec![Monitor::new(model.clone())?.state]);
    }
    let mut combinations: Vec<BTreeMap<String, Value>> = vec![BTreeMap::new()];
    for (name, ty) in &free {
        let domain = model.domain_values(ty)?;
        combinations = combinations
            .into_iter()
            .flat_map(|combination| {
                domain.iter().map(move |value| {
                    let mut next = combination.clone();
                    next.insert(name.clone(), value.clone());
                    next
                })
            })
            .collect();
    }
    let mut states = BTreeSet::new();
    for combination in combinations {
        let mut state = model
            .state
            .iter()
            .map(|(name, ty)| Ok((name.clone(), model.default_value(ty)?)))
            .collect::<Result<State, RuntimeError>>()?;
        state.extend(combination);
        let mut bindings = Bindings::new();
        let mut written = BTreeMap::new();
        for statement in &model.init {
            execute_init_statement(statement, &mut state, &mut bindings, model, &mut written)?;
        }
        states.insert(state);
    }
    Ok(states.into_iter().collect())
}

/// Exhaustively check bounded concrete refinement simulation.
///
/// The checker is solver-independent and evaluates every reachable bounded
/// implementation transition against its mapped abstract action or stutter.
///
/// # Errors
///
/// Returns [`RuntimeError`] for mapping evaluation, incompatible shared types,
/// or concrete Monitor failures.
#[allow(clippy::too_many_lines)]
pub fn check_refinement(
    implementation: &KernelModel,
    abstraction: &KernelModel,
    mapping: &Refinement,
    depth: usize,
) -> Result<RefinementCheck, RuntimeError> {
    // The impl spec must be internally consistent before its transitions are
    // compared against the abstraction at all: refinement fidelity is
    // meaningless to evaluate for a spec that already breaks its own type
    // bounds or invariants. Checking this first — and returning immediately
    // — means the correspondence walk below never needs to decide what a
    // mid-walk self-violation means; by construction it cannot encounter
    // one within the same `depth`. `impl_initial_states` covers every
    // concrete initial valuation a nondeterministic impl `init` permits
    // (issue #493), not one arbitrarily materialized default, so this
    // precondition cannot miss a self-violation reachable only from a
    // non-default initial branch either.
    let impl_initial_states = concrete_initial_states(implementation)?;
    if let Some((violation, trace)) =
        first_self_violation(implementation, &impl_initial_states, depth)?
    {
        return Ok(RefinementCheck {
            implementation: implementation.name.clone(),
            abstraction: abstraction.name.clone(),
            depth,
            action_map: BTreeMap::new(),
            abs_has_ensures: false,
            failure: None,
            impl_violation: Some((violation, trace)),
        });
    }
    let eval_model = merged_refinement_model(implementation, abstraction)?;
    // The set of every concrete state abs's own (possibly nondeterministic)
    // init permits — init correspondence below asks whether α(s₀) is a
    // *member* of this set, not whether it equals one materialized default
    // abs initial state.
    let abs_initial_states: BTreeSet<State> =
        concrete_initial_states(abstraction)?.into_iter().collect();
    let action_map = mapping
        .action_correspondences
        .iter()
        .map(|(name, mapping)| {
            let target = match &mapping.target {
                ActionCorrespondenceTarget::Stutter => "stutter".to_owned(),
                ActionCorrespondenceTarget::Action { action, .. } => action.0.clone(),
            };
            (name.clone(), target)
        })
        .collect();
    let mut check = RefinementCheck {
        implementation: implementation.name.clone(),
        abstraction: abstraction.name.clone(),
        depth,
        action_map,
        abs_has_ensures: abstraction
            .actions
            .iter()
            .any(|action| !action.ensures.is_empty()),
        failure: None,
        impl_violation: None,
    };

    // §2 step 1 (init correspondence): for *every* impl initial valuation
    // s₀ (plural — nondeterministic impl init has more than one), α(s₀)
    // must satisfy the abs init constraints. A candidate that fails seeds
    // no BFS root and is reported immediately (deterministic order: the
    // states are visited in `impl_initial_states`'s sorted order, so the
    // failure reported is stable). Every candidate that passes seeds its
    // own root below, so the walk explores the full reachable set of every
    // nondeterministic initial branch, not just one.
    let mut queue = trace::LeanFrontier::new();
    for impl_state in impl_initial_states {
        let alpha_initial = alpha_state(
            &impl_state,
            implementation,
            abstraction,
            mapping,
            &eval_model,
        )?;
        let initial_trace = vec![TraceStep {
            step: 0,
            state: impl_state.clone(),
            action: None,
            changes: BTreeMap::new(),
        }];
        // `alpha_initial` is already a complete concrete abs state (the
        // impl initial state mapped through the refinement correspondence),
        // so there is nothing for the abstraction's own `init` to compute —
        // `Monitor::from_state` skips it entirely rather than demanding a
        // determinism `abstraction.init` may not have (issue #519's gate
        // interacting with #493's nondeterministic-abs-init support: a
        // nondeterministic abs init is intentionally never fully
        // deterministic, so `Monitor::new(abstraction...)` fails closed here
        // even for a genuinely correct refinement).
        let initial_alpha_monitor = Monitor::from_state(abstraction.clone(), alpha_initial.clone());
        if let Some(violation) = initial_alpha_monitor.current_violation()? {
            let kind = if violation.kind == "type_bound" {
                "map_out_of_bounds"
            } else {
                "abs_state_mismatch"
            };
            check.failure = Some(refinement_failure(
                kind,
                Some("init"),
                0,
                &initial_trace,
                None,
                None,
                Some(alpha_initial),
            ));
            return Ok(check);
        }
        if !abs_initial_states.contains(&alpha_initial) {
            check.failure = Some(refinement_failure(
                "abs_state_mismatch",
                Some("init"),
                0,
                &initial_trace,
                None,
                abs_initial_states.iter().next().cloned(),
                Some(alpha_initial),
            ));
            return Ok(check);
        }
        queue.push(impl_state, 0);
    }

    // The frontier carries `State` only, not `Monitor` -- a full `Monitor`
    // clone per candidate transition duplicates the whole `KernelModel`
    // repeatedly at every layer (issue #783's 1.72 GB @ depth 3). `scratch`
    // is re-pointed at each state instead, following `first_self_violation`
    // and `bfs`'s established pattern. Traces are no longer carried in the
    // queue either; `parents` -- left empty for every root, per
    // `trace::reconstruct_trace`'s multi-root contract -- reconstructs one
    // only when a failure is about to be reported.
    let mut visited = BTreeSet::new();
    let mut parents = BTreeMap::<State, trace::ParentLink>::new();
    let mut scratch = Monitor::from_state(implementation.clone(), State::new());
    while let Some(step) = queue.front_step() {
        let mut layer = Vec::new();
        while queue.front_step() == Some(step) {
            let Some((state, _)) = queue.pop() else {
                unreachable!("queue front was present");
            };
            if visited.insert(state.clone()) && step < depth {
                layer.push(state);
            }
        }
        // `alpha_before` for each layer state is computed once here, in
        // layer order, and looked up by index below -- not recomputed per
        // candidate -- to keep both the values and their computation order
        // identical to the pre-#783 per-node walk.
        let mut alphas = Vec::with_capacity(layer.len());
        for state in &layer {
            alphas.push(alpha_state(
                state,
                implementation,
                abstraction,
                mapping,
                &eval_model,
            )?);
        }
        let mut candidates = Vec::new();
        for (state_index, state) in layer.iter().enumerate() {
            scratch.state = state.clone();
            scratch.step = step;
            for enabled in scratch.enabled()? {
                let action_index = implementation
                    .actions
                    .iter()
                    .position(|action| action.name == enabled.action)
                    .unwrap_or(usize::MAX);
                candidates.push((action_index, enabled.params.clone(), state_index, enabled));
            }
        }
        candidates.sort_by(|left, right| {
            (&left.0, &left.1, &left.2).cmp(&(&right.0, &right.1, &right.2))
        });
        for (_, _, state_index, enabled) in candidates {
            let state = &layer[state_index];
            let alpha_before = &alphas[state_index];
            scratch.state = state.clone();
            scratch.step = step;
            let stepped = scratch.step(&enabled)?;
            if stepped.violation.is_some() {
                // Unreachable in practice: `first_self_violation` above
                // already proved the impl has no self-violation within
                // `depth`, and this walk never explores past `depth`. Kept
                // as a defensive skip (never a silent `refines`) rather than
                // an `unreachable!()`, since only silence here — not a
                // panic — would resurrect the false-green #466 fixes.
                continue;
            }
            let child_state = stepped.state.clone();
            let alpha_after = alpha_state(
                &child_state,
                implementation,
                abstraction,
                mapping,
                &eval_model,
            )?;
            let action_map = &mapping.action_correspondences[&enabled.action];
            match &action_map.target {
                ActionCorrespondenceTarget::Stutter => {
                    if alpha_before != &alpha_after {
                        let child_trace =
                            refinement_child_trace(state, &parents, step + 1, &enabled, &stepped);
                        check.failure = Some(refinement_failure(
                            "stutter_changed_abs",
                            Some("step"),
                            step + 1,
                            &child_trace,
                            Some(alpha_before.clone()),
                            Some(alpha_before.clone()),
                            Some(alpha_after),
                        ));
                        return Ok(check);
                    }
                }
                ActionCorrespondenceTarget::Action { action, args } => {
                    let name = &action.0;
                    let abs_action = abstraction
                        .actions
                        .iter()
                        .find(|action| action.name == *name)
                        .ok_or_else(|| {
                            runtime_error(format!("unknown abstract action '{name}'"))
                        })?;
                    let mut bindings = enabled.params.clone();
                    let values = match args
                        .iter()
                        .map(|expr| eval(expr, state, &mut bindings, &eval_model, None))
                        .collect::<Result<Vec<_>, _>>()
                    {
                        Ok(values) => values,
                        // An action-correspondence argument expression (not
                        // the impl action's own body -- that self-violation
                        // is already excluded above) hit an undefined
                        // operation for this reachable impl instance, e.g. a
                        // `/`/`%` divisor that is zero only through the
                        // mapping's argument expression. `docs/DESIGN-divmod.md`
                        // §2.2's action-context partial_op check applies here
                        // by the same G5 rationale (constructing an abstract
                        // action call is action context, not the read-only
                        // "mapping expression" §2.3 exempts): this must be a
                        // located refinement finding, not an unclassified
                        // internal error that the CLI defaults to `kind:"type"`.
                        Err(error) if is_partial_operation_error(&error.message) => {
                            let child_trace = refinement_child_trace(
                                state,
                                &parents,
                                step + 1,
                                &enabled,
                                &stepped,
                            );
                            check.failure = Some(refinement_failure(
                                "map_partial_op",
                                Some("step"),
                                step + 1,
                                &child_trace,
                                Some(alpha_before.clone()),
                                Some(alpha_after.clone()),
                                Some(alpha_after),
                            ));
                            return Ok(check);
                        }
                        Err(error) => return Err(error),
                    };
                    let expected_params = abs_action
                        .params
                        .iter()
                        .zip(values)
                        .map(|(param, value)| (param.name().to_owned(), value))
                        .collect::<BTreeMap<_, _>>();
                    // See the step-0 `Monitor::from_state` note above: this
                    // state is already fully computed, so there is nothing
                    // for `abstraction.init` to determine here either.
                    let abs_state = abstract_action_state(
                        alpha_before,
                        abstraction,
                        abs_action,
                        &expected_params,
                    )?;
                    let mut abs_monitor = Monitor::from_state(abstraction.clone(), abs_state);
                    let Some(abs_enabled) =
                        refinement_action_instance(&abs_monitor, abs_action, expected_params)?
                    else {
                        let child_trace =
                            refinement_child_trace(state, &parents, step + 1, &enabled, &stepped);
                        check.failure = Some(refinement_failure(
                            "abs_requires_failed",
                            Some("step"),
                            step + 1,
                            &child_trace,
                            Some(alpha_before.clone()),
                            Some(alpha_after.clone()),
                            Some(alpha_after),
                        ));
                        return Ok(check);
                    };
                    let abs_step = abs_monitor.step(&abs_enabled)?;
                    let expected_state = project_abstract_state(&abs_step.state, abstraction)?;
                    if expected_state != alpha_after {
                        let child_trace =
                            refinement_child_trace(state, &parents, step + 1, &enabled, &stepped);
                        check.failure = Some(refinement_failure(
                            "abs_state_mismatch",
                            Some("step"),
                            step + 1,
                            &child_trace,
                            Some(alpha_before.clone()),
                            Some(expected_state),
                            Some(alpha_after),
                        ));
                        return Ok(check);
                    }
                }
            }
            let alpha_monitor = Monitor::from_state(abstraction.clone(), alpha_after.clone());
            if let Some(violation) = alpha_monitor.current_violation()? {
                let kind = if violation.kind == "type_bound" {
                    "map_out_of_bounds"
                } else {
                    "abs_state_mismatch"
                };
                let child_trace =
                    refinement_child_trace(state, &parents, step + 1, &enabled, &stepped);
                check.failure = Some(refinement_failure(
                    kind,
                    Some("step"),
                    step + 1,
                    &child_trace,
                    Some(alpha_before.clone()),
                    None,
                    Some(alpha_after),
                ));
                return Ok(check);
            }
            if !visited.contains(&child_state) {
                parents
                    .entry(child_state.clone())
                    .or_insert_with(|| trace::ParentLink {
                        parent: state.clone(),
                        action: TraceAction {
                            name: enabled.action.clone(),
                            params: enabled.params.clone(),
                        },
                    });
                queue.push(child_state, step + 1);
            }
        }
    }
    Ok(check)
}

/// The replayable trace ending at the step just taken from `state`, built by
/// walking `parents` back to the walk's root and appending that one step --
/// reconstructed only when a refinement failure is about to be reported,
/// mirroring `find_boundary_violation`'s and `first_self_violation`'s
/// on-demand trace construction (issue #783) instead of carrying a growing
/// `Vec<TraceStep>` clone in every queued frontier node.
fn refinement_child_trace(
    state: &State,
    parents: &BTreeMap<State, trace::ParentLink>,
    next_step: usize,
    enabled: &EnabledAction,
    stepped: &StepResult,
) -> Vec<TraceStep> {
    let mut child_trace = trace::reconstruct_trace(state, parents);
    child_trace.push(trace_step_from_result(next_step, state, enabled, stepped));
    child_trace
}

/// Exhaustively explore concrete reachable states to a bounded depth.
///
/// This is the independent oracle, not the future symbolic BMC engine.
///
/// # Errors
///
/// Returns [`RuntimeError`] if concrete evaluation or execution fails.
pub fn bfs(model: KernelModel, depth: usize) -> Result<BfsResult, RuntimeError> {
    // The queue carries `State` only, not `Monitor` -- a full `Monitor`
    // clone per explored state duplicates the whole `KernelModel` on every
    // node (issue #730). `scratch` is re-pointed at each popped state
    // instead; `BfsResult` never holds a trace, so there is no parent-link
    // bookkeeping to do here (contrast `find_boundary_violation` and
    // `first_self_violation`, which must reconstruct a trace on violation).
    let mut scratch = Monitor::new(model)?;
    let initial_state = scratch.state.clone();
    let mut result = BfsResult {
        spec: scratch.model.name.clone(),
        depth,
        states_explored: 0,
        violation: scratch.current_violation()?,
        reachables: scratch
            .model
            .reachables
            .iter()
            .map(|property| (property.name.clone(), None))
            .collect(),
        deadlock_step: None,
        action_coverage: scratch
            .model
            .actions
            .iter()
            .map(|action| (action.name.clone(), false))
            .collect(),
    };
    if let Some(violation) = record_reachables(&scratch, 0, &mut result)? {
        result.violation = Some(violation);
    }
    let mut queue = VecDeque::from([(initial_state.clone(), 0_usize)]);
    let mut visited = BTreeSet::from([initial_state]);
    while let Some((state, step)) = queue.pop_front() {
        result.states_explored += 1;
        scratch.state = state.clone();
        scratch.step = step;
        let enabled = scratch.enabled()?;
        if enabled.is_empty() {
            let terminal = match terminal_holds(&scratch) {
                Ok(value) => value,
                Err(error) if is_partial_operation_error(&error.message) => {
                    let violation = Violation {
                        kind: "partial_op".to_owned(),
                        name: "_partial_property_terminal".to_owned(),
                        step,
                    };
                    if result
                        .violation
                        .as_ref()
                        .is_none_or(|old| violation.step < old.step)
                    {
                        result.violation = Some(violation);
                    }
                    return Ok(result);
                }
                Err(error) => return Err(error),
            };
            if !terminal {
                result.deadlock_step = Some(result.deadlock_step.map_or(step, |old| old.min(step)));
            }
        }
        for instance in &enabled {
            result.action_coverage.insert(instance.action.clone(), true);
        }
        if step >= depth {
            continue;
        }
        for instance in &enabled {
            scratch.state = state.clone();
            scratch.step = step;
            let stepped = scratch.step(instance)?;
            if let Some(violation) = stepped.violation {
                if result
                    .violation
                    .as_ref()
                    .is_none_or(|old| violation.step < old.step)
                {
                    result.violation = Some(violation);
                }
                continue;
            }
            if let Some(violation) = record_reachables(&scratch, step + 1, &mut result)? {
                if result
                    .violation
                    .as_ref()
                    .is_none_or(|old| violation.step < old.step)
                {
                    result.violation = Some(violation);
                }
                continue;
            }
            let child_state = scratch.state.clone();
            if visited.insert(child_state.clone()) {
                queue.push_back((child_state, step + 1));
            }
        }
    }
    Ok(result)
}

/// The default budget for [`find_boundary_violation`]'s state count: the
/// number of distinct concrete states it will visit before giving up and
/// reporting [`BoundaryProbe::exhausted`] instead of continuing to grow
/// without limit (issue #697).
///
/// Calibrated, not guessed: a full sweep of `specs/` + `examples/` at their
/// default `--depth 8` (169 files with a deterministic initial state; every
/// other file is excluded by [`deterministic_initial_state`] before this
/// budget is ever consulted) found a maximum `states_explored` of 23,409
/// (`examples/named_predicate.fsl`), with every file reaching normal BFS
/// closure -- none came close to exhausting even a 500,000-state ceiling,
/// and the second-highest file needed only 16,182. 50,000 keeps a >=2.1x
/// margin over the observed maximum (and >=3x over every other corpus
/// file) while keeping the pathological case this budget exists for -- a
/// history-recording `Seq` that defeats BFS dedup, whose true reachable
/// closure is far larger than any budget this size would ever cover -- to a
/// low-single-digit-GB peak RSS even in an unoptimized debug build, instead
/// of the unbounded growth issue #697 reported past 11.4 GB. A larger
/// budget (e.g. 100,000) was measured and rejected: it left materially less
/// headroom before an unoptimized debug build's peak RSS reached the same
/// order of magnitude as the original failure. See
/// `docs/DESIGN-kernel-contract.md` "Concrete boundary pre-pass budget" for
/// the full measurement.
///
/// The value is bracketed from both sides by measurement, not chosen by feel:
/// below by the corpus (167 specs at depth 8; the largest pre-pass explored
/// 23,409 states, in `examples/named_predicate.fsl`, so 50,000 leaves ~2.1x of
/// headroom) and above by debug-build peak RSS. Because 2.1x is thin, the
/// property is protected by an executable control rather than by the margin:
/// `rust/fslc/tests/issue_697_corpus_probe_budget.rs` fails loudly if any
/// corpus spec would exhaust this budget and therefore lose its concrete
/// evidence. Raise the constant only together with that test's recorded
/// maximum.
pub const CONCRETE_PROBE_BUDGET: usize = 50_000;

/// The outcome of a budgeted [`find_boundary_violation`] search.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct BoundaryProbe {
    /// The first concrete `partial_op`/`type_bound` violation found, with its
    /// full replayable trace from the initial state.
    pub finding: Option<(Violation, Vec<TraceStep>)>,
    /// Whether the search stopped because it reached `budget` distinct
    /// states rather than because it exhausted every state reachable within
    /// `depth`. A caller must treat `exhausted && finding.is_none()`
    /// identically to a normal empty result: this probe is an evidence
    /// detour, not a verdict authority, so an inconclusive budgeted search
    /// falls through to the symbolic engine exactly as a completed empty
    /// search does.
    pub exhausted: bool,
    /// The number of distinct states visited, for diagnostics and for the
    /// corpus-conservation check that calibrates `CONCRETE_PROBE_BUDGET`.
    pub states_explored: usize,
}

/// Find the first concrete partial-operation or type-bound violation and its
/// trace, visiting at most `budget` distinct states.
///
/// This search is an evidence detour, not a verdict authority: it only ever
/// returns `partial_op`/`type_bound` findings, and a caller that gets
/// `exhausted: true` with no finding must fall through to the symbolic
/// engine, which finds every symbolically representable violation on its
/// own. The one outcome class this search uniquely covers -- a reachable
/// over-capacity `Seq` successor the bounded symbolic value cannot represent
/// -- still fails closed downstream (`rust/fsl-verifier/src/value.rs`'s
/// "model sequence length exceeds capacity") rather than passing, so
/// exhaustion never silently downgrades a real violation to a false green
/// (issue #697).
///
/// # Errors
///
/// Returns [`RuntimeError`] when concrete action evaluation fails for another reason.
pub fn find_boundary_violation(
    model: &KernelModel,
    depth: usize,
    budget: usize,
) -> Result<BoundaryProbe, RuntimeError> {
    let mut scratch = Monitor::new(model.clone())?;
    let initial_state = scratch.state.clone();
    let mut queue = VecDeque::from([(initial_state.clone(), 0_usize)]);
    let mut visited = BTreeSet::from([initial_state.clone()]);
    let mut parents = BTreeMap::<State, trace::ParentLink>::new();

    while let Some((state, step)) = queue.pop_front() {
        if step >= depth {
            continue;
        }
        scratch.state = state.clone();
        scratch.step = step;
        for instance in scratch.enabled()? {
            scratch.state = state.clone();
            scratch.step = step;
            let stepped = scratch.step(&instance)?;
            if let Some(violation) = stepped.violation.clone() {
                if matches!(violation.kind.as_str(), "partial_op" | "type_bound") {
                    let mut found_trace = trace::reconstruct_trace(&state, &parents);
                    found_trace.push(trace_step_from_result(
                        step + 1,
                        &state,
                        &instance,
                        &stepped,
                    ));
                    return Ok(BoundaryProbe {
                        finding: Some((violation, found_trace)),
                        exhausted: false,
                        states_explored: visited.len(),
                    });
                }
                continue;
            }
            let child_state = stepped.state.clone();
            if visited.insert(child_state.clone()) {
                parents.insert(
                    child_state.clone(),
                    trace::ParentLink {
                        parent: state.clone(),
                        action: TraceAction {
                            name: instance.action.clone(),
                            params: instance.params.clone(),
                        },
                    },
                );
                if visited.len() >= budget {
                    return Ok(BoundaryProbe {
                        finding: None,
                        exhausted: true,
                        states_explored: visited.len(),
                    });
                }
                queue.push_back((child_state, step + 1));
            }
        }
    }
    Ok(BoundaryProbe {
        finding: None,
        exhausted: false,
        states_explored: visited.len(),
    })
}

/// Find the first violation of ANY kind (type bound, user invariant, `trans`,
/// `ensures`, or `partial_op`) the model has against its own semantics,
/// concretely, within `depth` — i.e. whether the model is internally
/// consistent at all, independent of any refinement mapping.
///
/// `initial_states` is every concrete initial valuation
/// [`concrete_initial_states`] found for `model` — plural, because a
/// nondeterministic init has more than one, and a self-violation reachable
/// only from a non-default initial branch must not be missed (issue #493).
/// They are checked for an immediate violation in order first (stable,
/// deterministic reporting), then explored together as one BFS (a state
/// reachable from more than one root is only visited once).
///
/// Unlike [`find_boundary_violation`], which is scoped to
/// `partial_op`/`type_bound` for its own narrower callers, this checks every
/// violation kind `Monitor::current_violation`/`Monitor::step` can report,
/// including a violation already present in the initial state (init can
/// itself violate an invariant or type bound before any action runs).
///
/// # Errors
///
/// Returns [`RuntimeError`] when concrete evaluation or execution fails.
fn first_self_violation(
    model: &KernelModel,
    initial_states: &[State],
    depth: usize,
) -> Result<Option<(Violation, Vec<TraceStep>)>, RuntimeError> {
    // Queue nodes carry `State` only; a scratch `Monitor` is re-pointed at
    // each popped state instead of cloning the whole model per node, and the
    // trace is reconstructed from parent links only when a violation is
    // actually found instead of cloning the whole `Vec<TraceStep>` so far at
    // every node (issue #730 -- worse than `find_boundary_violation` was
    // before #697, since this cloned both the model *and* the trace).
    //
    // Multiple roots (issue #493: a nondeterministic init has more than one
    // concrete initial state) are handled by leaving every root out of
    // `parents` and letting `trace::reconstruct_trace` discover whichever
    // root a given state actually descends from -- see its doc comment.
    let mut scratch = Monitor::from_state(model.clone(), State::new());
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    let mut parents = BTreeMap::<State, trace::ParentLink>::new();
    for state in initial_states {
        scratch.state = state.clone();
        scratch.step = 0;
        if let Some(violation) = scratch.current_violation()? {
            return Ok(Some((violation, trace::reconstruct_trace(state, &parents))));
        }
        if visited.insert(state.clone()) {
            queue.push_back((state.clone(), 0_usize));
        }
    }
    while let Some((state, step)) = queue.pop_front() {
        if step >= depth {
            continue;
        }
        scratch.state = state.clone();
        scratch.step = step;
        for instance in scratch.enabled()? {
            scratch.state = state.clone();
            scratch.step = step;
            let stepped = scratch.step(&instance)?;
            if let Some(violation) = stepped.violation.clone() {
                let mut found_trace = trace::reconstruct_trace(&state, &parents);
                found_trace.push(trace_step_from_result(
                    step + 1,
                    &state,
                    &instance,
                    &stepped,
                ));
                return Ok(Some((violation, found_trace)));
            }
            let child_state = scratch.state.clone();
            if visited.insert(child_state.clone()) {
                parents.insert(
                    child_state.clone(),
                    trace::ParentLink {
                        parent: state.clone(),
                        action: TraceAction {
                            name: instance.action.clone(),
                            params: instance.params.clone(),
                        },
                    },
                );
                queue.push_back((child_state, step + 1));
            }
        }
    }
    Ok(None)
}

/// The outcome of a budgeted vacuity-reachability probe for one
/// antecedent/trigger expression (issue #729).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reachability {
    /// The expression became true in some concretely reached state within
    /// `depth`.
    Reachable,
    /// The BFS enumerated every state reachable within `depth` (or
    /// exhausted the frontier before `depth`) without the expression ever
    /// becoming true — a completed, not merely truncated, empty search.
    /// This is the verdict the former `expression_reachable` reported as a
    /// plain `false`, and it keeps meaning the same thing: `--vacuity
    /// error`'s `vacuous_implication`/`vacuous_leadsto` findings stay keyed
    /// on this variant, unchanged by this issue's budget.
    Unreachable,
    /// The BFS stopped, or a per-candidate evaluation failed, before either
    /// finding the expression true or exhausting the reachable state space
    /// within `depth` -- reachability was never decided. Reached two ways:
    /// the shared budget was hit while this candidate was still pending, or
    /// evaluating this candidate's expression in some visited state
    /// returned an error (rare on a checked model). Fail-closed by
    /// construction: a caller must not fold this into `Unreachable` for
    /// either reason — treating it as "confirmed vacuous" would be a false
    /// positive, and treating it as "confirmed not vacuous" (silently
    /// dropping it) would let `--vacuity error` pass a spec whose vacuity
    /// was never actually established. The two causes are deliberately not
    /// distinguished in this type: both mean the same thing to a caller
    /// ("no verdict"), and a per-candidate evaluation error and a shared
    /// state-budget cutoff are two ways of reaching the identical
    /// obligation, not two different obligations.
    Exhausted,
}

/// Evaluate the reachability of every expression in `expressions` with one
/// shared, budgeted BFS over `model`'s concrete state space (issue #729).
///
/// Sharing one BFS across every candidate removes the per-antecedent/
/// per-trigger multiplier the former per-expression `expression_reachable`
/// paid (a full BFS per property): each candidate still pending is
/// evaluated against every popped state, drops out of the pending set the
/// instant it is found true, and the walk stops early once every candidate
/// has resolved. `budget` bounds the number of distinct concrete states
/// visited exactly like [`find_boundary_violation`]'s budget (state count
/// checked right after insertion, before the state is queued): once the
/// pending set is non-empty and the budget is reached, every still-pending
/// candidate resolves [`Reachability::Exhausted`].
///
/// A per-expression evaluation error (rare on a checked model) resolves
/// *only that expression* as [`Reachability::Exhausted`] rather than
/// aborting the whole call: batching must not let one malformed candidate
/// suppress every other candidate's vacuity evidence in the same run. This
/// is a deliberate behavior change from the original `expression_reachable`,
/// where a `Result::Err` for one property's antecedent produced no warning
/// at all for that property (silently treated as "not vacuous"); resolving
/// it `Exhausted` instead keeps every no-verdict outcome on the same
/// fail-closed path regardless of *why* a verdict could not be reached, so
/// `--vacuity error` cannot be defeated by causing a candidate's evaluation
/// to error rather than causing its BFS to run long. A failure of the
/// state-space walk itself (an action's `enabled`/`step` cannot be
/// evaluated) is not a per-candidate condition and still propagates as
/// `Err` for the whole call, matching every other BFS in this module —
/// unlike a per-candidate evaluation error, there is no narrower unit to
/// attribute it to. When that happens, every candidate still pending at
/// that point (including ones that would otherwise have resolved
/// `Exhausted` from the budget moments later) loses its finding entirely
/// rather than being reported `Exhausted`; `verification_warnings` accepts
/// this as a known, narrow gap (see its own doc comment) rather than
/// threading partial results out of an `Err` path.
///
/// # Errors
///
/// Returns [`RuntimeError`] when the state-space walk itself fails.
pub fn expression_reachability(
    model: &KernelModel,
    expressions: &[Expr],
    depth: usize,
    budget: usize,
) -> Result<Vec<Reachability>, RuntimeError> {
    if expressions.is_empty() {
        return Ok(Vec::new());
    }
    let mut scratch = Monitor::new(model.clone())?;
    let initial_state = scratch.state.clone();
    let mut queue = VecDeque::from([(initial_state.clone(), 0_usize)]);
    let mut visited = BTreeSet::from([initial_state]);
    let mut results: Vec<Option<Reachability>> = vec![None; expressions.len()];
    let mut pending: BTreeSet<usize> = (0..expressions.len()).collect();

    'walk: while let Some((state, step)) = queue.pop_front() {
        scratch.state = state.clone();
        scratch.step = step;
        let mut resolved = Vec::new();
        for &index in &pending {
            let truth = with_total_division(|| {
                eval(
                    &expressions[index],
                    &scratch.state,
                    &mut Bindings::new(),
                    &scratch.model,
                    None,
                )
            })
            .and_then(as_bool);
            match truth {
                Ok(true) => {
                    results[index] = Some(Reachability::Reachable);
                    resolved.push(index);
                }
                Err(_) => {
                    // Absorbed per-candidate, not propagated: see the doc
                    // comment above. Fail-closed, unlike the pre-#729
                    // behavior this replaces.
                    results[index] = Some(Reachability::Exhausted);
                    resolved.push(index);
                }
                Ok(false) => {}
            }
        }
        for index in resolved {
            pending.remove(&index);
        }
        if pending.is_empty() {
            break 'walk;
        }
        if step >= depth {
            continue;
        }
        for instance in scratch.enabled()? {
            scratch.state = state.clone();
            scratch.step = step;
            let stepped = scratch.step(&instance)?;
            if stepped.violation.is_some() {
                continue;
            }
            if visited.insert(stepped.state.clone()) {
                if visited.len() >= budget {
                    for &index in &pending {
                        results[index] = Some(Reachability::Exhausted);
                    }
                    break 'walk;
                }
                queue.push_back((stepped.state, step + 1));
            }
        }
    }
    Ok(results
        .into_iter()
        .map(|result| result.unwrap_or(Reachability::Unreachable))
        .collect())
}

/// Wrap `expr` in a nested `exists` quantifier over `binders`, outermost
/// binder first — the same existential closure the frozen Python reference
/// (`bmc._exists_wrap`) uses to check reachability of a leadsTo trigger or
/// implication antecedent independent of any particular binding.
fn exists_wrap(binders: &[Binder], expr: Expr) -> Expr {
    binders
        .iter()
        .rev()
        .fold(expr, |body, binder| Expr::Quantified {
            quantifier: "exists".to_owned(),
            binder: binder.clone(),
            body: Box::new(body),
        })
}

/// The existentially-closed antecedent of each user invariant shaped
/// `forall* P => Q` (`docs/DESIGN-vacuity.md` lane 2), paired with the
/// index of the source invariant in `model.invariants`. An invariant
/// without that shape (after peeling leading `forall`s) contributes no
/// candidate, so the index travels with the expression rather than being
/// assumed to line up with position.
#[must_use]
pub fn vacuous_implication_candidates(model: &KernelModel) -> Vec<(usize, Expr)> {
    let mut candidates = Vec::new();
    for (index, property) in model.invariants.iter().enumerate() {
        // `forall* => ...` shape: peel every leading `forall` (nested
        // foralls included, matching the frozen Python reference's
        // `_implication_antecedent_candidate`), then existentially close
        // the antecedent over the collected binders. With zero leading
        // foralls this is a no-op, so the original top-level-`=>` shape
        // still works.
        let mut binders = Vec::new();
        let mut inner = &property.expr;
        while let Expr::Quantified {
            quantifier,
            binder,
            body,
        } = inner
        {
            if quantifier != "forall" {
                break;
            }
            binders.push(binder.clone());
            inner = body;
        }
        let Expr::Binary { op, left, .. } = inner else {
            continue;
        };
        if op != "=>" {
            continue;
        }
        candidates.push((index, exists_wrap(&binders, (**left).clone())));
    }
    candidates
}

/// The existentially-closed trigger of each `leadsTo` property
/// (`docs/DESIGN-vacuity.md` lane 3), one per `model.leadstos` entry in
/// declaration order.
#[must_use]
pub fn vacuous_leadsto_candidates(model: &KernelModel) -> Vec<Expr> {
    model
        .leadstos
        .iter()
        .map(|property| exists_wrap(&property.binders, property.before.clone()))
        .collect()
}

/// One `vacuous_implication`/`vacuous_leadsto` reachability finding shy of
/// its `kind`, shared by the two lanes below so the truncated variant
/// (`vacuity_probe_truncated`) does not need to duplicate the JSON shape.
fn vacuity_reachability_warning(
    kind: &str,
    subject: &str,
    name: &str,
    message: &str,
    hint: &str,
    loc: &JsonValue,
) -> JsonValue {
    json!({
        "kind": kind,
        "name": name,
        "message": message,
        "hint": hint,
        "loc": loc,
        "classification": if kind == "vacuity_probe_truncated" { "probe_truncated" } else { "insufficient_depth" },
        "blocking": [],
        "faithfulness_class": if kind == "vacuity_probe_truncated" { "reachability_unknown" } else { "intent_unexercised" },
        "recommended_action": if kind == "vacuity_probe_truncated" {
            format!("the {subject}'s reachability could not be established by the probe; this is unusual and typically means either the state shape (e.g. an order-sensitive history variable) defeats BFS dedup and exhausts the internal budget -- simplify it or reduce --depth -- or the {subject} itself fails to evaluate in some reached state -- check it for a construct the concrete evaluator cannot represent")
        } else {
            "add a single-shot reachable for the action / raise --depth".to_owned()
        },
    })
}

/// Build the verification warnings shared by native and browser frontends.
///
/// The two reachability vacuity lanes are computed here, with one shared,
/// budgeted BFS (issue #729) over every antecedent/trigger candidate
/// (`CONCRETE_PROBE_BUDGET`, the same constant/calibration
/// `find_boundary_violation` uses), and stay solver-independent.
/// `solver_vacuity` carries the already-rendered `docs/DESIGN-vacuity.md`
/// §2 lanes 4–7 that only `fsl-verifier` can decide; passing them in keeps
/// the documented warning order (model → vacuity → deadlock → action
/// coverage) owned by one function without giving `fsl-runtime` a solver
/// dependency.
///
/// `skip_vacuity_probe` skips the whole reachability BFS (neither
/// `vacuous_implication`/`vacuous_leadsto` nor `vacuity_probe_truncated` is
/// computed or emitted) rather than computing it and filtering the result.
/// INVARIANT (issue #729): only the `verify`/`sweep` CLI option parsing for
/// `--vacuity ignore` may pass `true` here. Every other caller -- the
/// `ledger`/`html`/`mutate` baseline, the wasm Worker surface (which has no
/// `--vacuity` option at all), induction's internal BMC base pass, and
/// tests -- must pass `false` so the probe always runs and `--vacuity
/// error` never silently loses evidence it would otherwise catch.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn verification_warnings(
    model: &KernelModel,
    depth: usize,
    warn_deadlock: bool,
    deadlock_step: Option<usize>,
    deadlock_state: Option<&State>,
    action_coverage: &BTreeMap<String, bool>,
    solver_vacuity: &[JsonValue],
    skip_vacuity_probe: bool,
) -> Vec<JsonValue> {
    let mut warnings = model_warnings(model);
    if !skip_vacuity_probe {
        let implication_candidates = vacuous_implication_candidates(model);
        let leadsto_candidates = vacuous_leadsto_candidates(model);
        let mut probe_expressions: Vec<Expr> = implication_candidates
            .iter()
            .map(|(_, expression)| expression.clone())
            .collect();
        probe_expressions.extend(leadsto_candidates.iter().cloned());
        // A `RuntimeError` from the state-space walk itself (see
        // `expression_reachability`'s doc comment -- distinct from a
        // per-candidate evaluation error, which resolves `Exhausted`
        // instead of propagating) loses every candidate's finding for this
        // run, with no narrower fallback to degrade to: `.unwrap_or_default`
        // yields an empty `Vec`, so `probe_results.get(_)` is `None` for
        // every index below and no warning is emitted for any candidate.
        // This is NOT observationally equivalent to the pre-#729 baseline,
        // where each property's own independent `expression_reachable` call
        // only lost that one property's warning on error -- it is a known,
        // narrow gap against this issue's own fail-closed contract: a
        // candidate that would have resolved `Exhausted` (budget truncation)
        // moments after the walk-level error occurred loses its
        // `vacuity_probe_truncated` finding entirely instead of reporting
        // it. Accepted rather than threading partial results out of an
        // `Err` path, because a walk-level `RuntimeError` here means an
        // action's `enabled`/`step` itself could not be evaluated -- the
        // same condition that already fails the surrounding BMC/explicit
        // run before vacuity warnings are ever rendered, so this path is
        // not reachable on any spec that reaches `verification_warnings` in
        // the first place. See `docs/DESIGN-vacuity.md`.
        let probe_results =
            expression_reachability(model, &probe_expressions, depth, CONCRETE_PROBE_BUDGET)
                .unwrap_or_default();
        for (candidate_index, (property_index, _)) in implication_candidates.iter().enumerate() {
            let property = &model.invariants[*property_index];
            let name = display_name(&property.name);
            let mut warning = match probe_results.get(candidate_index) {
                Some(Reachability::Unreachable) => vacuity_reachability_warning(
                    "vacuous_implication",
                    "implication antecedent",
                    &name,
                    &format!(
                        "invariant '{name}' has an implication antecedent that is unreachable within depth {depth}"
                    ),
                    "the antecedent is not reachable within this depth; check whether an action that should establish it is missing, or whether the antecedent expression is wrong",
                    &property.span.python_loc(),
                ),
                Some(Reachability::Exhausted) => vacuity_reachability_warning(
                    "vacuity_probe_truncated",
                    "implication antecedent",
                    &name,
                    &format!(
                        "invariant '{name}' has an implication antecedent whose reachability the probe could not establish within depth {depth}"
                    ),
                    "vacuity was not established either way for this antecedent; the probe either exhausted its internal state budget or failed to evaluate the antecedent in some reached state before reaching a verdict",
                    &property.span.python_loc(),
                ),
                Some(Reachability::Reachable) | None => continue,
            };
            if let JsonValue::Object(warning) = &mut warning {
                insert_requirement_metadata(warning, &property.annotations, property.meta.as_ref());
            }
            warnings.push(warning);
        }
        let leadsto_offset = implication_candidates.len();
        for (offset, property) in model.leadstos.iter().enumerate() {
            let name = display_name(&property.name);
            let mut warning = match probe_results.get(leadsto_offset + offset) {
                Some(Reachability::Unreachable) => vacuity_reachability_warning(
                    "vacuous_leadsto",
                    "leadsTo trigger",
                    &name,
                    &format!(
                        "leadsTo '{name}' has a trigger that is unreachable within depth {depth}"
                    ),
                    "the trigger is not reachable within this depth; check whether an action that should establish it is missing, or whether the trigger expression is wrong",
                    &property.span.python_loc(),
                ),
                Some(Reachability::Exhausted) => vacuity_reachability_warning(
                    "vacuity_probe_truncated",
                    "leadsTo trigger",
                    &name,
                    &format!(
                        "leadsTo '{name}' has a trigger whose reachability the probe could not establish within depth {depth}"
                    ),
                    "vacuity was not established either way for this trigger; the probe either exhausted its internal state budget or failed to evaluate the trigger in some reached state before reaching a verdict",
                    &property.span.python_loc(),
                ),
                Some(Reachability::Reachable) | None => continue,
            };
            if let JsonValue::Object(warning) = &mut warning {
                insert_requirement_metadata(warning, &property.annotations, property.meta.as_ref());
            }
            warnings.push(warning);
        }
    }
    warnings.extend(solver_vacuity.iter().cloned());
    if warn_deadlock && let Some(step) = deadlock_step {
        let summary = deadlock_state.map_or_else(String::new, |state| state_summary(model, state));
        warnings.push(json!({
            "kind": "deadlock",
            "message": format!("deadlock reachable at step {step} (state: {summary})"),
            "hint": "add an enabled action, declare intended stops in a terminal { } block, or use --deadlock=ignore if intentional",
        }));
    }
    for (name, covered) in action_coverage {
        if !covered && let Some(action) = model.actions.iter().find(|action| action.name == *name) {
            warnings.extend(never_enabled_action_warning(model, action, depth));
        }
    }
    warnings
}

/// Render the public, bounded action-coverage finding shared by verification
/// and `scenarios`. Returns `None` for internal scaffolding without an authored
/// origin or a source-backed action span. The action origin is the sole
/// authority for a lowered action's display name; the executable name remains
/// only as `generated_name` alongside the origin chain.
#[must_use]
pub fn never_enabled_action_warning(
    model: &KernelModel,
    action: &ActionDef,
    depth: usize,
) -> Option<JsonValue> {
    let origin = model.action_origin(&action.name);
    let name = origin
        .and_then(origin_display_name)
        .map_or_else(|| display_name(&action.name), str::to_owned);
    // Keep this public diagnostic aligned with all other action JSON: a
    // source-backed lowered action reports the authored primary location,
    // while a kernel action retains its own declaration span. Zero spans are
    // reserved for generated-only sentinels and are not public findings.
    let source_span = origin
        .and_then(|origin| origin.primary.as_ref().and_then(|site| site.span))
        .filter(|span| span.start.line > 0 && span.start.column > 0)
        .or_else(|| {
            (action.span.start.line > 0 && action.span.start.column > 0).then_some(action.span)
        })?;
    let loc = source_span.python_loc();
    let mut warning = json!({
        "kind": "never_enabled_action",
        "name": name,
        "loc": loc,
        "message": format!("action '{name}' is never enabled within depth {depth} — the spec may be vacuous (check its requires clauses)"),
        "hint": format!("these requires clauses are unsatisfiable at every step up to depth {depth}; weaken one of them, add an action that establishes them, or increase --depth"),
        "blocking_requires": [],
    });
    if let JsonValue::Object(entry) = &mut warning {
        insert_requirement_metadata(entry, &action.annotations, action.meta.as_ref());
        if let Some(origin) = origin {
            entry.insert(
                "generated_name".to_owned(),
                json!(display_name(&action.name)),
            );
            entry.insert("origin".to_owned(), internal_origin_json(origin));
        }
    }
    Some(warning)
}

/// Remove bounded deadlock findings from warnings promoted to an induction proof.
#[must_use]
pub fn induction_warnings(warnings: &[JsonValue]) -> Vec<JsonValue> {
    warnings
        .iter()
        .filter(|warning| warning.get("kind").and_then(JsonValue::as_str) != Some("deadlock"))
        .cloned()
        .collect()
}

/// Replay a symbolic trace through the independent concrete Monitor.
///
/// This is the semantic witness gate: every action must be enabled with the
/// reported parameters, and every resulting logical state and root-level change
/// must equal the symbolic projection.
///
/// # Errors
///
/// Returns [`RuntimeError`] for an empty/malformed trace, a disabled action, or
/// any state/change mismatch.
pub fn replay_trace(model: KernelModel, trace: &[TraceStep]) -> Result<(), RuntimeError> {
    replay_trace_with_initial(model, trace, None)
}

/// Replay a trace whose step zero is a caller-provided complete logical state.
///
/// # Errors
///
/// Returns [`RuntimeError`] under the same malformed/disabled/mismatch
/// conditions as [`replay_trace`].
pub fn replay_trace_from_state(
    model: KernelModel,
    trace: &[TraceStep],
    initial_state: &State,
) -> Result<(), RuntimeError> {
    replay_trace_with_initial(model, trace, Some(initial_state))
}

fn replay_trace_with_initial(
    model: KernelModel,
    trace: &[TraceStep],
    initial_state: Option<&State>,
) -> Result<(), RuntimeError> {
    let first = trace
        .first()
        .ok_or_else(|| runtime_error("cannot replay an empty trace"))?;
    if first.step != 0 || first.action.is_some() {
        return Err(runtime_error("trace must begin with an action-free step 0"));
    }
    // A caller-provided initial state (the trace's own witnessed step 0, or
    // an explicit `--initial-state`) makes `Monitor::from_state` the right
    // constructor here — see its doc comment (#519).
    let mut monitor = match initial_state {
        Some(initial_state) => Monitor::from_state(model, initial_state.clone()),
        None => Monitor::new(model)?,
    };
    if monitor.state != first.state {
        return Err(runtime_error(
            "trace initial state does not match Monitor init",
        ));
    }
    for (expected_step, entry) in trace.iter().enumerate().skip(1) {
        if entry.step != expected_step {
            return Err(runtime_error(format!(
                "trace step numbering mismatch: expected {expected_step}, got {}",
                entry.step
            )));
        }
        let action = entry
            .action
            .as_ref()
            .ok_or_else(|| runtime_error(format!("trace step {expected_step} has no action")))?;
        let before = monitor.state.clone();
        let stepped = match monitor.enabled() {
            Ok(enabled) => {
                let instance = enabled
                    .iter()
                    .find(|instance| {
                        instance.action == action.name && instance.params == action.params
                    })
                    .ok_or_else(|| {
                        runtime_error(format!(
                            "trace action '{}' is not enabled at step {expected_step}",
                            action.name
                        ))
                    })?;
                monitor.step(instance)?
            }
            Err(error)
                if expected_step + 1 == trace.len()
                    && is_partial_operation_error(&error.message) =>
            {
                let attempted = monitor.attempt(&action.name, &action.params)?;
                if attempted
                    .violation
                    .as_ref()
                    .is_none_or(|violation| violation.kind != "partial_op")
                {
                    return Err(runtime_error(format!(
                        "trace action '{}' does not reproduce a partial operation at step {expected_step}",
                        action.name
                    )));
                }
                attempted
            }
            Err(error) => return Err(error),
        };
        let observed_state = stepped.attempted_state.as_ref().unwrap_or(&stepped.state);
        if observed_state != &entry.state {
            return Err(runtime_error(format!(
                "trace state mismatch at step {expected_step}"
            )));
        }
        let changes = observed_state
            .iter()
            .filter_map(|(name, value)| {
                let old = &before[name];
                (old != value).then(|| {
                    (
                        name.clone(),
                        TraceChange {
                            from: old.clone(),
                            to: value.clone(),
                        },
                    )
                })
            })
            .collect::<BTreeMap<_, _>>();
        if changes != entry.changes {
            return Err(runtime_error(format!(
                "trace change set mismatch at step {expected_step}"
            )));
        }
    }
    Ok(())
}

/// Find a concrete replayable cover trace for each action within `depth`.
///
/// # Errors
///
/// Returns [`RuntimeError`] when concrete evaluation or stepping fails.
pub fn action_cover_traces(
    model: KernelModel,
    depth: usize,
) -> Result<BTreeMap<String, Vec<TraceStep>>, RuntimeError> {
    // `scratch` re-pointed per state, `parents` reconstructed only on
    // witness discovery -- the same #783 pattern as `check_refinement`'s
    // walk. The witness for a covered action is built from the *parent*
    // state's own reconstructed trace plus the covering step, not from the
    // child's `parents` entry: the witness registration below fires even
    // when the child was already visited by an earlier path (it runs before
    // the `visited` check), so it cannot depend on the child having a
    // `ParentLink` at all.
    let mut scratch = Monitor::new(model)?;
    let initial_state = scratch.state.clone();
    let mut covered = BTreeMap::new();
    let mut visited = BTreeSet::from([initial_state.clone()]);
    let mut parents = BTreeMap::<State, trace::ParentLink>::new();
    let mut queue = trace::LeanFrontier::new();
    queue.push(initial_state, 0);
    while let Some((state, step)) = queue.pop() {
        if step >= depth {
            continue;
        }
        scratch.state = state.clone();
        scratch.step = step;
        let enabled = scratch.enabled()?;
        for instance in enabled {
            scratch.state = state.clone();
            scratch.step = step;
            let result = scratch.step(&instance)?;
            if result.violation.is_none() {
                let child_state = result.state.clone();
                if !covered.contains_key(&instance.action) {
                    let mut witness = trace::reconstruct_trace(&state, &parents);
                    witness.push(trace_step_from_result(step + 1, &state, &instance, &result));
                    covered.insert(instance.action.clone(), witness);
                }
                if visited.insert(child_state.clone()) {
                    parents
                        .entry(child_state.clone())
                        .or_insert_with(|| trace::ParentLink {
                            parent: state.clone(),
                            action: TraceAction {
                                name: instance.action.clone(),
                                params: instance.params.clone(),
                            },
                        });
                    queue.push(child_state, step + 1);
                }
            }
        }
    }
    Ok(covered)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeadstoResponse {
    pub property: String,
    pub bindings: BTreeMap<String, Value>,
    pub pending_at: usize,
    pub satisfied_at: usize,
    pub trace: Vec<TraceStep>,
}

/// `(found responses, missing (property, binding, triggered) triples)` — see
/// [`leadsto_response_traces`]. `triggered` is true when the antecedent held
/// at some visited state (a genuine incomplete response) and false when it
/// never held within `depth` (nothing was ever pending for that binding);
/// callers must word the two differently rather than reporting both as "no
/// response scenario" (issue #526).
pub type LeadstoResponseTraces = (Vec<LeadstoResponse>, Vec<(String, Bindings, bool)>);

/// Find concrete response examples for each finite `leadsTo` binding.
///
/// Also returns every `(property, binding)` with no response witness within
/// `depth`, tagged with whether its antecedent ever held — scenario
/// completeness (issue #526) must warn for each such binding individually
/// rather than collapsing to one warning per property, since a single
/// witnessed binding would otherwise hide every other binding's gap.
///
/// # Errors
///
/// Returns [`RuntimeError`] when binder or property evaluation fails.
pub fn leadsto_response_traces(
    model: &KernelModel,
    depth: usize,
) -> Result<LeadstoResponseTraces, RuntimeError> {
    if model.leadstos.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    // Only the per-node `Monitor` clone (a whole `KernelModel` clone) is
    // removed here, via a re-pointed `scratch` -- issue #783 scoped this
    // lane to that clone alone. The per-node `Vec<TraceStep>` clone stays:
    // this walk has no `visited` dedup (a `pending`/response history is
    // path-dependent, so two routes to the same state must stay distinct
    // path-trees, not merge behind a shared `ParentLink`), and a
    // `reconstruct_trace`-style backward walk has no well-defined single
    // parent to walk back through when routes fork and rejoin.
    let mut scratch = Monitor::new(model.clone())?;
    let initial_state = scratch.state.clone();
    let bindings = model
        .leadstos
        .iter()
        .map(|property| {
            Ok((
                property.name.clone(),
                leadsto_bindings(property, &initial_state, model)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, RuntimeError>>()?;
    let target_count = bindings.values().map(Vec::len).sum::<usize>();
    let initial_trace = vec![TraceStep {
        step: 0,
        state: initial_state.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    let mut responses = BTreeMap::<(String, Bindings), LeadstoResponse>::new();
    let mut triggered = BTreeSet::<(String, Bindings)>::new();
    let mut queue = trace::PathFrontier::new();
    queue.push(initial_state, initial_trace, 0);
    while let Some((state, trace, step)) = queue.pop() {
        for property in &model.leadstos {
            for binding in &bindings[&property.name] {
                let key = (property.name.clone(), binding.clone());
                if responses.contains_key(&key) {
                    continue;
                }
                if let Some(pending_at) = response_pending_at(property, binding, &trace, model)? {
                    responses.insert(
                        key.clone(),
                        LeadstoResponse {
                            property: property.name.clone(),
                            bindings: binding.clone(),
                            pending_at,
                            satisfied_at: step,
                            trace: trace.clone(),
                        },
                    );
                    triggered.insert(key);
                    continue;
                }
                if let Some(last) = trace.last() {
                    let mut probe = binding.clone();
                    if as_bool(eval(
                        &property.before,
                        &last.state,
                        &mut probe,
                        model,
                        None,
                    )?)? {
                        triggered.insert(key);
                    }
                }
            }
        }
        if responses.len() == target_count || step >= depth {
            continue;
        }
        scratch.state = state.clone();
        scratch.step = step;
        for instance in scratch.enabled()? {
            scratch.state = state.clone();
            scratch.step = step;
            let result = scratch.step(&instance)?;
            if result.violation.is_some() {
                continue;
            }
            let mut child_trace = trace.clone();
            child_trace.push(trace_step_from_result(step + 1, &state, &instance, &result));
            queue.push(result.state.clone(), child_trace, step + 1);
        }
    }
    let missing = bindings
        .iter()
        .flat_map(|(name, property_bindings)| {
            property_bindings.iter().filter_map(|binding| {
                let key = (name.clone(), binding.clone());
                (!responses.contains_key(&key))
                    .then(|| (key.0.clone(), key.1.clone(), triggered.contains(&key)))
            })
        })
        .collect();
    Ok((responses.into_values().collect(), missing))
}

fn leadsto_bindings(
    property: &fsl_core::LeadsToDef,
    state: &State,
    model: &KernelModel,
) -> Result<Vec<Bindings>, RuntimeError> {
    with_total_division(|| leadsto_bindings_inner(property, state, model))
}

fn leadsto_bindings_inner(
    property: &fsl_core::LeadsToDef,
    state: &State,
    model: &KernelModel,
) -> Result<Vec<Bindings>, RuntimeError> {
    let mut candidates = vec![Bindings::new()];
    for binder in &property.binders {
        let mut next = Vec::new();
        for binding in candidates {
            for (value, mut local) in binder_values(binder, state, &binding, model, None)? {
                local.insert(binder_name(binder).to_owned(), value);
                if binder_where_holds(binder, state, &mut local, model, None)? {
                    next.push(local);
                }
            }
        }
        candidates = next;
    }
    Ok(candidates)
}

fn response_pending_at(
    property: &fsl_core::LeadsToDef,
    binding: &Bindings,
    trace: &[TraceStep],
    model: &KernelModel,
) -> Result<Option<usize>, RuntimeError> {
    with_total_division(|| response_pending_at_inner(property, binding, trace, model))
}

fn response_pending_at_inner(
    property: &fsl_core::LeadsToDef,
    binding: &Bindings,
    trace: &[TraceStep],
    model: &KernelModel,
) -> Result<Option<usize>, RuntimeError> {
    let Some(last) = trace.last() else {
        return Ok(None);
    };
    let mut current_binding = binding.clone();
    if !as_bool(eval(
        &property.after,
        &last.state,
        &mut current_binding,
        model,
        None,
    )?)? {
        return Ok(None);
    }
    for pending_at in 0..trace.len() {
        let mut candidate_binding = binding.clone();
        if !as_bool(eval(
            &property.before,
            &trace[pending_at].state,
            &mut candidate_binding,
            model,
            None,
        )?)? {
            continue;
        }
        let mut q_held_early = false;
        for entry in &trace[pending_at..trace.len() - 1] {
            let mut q_binding = binding.clone();
            if as_bool(eval(
                &property.after,
                &entry.state,
                &mut q_binding,
                model,
                None,
            )?)? {
                q_held_early = true;
                break;
            }
        }
        if !q_held_early {
            return Ok(Some(pending_at));
        }
    }
    Ok(None)
}

/// Project one executed [`StepResult`] onto the shared trace-entry shape.
///
/// The recorded state is `attempted_state` when the Monitor rolled the step
/// back, so a violating entry carries the successor the spec states rather
/// than the pre-step state the rollback restored (issue #843).
#[must_use]
pub fn trace_step_from_result(
    step: usize,
    before: &State,
    instance: &EnabledAction,
    result: &StepResult,
) -> TraceStep {
    let observed_state = result.attempted_state.as_ref().unwrap_or(&result.state);
    let changes = observed_state
        .iter()
        .filter_map(|(name, value)| {
            let old = &before[name];
            (old != value).then(|| {
                (
                    name.clone(),
                    TraceChange {
                        from: old.clone(),
                        to: value.clone(),
                    },
                )
            })
        })
        .collect();
    TraceStep {
        step,
        state: observed_state.clone(),
        action: Some(TraceAction {
            name: instance.action.clone(),
            params: instance.params.clone(),
        }),
        changes,
    }
}

fn record_reachables(
    monitor: &Monitor,
    step: usize,
    result: &mut BfsResult,
) -> Result<Option<Violation>, RuntimeError> {
    with_total_division(|| {
        for property in &monitor.model.reachables {
            if result.reachables[&property.name].is_some() {
                continue;
            }
            let mut bindings = Bindings::new();
            let value = match eval(
                &property.expr,
                &monitor.state,
                &mut bindings,
                &monitor.model,
                None,
            ) {
                Ok(value) => value,
                Err(error) if is_partial_operation_error(&error.message) => {
                    return Ok(Some(Violation {
                        kind: "partial_op".to_owned(),
                        name: format!("_partial_property_{}", property.name),
                        step,
                    }));
                }
                Err(error) => return Err(error),
            };
            if as_bool(value)? {
                result
                    .reachables
                    .insert(property.name.clone(), Some(ReachableWitness { step }));
            }
        }
        Ok(None)
    })
}

fn check_state(
    state: &State,
    old_state: Option<&State>,
    model: &KernelModel,
    step: usize,
) -> Result<Option<Violation>, RuntimeError> {
    check_state_selected(state, old_state, model, step, None)
}

fn check_state_selected(
    state: &State,
    old_state: Option<&State>,
    model: &KernelModel,
    step: usize,
    checked_bounds: Option<&BTreeSet<String>>,
) -> Result<Option<Violation>, RuntimeError> {
    with_total_division(|| {
        check_state_selected_inner(state, old_state, model, step, checked_bounds)
    })
}

fn check_state_selected_inner(
    state: &State,
    old_state: Option<&State>,
    model: &KernelModel,
    step: usize,
    checked_bounds: Option<&BTreeSet<String>>,
) -> Result<Option<Violation>, RuntimeError> {
    for (name, ty) in &model.state {
        let property_name = format!("_bounds_{name}");
        if checked_bounds.is_some_and(|selected| !selected.contains(&property_name)) {
            continue;
        }
        if !value_conforms(
            state
                .get(name)
                .ok_or_else(|| runtime_error(format!("missing state variable '{name}'")))?,
            ty,
            model,
        )? {
            return Ok(Some(Violation {
                kind: "type_bound".to_owned(),
                name: property_name,
                step,
            }));
        }
    }
    for property in &model.invariants {
        let mut bindings = Bindings::new();
        let value = match eval(&property.expr, state, &mut bindings, model, old_state) {
            Ok(value) => value,
            Err(error) if is_partial_operation_error(&error.message) => {
                return Ok(Some(Violation {
                    kind: "partial_op".to_owned(),
                    name: format!("_partial_property_{}", property.name),
                    step,
                }));
            }
            Err(error) => return Err(error),
        };
        if !as_bool(value)? {
            return Ok(Some(Violation {
                kind: "invariant".to_owned(),
                name: property.name.clone(),
                step,
            }));
        }
    }
    if let Some(old_state) = old_state {
        for property in &model.transitions {
            let mut bindings = Bindings::new();
            let value = match eval(&property.expr, state, &mut bindings, model, Some(old_state)) {
                Ok(value) => value,
                Err(error) if is_partial_operation_error(&error.message) => {
                    return Ok(Some(Violation {
                        kind: "partial_op".to_owned(),
                        name: format!("_partial_property_{}", property.name),
                        step,
                    }));
                }
                Err(error) => return Err(error),
            };
            if !as_bool(value)? {
                return Ok(Some(Violation {
                    kind: "trans".to_owned(),
                    name: property.name.clone(),
                    step,
                }));
            }
        }
    }
    Ok(None)
}

fn action_parameter_bindings(
    action: &ActionDef,
    model: &KernelModel,
) -> Result<Vec<Bindings>, RuntimeError> {
    let mut bindings = vec![Bindings::new()];
    for param in &action.params {
        let values = match param {
            ParamDef::Typed { ty, .. } => model.domain_values(ty)?,
            ParamDef::Range { lo, hi, .. } => (*lo..=*hi).map(Value::Int).collect(),
        };
        let mut next = Vec::new();
        for existing in bindings {
            for value in &values {
                let mut candidate = existing.clone();
                candidate.insert(param.name().to_owned(), value.clone());
                next.push(candidate);
            }
        }
        bindings = next;
    }
    Ok(bindings)
}

fn validate_action_parameters(
    action: &ActionDef,
    params: &Bindings,
    model: &KernelModel,
) -> Result<(), RuntimeError> {
    if action.params.len() != params.len() {
        return Err(runtime_error(format!(
            "parameters do not match action '{}'",
            action.name
        )));
    }
    for parameter in &action.params {
        let value = params.get(parameter.name()).ok_or_else(|| {
            runtime_error(format!("parameters do not match action '{}'", action.name))
        })?;
        let belongs = match parameter {
            ParamDef::Typed { ty, .. } => value_conforms(value, ty, model)?,
            ParamDef::Range { lo, hi, .. } => {
                matches!(value, Value::Int(value) if lo <= value && value <= hi)
            }
        };
        if !belongs {
            return Err(runtime_error(format!(
                "parameter '{}' does not belong to its declared domain for action '{}'",
                parameter.name(),
                action.name
            )));
        }
    }
    Ok(())
}

fn evaluate_action_guards(
    action: &ActionDef,
    params: &Bindings,
    state: &State,
    model: &KernelModel,
) -> Result<Option<Bindings>, RuntimeError> {
    validate_action_parameters(action, params, model)?;
    let mut bindings = params.clone();
    for guard in &action.guards {
        match guard {
            ActionGuard::Let(name, expression) => {
                let value = eval(expression, state, &mut bindings, model, None)?;
                bindings.insert(name.clone(), value);
            }
            ActionGuard::Requires(expression) => {
                if !as_bool(eval(expression, state, &mut bindings, model, None)?)? {
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(bindings))
}

/// Execute one `init` statement, threading `written` — the concrete value
/// already assigned to each resolved lvalue location during this `init`
/// execution — through every branch and `forall` iteration.
///
/// `forall` bulk-initializes by executing its body once per binder value.
/// When distinct binder values resolve to the *same* concrete location
/// (typically a target that does not index by the binder), imperative
/// last-write-wins would silently discard every assignment but the last —
/// masking exactly the case the symbolic engine reports as unsatisfiable
/// init (`forall k: K { x = k }` demands `x` equal every member of `K`
/// simultaneously). Detecting a location written to two different concrete
/// values keeps the concrete and symbolic engines in agreement without
/// requiring a solver: unsatisfiability is witnessed directly by the
/// conflicting concrete values, no search needed.
fn execute_init_statement(
    statement: &Statement,
    state: &mut State,
    bindings: &mut Bindings,
    model: &KernelModel,
    written: &mut BTreeMap<String, Value>,
) -> Result<(), RuntimeError> {
    match statement {
        Statement::Assign { target, value, .. } => {
            let value = eval(value, state, bindings, model, None)?;
            let read_state = state.clone();
            let key = lvalue_key(target, &read_state, bindings, model)?;
            match written.get(&key) {
                Some(previous) if *previous != value => {
                    return Err(runtime_error("init constraints are unsatisfiable"));
                }
                _ => {
                    written.insert(key, value.clone());
                }
            }
            assign(target, value, &read_state, state, bindings, model)?;
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            let branch = if as_bool(eval(condition, state, bindings, model, None)?)? {
                then_statements
            } else {
                else_statements
            };
            for statement in branch {
                execute_init_statement(statement, state, bindings, model, written)?;
            }
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            let candidates = binder_values(binder, state, bindings, model, None)?;
            for (value, mut local) in candidates {
                local.insert(binder_name(binder).to_owned(), value);
                if !binder_where_holds(binder, state, &mut local, model, None)? {
                    continue;
                }
                for statement in statements {
                    execute_init_statement(statement, state, &mut local, model, written)?;
                }
            }
        }
    }
    Ok(())
}

fn compute_statement_update(
    statement: &Statement,
    old_state: &State,
    pending: &mut State,
    writes: &mut BTreeSet<String>,
    bindings: &mut Bindings,
    model: &KernelModel,
) -> Result<(), RuntimeError> {
    match statement {
        Statement::Assign { target, value, .. } => {
            let value = eval(value, old_state, bindings, model, None)?;
            let write_key = lvalue_key(target, old_state, bindings, model)?;
            if !writes.insert(write_key.clone()) {
                return Err(runtime_error(format!(
                    "double assignment to '{write_key}' on the same execution path"
                )));
            }
            assign(target, value, old_state, pending, bindings, model)?;
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            let branch = if as_bool(eval(condition, old_state, bindings, model, None)?)? {
                then_statements
            } else {
                else_statements
            };
            for statement in branch {
                compute_statement_update(statement, old_state, pending, writes, bindings, model)?;
            }
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            for (value, mut local) in binder_values(binder, old_state, bindings, model, None)? {
                local.insert(binder_name(binder).to_owned(), value);
                if !binder_where_holds(binder, old_state, &mut local, model, None)? {
                    continue;
                }
                for statement in statements {
                    compute_statement_update(
                        statement, old_state, pending, writes, &mut local, model,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn assign(
    target: &LValue,
    value: Value,
    read_state: &State,
    target_state: &mut State,
    bindings: &mut Bindings,
    model: &KernelModel,
) -> Result<(), RuntimeError> {
    match target {
        LValue::Var(name) => {
            target_state.insert(name.clone(), coerce_relation_literal(name, value, model));
        }
        LValue::Index(name, index_expr) => {
            let index = eval(index_expr, read_state, bindings, model, None)?;
            let mut root = target_state
                .get(name)
                .or_else(|| read_state.get(name))
                .cloned()
                .ok_or_else(|| runtime_error(format!("unknown assignment target '{name}'")))?;
            match &mut root {
                Value::Map(values) => {
                    if !values.contains_key(&index) {
                        return Err(runtime_error("map assignment index outside key domain"));
                    }
                    values.insert(index, value);
                }
                Value::Seq(values) => {
                    let index = as_usize(index, "sequence index out of range")?;
                    let slot = values
                        .get_mut(index)
                        .ok_or_else(|| runtime_error("sequence assignment index out of range"))?;
                    *slot = value;
                }
                _ => return Err(runtime_error("indexed assignment requires map or sequence")),
            }
            target_state.insert(name.clone(), root);
        }
        LValue::Field(base, field) => match base.as_ref() {
            LValue::Var(name) => {
                let mut root = target_state
                    .get(name)
                    .or_else(|| read_state.get(name))
                    .cloned()
                    .ok_or_else(|| runtime_error(format!("unknown assignment target '{name}'")))?;
                let Value::Struct { fields, .. } = &mut root else {
                    return Err(runtime_error("field assignment requires struct state"));
                };
                let slot = fields
                    .get_mut(field)
                    .ok_or_else(|| runtime_error(format!("unknown struct field '{field}'")))?;
                *slot = value;
                target_state.insert(name.clone(), root);
            }
            LValue::Index(name, index_expr) => {
                let index = eval(index_expr, read_state, bindings, model, None)?;
                let mut root = target_state
                    .get(name)
                    .or_else(|| read_state.get(name))
                    .cloned()
                    .ok_or_else(|| runtime_error(format!("unknown assignment target '{name}'")))?;
                let Value::Map(values) = &mut root else {
                    return Err(runtime_error("map field assignment requires map state"));
                };
                let Value::Struct { fields, .. } = values
                    .get_mut(&index)
                    .ok_or_else(|| runtime_error("map assignment index outside key domain"))?
                else {
                    return Err(runtime_error("map field assignment requires struct values"));
                };
                let slot = fields
                    .get_mut(field)
                    .ok_or_else(|| runtime_error(format!("unknown struct field '{field}'")))?;
                *slot = value;
                target_state.insert(name.clone(), root);
            }
            LValue::Field(_, _) => {
                return Err(runtime_error(
                    "nested field lvalue is not supported by grammar",
                ));
            }
        },
    }
    Ok(())
}

/// `Set {}` is the only relation literal surface syntax accepts (public
/// Kernel typing rejects a non-empty one), but `eval`'s `Expr::Set` arm has
/// no assignment-target type context and always produces `Value::Set`.
/// Coerce it here, where the target variable's declared type is known --
/// mirrors the symbolic evaluator's `SymbolicValue::SetLiteral` ->
/// `SymbolicValue::Relation` coercion (`fsl-verifier/src/value.rs::coerce`).
fn coerce_relation_literal(name: &str, value: Value, model: &KernelModel) -> Value {
    if let Value::Set(items) = &value
        && items.is_empty()
        && matches!(model.state_type(name), Some(TypeRef::Relation(_, _)))
    {
        return Value::Relation(BTreeSet::new());
    }
    value
}

fn lvalue_key(
    target: &LValue,
    state: &State,
    bindings: &mut Bindings,
    model: &KernelModel,
) -> Result<String, RuntimeError> {
    Ok(match target {
        LValue::Var(name) => name.clone(),
        LValue::Index(name, index) => {
            format!("{name}[{:?}]", eval(index, state, bindings, model, None)?)
        }
        LValue::Field(base, field) => match base.as_ref() {
            LValue::Var(name) => format!("{name}.{field}"),
            LValue::Index(name, index) => format!(
                "{name}[{:?}].{field}",
                eval(index, state, bindings, model, None)?
            ),
            LValue::Field(_, _) => return Err(runtime_error("nested field lvalue")),
        },
    })
}

/// Check a concrete value against a resolved kernel type.
///
/// # Errors
///
/// Returns [`RuntimeError`] when a nested type cannot be resolved.
pub fn value_conforms(
    value: &Value,
    ty: &TypeRef,
    model: &KernelModel,
) -> Result<bool, RuntimeError> {
    Ok(match (value, ty) {
        (Value::Int(_), TypeRef::Int)
        | (Value::Bool(_), TypeRef::Bool)
        | (Value::None, TypeRef::Option(_)) => true,
        (Value::Int(value), TypeRef::Range(lo, hi)) => lo <= value && value <= hi,
        (value, TypeRef::Named(name)) => match model.types.get(name) {
            Some(fsl_core::TypeDef::Domain { lo, hi, .. }) => {
                matches!(value, Value::Int(value) if lo <= value && value <= hi)
            }
            Some(fsl_core::TypeDef::Enum { members, .. }) => matches!(
                value,
                Value::Enum { type_name, member }
                    if type_name == name && members.contains(member)
            ),
            Some(fsl_core::TypeDef::Struct { fields: expected }) => {
                let Value::Struct { type_name, fields } = value else {
                    return Ok(false);
                };
                type_name == name
                    && expected.iter().all(|(field, ty)| {
                        fields
                            .get(field)
                            .is_some_and(|value| value_conforms(value, ty, model).unwrap_or(false))
                    })
            }
            None => false,
        },
        (Value::Map(values), TypeRef::Map(key_ty, value_ty)) => {
            let domain = model.map_key_values(key_ty)?;
            values.len() == domain.len()
                && domain.iter().all(|key| values.contains_key(key))
                && values.iter().all(|(key, value)| {
                    value_conforms(key, key_ty, model).unwrap_or(false)
                        && value_conforms(value, value_ty, model).unwrap_or(false)
                })
        }
        (Value::Relation(values), TypeRef::Relation(source, target)) => {
            values.iter().all(|(source_value, target_value)| {
                value_conforms(source_value, source, model).unwrap_or(false)
                    && value_conforms(target_value, target, model).unwrap_or(false)
            })
        }
        (Value::Set(values), TypeRef::Set(inner)) => values
            .iter()
            .all(|value| value_conforms(value, inner, model).unwrap_or(false)),
        (Value::Seq(values), TypeRef::Seq(inner, capacity)) => {
            values.len() <= *capacity
                && values
                    .iter()
                    .all(|value| value_conforms(value, inner, model).unwrap_or(false))
        }
        (Value::Some(value), TypeRef::Option(inner)) => value_conforms(value, inner, model)?,
        _ => false,
    })
}

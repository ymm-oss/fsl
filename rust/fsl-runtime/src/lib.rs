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
    insert_requirement_metadata, model_warnings, state_summary, static_leadsto_bindings,
};
use serde_json::{Value as JsonValue, json};

mod explicit;

pub use explicit::{
    ExplicitReachableWitness, ExplicitResult, ExplicitViolation, deterministic_initial_state,
    explicit_unsupported_reason, verify_explicit, verify_explicit_selected,
};

pub type State = BTreeMap<String, Value>;
pub type Bindings = BTreeMap<String, Value>;

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
                    .get(as_usize(index)?)
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
            eval(second, state, bindings, model, old_state)?,
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

    search(expr, state, &Bindings::new(), model)
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
                .get(as_usize(index.clone())?)
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
                Err(runtime_error("division by zero"))
            } else {
                Ok(Value::Int(left.div_euclid(right)))
            }
        }
        "%" => {
            let left = as_int(left)?;
            let right = as_int(right)?;
            if right == 0 {
                Err(runtime_error("remainder by zero"))
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
    source: Value,
    target: &Value,
) -> Result<Value, RuntimeError> {
    let Value::Relation(edges) = relation else {
        return Err(runtime_error("reachable() requires a relation"));
    };
    let mut seen = BTreeSet::from([source.clone()]);
    let mut frontier = vec![source];
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
                        next.clone(),
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

fn as_usize(value: Value) -> Result<usize, RuntimeError> {
    usize::try_from(as_int(value)?).map_err(|_| runtime_error("index must be non-negative"))
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
pub struct Violation {
    pub kind: String,
    pub name: String,
    pub step: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepResult {
    pub action: String,
    pub params: BTreeMap<String, Value>,
    pub state: State,
    pub attempted_state: Option<State>,
    pub violation: Option<Violation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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

    /// Initialize a solver-independent concrete monitor.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] when default construction or sequential init
    /// execution fails.
    pub fn new(model: KernelModel) -> Result<Self, RuntimeError> {
        let mut state = model
            .state
            .iter()
            .map(|(name, ty)| Ok((name.clone(), model.default_value(ty)?)))
            .collect::<Result<State, RuntimeError>>()?;
        let mut bindings = Bindings::new();
        for statement in &model.init {
            execute_init_statement(statement, &mut state, &mut bindings, &model)?;
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
pub struct ReachableWitness {
    pub step: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BfsResult {
    pub spec: String,
    pub depth: usize,
    pub states_explored: usize,
    pub violation: Option<Violation>,
    pub reachables: BTreeMap<String, Option<ReachableWitness>>,
    pub deadlock_step: Option<usize>,
    pub action_coverage: BTreeMap<String, bool>,
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
pub struct RefinementCheck {
    pub implementation: String,
    pub abstraction: String,
    pub depth: usize,
    pub action_map: BTreeMap<String, String>,
    pub abs_has_ensures: bool,
    pub failure: Option<RefinementFailure>,
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
    let eval_model = merged_refinement_model(implementation, abstraction)?;
    let impl_initial = Monitor::new(implementation.clone())?;
    let abs_initial = Monitor::new(abstraction.clone())?;
    let alpha_initial = alpha_state(
        &impl_initial.state,
        implementation,
        abstraction,
        mapping,
        &eval_model,
    )?;
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
    };
    let initial_trace = vec![TraceStep {
        step: 0,
        state: impl_initial.state.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    let mut initial_alpha_monitor = Monitor::new(abstraction.clone())?;
    initial_alpha_monitor.state = alpha_initial.clone();
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
    if alpha_initial != abs_initial.state {
        check.failure = Some(refinement_failure(
            "abs_state_mismatch",
            Some("init"),
            0,
            &initial_trace,
            None,
            Some(abs_initial.state),
            Some(alpha_initial),
        ));
        return Ok(check);
    }

    let mut queue = VecDeque::from([(impl_initial, 0_usize, initial_trace)]);
    let mut visited = BTreeSet::new();
    while let Some((_, step, _)) = queue.front() {
        let step = *step;
        let mut layer = Vec::new();
        while queue
            .front()
            .is_some_and(|(_, queued_step, _)| *queued_step == step)
        {
            let Some((monitor, _, trace)) = queue.pop_front() else {
                unreachable!("queue front was present");
            };
            if visited.insert(monitor.state.clone()) && step < depth {
                layer.push((monitor, trace));
            }
        }
        let mut candidates = Vec::new();
        for (state_index, (monitor, trace)) in layer.into_iter().enumerate() {
            let alpha_before = alpha_state(
                &monitor.state,
                implementation,
                abstraction,
                mapping,
                &eval_model,
            )?;
            for enabled in monitor.enabled()? {
                let action_index = implementation
                    .actions
                    .iter()
                    .position(|action| action.name == enabled.action)
                    .unwrap_or(usize::MAX);
                candidates.push((
                    action_index,
                    enabled.params.clone(),
                    state_index,
                    monitor.clone(),
                    trace.clone(),
                    alpha_before.clone(),
                    enabled,
                ));
            }
        }
        candidates.sort_by(|left, right| {
            (&left.0, &left.1, &left.2).cmp(&(&right.0, &right.1, &right.2))
        });
        for (_, _, _, monitor, trace, alpha_before, enabled) in candidates {
            let mut child = monitor.clone();
            let stepped = child.step(&enabled)?;
            if stepped.violation.is_some() {
                continue;
            }
            let mut child_trace = trace.clone();
            child_trace.push(trace_step_from_result(
                step + 1,
                &monitor.state,
                &enabled,
                &stepped,
            ));
            let alpha_after = alpha_state(
                &child.state,
                implementation,
                abstraction,
                mapping,
                &eval_model,
            )?;
            let action_map = &mapping.action_correspondences[&enabled.action];
            match &action_map.target {
                ActionCorrespondenceTarget::Stutter => {
                    if alpha_before != alpha_after {
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
                    let values = args
                        .iter()
                        .map(|expr| eval(expr, &monitor.state, &mut bindings, &eval_model, None))
                        .collect::<Result<Vec<_>, _>>()?;
                    let expected_params = abs_action
                        .params
                        .iter()
                        .zip(values)
                        .map(|(param, value)| (param.name().to_owned(), value))
                        .collect::<BTreeMap<_, _>>();
                    let mut abs_monitor = Monitor::new(abstraction.clone())?;
                    abs_monitor.state = abstract_action_state(
                        &alpha_before,
                        abstraction,
                        abs_action,
                        &expected_params,
                    )?;
                    let Some(abs_enabled) =
                        refinement_action_instance(&abs_monitor, abs_action, expected_params)?
                    else {
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
            let mut alpha_monitor = Monitor::new(abstraction.clone())?;
            alpha_monitor.state = alpha_after.clone();
            if let Some(violation) = alpha_monitor.current_violation()? {
                let kind = if violation.kind == "type_bound" {
                    "map_out_of_bounds"
                } else {
                    "abs_state_mismatch"
                };
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
            if !visited.contains(&child.state) {
                queue.push_back((child, step + 1, child_trace));
            }
        }
    }
    Ok(check)
}

/// Exhaustively explore concrete reachable states to a bounded depth.
///
/// This is the independent oracle, not the future symbolic BMC engine.
///
/// # Errors
///
/// Returns [`RuntimeError`] if concrete evaluation or execution fails.
pub fn bfs(model: KernelModel, depth: usize) -> Result<BfsResult, RuntimeError> {
    let initial = Monitor::new(model)?;
    let mut result = BfsResult {
        spec: initial.model.name.clone(),
        depth,
        states_explored: 0,
        violation: initial.current_violation()?,
        reachables: initial
            .model
            .reachables
            .iter()
            .map(|property| (property.name.clone(), None))
            .collect(),
        deadlock_step: None,
        action_coverage: initial
            .model
            .actions
            .iter()
            .map(|action| (action.name.clone(), false))
            .collect(),
    };
    record_reachables(&initial, 0, &mut result)?;
    let mut queue = VecDeque::from([(initial.clone(), 0_usize)]);
    let mut visited = BTreeSet::from([initial.state.clone()]);
    while let Some((monitor, step)) = queue.pop_front() {
        result.states_explored += 1;
        let enabled = monitor.enabled()?;
        if enabled.is_empty() {
            result.deadlock_step = Some(result.deadlock_step.map_or(step, |old| old.min(step)));
        }
        for instance in &enabled {
            result.action_coverage.insert(instance.action.clone(), true);
        }
        if step >= depth {
            continue;
        }
        for instance in enabled {
            let mut child = monitor.clone();
            let stepped = child.step(&instance)?;
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
            record_reachables(&child, step + 1, &mut result)?;
            if visited.insert(child.state.clone()) {
                queue.push_back((child, step + 1));
            }
        }
    }
    Ok(result)
}

/// Find the first concrete partial-operation or type-bound violation and its trace.
///
/// # Errors
///
/// Returns [`RuntimeError`] when concrete action evaluation fails for another reason.
pub fn find_boundary_violation(
    model: KernelModel,
    depth: usize,
) -> Result<Option<(Violation, Vec<TraceStep>)>, RuntimeError> {
    let initial = Monitor::new(model)?;
    let initial_trace = vec![TraceStep {
        step: 0,
        state: initial.state.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    let mut queue = VecDeque::from([(initial.clone(), initial_trace, 0_usize)]);
    let mut visited = BTreeSet::from([initial.state.clone()]);
    while let Some((monitor, trace, step)) = queue.pop_front() {
        if step >= depth {
            continue;
        }
        for instance in monitor.enabled()? {
            let mut child = monitor.clone();
            let before = child.state.clone();
            let stepped = child.step(&instance)?;
            let mut child_trace = trace.clone();
            child_trace.push(trace_step_from_result(
                step + 1,
                &before,
                &instance,
                &stepped,
            ));
            if let Some(violation) = stepped.violation {
                if matches!(violation.kind.as_str(), "partial_op" | "type_bound") {
                    return Ok(Some((violation, child_trace)));
                }
                continue;
            }
            if visited.insert(child.state.clone()) {
                queue.push_back((child, child_trace, step + 1));
            }
        }
    }
    Ok(None)
}

/// Return whether a Boolean expression holds in any concrete state up to `depth`.
///
/// # Errors
///
/// Returns [`RuntimeError`] when the expression or a reachable action cannot be
/// evaluated concretely.
pub fn expression_reachable(
    model: KernelModel,
    expression: &Expr,
    depth: usize,
) -> Result<bool, RuntimeError> {
    let initial = Monitor::new(model)?;
    let mut queue = VecDeque::from([(initial.clone(), 0_usize)]);
    let mut visited = BTreeSet::from([initial.state.clone()]);
    while let Some((monitor, step)) = queue.pop_front() {
        if as_bool(eval(
            expression,
            &monitor.state,
            &mut Bindings::new(),
            &monitor.model,
            None,
        )?)? {
            return Ok(true);
        }
        if step >= depth {
            continue;
        }
        for instance in monitor.enabled()? {
            let mut child = monitor.clone();
            let stepped = child.step(&instance)?;
            if stepped.violation.is_none() && visited.insert(child.state.clone()) {
                queue.push_back((child, step + 1));
            }
        }
    }
    Ok(false)
}

/// Build solver-independent verification warnings shared by native and browser frontends.
#[must_use]
pub fn verification_warnings(
    model: &KernelModel,
    depth: usize,
    warn_deadlock: bool,
    deadlock_step: Option<usize>,
    deadlock_state: Option<&State>,
    action_coverage: &BTreeMap<String, bool>,
) -> Vec<JsonValue> {
    let mut warnings = model_warnings(model);
    for property in &model.invariants {
        let Expr::Binary { op, left, .. } = &property.expr else {
            continue;
        };
        if op != "=>" {
            continue;
        }
        if matches!(expression_reachable(model.clone(), left, depth), Ok(false)) {
            let mut warning = json!({
                "kind": "vacuous_implication",
                "name": display_name(&property.name),
                "message": format!("invariant '{}' has an implication antecedent that is unreachable within depth {depth}", display_name(&property.name)),
                "hint": "the antecedent is not reachable within this depth; check whether an action that should establish it is missing, or whether the antecedent expression is wrong",
                "loc": property.span.python_loc(),
                "classification": "insufficient_depth",
                "blocking": [],
                "faithfulness_class": "intent_unexercised",
                "recommended_action": "add a single-shot reachable for the action / raise --depth",
            });
            if let JsonValue::Object(warning) = &mut warning {
                insert_requirement_metadata(warning, &property.annotations, property.meta.as_ref());
            }
            warnings.push(warning);
        }
    }
    if warn_deadlock && let Some(step) = deadlock_step {
        let summary = deadlock_state.map_or_else(String::new, |state| state_summary(model, state));
        warnings.push(json!({
            "kind": "deadlock",
            "message": format!("deadlock reachable at step {step} (state: {summary})"),
            "hint": "add an enabled action, declare intended stops in a terminal { } block, or use --deadlock=ignore if intentional",
        }));
    }
    for (name, covered) in action_coverage {
        if !covered {
            warnings.push(json!({
                "message": format!("action '{}' is never enabled within depth {depth} — the spec may be vacuous (check its requires clauses)", display_name(name)),
                "hint": format!("these requires clauses are unsatisfiable at every step up to depth {depth}; weaken one of them, add an action that establishes them, or increase --depth"),
            }));
        }
    }
    warnings
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
    let mut monitor = Monitor::new(model)?;
    if let Some(initial_state) = initial_state {
        monitor.state.clone_from(initial_state);
    }
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
        let enabled = monitor.enabled()?;
        let instance = enabled
            .iter()
            .find(|instance| instance.action == action.name && instance.params == action.params)
            .ok_or_else(|| {
                runtime_error(format!(
                    "trace action '{}' is not enabled at step {expected_step}",
                    action.name
                ))
            })?;
        let before = monitor.state.clone();
        let stepped = monitor.step(instance)?;
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
    let initial = Monitor::new(model)?;
    let initial_trace = vec![TraceStep {
        step: 0,
        state: initial.state.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    let mut covered = BTreeMap::new();
    let mut first_enabled_seen = BTreeSet::new();
    let mut visited = BTreeSet::from([initial.state.clone()]);
    let mut queue = VecDeque::from([(initial, initial_trace, 0_usize)]);
    while let Some((monitor, trace, step)) = queue.pop_front() {
        if step >= depth {
            continue;
        }
        let enabled = monitor.enabled()?;
        for instance in enabled {
            let mut child = monitor.clone();
            let result = child.step(&instance)?;
            let mut child_trace = trace.clone();
            child_trace.push(trace_step_from_result(
                step + 1,
                &monitor.state,
                &instance,
                &result,
            ));
            if first_enabled_seen.insert(instance.action.clone())
                && can_extend_exactly(&child, depth - step - 1)?
            {
                covered
                    .entry(instance.action.clone())
                    .or_insert_with(|| child_trace.clone());
            }
            if result.violation.is_none() && visited.insert(child.state.clone()) {
                queue.push_back((child, child_trace, step + 1));
            }
        }
    }
    Ok(covered)
}

fn can_extend_exactly(monitor: &Monitor, remaining: usize) -> Result<bool, RuntimeError> {
    if remaining == 0 {
        return Ok(true);
    }
    for instance in monitor.enabled()? {
        let mut child = monitor.clone();
        let result = child.step(&instance)?;
        if result.violation.is_none() && can_extend_exactly(&child, remaining - 1)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeadstoResponse {
    pub property: String,
    pub bindings: BTreeMap<String, Value>,
    pub pending_at: usize,
    pub satisfied_at: usize,
    pub trace: Vec<TraceStep>,
}

/// Find concrete response examples for each finite `leadsTo` binding.
///
/// # Errors
///
/// Returns [`RuntimeError`] when binder or property evaluation fails.
pub fn leadsto_response_traces(
    model: &KernelModel,
    depth: usize,
) -> Result<Vec<LeadstoResponse>, RuntimeError> {
    if model.leadstos.is_empty() {
        return Ok(Vec::new());
    }
    let initial = Monitor::new(model.clone())?;
    let bindings = model
        .leadstos
        .iter()
        .map(|property| {
            Ok((
                property.name.clone(),
                leadsto_bindings(property, &initial.state, model)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, RuntimeError>>()?;
    let target_count = bindings.values().map(Vec::len).sum::<usize>();
    let initial_trace = vec![TraceStep {
        step: 0,
        state: initial.state.clone(),
        action: None,
        changes: BTreeMap::new(),
    }];
    let mut responses = BTreeMap::<(String, Bindings), LeadstoResponse>::new();
    let mut queue = VecDeque::from([(initial, initial_trace, 0_usize)]);
    while let Some((monitor, trace, step)) = queue.pop_front() {
        for property in &model.leadstos {
            for binding in &bindings[&property.name] {
                let key = (property.name.clone(), binding.clone());
                if responses.contains_key(&key) {
                    continue;
                }
                if let Some(pending_at) = response_pending_at(property, binding, &trace, model)? {
                    responses.insert(
                        key,
                        LeadstoResponse {
                            property: property.name.clone(),
                            bindings: binding.clone(),
                            pending_at,
                            satisfied_at: step,
                            trace: trace.clone(),
                        },
                    );
                }
            }
        }
        if responses.len() == target_count || step >= depth {
            continue;
        }
        for instance in monitor.enabled()? {
            let mut child = monitor.clone();
            let result = child.step(&instance)?;
            if result.violation.is_some() {
                continue;
            }
            let mut child_trace = trace.clone();
            child_trace.push(trace_step_from_result(
                step + 1,
                &monitor.state,
                &instance,
                &result,
            ));
            queue.push_back((child, child_trace, step + 1));
        }
    }
    Ok(responses.into_values().collect())
}

fn leadsto_bindings(
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

fn trace_step_from_result(
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
) -> Result<(), RuntimeError> {
    for property in &monitor.model.reachables {
        if result.reachables[&property.name].is_some() {
            continue;
        }
        let mut bindings = Bindings::new();
        if as_bool(eval(
            &property.expr,
            &monitor.state,
            &mut bindings,
            &monitor.model,
            None,
        )?)? {
            result
                .reachables
                .insert(property.name.clone(), Some(ReachableWitness { step }));
        }
    }
    Ok(())
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
        if !as_bool(eval(
            &property.expr,
            state,
            &mut bindings,
            model,
            old_state,
        )?)? {
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
            if !as_bool(eval(
                &property.expr,
                state,
                &mut bindings,
                model,
                Some(old_state),
            )?)? {
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

fn execute_init_statement(
    statement: &Statement,
    state: &mut State,
    bindings: &mut Bindings,
    model: &KernelModel,
) -> Result<(), RuntimeError> {
    match statement {
        Statement::Assign { target, value, .. } => {
            let value = eval(value, state, bindings, model, None)?;
            let read_state = state.clone();
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
                execute_init_statement(statement, state, bindings, model)?;
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
                    execute_init_statement(statement, state, &mut local, model)?;
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
            target_state.insert(name.clone(), value);
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
                    let index = as_usize(index)?;
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

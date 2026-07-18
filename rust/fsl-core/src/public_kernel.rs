// SPDX-License-Identifier: Apache-2.0

//! Versioned normalized Kernel JSON for external compilers.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use fsl_syntax::{
    AggregateKind, Binder, ConditionalSpans, Expr, LValue, MetaTag, Pattern, SourcePos, Span,
    SpecItem, Statement,
};
use serde_json::{Map, Value, json};

use crate::{
    ActionGuard, KernelModel, KernelSpec, OriginChain, OriginSite, ParamDef, TypeDef, TypeRef,
    action_target, property_target, state_target, type_target,
};

pub const KERNEL_V1_SCHEMA_VERSION: &str = "1.0.0";
pub const KERNEL_V1_SCHEMA_ID: &str = "https://fsl.dev/schemas/fslc/kernel/kernel.v1.schema.json";
pub const KERNEL_V2_SCHEMA_VERSION: &str = "2.0.0";
pub const KERNEL_V2_SCHEMA_ID: &str = "https://fsl.dev/schemas/fslc/kernel/kernel.v2.schema.json";
pub const TESTGEN_TRACE_V1_SCHEMA_VERSION: &str = "1.0.0";
pub const TESTGEN_TRACE_V1_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/kernel/testgen-trace.v1.schema.json";
pub const REPLAY_TRACE_V1_INITIAL_SCHEMA_VERSION: &str = "1.0.0";
pub const REPLAY_TRACE_V1_STUTTER_SCHEMA_VERSION: &str = "1.1.0";
pub const REPLAY_TRACE_V1_SCHEMA_VERSION: &str = "1.2.0";
pub const REPLAY_TRACE_V1_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/kernel/replay-trace.v1.schema.json";

/// Backwards-compatible aliases for the default Public Kernel v1 contract.
pub const KERNEL_SCHEMA_VERSION: &str = KERNEL_V1_SCHEMA_VERSION;
pub const KERNEL_SCHEMA_ID: &str = KERNEL_V1_SCHEMA_ID;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicKernelVersion {
    V1,
    V2,
}

impl PublicKernelVersion {
    /// Parse an explicitly negotiated Public Kernel major.
    ///
    /// # Errors
    ///
    /// Returns a fail-closed error for an unsupported major or malformed value.
    pub fn parse(value: &str) -> Result<Self, PublicKernelError> {
        match value {
            "1" => Ok(Self::V1),
            "2" => Ok(Self::V2),
            _ => Err(error(format!(
                "unsupported public Kernel major '{value}'; supported majors are 1 and 2"
            ))),
        }
    }

    #[must_use]
    pub const fn schema_version(self) -> &'static str {
        match self {
            Self::V1 => KERNEL_V1_SCHEMA_VERSION,
            Self::V2 => KERNEL_V2_SCHEMA_VERSION,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicKernelError {
    pub message: String,
    pub span: Option<Span>,
}

impl PublicKernelError {
    fn with_span(mut self, span: Span) -> Self {
        self.span.get_or_insert(span);
        self
    }
}

impl fmt::Display for PublicKernelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PublicKernelError {}

fn error(message: impl Into<String>) -> PublicKernelError {
    PublicKernelError {
        message: message.into(),
        span: None,
    }
}

fn span_json(path: &str, span: Span) -> Value {
    json!({
        "file": path,
        "line": span.start.line,
        "column": span.start.column,
        "end_line": span.end.line,
        "end_column": span.end.column,
    })
}

fn unknown_span() -> Span {
    let position = SourcePos {
        offset: 0,
        line: 1,
        column: 1,
    };
    Span {
        start: position,
        end: position,
    }
}

fn statement_span(statement: &Statement) -> Span {
    match statement {
        Statement::Assign { span, .. }
        | Statement::If { span, .. }
        | Statement::ForAll { span, .. } => *span,
    }
}

fn type_json(ty: &TypeRef) -> Value {
    match ty {
        TypeRef::Int => json!({"kind":"int"}),
        TypeRef::Bool => json!({"kind":"bool"}),
        TypeRef::Named(name) => json!({"kind":"named","name":name}),
        TypeRef::Range(lo, hi) => json!({"kind":"domain","lo":lo,"hi":hi}),
        TypeRef::Map(key, value) => {
            json!({"kind":"map","key":type_json(key),"value":type_json(value)})
        }
        TypeRef::Relation(source, target) => json!({
            "kind":"relation","source":type_json(source),"target":type_json(target)
        }),
        TypeRef::Set(item) => json!({"kind":"set","item":type_json(item)}),
        TypeRef::Seq(item, capacity) => {
            json!({"kind":"seq","item":type_json(item),"capacity":capacity})
        }
        TypeRef::Option(item) => json!({"kind":"option","item":type_json(item)}),
    }
}

fn resolve<'a>(model: &'a KernelModel, ty: &'a TypeRef) -> Result<TypeRef, PublicKernelError> {
    match ty {
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => Ok(TypeRef::Range(*lo, *hi)),
            Some(TypeDef::Enum { .. } | TypeDef::Struct { .. }) => Ok(ty.clone()),
            None => Err(error(format!("public Kernel cannot resolve type '{name}'"))),
        },
        _ => Ok(ty.clone()),
    }
}

type TypeEnv = BTreeMap<String, TypeRef>;
type FiniteBinderCandidates = (String, Vec<(Expr, Expr)>, Option<Expr>);

fn base_env(model: &KernelModel) -> TypeEnv {
    let mut env = model.state.iter().cloned().collect::<TypeEnv>();
    for (name, value) in &model.consts {
        let ty = match value {
            crate::FslValue::Bool(_) => TypeRef::Bool,
            _ => TypeRef::Int,
        };
        env.entry(name.clone()).or_insert(ty);
    }
    for (type_name, definition) in &model.types {
        if let TypeDef::Enum { members, .. } = definition {
            for member in members {
                env.entry(member.clone())
                    .or_insert_with(|| TypeRef::Named(type_name.clone()));
            }
        }
    }
    env
}

/// Project one already-checked expression with the same typed JSON contract
/// used inside Public Kernel v2. Sidecar contracts use this instead of
/// publishing the historical Python-shaped AST.
///
/// # Errors
///
/// Returns [`PublicKernelError`] when the expression cannot be represented or
/// does not have the expected type.
pub fn public_kernel_expression(
    expression: &Expr,
    model: &KernelModel,
    source_path: &str,
    span: Span,
    expected_type: Option<&TypeRef>,
) -> Result<Value, PublicKernelError> {
    expr_json(
        expression,
        &base_env(model),
        model,
        source_path,
        span,
        expected_type,
    )
}

pub(crate) fn validate_expression_type(
    expr: &Expr,
    expected: &TypeRef,
    bindings: &[(String, TypeRef)],
    model: &KernelModel,
) -> Result<(), PublicKernelError> {
    let mut env = base_env(model);
    env.extend(bindings.iter().cloned());
    expr_json(expr, &env, model, "", unknown_span(), Some(expected)).map(|_| ())
}

pub(crate) fn expression_binder_type(
    binder: &Binder,
    model: &KernelModel,
) -> Result<TypeRef, PublicKernelError> {
    binder_type(binder, &base_env(model), model)
}

fn qualified_name(name: &fsl_syntax::QualifiedName) -> Result<String, PublicKernelError> {
    if name.namespace.is_some() {
        return Err(error(
            "qualified types must be lowered before public Kernel export",
        ));
    }
    Ok(name.name.clone())
}

fn binder_type(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<TypeRef, PublicKernelError> {
    match binder {
        Binder::Typed { type_name, .. } => Ok(TypeRef::Named(qualified_name(type_name)?)),
        Binder::Range { lo, hi, .. } => {
            ensure_assignable(lo, &TypeRef::Int, env, model, unknown_span())?;
            ensure_assignable(hi, &TypeRef::Int, env, model, unknown_span())?;
            match (lo.as_ref(), hi.as_ref()) {
                (Expr::Num(lo), Expr::Num(hi)) => Ok(TypeRef::Range(*lo, *hi)),
                _ => Ok(TypeRef::Int),
            }
        }
        Binder::Collection { collection, .. } => {
            match resolve(model, &infer_type(collection, env, model, None)?)? {
                TypeRef::Set(item) | TypeRef::Seq(item, _) => Ok(*item),
                _ => Err(error("collection binder requires Set or Seq")),
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn infer_type(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    expected: Option<&TypeRef>,
) -> Result<TypeRef, PublicKernelError> {
    match expr {
        Expr::Num(_) => match expected.map(|ty| resolve(model, ty)).transpose()? {
            Some(ty @ (TypeRef::Int | TypeRef::Range(_, _))) => Ok(ty),
            _ => Ok(TypeRef::Int),
        },
        Expr::Bool(_) | Expr::Not(_) | Expr::Is { .. } | Expr::Quantified { .. } => {
            Ok(TypeRef::Bool)
        }
        Expr::None => match expected.map(|ty| resolve(model, ty)).transpose()? {
            Some(ty @ TypeRef::Option(_)) => Ok(ty),
            _ => Err(error("public Kernel cannot infer uncontextualized none")),
        },
        Expr::Some(item) => {
            if let Some(expected) = expected
                && matches!(resolve(model, expected)?, TypeRef::Option(_))
            {
                return Ok(expected.clone());
            }
            Ok(TypeRef::Option(Box::new(infer_type(
                item, env, model, None,
            )?)))
        }
        Expr::Set(items) => {
            if let Some(expected) = expected
                && matches!(resolve(model, expected)?, TypeRef::Set(_))
            {
                return Ok(expected.clone());
            }
            let first = items
                .first()
                .ok_or_else(|| error("public Kernel cannot infer empty Set"))?;
            Ok(TypeRef::Set(Box::new(infer_type(first, env, model, None)?)))
        }
        Expr::Seq(items) => {
            if let Some(expected) = expected
                && matches!(resolve(model, expected)?, TypeRef::Seq(_, _))
            {
                return Ok(expected.clone());
            }
            let first = items
                .first()
                .ok_or_else(|| error("public Kernel cannot infer empty Seq"))?;
            Ok(TypeRef::Seq(
                Box::new(infer_type(first, env, model, None)?),
                items.len(),
            ))
        }
        Expr::Struct { name, .. } => Ok(TypeRef::Named(name.clone())),
        Expr::Var(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| error(format!("public Kernel cannot type identifier '{name}'"))),
        Expr::Call { name, .. } => Err(error(format!(
            "unlowered predicate call '{name}' in public Kernel"
        ))),
        Expr::Stage { .. } => Err(error("unlowered stage access in public Kernel")),
        Expr::Index(base, _) => match resolve(model, &infer_type(base, env, model, None)?)? {
            TypeRef::Map(_, value) | TypeRef::Relation(_, value) => Ok(*value),
            TypeRef::Seq(item, _) => Ok(*item),
            _ => Err(error("public Kernel index requires Map, Relation, or Seq")),
        },
        Expr::Field(base, field) => {
            let TypeRef::Named(name) = infer_type(base, env, model, None)? else {
                return Err(error("public Kernel field access requires named struct"));
            };
            match model.types.get(&name) {
                Some(TypeDef::Struct { fields }) => fields
                    .iter()
                    .find_map(|(candidate, ty)| (candidate == field).then(|| ty.clone()))
                    .ok_or_else(|| error(format!("unknown struct field '{name}.{field}'"))),
                _ => Err(error("public Kernel field access requires struct")),
            }
        }
        Expr::Method { receiver, name, .. } => {
            let receiver_ty = infer_type(receiver, env, model, None)?;
            let resolved = resolve(model, &receiver_ty)?;
            match name.as_str() {
                "contains" => Ok(TypeRef::Bool),
                "size" => Ok(TypeRef::Int),
                "add" | "remove" | "push" | "pop" => Ok(receiver_ty),
                "head" | "at" => match resolved {
                    TypeRef::Seq(item, _) => Ok(*item),
                    _ => Err(error(format!("public Kernel cannot type method '{name}'"))),
                },
                _ => Err(error(format!("public Kernel cannot type method '{name}'"))),
            }
        }
        Expr::Binary { op, .. } => {
            if matches!(
                op.as_str(),
                "and" | "or" | "=>" | "==" | "!=" | "<" | "<=" | ">" | ">="
            ) {
                Ok(TypeRef::Bool)
            } else {
                Ok(TypeRef::Int)
            }
        }
        Expr::Neg(_) | Expr::BinaryNamed { .. } => Ok(TypeRef::Int),
        Expr::Aggregate { kind, .. } => Ok(
            if matches!(kind, AggregateKind::Count | AggregateKind::Sum) {
                TypeRef::Int
            } else {
                TypeRef::Bool
            },
        ),
        Expr::Conditional { then_expr, .. } => infer_type(then_expr, env, model, expected),
        Expr::UnaryNamed { name, expr, .. } => match name.as_str() {
            "old" => infer_type(expr, env, model, expected),
            "abs" => Ok(TypeRef::Int),
            "rel_acyclic" | "rel_functional" | "rel_injective" => Ok(TypeRef::Bool),
            "rel_domain" | "rel_range" => Ok(TypeRef::Set(Box::new(TypeRef::Int))),
            _ => Err(error(format!("unsupported unary expression '{name}'"))),
        },
        Expr::TernaryNamed { name, .. } if name == "rel_reachable" => Ok(TypeRef::Bool),
        Expr::TernaryNamed { name, .. } => {
            Err(error(format!("unsupported ternary expression '{name}'")))
        }
    }
}

fn types_compatible(
    actual: &TypeRef,
    expected: &TypeRef,
    model: &KernelModel,
) -> Result<bool, PublicKernelError> {
    let actual = resolve(model, actual)?;
    let expected = resolve(model, expected)?;
    Ok(match (&actual, &expected) {
        (TypeRef::Int | TypeRef::Range(_, _), TypeRef::Int | TypeRef::Range(_, _))
        | (TypeRef::Bool, TypeRef::Bool) => true,
        (TypeRef::Named(actual), TypeRef::Named(expected)) => actual == expected,
        (TypeRef::Set(actual), TypeRef::Set(expected))
        | (TypeRef::Option(actual), TypeRef::Option(expected))
        | (TypeRef::Seq(actual, _), TypeRef::Seq(expected, _)) => {
            types_compatible(actual, expected, model)?
        }
        (TypeRef::Map(actual_key, actual_value), TypeRef::Map(expected_key, expected_value))
        | (
            TypeRef::Relation(actual_key, actual_value),
            TypeRef::Relation(expected_key, expected_value),
        ) => {
            types_compatible(actual_key, expected_key, model)?
                && types_compatible(actual_value, expected_value, model)?
        }
        _ => false,
    })
}

fn ensure_assignable(
    expr: &Expr,
    expected: &TypeRef,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
) -> Result<(), PublicKernelError> {
    let resolved = resolve(model, expected).map_err(|error| error.with_span(span))?;
    match (expr, &resolved) {
        (Expr::None, TypeRef::Option(_)) => return Ok(()),
        (Expr::Some(item), TypeRef::Option(expected_item)) => {
            return ensure_assignable(item, expected_item, env, model, span);
        }
        (Expr::Set(items), TypeRef::Set(expected_item))
        | (Expr::Seq(items), TypeRef::Seq(expected_item, _)) => {
            for item in items {
                ensure_assignable(item, expected_item, env, model, span)?;
            }
            return Ok(());
        }
        (
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
                spans,
            },
            _,
        ) => {
            ensure_assignable(condition, &TypeRef::Bool, env, model, spans.condition)?;
            ensure_assignable(then_expr, expected, env, model, spans.then_expr)?;
            ensure_assignable(else_expr, expected, env, model, spans.else_expr)?;
            return Ok(());
        }
        _ => {}
    }
    let actual = infer_type(expr, env, model, None).map_err(|error| error.with_span(span))?;
    if types_compatible(&actual, expected, model).map_err(|error| error.with_span(span))? {
        Ok(())
    } else {
        Err(error(format!(
            "expression of type {actual:?} is not assignable to {expected:?}"
        ))
        .with_span(span))
    }
}

fn extend_pattern_binding(
    expression: &Expr,
    env: &mut TypeEnv,
    model: &KernelModel,
) -> Result<(), PublicKernelError> {
    match expression {
        Expr::Is {
            expr,
            pattern: Pattern::Some(binding),
        } => {
            let TypeRef::Option(inner) = resolve(model, &infer_type(expr, env, model, None)?)?
            else {
                return Err(error("some pattern requires an Option operand"));
            };
            env.insert(binding.clone(), *inner);
        }
        Expr::Binary { left, right, .. } => {
            extend_pattern_binding(left, env, model)?;
            extend_pattern_binding(right, env, model)?;
        }
        _ => {}
    }
    Ok(())
}

fn binder_json(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    span: Span,
) -> Result<Value, PublicKernelError> {
    let ty = binder_type(binder, env, model)?;
    let (kind, name, lo, hi, collection, where_expr) = match binder {
        Binder::Typed {
            name, where_expr, ..
        } => ("typed", name, None, None, None, where_expr.as_deref()),
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => (
            "range",
            name,
            Some(lo.as_ref()),
            Some(hi.as_ref()),
            None,
            where_expr.as_deref(),
        ),
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => (
            "collection",
            name,
            None,
            None,
            Some(collection.as_ref()),
            where_expr.as_deref(),
        ),
    };
    let mut output = json!({"name":name,"type":type_json(&ty),"kind":kind});
    let object = output.as_object_mut().expect("binder object");
    if let Some(lo) = lo {
        object.insert(
            "lo".to_owned(),
            expr_json(lo, env, model, path, span, Some(&ty))?,
        );
    }
    if let Some(hi) = hi {
        object.insert(
            "hi".to_owned(),
            expr_json(hi, env, model, path, span, Some(&ty))?,
        );
    }
    if let Some(collection) = collection {
        object.insert(
            "collection".to_owned(),
            expr_json(collection, env, model, path, span, None)?,
        );
    }
    if let Some(where_expr) = where_expr {
        let mut local = env.clone();
        local.insert(name.clone(), ty);
        object.insert(
            "where".to_owned(),
            expr_json(where_expr, &local, model, path, span, Some(&TypeRef::Bool))?,
        );
    }
    Ok(output)
}

fn normalize_aggregate(
    kind: AggregateKind,
    binder: &Binder,
    value: Option<&Expr>,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<Expr, PublicKernelError> {
    let (name, candidates, filter) = finite_binder_candidates(binder, env, model)?;
    let mut terms = Vec::new();
    for (candidate, membership) in candidates {
        let replacements = HashMap::from([(name.clone(), candidate)]);
        let condition = aggregate_condition(membership, filter.as_ref(), &replacements);
        let selected = match kind {
            AggregateKind::Count | AggregateKind::Unique | AggregateKind::ExactlyOne => {
                Expr::Num(1)
            }
            AggregateKind::Sum => crate::substitute_expr(
                value
                    .ok_or_else(|| error("sum aggregate requires a value"))?
                    .clone(),
                &replacements,
            ),
        };
        let span = unknown_span();
        terms.push(Expr::Conditional {
            condition: Box::new(condition),
            then_expr: Box::new(selected),
            else_expr: Box::new(Expr::Num(0)),
            spans: Box::new(ConditionalSpans {
                condition: span,
                then_expr: span,
                else_expr: span,
            }),
        });
    }
    Ok(aggregate_result(kind, terms))
}

fn finite_binder_candidates(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<FiniteBinderCandidates, PublicKernelError> {
    let (name, candidates, filter) = match binder {
        Binder::Typed {
            name, where_expr, ..
        } => {
            let candidates = model
                .domain_values(&binder_type(binder, env, model)?)
                .map_err(|model_error| error(model_error.to_string()))?
                .into_iter()
                .map(|candidate| Ok((value_expression(candidate)?, Expr::Bool(true))))
                .collect::<Result<Vec<_>, PublicKernelError>>()?;
            (name, candidates, where_expr.as_deref())
        }
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => {
            let lo = static_public_int(lo, model)?;
            let hi = static_public_int(hi, model)?;
            let candidates = (lo..=hi)
                .map(|candidate| (Expr::Num(candidate), Expr::Bool(true)))
                .collect();
            (name, candidates, where_expr.as_deref())
        }
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => {
            let collection_ty = resolve(model, &infer_type(collection, env, model, None)?)?;
            let candidates = match collection_ty {
                TypeRef::Set(element_ty) => model
                    .domain_values(&element_ty)
                    .map_err(|model_error| error(model_error.to_string()))?
                    .into_iter()
                    .map(|candidate| {
                        let candidate = value_expression(candidate)?;
                        let present = Expr::Method {
                            receiver: collection.clone(),
                            name: "contains".to_owned(),
                            args: vec![candidate.clone()],
                        };
                        Ok((candidate, present))
                    })
                    .collect::<Result<Vec<_>, PublicKernelError>>()?,
                TypeRef::Seq(_, capacity) => (0..capacity)
                    .map(|index| {
                        let index = i64::try_from(index)
                            .map_err(|_| error("Seq capacity exceeds public integer range"))?;
                        let candidate = Expr::Method {
                            receiver: collection.clone(),
                            name: "at".to_owned(),
                            args: vec![Expr::Num(index)],
                        };
                        let present = Expr::Binary {
                            op: "<".to_owned(),
                            left: Box::new(Expr::Num(index)),
                            right: Box::new(Expr::Method {
                                receiver: collection.clone(),
                                name: "size".to_owned(),
                                args: Vec::new(),
                            }),
                        };
                        Ok((candidate, present))
                    })
                    .collect::<Result<Vec<_>, PublicKernelError>>()?,
                _ => return Err(error("collection binder requires Set or Seq")),
            };
            (name, candidates, where_expr.as_deref())
        }
    };
    Ok((name.clone(), candidates, filter.cloned()))
}

fn aggregate_condition(
    membership: Expr,
    filter: Option<&Expr>,
    replacements: &HashMap<String, Expr>,
) -> Expr {
    filter.map_or(membership.clone(), |filter| {
        let span = unknown_span();
        Expr::Conditional {
            condition: Box::new(membership),
            then_expr: Box::new(crate::substitute_expr(filter.clone(), replacements)),
            else_expr: Box::new(Expr::Bool(false)),
            spans: Box::new(ConditionalSpans {
                condition: span,
                then_expr: span,
                else_expr: span,
            }),
        }
    })
}

fn aggregate_result(kind: AggregateKind, terms: Vec<Expr>) -> Expr {
    let total = terms
        .into_iter()
        .fold(Expr::Num(0), |left, right| Expr::Binary {
            op: "+".to_owned(),
            left: Box::new(left),
            right: Box::new(right),
        });
    match kind {
        AggregateKind::Count | AggregateKind::Sum => total,
        AggregateKind::Unique => Expr::Binary {
            op: "<=".to_owned(),
            left: Box::new(total),
            right: Box::new(Expr::Num(1)),
        },
        AggregateKind::ExactlyOne => Expr::Binary {
            op: "==".to_owned(),
            left: Box::new(total),
            right: Box::new(Expr::Num(1)),
        },
    }
}

fn static_public_int(expr: &Expr, model: &KernelModel) -> Result<i64, PublicKernelError> {
    match expr {
        Expr::Num(value) => Ok(*value),
        Expr::Var(name) => match model.consts.get(name) {
            Some(crate::FslValue::Int(value)) => Ok(*value),
            _ => Err(error(format!("'{name}' is not an integer const"))),
        },
        Expr::Neg(value) => static_public_int(value, model)?
            .checked_neg()
            .ok_or_else(|| error("integer overflow in range bound")),
        _ => Err(error(
            "public Kernel requires static aggregate range bounds",
        )),
    }
}

fn value_expression(value: crate::FslValue) -> Result<Expr, PublicKernelError> {
    Ok(match value {
        crate::FslValue::Int(value) => Expr::Num(value),
        crate::FslValue::Bool(value) => Expr::Bool(value),
        crate::FslValue::Enum { member, .. } => Expr::Var(member),
        crate::FslValue::None => Expr::None,
        crate::FslValue::Some(value) => Expr::Some(Box::new(value_expression(*value)?)),
        crate::FslValue::Struct { type_name, fields } => Expr::Struct {
            name: type_name,
            fields: fields
                .into_iter()
                .map(|(name, value)| Ok((name, value_expression(value)?)))
                .collect::<Result<Vec<_>, PublicKernelError>>()?,
        },
        crate::FslValue::Set(values) => Expr::Set(
            values
                .into_iter()
                .map(value_expression)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        crate::FslValue::Seq(values) => Expr::Seq(
            values
                .into_iter()
                .map(value_expression)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        crate::FslValue::Map(_) | crate::FslValue::Relation(_) => {
            return Err(error(
                "aggregate element has no public literal representation",
            ));
        }
    })
}

#[allow(clippy::too_many_lines)]
fn expr_json(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    span: Span,
    expected: Option<&TypeRef>,
) -> Result<Value, PublicKernelError> {
    let ty = infer_type(expr, env, model, expected).map_err(|error| error.with_span(span))?;
    if let Some(expected) = expected
        && !types_compatible(&ty, expected, model).map_err(|error| error.with_span(span))?
    {
        return Err(error(format!(
            "expression of type {ty:?} is not assignable to {expected:?}"
        ))
        .with_span(span));
    }
    let mut output = Map::from_iter([
        ("kind".to_owned(), Value::String("unknown".to_owned())),
        ("type".to_owned(), type_json(&ty)),
        ("span".to_owned(), span_json(path, span)),
    ]);
    match expr {
        Expr::Num(value) => {
            output.insert("kind".to_owned(), json!("num"));
            output.insert("value".to_owned(), json!(value));
        }
        Expr::Bool(value) => {
            output.insert("kind".to_owned(), json!("bool"));
            output.insert("value".to_owned(), json!(value));
        }
        Expr::None => {
            output.insert("kind".to_owned(), json!("none"));
        }
        Expr::Some(item) => {
            let TypeRef::Option(inner) = resolve(model, &ty)? else {
                return Err(error("some expression did not infer Option"));
            };
            output.insert("kind".to_owned(), json!("some"));
            output.insert(
                "operand".to_owned(),
                expr_json(item, env, model, path, span, Some(&inner))?,
            );
        }
        Expr::Set(items) | Expr::Seq(items) => {
            let (kind, item_ty) = match resolve(model, &ty)? {
                TypeRef::Set(item) => ("set_lit", item),
                TypeRef::Seq(item, _) => ("seq_lit", item),
                _ => return Err(error("collection literal type mismatch")),
            };
            output.insert("kind".to_owned(), json!(kind));
            output.insert(
                "items".to_owned(),
                Value::Array(
                    items
                        .iter()
                        .map(|item| expr_json(item, env, model, path, span, Some(&item_ty)))
                        .collect::<Result<_, _>>()?,
                ),
            );
        }
        Expr::Struct { name, fields } => {
            output.insert("kind".to_owned(), json!("struct_lit"));
            output.insert("name".to_owned(), json!(name));
            let Some(TypeDef::Struct {
                fields: definitions,
            }) = model.types.get(name)
            else {
                return Err(error(format!("unknown struct '{name}'")));
            };
            let definitions = definitions.iter().cloned().collect::<BTreeMap<_, _>>();
            output.insert(
                "fields".to_owned(),
                Value::Object(
                    fields
                        .iter()
                        .map(|(field, value)| {
                            let expected = definitions.get(field).ok_or_else(|| {
                                error(format!("unknown struct field '{name}.{field}'"))
                            })?;
                            Ok((
                                field.clone(),
                                expr_json(value, env, model, path, span, Some(expected))?,
                            ))
                        })
                        .collect::<Result<_, PublicKernelError>>()?,
                ),
            );
        }
        Expr::Var(name) => {
            output.insert("kind".to_owned(), json!("var"));
            output.insert("name".to_owned(), json!(name));
        }
        Expr::Call { name, .. } => {
            return Err(error(format!(
                "unlowered predicate call '{name}' in public Kernel"
            )));
        }
        Expr::Stage { .. } => {
            return Err(error("unlowered stage access in public Kernel"));
        }
        Expr::Index(collection, index) => {
            let collection_ty = resolve(model, &infer_type(collection, env, model, None)?)?;
            let key_ty = match collection_ty {
                TypeRef::Map(key, _) | TypeRef::Relation(key, _) => *key,
                TypeRef::Seq(_, _) => TypeRef::Int,
                _ => return Err(error("index expression requires collection")),
            };
            output.insert("kind".to_owned(), json!("index"));
            output.insert(
                "collection".to_owned(),
                expr_json(collection, env, model, path, span, None)?,
            );
            output.insert(
                "index".to_owned(),
                expr_json(index, env, model, path, span, Some(&key_ty))?,
            );
        }
        Expr::Field(value, field) => {
            output.insert("kind".to_owned(), json!("field"));
            output.insert(
                "value".to_owned(),
                expr_json(value, env, model, path, span, None)?,
            );
            output.insert("field".to_owned(), json!(field));
        }
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            let receiver_ty = resolve(model, &infer_type(receiver, env, model, None)?)?;
            let argument_type = match (name.as_str(), &receiver_ty) {
                ("contains" | "add" | "remove", TypeRef::Set(item))
                | ("contains" | "push", TypeRef::Seq(item, _)) => Some(item.as_ref()),
                ("at", TypeRef::Seq(_, _)) => Some(&TypeRef::Int),
                _ => None,
            };
            output.insert("kind".to_owned(), json!("method"));
            output.insert(
                "receiver".to_owned(),
                expr_json(receiver, env, model, path, span, None)?,
            );
            output.insert("method".to_owned(), json!(name));
            output.insert(
                "arguments".to_owned(),
                Value::Array(
                    args.iter()
                        .map(|arg| expr_json(arg, env, model, path, span, argument_type))
                        .collect::<Result<_, _>>()?,
                ),
            );
        }
        Expr::Binary { op, left, right } => {
            let (left_ty, right_ty) = if matches!(op.as_str(), "and" | "or" | "=>") {
                (TypeRef::Bool, TypeRef::Bool)
            } else if matches!(
                op.as_str(),
                "+" | "-" | "*" | "/" | "%" | "<" | "<=" | ">" | ">="
            ) {
                (TypeRef::Int, TypeRef::Int)
            } else {
                infer_type(left, env, model, None).map_or_else(
                    |_| infer_type(right, env, model, None).map(|ty| (ty.clone(), ty)),
                    |ty| Ok((ty.clone(), ty)),
                )?
            };
            output.insert("kind".to_owned(), json!("binary"));
            output.insert("operator".to_owned(), json!(op));
            let left_json = expr_json(left, env, model, path, span, Some(&left_ty))?;
            let mut right_env = env.clone();
            extend_pattern_binding(left, &mut right_env, model)?;
            output.insert("left".to_owned(), left_json);
            output.insert(
                "right".to_owned(),
                expr_json(right, &right_env, model, path, span, Some(&right_ty))?,
            );
        }
        Expr::Neg(operand) | Expr::Not(operand) => {
            let kind = if matches!(expr, Expr::Neg(_)) {
                "neg"
            } else {
                "not"
            };
            output.insert("kind".to_owned(), json!(kind));
            output.insert(
                "operand".to_owned(),
                expr_json(operand, env, model, path, span, None)?,
            );
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => {
            output.insert("kind".to_owned(), json!("ite"));
            output.insert(
                "condition".to_owned(),
                expr_json(
                    condition,
                    env,
                    model,
                    path,
                    spans.condition,
                    Some(&TypeRef::Bool),
                )?,
            );
            output.insert(
                "then".to_owned(),
                expr_json(then_expr, env, model, path, spans.then_expr, Some(&ty))?,
            );
            output.insert(
                "else".to_owned(),
                expr_json(else_expr, env, model, path, spans.else_expr, Some(&ty))?,
            );
        }
        Expr::Is { expr, pattern } => {
            output.insert("kind".to_owned(), json!("is"));
            output.insert(
                "operand".to_owned(),
                expr_json(expr, env, model, path, span, None)?,
            );
            output.insert(
                "pattern".to_owned(),
                match pattern {
                    Pattern::None => json!({"kind":"none","binding":Value::Null}),
                    Pattern::Some(binding) => json!({"kind":"some","binding":binding}),
                },
            );
        }
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => {
            let binder_ty = binder_type(binder, env, model)?;
            let name = match binder {
                Binder::Typed { name, .. }
                | Binder::Range { name, .. }
                | Binder::Collection { name, .. } => name,
            };
            let mut local = env.clone();
            local.insert(name.clone(), binder_ty);
            output.insert("kind".to_owned(), json!(quantifier));
            output.insert("quantifier".to_owned(), json!(quantifier));
            output.insert(
                "binder".to_owned(),
                binder_json(binder, env, model, path, span)?,
            );
            output.insert(
                "body".to_owned(),
                expr_json(body, &local, model, path, span, Some(&TypeRef::Bool))?,
            );
        }
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => {
            if matches!(kind, AggregateKind::Count | AggregateKind::Sum)
                && !matches!(binder, Binder::Typed { .. })
            {
                let normalized = normalize_aggregate(*kind, binder, value.as_deref(), env, model)?;
                return expr_json(&normalized, env, model, path, span, expected);
            }
            let name = match binder {
                Binder::Typed { name, .. }
                | Binder::Range { name, .. }
                | Binder::Collection { name, .. } => name,
            };
            let binder_ty = binder_type(binder, env, model)?;
            let mut local = env.clone();
            local.insert(name.clone(), binder_ty);
            output.insert(
                "kind".to_owned(),
                json!(match kind {
                    AggregateKind::Count => "count",
                    AggregateKind::Sum => "sum",
                    AggregateKind::Unique => "unique",
                    AggregateKind::ExactlyOne => "exactly_one",
                }),
            );
            if let Binder::Typed {
                type_name,
                where_expr,
                ..
            } = binder
                && matches!(kind, AggregateKind::Count | AggregateKind::Sum)
            {
                output.insert("binding".to_owned(), json!(name));
                output.insert("domain".to_owned(), json!(qualified_name(type_name)?));
                output.insert(
                    "condition".to_owned(),
                    match where_expr.as_deref() {
                        Some(condition) => {
                            expr_json(condition, &local, model, path, span, Some(&TypeRef::Bool))?
                        }
                        None if *kind == AggregateKind::Sum => Value::Null,
                        None => expr_json(
                            &Expr::Bool(true),
                            &local,
                            model,
                            path,
                            span,
                            Some(&TypeRef::Bool),
                        )?,
                    },
                );
            } else {
                output.insert(
                    "binder".to_owned(),
                    binder_json(binder, env, model, path, span)?,
                );
            }
            if let Some(value) = value {
                output.insert(
                    "value".to_owned(),
                    expr_json(value, &local, model, path, span, Some(&TypeRef::Int))?,
                );
            }
        }
        Expr::UnaryNamed { name, expr, .. } => {
            output.insert("kind".to_owned(), json!(name));
            output.insert(
                "operand".to_owned(),
                expr_json(expr, env, model, path, span, None)?,
            );
        }
        Expr::BinaryNamed { name, left, right } => {
            output.insert("kind".to_owned(), json!(name));
            output.insert(
                "arguments".to_owned(),
                json!([
                    expr_json(left, env, model, path, span, Some(&TypeRef::Int))?,
                    expr_json(right, env, model, path, span, Some(&TypeRef::Int))?
                ]),
            );
        }
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => {
            output.insert("kind".to_owned(), json!(name));
            output.insert(
                "relation".to_owned(),
                expr_json(first, env, model, path, span, None)?,
            );
            output.insert(
                "source".to_owned(),
                expr_json(second, env, model, path, span, None)?,
            );
            output.insert(
                "target".to_owned(),
                expr_json(third, env, model, path, span, None)?,
            );
        }
    }
    Ok(Value::Object(output))
}

fn lvalue_type(
    target: &LValue,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<TypeRef, PublicKernelError> {
    match target {
        LValue::Var(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| error(format!("unknown update target '{name}'"))),
        LValue::Index(name, _) => match resolve(
            model,
            env.get(name)
                .ok_or_else(|| error(format!("unknown update target '{name}'")))?,
        )? {
            TypeRef::Map(_, value) | TypeRef::Relation(_, value) => Ok(*value),
            _ => Err(error("indexed update target requires Map or Relation")),
        },
        LValue::Field(base, field) => {
            let TypeRef::Named(name) = lvalue_type(base, env, model)? else {
                return Err(error("field update target requires named struct"));
            };
            match model.types.get(&name) {
                Some(TypeDef::Struct { fields }) => fields
                    .iter()
                    .find_map(|(candidate, ty)| (candidate == field).then(|| ty.clone()))
                    .ok_or_else(|| error(format!("unknown struct field '{name}.{field}'"))),
                _ => Err(error("field update target requires struct")),
            }
        }
    }
}

fn lvalue_json(
    target: &LValue,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    span: Span,
) -> Result<Value, PublicKernelError> {
    let ty = lvalue_type(target, env, model)?;
    Ok(match target {
        LValue::Var(name) => json!({
            "kind":"var","type":type_json(&ty),"span":span_json(path,span),"name":name
        }),
        LValue::Index(name, index) => {
            let (TypeRef::Map(key, _) | TypeRef::Relation(key, _)) = resolve(
                model,
                env.get(name)
                    .ok_or_else(|| error(format!("unknown update target '{name}'")))?,
            )?
            else {
                return Err(error("indexed update target requires Map or Relation"));
            };
            json!({
                "kind":"index","type":type_json(&ty),"span":span_json(path,span),
                "name":name,"index":expr_json(index,env,model,path,span,Some(&key))?
            })
        }
        LValue::Field(base, field) => json!({
            "kind":"field_lv","type":type_json(&ty),"span":span_json(path,span),
            "target":lvalue_json(base,env,model,path,span)?,"field":field
        }),
    })
}

fn statement_json(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
) -> Result<Value, PublicKernelError> {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => {
            let ty = lvalue_type(target, env, model)?;
            ensure_assignable(value, &ty, env, model, *span)?;
            Ok(json!({
                "kind":"assign","type":{"kind":"statement"},"span":span_json(path,*span),
                "target":lvalue_json(target,env,model,path,*span)?,
                "value":expr_json(value,env,model,path,*span,Some(&ty))?
            }))
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => Ok(json!({
            "kind":"if","type":{"kind":"statement"},"span":span_json(path,*span),
            "condition":expr_json(condition,env,model,path,*span,Some(&TypeRef::Bool))?,
            "then":then_statements.iter().map(|item|statement_json(item,env,model,path)).collect::<Result<Vec<_>,_>>()?,
            "else":else_statements.iter().map(|item|statement_json(item,env,model,path)).collect::<Result<Vec<_>,_>>()?
        })),
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let ty = binder_type(binder, env, model)?;
            let name = match binder {
                Binder::Typed { name, .. }
                | Binder::Range { name, .. }
                | Binder::Collection { name, .. } => name,
            };
            let mut local = env.clone();
            local.insert(name.clone(), ty);
            Ok(json!({
                "kind":"forall","type":{"kind":"statement"},"span":span_json(path,*span),
                "binder":binder_json(binder,env,model,path,*span)?,
                "statements":statements.iter().map(|item|statement_json(item,&local,model,path)).collect::<Result<Vec<_>,_>>()?
            }))
        }
    }
}

/// Run the public Kernel expression/type checker for one normalized statement.
///
/// Inline state initializers lower to ordinary assignments, so keeping this
/// entry point beside `statement_json` prevents their validation rules from
/// drifting away from the existing normalized Kernel contract.
pub(crate) fn validate_statement_types(
    statement: &Statement,
    model: &KernelModel,
) -> Result<(), PublicKernelError> {
    validate_statement_assignments(statement, &base_env(model), model)
}

/// Run the shared expression/type checker over every non-init model expression.
pub(crate) fn validate_model_expression_types(
    model: &KernelModel,
) -> Result<(), PublicKernelError> {
    let env = base_env(model);
    for action in &model.actions {
        let mut local = env.clone();
        for param in &action.params {
            match param {
                ParamDef::Typed { name, ty } => {
                    local.insert(name.clone(), ty.clone());
                }
                ParamDef::Range { name, lo, hi } => {
                    local.insert(name.clone(), TypeRef::Range(*lo, *hi));
                }
            }
        }
        for guard in &action.guards {
            match guard {
                ActionGuard::Requires(expr) => {
                    expr_json(expr, &local, model, "", action.span, Some(&TypeRef::Bool))?;
                    extend_pattern_binding(expr, &mut local, model)?;
                }
                ActionGuard::Let(name, expr) => {
                    expr_json(expr, &local, model, "", action.span, None)?;
                    local.insert(name.clone(), infer_type(expr, &local, model, None)?);
                }
            }
        }
        for statement in &action.statements {
            statement_json(statement, &local, model, "")?;
        }
        for expr in &action.ensures {
            expr_json(expr, &local, model, "", action.span, Some(&TypeRef::Bool))?;
        }
    }
    for property in model
        .invariants
        .iter()
        .chain(&model.transitions)
        .chain(&model.reachables)
    {
        expr_json(
            &property.expr,
            &env,
            model,
            "",
            property.span,
            Some(&TypeRef::Bool),
        )?;
    }
    for leadsto in &model.leadstos {
        let mut local = env.clone();
        for binder in &leadsto.binders {
            binder_json(binder, &local, model, "", leadsto.span)?;
            let name = match binder {
                Binder::Typed { name, .. }
                | Binder::Range { name, .. }
                | Binder::Collection { name, .. } => name,
            };
            local.insert(name.clone(), binder_type(binder, &local, model)?);
        }
        expr_json(
            &leadsto.before,
            &local,
            model,
            "",
            leadsto.span,
            Some(&TypeRef::Bool),
        )?;
        expr_json(
            &leadsto.after,
            &local,
            model,
            "",
            leadsto.span,
            Some(&TypeRef::Bool),
        )?;
        if let Some(expr) = &leadsto.decreases {
            expr_json(expr, &local, model, "", leadsto.span, Some(&TypeRef::Int))?;
        }
    }
    if let Some(expr) = &model.terminal {
        let position = SourcePos {
            offset: 0,
            line: 1,
            column: 1,
        };
        expr_json(
            expr,
            &env,
            model,
            "",
            Span {
                start: position,
                end: position,
            },
            Some(&TypeRef::Bool),
        )?;
    }
    Ok(())
}

fn validate_statement_assignments(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<(), PublicKernelError> {
    match statement {
        Statement::Assign { .. } => statement_json(statement, env, model, "").map(|_| ()),
        Statement::If {
            then_statements,
            else_statements,
            ..
        } => then_statements
            .iter()
            .chain(else_statements)
            .try_for_each(|item| validate_statement_assignments(item, env, model)),
        Statement::ForAll {
            binder, statements, ..
        } => {
            let (name, ty) = match binder {
                Binder::Typed {
                    name, type_name, ..
                } => (name, TypeRef::Named(qualified_name(type_name)?)),
                Binder::Range { name, .. } => (name, TypeRef::Int),
                Binder::Collection {
                    name, collection, ..
                } => {
                    let collection_ty = resolve(model, &infer_type(collection, env, model, None)?)?;
                    let (TypeRef::Set(item) | TypeRef::Seq(item, _)) = collection_ty else {
                        return Err(error("collection binder requires Set or Seq"));
                    };
                    (name, *item)
                }
            };
            let mut local = env.clone();
            local.insert(name.clone(), ty);
            statements
                .iter()
                .try_for_each(|item| validate_statement_assignments(item, &local, model))
        }
    }
}

fn requirement(meta: Option<&MetaTag>) -> Value {
    meta.map_or(Value::Null, |meta| json!({"id":meta.id,"text":meta.text}))
}

fn origin(meta: Option<&MetaTag>, dialect: &str, name: &str, generated: bool) -> Value {
    json!({
        "dialect":dialect,
        "declaration":meta.map_or(name,|meta|meta.id.as_str()),
        "lowered":dialect != "kernel",
        "generated":generated || name.starts_with('_'),
    })
}

#[allow(clippy::too_many_lines)]
fn walk_partial(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    span: Span,
    path_condition: Option<&Expr>,
    output: &mut Vec<Value>,
) -> Result<(), PublicKernelError> {
    if let Expr::Method {
        receiver,
        name,
        args,
    } = expr
        && matches!(name.as_str(), "head" | "pop" | "at")
    {
        let size = Expr::Method {
            receiver: receiver.clone(),
            name: "size".to_owned(),
            args: Vec::new(),
        };
        let failure = if name == "at" {
            Expr::Binary {
                op: "or".to_owned(),
                left: Box::new(Expr::Binary {
                    op: "<".to_owned(),
                    left: Box::new(args[0].clone()),
                    right: Box::new(Expr::Num(0)),
                }),
                right: Box::new(Expr::Binary {
                    op: ">=".to_owned(),
                    left: Box::new(args[0].clone()),
                    right: Box::new(size),
                }),
            }
        } else {
            Expr::Binary {
                op: "==".to_owned(),
                left: Box::new(size),
                right: Box::new(Expr::Num(0)),
            }
        };
        let failure = guard_failure(failure, path_condition);
        output.push(json!({
            "operation":name,
            "failure_condition":expr_json(&failure,env,model,path,span,Some(&TypeRef::Bool))?,
            "state_effect_on_failure":"rollback",
            "span":span_json(path,span),
        }));
    }
    if let Expr::Index(collection, index) = expr
        && matches!(
            resolve(model, &infer_type(collection, env, model, None)?)?,
            TypeRef::Seq(_, _)
        )
    {
        let size = Expr::Method {
            receiver: collection.clone(),
            name: "size".to_owned(),
            args: Vec::new(),
        };
        let failure = Expr::Binary {
            op: "or".to_owned(),
            left: Box::new(Expr::Binary {
                op: "<".to_owned(),
                left: index.clone(),
                right: Box::new(Expr::Num(0)),
            }),
            right: Box::new(Expr::Binary {
                op: ">=".to_owned(),
                left: index.clone(),
                right: Box::new(size),
            }),
        };
        let failure = guard_failure(failure, path_condition);
        output.push(json!({
            "operation":"index",
            "failure_condition":expr_json(&failure,env,model,path,span,Some(&TypeRef::Bool))?,
            "state_effect_on_failure":"rollback",
            "span":span_json(path,span),
        }));
    }
    if let Expr::Binary { op, right, .. } = expr
        && matches!(op.as_str(), "/" | "%")
    {
        let failure = Expr::Binary {
            op: "==".to_owned(),
            left: right.clone(),
            right: Box::new(Expr::Num(0)),
        };
        let failure = guard_failure(failure, path_condition);
        output.push(json!({
            "operation":if op == "/" {"divide"} else {"remainder"},
            "failure_condition":expr_json(&failure,env,model,path,span,Some(&TypeRef::Bool))?,
            "state_effect_on_failure":"rollback",
            "span":span_json(path,span),
        }));
    }
    if let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
        ..
    } = expr
    {
        walk_partial(condition, env, model, path, span, path_condition, output)?;
        let then_path = extend_path_condition(path_condition, condition.as_ref(), false);
        walk_partial(then_expr, env, model, path, span, Some(&then_path), output)?;
        let else_path = extend_path_condition(path_condition, condition.as_ref(), true);
        walk_partial(else_expr, env, model, path, span, Some(&else_path), output)?;
        return Ok(());
    }
    if let Expr::Quantified {
        quantifier,
        binder,
        body,
    } = expr
    {
        walk_quantified_partial(
            quantifier,
            binder,
            body,
            env,
            model,
            path,
            span,
            path_condition,
            output,
        )?;
        return Ok(());
    }
    if let Expr::Aggregate {
        kind,
        binder,
        value,
    } = expr
    {
        let normalized = normalize_aggregate(*kind, binder, value.as_deref(), env, model)?;
        walk_partial(&normalized, env, model, path, span, path_condition, output)?;
        return Ok(());
    }
    let mut children: Vec<&Expr> = Vec::new();
    match expr {
        Expr::Some(item) | Expr::Neg(item) | Expr::Not(item) => children.push(item),
        Expr::Set(items) | Expr::Seq(items) => children.extend(items),
        Expr::Struct { fields, .. } => children.extend(fields.iter().map(|(_, item)| item)),
        Expr::Index(left, right)
        | Expr::Binary { left, right, .. }
        | Expr::BinaryNamed { left, right, .. } => {
            children.push(left);
            children.push(right);
        }
        Expr::Field(value, _)
        | Expr::Stage { entity: value, .. }
        | Expr::UnaryNamed { expr: value, .. } => children.push(value),
        Expr::Method { receiver, args, .. } => {
            children.push(receiver);
            children.extend(args);
        }
        Expr::Conditional { .. } | Expr::Quantified { .. } | Expr::Aggregate { .. } => {
            unreachable!("handled above")
        }
        Expr::Is { expr, .. } => children.push(expr),
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => children.extend([first.as_ref(), second.as_ref(), third.as_ref()]),
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::Call { .. } => {}
    }
    for child in children {
        walk_partial(child, env, model, path, span, path_condition, output)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn walk_quantified_partial(
    quantifier: &str,
    binder: &Binder,
    body: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    span: Span,
    path_condition: Option<&Expr>,
    output: &mut Vec<Value>,
) -> Result<(), PublicKernelError> {
    let (name, candidates, filter) = finite_binder_candidates(binder, env, model)?;
    let mut continuation = path_condition.cloned();
    for (candidate, membership) in candidates {
        let replacements = HashMap::from([(name.clone(), candidate)]);
        let effective = aggregate_condition(membership, filter.as_ref(), &replacements);
        let selected = crate::substitute_expr(body.clone(), &replacements);
        let candidate_result = Expr::Conditional {
            condition: Box::new(effective.clone()),
            then_expr: Box::new(selected.clone()),
            else_expr: Box::new(Expr::Bool(quantifier == "forall")),
            spans: Box::new(ConditionalSpans {
                condition: unknown_span(),
                then_expr: unknown_span(),
                else_expr: unknown_span(),
            }),
        };
        walk_partial(
            &candidate_result,
            env,
            model,
            path,
            span,
            continuation.as_ref(),
            output,
        )?;
        let selected_continues = if quantifier == "forall" {
            selected
        } else {
            Expr::Not(Box::new(selected))
        };
        let candidate_continues = Expr::Conditional {
            condition: Box::new(effective),
            then_expr: Box::new(selected_continues),
            else_expr: Box::new(Expr::Bool(true)),
            spans: Box::new(ConditionalSpans {
                condition: unknown_span(),
                then_expr: unknown_span(),
                else_expr: unknown_span(),
            }),
        };
        continuation = Some(match continuation {
            Some(previous) => Expr::Binary {
                op: "and".to_owned(),
                left: Box::new(previous),
                right: Box::new(candidate_continues),
            },
            None => candidate_continues,
        });
    }
    Ok(())
}

fn guard_failure(failure: Expr, path_condition: Option<&Expr>) -> Expr {
    match path_condition {
        Some(condition) => Expr::Binary {
            op: "and".to_owned(),
            left: Box::new(condition.clone()),
            right: Box::new(failure),
        },
        None => failure,
    }
}

fn extend_path_condition(path: Option<&Expr>, condition: &Expr, negated: bool) -> Expr {
    let condition = if negated {
        Expr::Not(Box::new(condition.clone()))
    } else {
        condition.clone()
    };
    path.map_or(condition.clone(), |path| Expr::Binary {
        op: "and".to_owned(),
        left: Box::new(path.clone()),
        right: Box::new(condition),
    })
}

fn statement_partial(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
    path: &str,
    output: &mut Vec<Value>,
) -> Result<(), PublicKernelError> {
    match statement {
        Statement::Assign { value, span, .. } => {
            walk_partial(value, env, model, path, *span, None, output)?;
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            walk_partial(condition, env, model, path, *span, None, output)?;
            for item in then_statements.iter().chain(else_statements) {
                statement_partial(item, env, model, path, output)?;
            }
        }
        Statement::ForAll {
            statements, span, ..
        } => {
            for item in statements {
                statement_partial(item, env, model, path, output)?;
            }
            let _ = span;
        }
    }
    Ok(())
}

fn source_property_kinds(kernel: &KernelSpec) -> BTreeMap<String, String> {
    let mut kinds = BTreeMap::new();
    for item in &kernel.syntax().items {
        match item {
            SpecItem::Invariant { name, .. } => {
                kinds.insert(name.clone(), "invariant".to_owned());
            }
            SpecItem::Trans { name, .. } => {
                kinds.insert(name.clone(), "trans".to_owned());
            }
            SpecItem::Reachable { name, .. } => {
                kinds.insert(name.clone(), "reachable".to_owned());
            }
            SpecItem::LeadsTo { name, .. } => {
                kinds.insert(name.clone(), "leadsto".to_owned());
            }
            SpecItem::Unless { name, .. } => {
                kinds.insert(name.clone(), "unless".to_owned());
            }
            SpecItem::Until { name, .. } => {
                kinds.insert(name.clone(), "until".to_owned());
                kinds.insert(format!("{name}_until_safety"), "until".to_owned());
            }
            _ => {}
        }
    }
    kinds
}

fn portable_source_identity(raw: &str) -> Result<(String, &'static str), PublicKernelError> {
    let normalized = raw.replace('\\', "/");
    let windows_drive = normalized
        .as_bytes()
        .get(1)
        .is_some_and(|separator| *separator == b':');
    let uri = normalized.split_once(':').is_some_and(|(scheme, _)| {
        !windows_drive
            && scheme
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    });
    if uri {
        if normalized.to_ascii_lowercase().starts_with("file:") {
            return Err(error(
                "public Kernel v2 source identity must not be a developer-local file URI",
            ));
        }
        return Ok((normalized, "uri"));
    }
    if normalized.starts_with('/') || windows_drive {
        return Err(error(format!(
            "public Kernel v2 source identity '{raw}' must be repository-relative or a portable URI"
        )));
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                return Err(error(format!(
                    "public Kernel v2 source identity '{raw}' must not escape its repository"
                )));
            }
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return Err(error("public Kernel v2 source identity must not be empty"));
    }
    Ok((parts.join("/"), "repository_path"))
}

fn v2_span_json(span: Span) -> Value {
    json!({
        "byte_start":span.start.offset,
        "byte_end":span.end.offset,
        "line":span.start.line,
        "column":span.start.column,
        "end_line":span.end.line,
        "end_column":span.end.column,
    })
}

fn origin_site_v2_json(site: &OriginSite) -> Result<Value, PublicKernelError> {
    let source = site
        .source_file
        .as_deref()
        .map(portable_source_identity)
        .transpose()?
        .map_or(
            Value::Null,
            |(value, kind)| json!({"kind":kind,"value":value}),
        );
    Ok(json!({
        "source":source,
        "span":site.span.map_or(Value::Null,v2_span_json),
        "dialect":site.dialect,
        "declaration_path":site.declaration_path,
    }))
}

fn public_origin_id(origin: &OriginChain) -> Result<String, PublicKernelError> {
    let prefix = origin
        .primary
        .as_ref()
        .and_then(|site| site.source_file.as_deref())
        .map(portable_source_identity)
        .transpose()?
        .map_or_else(
            || "source:unknown".to_owned(),
            |(value, kind)| format!("source:{kind}:{value}"),
        );
    Ok(format!("{prefix}#{}", origin.id.0))
}

fn origin_assurance(origin: &OriginChain) -> &'static str {
    let source_backed = origin
        .primary
        .as_ref()
        .is_some_and(|site| site.source_file.as_ref().is_some() && site.span.is_some());
    match (origin.generated, source_backed) {
        (true, true) => "generated_from_source",
        (true, false) => "generated_only",
        (false, true) => "source_backed",
        (false, false) => "unknown",
    }
}

fn origin_v2_json(origin: &OriginChain) -> Result<Value, PublicKernelError> {
    let assurance = origin_assurance(origin);
    let mut secondary = origin
        .secondary
        .iter()
        .map(origin_site_v2_json)
        .collect::<Result<Vec<_>, _>>()?;
    secondary.sort_by_key(|site| serde_json::to_string(site).unwrap_or_default());
    secondary.dedup();
    let mut seen_steps = BTreeSet::new();
    let lowering_steps = origin
        .lowering_steps
        .iter()
        .filter_map(|step| {
            let value = json!({"kind":step.kind,"detail":step.detail});
            let key = serde_json::to_string(&value).ok()?;
            seen_steps.insert(key).then_some(value)
        })
        .collect::<Vec<_>>();
    let primary = if assurance == "unknown" || assurance == "generated_only" {
        Value::Null
    } else {
        origin
            .primary
            .as_ref()
            .map(origin_site_v2_json)
            .transpose()?
            .unwrap_or(Value::Null)
    };
    Ok(json!({
        "kind":"source_chain",
        "id":public_origin_id(origin)?,
        "dialect":origin.dialect,
        "assurance":assurance,
        "primary":primary,
        "secondary":secondary,
        "lowering_steps":lowering_steps,
        "generated":origin.generated,
        "extensions":{},
    }))
}

fn unknown_origin(target: &str, dialect: &str) -> OriginChain {
    OriginChain {
        id: crate::OriginId(format!("unknown:{target}")),
        dialect: dialect.to_owned(),
        primary: None,
        secondary: Vec::new(),
        lowering_steps: Vec::new(),
        generated: false,
    }
}

fn set_origin_target(value: &mut Value, target: &str, required: &mut BTreeSet<String>) {
    required.insert(target.to_owned());
    value
        .as_object_mut()
        .expect("public Kernel node object")
        .insert("origin".to_owned(), json!({"target":target}));
}

fn retarget_v2_origins(contract: &mut Value) -> BTreeSet<String> {
    let root = contract.as_object_mut().expect("public Kernel object");
    let mut required = BTreeSet::new();
    for item in root["constants"].as_array_mut().expect("constants") {
        let name = item["name"].as_str().expect("constant name");
        set_origin_target(item, &format!("constant:{name}"), &mut required);
    }
    for item in root["types"].as_array_mut().expect("types") {
        let name = item["name"].as_str().expect("type name").to_owned();
        set_origin_target(item, &type_target(&name), &mut required);
    }
    for item in root["state"].as_array_mut().expect("state") {
        let name = item["name"].as_str().expect("state name").to_owned();
        set_origin_target(item, &state_target(&name), &mut required);
    }
    set_origin_target(&mut root["init"], "init", &mut required);
    for item in root["actions"].as_array_mut().expect("actions") {
        let name = item["name"].as_str().expect("action name").to_owned();
        set_origin_target(item, &action_target(&name), &mut required);
    }
    let properties = root["properties"].as_object_mut().expect("properties");
    for (field, kind) in [
        ("invariants", "invariant"),
        ("transitions", "trans"),
        ("reachables", "reachable"),
        ("leads_to", "leadsto"),
    ] {
        for item in properties[field].as_array_mut().expect("property list") {
            let name = item["name"].as_str().expect("property name").to_owned();
            set_origin_target(item, &property_target(kind, &name), &mut required);
        }
    }
    required
}

fn provenance_v2_json(
    model: &KernelModel,
    required_targets: &BTreeSet<String>,
    dialect: &str,
) -> Result<Value, PublicKernelError> {
    let mut target_origins = model
        .origins()
        .targets()
        .map(|(target, origins)| (target.to_owned(), origins.to_vec()))
        .collect::<BTreeMap<_, _>>();
    for target in required_targets {
        target_origins
            .entry(target.clone())
            .or_insert_with(|| vec![unknown_origin(target, dialect)]);
    }

    let mut origins = BTreeMap::<String, Value>::new();
    let mut bindings = Vec::new();
    let mut reverse = BTreeMap::<String, BTreeSet<String>>::new();
    for (target, chains) in target_origins {
        let mut ids = BTreeSet::new();
        for (index, chain) in chains.iter().enumerate() {
            let source_node_id = public_origin_id(chain)?;
            let id = format!("{source_node_id}@{target}:{index}");
            let mut record = origin_v2_json(chain)?;
            record["id"] = json!(id);
            record["source_node_id"] = json!(source_node_id);
            origins.insert(id.clone(), record);
            reverse
                .entry(source_node_id)
                .or_default()
                .insert(target.clone());
            ids.insert(id);
        }
        bindings.push(json!({"target":target,"origin_ids":ids.into_iter().collect::<Vec<_>>()}));
    }
    let origin_values = origins.into_values().collect::<Vec<_>>();
    let known = origin_values
        .iter()
        .filter(|origin| origin["assurance"] != "unknown")
        .count();
    let unknown = origin_values.len() - known;
    let completeness = match (known, unknown) {
        (0, _) => "unknown",
        (_, 0) => "complete",
        _ => "partial",
    };
    Ok(json!({
        "schema_version":"2.0.0",
        "identity_stability":"exact_source_revision",
        "completeness":completeness,
        "assurance_counts":{"known":known,"unknown":unknown},
        "coordinates":{
            "bytes":"utf8_zero_based_half_open",
            "lines":"unicode_scalar_one_based_end_exclusive"
        },
        "origins":origin_values,
        "bindings":bindings,
        "reverse_index":reverse.into_iter().map(|(source_node_id,targets)|json!({
            "source_node_id":source_node_id,"targets":targets.into_iter().collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
    }))
}

/// Project a checked, lowered model into the stable public Kernel JSON value.
///
/// # Errors
///
/// Returns [`PublicKernelError`] instead of dropping any expression or type
/// that the public schema cannot represent.
#[allow(clippy::too_many_lines)]
pub fn public_kernel_contract(
    kernel: &KernelSpec,
    model: &KernelModel,
    source_path: &str,
    dialect: &str,
) -> Result<Value, PublicKernelError> {
    if dialect == "compose" {
        return Err(error(
            "public Kernel v1 cannot preserve component source filenames after compose lowering",
        ));
    }
    let env = base_env(model);
    let kinds = source_property_kinds(kernel);
    let constants = model
        .consts
        .iter()
        .map(|(name, value)| {
            let (ty, value) = match value {
                crate::FslValue::Int(value) => (type_json(&TypeRef::Int), json!(value)),
                crate::FslValue::Bool(value) => (type_json(&TypeRef::Bool), json!(value)),
                _ => return Err(error("public Kernel constants must be scalar")),
            };
            Ok(json!({
                "name":name,"type":ty,"value":value,"span":Value::Null,
                "origin":origin(None,dialect,name,false)
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let types = model
        .types
        .iter()
        .map(|(name, definition)| {
            let (symmetric, definition) = match definition {
                TypeDef::Domain { lo, hi, symmetric } => {
                    (*symmetric, json!({"kind":"domain","lo":lo,"hi":hi}))
                }
                TypeDef::Enum { members, symmetric } => {
                    (*symmetric, json!({"kind":"enum","members":members}))
                }
                TypeDef::Struct { fields } => {
                    let mut fields = fields.clone();
                    fields.sort_by(|left, right| left.0.cmp(&right.0));
                    (false, json!({
                        "kind":"struct",
                        "fields":fields.iter().map(|(field,ty)|json!({"name":field,"type":type_json(ty)})).collect::<Vec<_>>()
                    }))
                }
            };
            json!({
                "name":name,"span":Value::Null,"symmetric":symmetric,"definition":definition,
                "origin":origin(None,dialect,name,false)
            })
        })
        .collect::<Vec<_>>();
    let mut state = model.state.clone();
    state.sort_by(|left, right| left.0.cmp(&right.0));
    let state = state
        .iter()
        .map(|(name, ty)| {
            json!({
                "name":name,"type":type_json(ty),"span":Value::Null,
                "origin":origin(None,dialect,name,false)
            })
        })
        .collect::<Vec<_>>();
    let init = model
        .init
        .iter()
        .map(|statement| statement_json(statement, &env, model, source_path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut action_refs = model.actions.iter().collect::<Vec<_>>();
    action_refs.sort_by(|left, right| left.name.cmp(&right.name));
    let actions = action_refs
        .into_iter()
        .map(|action| {
            let mut local = env.clone();
            let parameters = action
                .params
                .iter()
                .map(|param| match param {
                    ParamDef::Typed { name, ty } => {
                        local.insert(name.clone(), ty.clone());
                        let (lo, hi) = model
                            .domain_values(ty)
                            .map_err(|err| error(err.message))?
                            .into_iter()
                            .enumerate()
                            .fold((i64::MAX, i64::MIN), |(lo, hi), (index, value)| {
                                let value = match value {
                                    crate::FslValue::Int(value) => value,
                                    crate::FslValue::Bool(value) => i64::from(value),
                                    crate::FslValue::Enum { .. } => i64::try_from(index).unwrap_or_default(),
                                    _ => 0,
                                };
                                (lo.min(value), hi.max(value))
                            });
                        Ok(json!({"name":name,"type":type_json(ty),"finite_domain":{"lo":lo,"hi":hi}}))
                    }
                    ParamDef::Range { name, lo, hi } => {
                        let ty = TypeRef::Range(*lo, *hi);
                        local.insert(name.clone(), ty.clone());
                        Ok(json!({"name":name,"type":type_json(&ty),"finite_domain":{"lo":lo,"hi":hi}}))
                    }
                })
                .collect::<Result<Vec<_>, PublicKernelError>>()?;
            let mut requires = Vec::new();
            let mut lets = Vec::new();
            let mut guards = Vec::new();
            let mut partial = Vec::new();
            let mut require_spans = action.require_spans.iter();
            for guard in &action.guards {
                match guard {
                    ActionGuard::Requires(expression) => {
                        let span = *require_spans.next().ok_or_else(|| {
                            error(format!("missing requires span for action '{}'", action.name))
                        })?;
                        let value = expr_json(
                            expression,
                            &local,
                            model,
                            source_path,
                            span,
                            Some(&TypeRef::Bool),
                        )?;
                        walk_partial(
                            expression,
                            &local,
                            model,
                            source_path,
                            span,
                            None,
                            &mut partial,
                        )?;
                        requires.push(value.clone());
                        guards.push(json!({"kind":"requires","expression":value}));
                        extend_pattern_binding(expression, &mut local, model)?;
                    }
                    ActionGuard::Let(name, expression) => {
                        let value = expr_json(
                            expression,
                            &local,
                            model,
                            source_path,
                            action.span,
                            None,
                        )?;
                        walk_partial(
                            expression,
                            &local,
                            model,
                            source_path,
                            action.span,
                            None,
                            &mut partial,
                        )?;
                        let ty = infer_type(expression, &local, model, None)?;
                        local.insert(name.clone(), ty);
                        lets.push(json!({"name":name,"value":value}));
                        guards.push(json!({"kind":"let","name":name,"value":value}));
                    }
                }
            }
            let updates = action
                .statements
                .iter()
                .map(|statement| statement_json(statement, &local, model, source_path))
                .collect::<Result<Vec<_>, _>>()?;
            let ensures = action
                .ensures
                .iter()
                .zip(&action.ensure_spans)
                .map(|(expression, span)| {
                    expr_json(
                        expression,
                        &local,
                        model,
                        source_path,
                        *span,
                        Some(&TypeRef::Bool),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            for expression in &action.ensures {
                walk_partial(
                    expression,
                    &local,
                    model,
                    source_path,
                    action.span,
                    None,
                    &mut partial,
                )?;
            }
            for statement in &action.statements {
                statement_partial(
                    statement,
                    &local,
                    model,
                    source_path,
                    &mut partial,
                )?;
            }
            Ok(json!({
                "name":action.name,"parameters":parameters,"fair":action.fair,
                "guards":guards,"requires":requires,"lets":lets,"updates":updates,
                "update_semantics":"simultaneous","ensures":ensures,
                "partial_operations":partial,"requirement":requirement(action.meta.as_ref()),
                "origin":origin(action.meta.as_ref(),dialect,&action.name,false),
                "span":span_json(source_path,action.span),
            }))
        })
        .collect::<Result<Vec<_>, PublicKernelError>>()?;
    let property_list = |items: &[crate::PropertyDef], default_kind: &str| {
        let mut items = items.iter().collect::<Vec<_>>();
        items.sort_by(|left, right| left.name.cmp(&right.name));
        items
            .into_iter()
            .map(|item| {
                Ok(json!({
                    "name":item.name,
                    "source_kind":kinds.get(&item.name).map_or(default_kind,String::as_str),
                    "expression":expr_json(&item.expr,&env,model,source_path,item.span,Some(&TypeRef::Bool))?,
                    "requirement":requirement(item.meta.as_ref()),
                    "origin":origin(item.meta.as_ref(),dialect,&item.name,false),
                    "span":span_json(source_path,item.span),
                }))
            })
            .collect::<Result<Vec<_>, PublicKernelError>>()
    };
    let mut leads = model.leadstos.iter().collect::<Vec<_>>();
    leads.sort_by(|left, right| left.name.cmp(&right.name));
    let leads = leads
        .into_iter()
        .map(|item| {
            let mut local = env.clone();
            let mut binders = Vec::new();
            for binder in &item.binders {
                binders.push(binder_json(binder, &local, model, source_path, item.span)?);
                let name = match binder {
                    Binder::Typed { name, .. }
                    | Binder::Range { name, .. }
                    | Binder::Collection { name, .. } => name,
                };
                local.insert(name.clone(), binder_type(binder, &local, model)?);
            }
            Ok(json!({
                "name":item.name,
                "source_kind":kinds.get(&item.name).map_or("leadsTo",String::as_str),
                "binders":binders,
                "before":expr_json(&item.before,&local,model,source_path,item.span,Some(&TypeRef::Bool))?,
                "after":expr_json(&item.after,&local,model,source_path,item.span,Some(&TypeRef::Bool))?,
                "within":item.within,
                "decreases":item.decreases.as_ref().map_or(Ok(Value::Null),|expr|expr_json(expr,&local,model,source_path,item.span,Some(&TypeRef::Int)))?,
                "requirement":requirement(item.meta.as_ref()),
                "origin":origin(item.meta.as_ref(),dialect,&item.name,false),
                "span":span_json(source_path,item.span),
            }))
        })
        .collect::<Result<Vec<_>, PublicKernelError>>()?;
    let terminal_span = kernel.syntax().items.iter().find_map(|item| match item {
        SpecItem::Terminal { span, .. } => Some(*span),
        _ => None,
    });
    let terminal = match (&model.terminal, terminal_span) {
        (Some(expression), Some(span)) => json!({
            "source_kind":"terminal",
            "expression":expr_json(expression,&env,model,source_path,span,Some(&TypeRef::Bool))?,
            "span":span_json(source_path,span),
        }),
        _ => Value::Null,
    };
    Ok(json!({
        "$schema":KERNEL_SCHEMA_ID,
        "schema_version":KERNEL_SCHEMA_VERSION,
        "language_version":"1.0",
        "spec":{"name":model.name,"source":{"file":source_path,"dialect":dialect}},
        "semantics":{
            "assignment":"simultaneous","reads":"pre_state","requires_false":"not_enabled",
            "failure_state":"rollback","old":"pre_state","integer_division":"euclidean",
            "terminal_deadlock":"terminal_states_excluded","fairness":"weak"
        },
        "constants":constants,"types":types,"state":state,
        "init":{
            "statements":init,
            "requirement":requirement(model.init_meta.as_ref()),
            "origin":origin(model.init_meta.as_ref(),dialect,"init",false),
            "span":model.init.first().map_or(Value::Null,|statement|span_json(source_path,statement_span(statement)))
        },
        "actions":actions,
        "properties":{
            "invariants":property_list(&model.invariants,"invariant")?,
            "transitions":property_list(&model.transitions,"trans")?,
            "reachables":property_list(&model.reachables,"reachable")?,
            "leads_to":leads,"terminal":terminal,
        }
    }))
}

/// Project a checked model into an explicitly negotiated Public Kernel major.
///
/// The legacy [`public_kernel_contract`] entrypoint remains a v1-only alias so
/// existing Rust callers and the default CLI output cannot change silently.
///
/// # Errors
///
/// Returns [`PublicKernelError`] when the selected major cannot truthfully
/// represent the model or a source identity is not portable.
pub fn public_kernel_contract_for_version(
    kernel: &KernelSpec,
    model: &KernelModel,
    source_path: &str,
    dialect: &str,
    version: PublicKernelVersion,
) -> Result<Value, PublicKernelError> {
    if version == PublicKernelVersion::V1 {
        return public_kernel_contract(kernel, model, source_path, dialect);
    }
    if dialect == "compose" {
        return Err(error(
            "public Kernel v2 cannot preserve component source filenames after compose lowering",
        ));
    }
    let (source_path, _) = portable_source_identity(source_path)?;
    let mut contract = public_kernel_contract(kernel, model, &source_path, dialect)?;
    let required_targets = retarget_v2_origins(&mut contract);
    let provenance = provenance_v2_json(model, &required_targets, dialect)?;
    let root = contract
        .as_object_mut()
        .ok_or_else(|| error("public Kernel v1 projection did not produce an object"))?;
    root.insert("$schema".to_owned(), json!(KERNEL_V2_SCHEMA_ID));
    root.insert("schema_version".to_owned(), json!(KERNEL_V2_SCHEMA_VERSION));
    root.insert("provenance".to_owned(), provenance);
    Ok(contract)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_model, parse_direct_kernel_spec};

    #[test]
    fn unsupported_calls_fail_instead_of_disappearing() {
        let source = "spec S { state { x: Int } init { x = 0 } action a() { x = 0 } invariant I { x == 0 } }";
        let kernel = parse_direct_kernel_spec(source).expect("parse");
        let model = build_model(kernel.clone()).expect("model");
        let contract =
            public_kernel_contract(&kernel, &model, "s.fsl", "kernel").expect("contract");
        assert_eq!(contract["schema_version"], "1.0.0");
    }

    #[test]
    fn option_equality_uses_existing_binary_and_constructor_nodes() {
        let source = "spec S { type K = 0..1 state { x: Option<K> } init { x = some(0) } action clear() { requires x == some(0) x = none } invariant I { true } }";
        let kernel = parse_direct_kernel_spec(source).expect("parse");
        let model = build_model(kernel.clone()).expect("model");
        let contract =
            public_kernel_contract(&kernel, &model, "s.fsl", "kernel").expect("contract");
        let expression = &contract["actions"][0]["guards"][0]["expression"];

        assert_eq!(expression["kind"], "binary");
        assert_eq!(expression["operator"], "==");
        assert_eq!(expression["left"]["type"]["kind"], "option");
        assert_eq!(expression["right"]["kind"], "some");
        assert_eq!(expression["right"]["operand"]["kind"], "num");
    }

    #[test]
    fn option_equality_rejects_mismatched_inner_types() {
        let source = "spec S { enum A { A1 } enum B { B1 } state { a: Option<A>, b: Option<B> } init { a = none b = none } action stay() { a = a b = b } invariant Bad { a == b } }";
        let kernel = parse_direct_kernel_spec(source).expect("parse");
        let error = build_model(kernel).expect_err("mismatched Option payloads must fail check");

        assert!(error.message.contains("is not assignable"));
    }

    #[test]
    fn conditional_partial_operation_is_guarded_by_its_branch() {
        let source = "spec S { type N = 0..1 state { x: N, gate: Bool } init { x = 0 gate = true } action choose() { x = if gate then 1 else 1 / 0 } invariant I { true } }";
        let kernel = parse_direct_kernel_spec(source).expect("parse");
        let model = build_model(kernel.clone()).expect("model");
        let contract =
            public_kernel_contract(&kernel, &model, "s.fsl", "kernel").expect("contract");
        let partial = &contract["actions"][0]["partial_operations"][0];
        let value = &contract["actions"][0]["updates"][0]["value"];

        assert_eq!(value["kind"], "ite");
        assert_eq!(value["condition"]["name"], "gate");
        assert_eq!(partial["operation"], "divide");
        assert_eq!(partial["failure_condition"]["kind"], "binary");
        assert_eq!(partial["failure_condition"]["operator"], "and");
        assert_eq!(partial["failure_condition"]["left"]["kind"], "not");
        assert_eq!(
            partial["failure_condition"]["left"]["operand"]["name"],
            "gate"
        );
    }
}

// SPDX-License-Identifier: Apache-2.0

//! Checked-model expression and statement type validation.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use fsl_syntax::{
    AggregateKind, Binder, ConditionalSpans, Expr, LValue, Pattern, SourcePos, Span, Statement,
};

use crate::{ActionGuard, KernelModel, ParamDef, TypeDef, TypeRef};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypecheckError {
    pub(crate) message: String,
    pub(crate) span: Option<Span>,
}

impl TypecheckError {
    fn with_span(mut self, span: Span) -> Self {
        self.span.get_or_insert(span);
        self
    }
}

impl fmt::Display for TypecheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TypecheckError {}

fn error(message: impl Into<String>) -> TypecheckError {
    TypecheckError {
        message: message.into(),
        span: None,
    }
}

pub(crate) fn unknown_span() -> Span {
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

pub(crate) type TypeEnv = BTreeMap<String, TypeRef>;
pub(crate) type FiniteBinderCandidates = (String, Vec<(Expr, Expr)>, Option<Expr>);

pub(crate) fn base_env(model: &KernelModel) -> TypeEnv {
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

pub(crate) fn resolve(model: &KernelModel, ty: &TypeRef) -> Result<TypeRef, TypecheckError> {
    match ty {
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => Ok(TypeRef::Range(*lo, *hi)),
            Some(TypeDef::Enum { .. } | TypeDef::Struct { .. }) => Ok(ty.clone()),
            None => Err(error(format!("public Kernel cannot resolve type '{name}'"))),
        },
        _ => Ok(ty.clone()),
    }
}

pub(crate) fn qualified_name(name: &fsl_syntax::QualifiedName) -> Result<String, TypecheckError> {
    if name.namespace.is_some() {
        return Err(error(
            "qualified types must be lowered before public Kernel export",
        ));
    }
    Ok(name.name.clone())
}

pub(crate) fn binder_type(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<TypeRef, TypecheckError> {
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
pub(crate) fn infer_type(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    expected: Option<&TypeRef>,
) -> Result<TypeRef, TypecheckError> {
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
        Expr::EnumMember { type_name, member } => match model.types.get(type_name) {
            Some(TypeDef::Enum { members, .. }) if members.contains(member) => {
                Ok(TypeRef::Named(type_name.clone()))
            }
            Some(TypeDef::Enum { .. }) => {
                Err(error(format!("unknown enum member '{type_name}.{member}'")))
            }
            Some(_) => Err(error(format!("'{type_name}' is not an enum"))),
            None => Err(error(format!("unknown enum type '{type_name}'"))),
        },
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

pub(crate) fn types_compatible(
    actual: &TypeRef,
    expected: &TypeRef,
    model: &KernelModel,
) -> Result<bool, TypecheckError> {
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

pub(crate) fn expression_type(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    expected: Option<&TypeRef>,
) -> Result<TypeRef, TypecheckError> {
    let ty = infer_type(expr, env, model, expected).map_err(|error| error.with_span(span))?;
    if let Some(expected) = expected
        && !types_compatible(&ty, expected, model).map_err(|error| error.with_span(span))?
    {
        return Err(error(format!(
            "expression of type {ty:?} is not assignable to {expected:?}"
        ))
        .with_span(span));
    }
    Ok(ty)
}

pub(crate) fn collection_item_type(
    ty: &TypeRef,
    model: &KernelModel,
) -> Result<TypeRef, TypecheckError> {
    match resolve(model, ty)? {
        TypeRef::Set(item) | TypeRef::Seq(item, _) => Ok(*item),
        _ => Err(error("collection literal type mismatch")),
    }
}

pub(crate) fn index_key_type(
    collection: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<TypeRef, TypecheckError> {
    match resolve(model, &infer_type(collection, env, model, None)?)? {
        TypeRef::Map(key, _) | TypeRef::Relation(key, _) => Ok(*key),
        TypeRef::Seq(_, _) => Ok(TypeRef::Int),
        _ => Err(error("index expression requires collection")),
    }
}

pub(crate) fn method_argument_type(name: &str, receiver: &TypeRef) -> Option<TypeRef> {
    match (name, receiver) {
        ("contains" | "add" | "remove", TypeRef::Set(item))
        | ("contains" | "push", TypeRef::Seq(item, _)) => Some((**item).clone()),
        ("at", TypeRef::Seq(_, _)) => Some(TypeRef::Int),
        _ => None,
    }
}

pub(crate) fn binary_operand_types(
    op: &str,
    left: &Expr,
    right: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<(TypeRef, TypeRef), TypecheckError> {
    if matches!(op, "and" | "or" | "=>") {
        Ok((TypeRef::Bool, TypeRef::Bool))
    } else if matches!(op, "+" | "-" | "*" | "/" | "%" | "<" | "<=" | ">" | ">=") {
        Ok((TypeRef::Int, TypeRef::Int))
    } else {
        infer_type(left, env, model, None).map_or_else(
            |_| infer_type(right, env, model, None).map(|ty| (ty.clone(), ty)),
            |ty| Ok((ty.clone(), ty)),
        )
    }
}

pub(crate) fn struct_field_type(
    model: &KernelModel,
    name: &str,
    field: &str,
) -> Result<TypeRef, TypecheckError> {
    let Some(TypeDef::Struct { fields }) = model.types.get(name) else {
        return Err(error(format!("unknown struct '{name}'")));
    };
    fields
        .iter()
        .find_map(|(candidate, ty)| (candidate == field).then(|| ty.clone()))
        .ok_or_else(|| error(format!("unknown struct field '{name}.{field}'")))
}

pub(crate) fn ensure_assignable(
    expr: &Expr,
    expected: &TypeRef,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
) -> Result<(), TypecheckError> {
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

pub(crate) fn extend_pattern_binding(
    expression: &Expr,
    env: &mut TypeEnv,
    model: &KernelModel,
) -> Result<(), TypecheckError> {
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

pub(crate) fn lvalue_type(
    target: &LValue,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<TypeRef, TypecheckError> {
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

pub(crate) fn normalize_aggregate(
    kind: AggregateKind,
    binder: &Binder,
    value: Option<&Expr>,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<Expr, TypecheckError> {
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

pub(crate) fn finite_binder_candidates(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<FiniteBinderCandidates, TypecheckError> {
    let (name, candidates, filter) = match binder {
        Binder::Typed {
            name, where_expr, ..
        } => {
            let candidates = model
                .domain_values(&binder_type(binder, env, model)?)
                .map_err(|model_error| error(model_error.to_string()))?
                .into_iter()
                .map(|candidate| Ok((value_expression(candidate)?, Expr::Bool(true))))
                .collect::<Result<Vec<_>, TypecheckError>>()?;
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
                    .collect::<Result<Vec<_>, TypecheckError>>()?,
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
                    .collect::<Result<Vec<_>, TypecheckError>>()?,
                _ => return Err(error("collection binder requires Set or Seq")),
            };
            (name, candidates, where_expr.as_deref())
        }
    };
    Ok((name.clone(), candidates, filter.cloned()))
}

pub(crate) fn aggregate_condition(
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

fn static_public_int(expr: &Expr, model: &KernelModel) -> Result<i64, TypecheckError> {
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

fn value_expression(value: crate::FslValue) -> Result<Expr, TypecheckError> {
    Ok(match value {
        crate::FslValue::Int(value) => Expr::Num(value),
        crate::FslValue::Bool(value) => Expr::Bool(value),
        crate::FslValue::Enum { type_name, member } => Expr::EnumMember { type_name, member },
        crate::FslValue::None => Expr::None,
        crate::FslValue::Some(value) => Expr::Some(Box::new(value_expression(*value)?)),
        crate::FslValue::Struct { type_name, fields } => Expr::Struct {
            name: type_name,
            fields: fields
                .into_iter()
                .map(|(name, value)| Ok((name, value_expression(value)?)))
                .collect::<Result<Vec<_>, TypecheckError>>()?,
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

fn validate_binder(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
) -> Result<TypeRef, TypecheckError> {
    let ty = binder_type(binder, env, model)?;
    let (name, where_expr) = match binder {
        Binder::Typed {
            name, where_expr, ..
        } => (name, where_expr.as_deref()),
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => {
            validate_expression(lo, env, model, span, Some(&ty))?;
            validate_expression(hi, env, model, span, Some(&ty))?;
            (name, where_expr.as_deref())
        }
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => {
            validate_expression(collection, env, model, span, None)?;
            (name, where_expr.as_deref())
        }
    };
    if let Some(where_expr) = where_expr {
        let mut local = env.clone();
        local.insert(name.clone(), ty.clone());
        validate_expression(where_expr, &local, model, span, Some(&TypeRef::Bool))?;
    }
    Ok(ty)
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}

#[allow(clippy::too_many_lines)]
fn validate_expression(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    expected: Option<&TypeRef>,
) -> Result<(), TypecheckError> {
    let ty = expression_type(expr, env, model, span, expected)?;
    match expr {
        Expr::Num(_)
        | Expr::Bool(_)
        | Expr::None
        | Expr::Var(_)
        | Expr::EnumMember { .. }
        | Expr::Call { .. }
        | Expr::Stage { .. } => {}
        Expr::Some(item) => {
            let TypeRef::Option(inner) = resolve(model, &ty)? else {
                return Err(error("some expression did not infer Option"));
            };
            validate_expression(item, env, model, span, Some(&inner))?;
        }
        Expr::Set(items) | Expr::Seq(items) => {
            let item_ty = collection_item_type(&ty, model)?;
            for item in items {
                validate_expression(item, env, model, span, Some(&item_ty))?;
            }
        }
        Expr::Struct { name, fields } => {
            for (field, value) in fields {
                let expected = struct_field_type(model, name, field)?;
                validate_expression(value, env, model, span, Some(&expected))?;
            }
        }
        Expr::Index(collection, index) => {
            let key_ty = index_key_type(collection, env, model)?;
            validate_expression(collection, env, model, span, None)?;
            validate_expression(index, env, model, span, Some(&key_ty))?;
        }
        Expr::Field(value, _) => validate_expression(value, env, model, span, None)?,
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            let receiver_ty = resolve(model, &infer_type(receiver, env, model, None)?)?;
            let argument_type = method_argument_type(name, &receiver_ty);
            validate_expression(receiver, env, model, span, None)?;
            for argument in args {
                validate_expression(argument, env, model, span, argument_type.as_ref())?;
            }
        }
        Expr::Binary { op, left, right } => {
            let (left_ty, right_ty) = binary_operand_types(op, left, right, env, model)?;
            validate_expression(left, env, model, span, Some(&left_ty))?;
            let mut right_env = env.clone();
            extend_pattern_binding(left, &mut right_env, model)?;
            validate_expression(right, &right_env, model, span, Some(&right_ty))?;
        }
        Expr::Neg(operand) | Expr::Not(operand) => {
            validate_expression(operand, env, model, span, None)?;
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => {
            validate_expression(condition, env, model, spans.condition, Some(&TypeRef::Bool))?;
            validate_expression(then_expr, env, model, spans.then_expr, Some(&ty))?;
            validate_expression(else_expr, env, model, spans.else_expr, Some(&ty))?;
        }
        Expr::Is { expr, .. } | Expr::UnaryNamed { expr, .. } => {
            validate_expression(expr, env, model, span, None)?;
        }
        Expr::Quantified { binder, body, .. } => {
            let binder_ty = validate_binder(binder, env, model, span)?;
            let mut local = env.clone();
            local.insert(binder_name(binder).to_owned(), binder_ty);
            validate_expression(body, &local, model, span, Some(&TypeRef::Bool))?;
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
                validate_expression(&normalized, env, model, span, expected)?;
            } else {
                let binder_ty = validate_binder(binder, env, model, span)?;
                let mut local = env.clone();
                local.insert(binder_name(binder).to_owned(), binder_ty);
                if let Some(value) = value {
                    validate_expression(value, &local, model, span, Some(&TypeRef::Int))?;
                }
            }
        }
        Expr::BinaryNamed { left, right, .. } => {
            validate_expression(left, env, model, span, Some(&TypeRef::Int))?;
            validate_expression(right, env, model, span, Some(&TypeRef::Int))?;
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            validate_expression(first, env, model, span, None)?;
            validate_expression(second, env, model, span, None)?;
            validate_expression(third, env, model, span, None)?;
        }
    }
    Ok(())
}

fn validate_statement(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<(), TypecheckError> {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => {
            let ty = lvalue_type(target, env, model)?;
            ensure_assignable(value, &ty, env, model, *span)?;
            validate_lvalue(target, env, model, *span)?;
            validate_expression(value, env, model, *span, Some(&ty))
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            validate_expression(condition, env, model, *span, Some(&TypeRef::Bool))?;
            then_statements
                .iter()
                .chain(else_statements)
                .try_for_each(|item| validate_statement(item, env, model))
        }
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let ty = validate_binder(binder, env, model, *span)?;
            let mut local = env.clone();
            local.insert(binder_name(binder).to_owned(), ty);
            statements
                .iter()
                .try_for_each(|item| validate_statement(item, &local, model))
        }
    }
}

fn validate_lvalue(
    target: &LValue,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
) -> Result<(), TypecheckError> {
    match target {
        LValue::Var(_) => Ok(()),
        LValue::Index(name, index) => {
            let (TypeRef::Map(key, _) | TypeRef::Relation(key, _)) = resolve(
                model,
                env.get(name)
                    .ok_or_else(|| error(format!("unknown update target '{name}'")))?,
            )?
            else {
                return Err(error("indexed update target requires Map or Relation"));
            };
            validate_expression(index, env, model, span, Some(&key))
        }
        LValue::Field(base, _) => validate_lvalue(base, env, model, span),
    }
}

fn validate_statement_assignments(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<(), TypecheckError> {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => {
            let ty = lvalue_type(target, env, model)?;
            ensure_assignable(value, &ty, env, model, *span)?;
            validate_lvalue(target, env, model, *span)?;
            validate_expression(value, env, model, *span, Some(&ty))
        }
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

pub(crate) fn validate_expression_type(
    expr: &Expr,
    expected: &TypeRef,
    bindings: &[(String, TypeRef)],
    model: &KernelModel,
) -> Result<(), TypecheckError> {
    let mut env = base_env(model);
    env.extend(bindings.iter().cloned());
    let span = unknown_span();
    validate_expression(expr, &env, model, span, Some(expected))
        .map_err(|error| error.with_span(span))
}

pub(crate) fn validate_public_expression(
    expr: &Expr,
    model: &KernelModel,
    span: Span,
    expected: Option<&TypeRef>,
) -> Result<(), TypecheckError> {
    validate_expression(expr, &base_env(model), model, span, expected)
}

pub(crate) fn expression_binder_type(
    binder: &Binder,
    model: &KernelModel,
) -> Result<TypeRef, TypecheckError> {
    binder_type(binder, &base_env(model), model)
}

pub(crate) fn validate_statement_types(
    statement: &Statement,
    model: &KernelModel,
) -> Result<(), TypecheckError> {
    validate_statement_assignments(statement, &base_env(model), model)
}

pub(crate) fn validate_checked_model_types(model: &KernelModel) -> Result<(), TypecheckError> {
    let env = base_env(model);
    for statement in &model.init {
        validate_statement(statement, &env, model)?;
    }
    validate_model_expression_types(model)?;
    for projection in &model.projections {
        validate_expression(
            &projection.expr,
            &env,
            model,
            projection.span,
            Some(&TypeRef::Int),
        )?;
    }
    Ok(())
}

pub(crate) fn validate_model_expression_types(model: &KernelModel) -> Result<(), TypecheckError> {
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
                    validate_expression(expr, &local, model, action.span, Some(&TypeRef::Bool))?;
                    extend_pattern_binding(expr, &mut local, model)?;
                }
                ActionGuard::Let(name, expr) => {
                    validate_expression(expr, &local, model, action.span, None)?;
                    local.insert(name.clone(), infer_type(expr, &local, model, None)?);
                }
            }
        }
        for statement in &action.statements {
            validate_statement(statement, &local, model)?;
        }
        for expr in &action.ensures {
            validate_expression(expr, &local, model, action.span, Some(&TypeRef::Bool))?;
        }
    }
    for property in model
        .invariants
        .iter()
        .chain(&model.transitions)
        .chain(&model.reachables)
    {
        validate_expression(
            &property.expr,
            &env,
            model,
            property.span,
            Some(&TypeRef::Bool),
        )?;
    }
    for leadsto in &model.leadstos {
        let mut local = env.clone();
        for binder in &leadsto.binders {
            let ty = validate_binder(binder, &local, model, leadsto.span)?;
            local.insert(binder_name(binder).to_owned(), ty);
        }
        validate_expression(
            &leadsto.before,
            &local,
            model,
            leadsto.span,
            Some(&TypeRef::Bool),
        )?;
        validate_expression(
            &leadsto.after,
            &local,
            model,
            leadsto.span,
            Some(&TypeRef::Bool),
        )?;
        if let Some(expr) = &leadsto.decreases {
            validate_expression(expr, &local, model, leadsto.span, Some(&TypeRef::Int))?;
        }
    }
    if let Some(expr) = &model.terminal {
        validate_expression(expr, &env, model, unknown_span(), Some(&TypeRef::Bool))?;
    }
    Ok(())
}

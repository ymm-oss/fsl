// SPDX-License-Identifier: Apache-2.0

//! Checked-model expression and statement type validation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use fsl_syntax::{
    AggregateKind, Binder, ConditionalSpans, Expr, LValue, Pattern, SourcePos, Span, Statement,
};

use crate::binder_range::{
    InclusiveI64, IntegerEnvelopeFact, RangeBinding, RangePlan, RangePrerequisite,
};
use crate::recursion;
use crate::{ActionGuard, KernelModel, TypeDef, TypeRef};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypecheckError {
    pub(crate) message: String,
    pub(crate) span: Option<Span>,
    pub(crate) needs_context: bool,
    pub(crate) constant_shadow: bool,
    /// A statement or quantifier refusal keeps the engine's unprefixed message.
    pub(crate) unprefixed_range_refusal: bool,
    /// The located target's origin, for a refusal rendered without a context prefix.
    pub(crate) target_origin: Option<Box<crate::OriginChain>>,
}

impl TypecheckError {
    pub(crate) fn with_span(mut self, span: Span) -> Self {
        self.span.get_or_insert(span);
        self
    }

    fn in_target(self, model: &KernelModel, target: &str, span: Span) -> Self {
        let located = model.error_at_target(self.message, self.span.unwrap_or(span), target);
        Self {
            message: located.message,
            span: located.span,
            target_origin: located.origin.or(self.target_origin),
            ..self
        }
    }

    fn in_component_item(self, model: &KernelModel, target: &str) -> Self {
        if let Some(span) = model.component_item_span(target) {
            self.in_target(model, target, span)
        } else {
            self
        }
    }
}

impl fmt::Display for TypecheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TypecheckError {}

pub(crate) fn error(message: impl Into<String>) -> TypecheckError {
    TypecheckError {
        message: message.into(),
        span: None,
        needs_context: false,
        constant_shadow: false,
        unprefixed_range_refusal: false,
        target_origin: None,
    }
}

fn context_error(message: impl Into<String>) -> TypecheckError {
    TypecheckError {
        needs_context: true,
        ..error(message)
    }
}

fn with_optional_span(error: TypecheckError, span: Option<Span>) -> TypecheckError {
    match span {
        Some(span) => error.with_span(span),
        None => error,
    }
}

/// A Kernel name as a diagnostic shows it: the public spelling every output
/// uses (`acct.bal` for the compose component state `acct__bal`).
fn shown(name: &str) -> String {
    crate::unmapped_public_spelling(name)
}

pub(crate) fn unresolved_identifier(name: &str, owners: &[String]) -> TypecheckError {
    if owners.len() > 1 {
        error(format!(
            "ambiguous enum member '{name}' belongs to [{}]; use enum conversion/convert for a bijection or enum abstraction/abstract for a many-to-one mapping",
            owners.join(", ")
        ))
    } else {
        error(format!(
            "public Kernel cannot type identifier '{}'",
            shown(name)
        ))
    }
}

/// `ty` with every named type respelled by [`shown`], for a message that
/// prints the type's debug form.
fn shown_type(ty: &TypeRef) -> TypeRef {
    let boxed = |ty: &TypeRef| Box::new(shown_type(ty));
    match ty {
        TypeRef::Named(name) => TypeRef::Named(shown(name)),
        TypeRef::Map(key, value) => TypeRef::Map(boxed(key), boxed(value)),
        TypeRef::Relation(left, right) => TypeRef::Relation(boxed(left), boxed(right)),
        TypeRef::Set(item) => TypeRef::Set(boxed(item)),
        TypeRef::Seq(item, length) => TypeRef::Seq(boxed(item), *length),
        TypeRef::Option(item) => TypeRef::Option(boxed(item)),
        TypeRef::Int | TypeRef::Bool | TypeRef::Range(..) => ty.clone(),
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

#[derive(Clone, Default)]
pub(crate) struct TypeScope {
    types: BTreeMap<String, TypeRef>,
    // Distinguish locals (even without a value fact) from state/constant entries.
    lexical: BTreeSet<String>,
    /// Scalar candidate values captured when a collection let is evaluated.
    pub(crate) collections: HashMap<String, Vec<Vec<Expr>>>,
}

pub(crate) type TypeEnv = TypeScope;

impl std::ops::Deref for TypeEnv {
    type Target = BTreeMap<String, TypeRef>;

    fn deref(&self) -> &Self::Target {
        &self.types
    }
}

impl std::ops::DerefMut for TypeEnv {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.types
    }
}

impl FromIterator<(String, TypeRef)> for TypeEnv {
    fn from_iter<T: IntoIterator<Item = (String, TypeRef)>>(iter: T) -> Self {
        Self {
            types: iter.into_iter().collect(),
            ..Self::default()
        }
    }
}

impl TypeEnv {
    pub(crate) fn insert(&mut self, name: String, ty: TypeRef) -> Option<TypeRef> {
        // A binder or a subsequent binding must not inherit an alias it shadows.
        self.collections.remove(&name);
        self.lexical.insert(name.clone());
        self.types.insert(name, ty)
    }

    pub(crate) fn remove(&mut self, name: &str) -> Option<TypeRef> {
        self.collections.remove(name);
        self.lexical.remove(name);
        self.types.remove(name)
    }

    fn extend_lexical(&mut self, bindings: &[(String, TypeRef)]) {
        self.lexical
            .extend(bindings.iter().map(|(name, _)| name.clone()));
        self.types.extend(bindings.iter().cloned());
    }

    pub(crate) fn collection_candidates(
        &self,
        expression: &Expr,
        ty: &TypeRef,
        model: &KernelModel,
    ) -> Result<Vec<Vec<Expr>>, TypecheckError> {
        let endpoints = match ty {
            TypeRef::Set(element) => vec![(None, element)],
            TypeRef::Relation(source, target) => vec![(Some(0), source), (Some(1), target)],
            _ => Vec::new(),
        };
        endpoints
            .into_iter()
            .map(|(endpoint, element)| {
                collection_candidates(expression, endpoint, element, self, model)
            })
            .collect()
    }
}
// A let's inferred type alone can lose a non-self conditional branch. Keep the
// identity guarantee separately, captured before installing each new binding.
type SelfRelations = BTreeMap<String, bool>;
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
    let mut enum_members = BTreeMap::<&str, Option<&str>>::new();
    for (type_name, definition) in &model.types {
        if let TypeDef::Enum { members, .. } = definition {
            for member in members {
                enum_members
                    .entry(member)
                    .and_modify(|owner| *owner = None)
                    .or_insert(Some(type_name));
            }
        }
    }
    for (member, owner) in enum_members {
        if let Some(type_name) = owner {
            env.entry(member.to_owned())
                .or_insert_with(|| TypeRef::Named(type_name.to_owned()));
        }
    }
    env
}

pub(crate) fn expression_env(model: &KernelModel, bindings: &[(String, TypeRef)]) -> TypeEnv {
    let mut env = base_env(model);
    env.extend_lexical(bindings);
    env
}

fn enum_member_owners(model: &KernelModel, member: &str) -> Vec<String> {
    model
        .types
        .iter()
        .filter_map(|(type_name, definition)| match definition {
            TypeDef::Enum { members, .. } if members.iter().any(|item| item == member) => {
                Some(type_name.clone())
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn resolve(model: &KernelModel, ty: &TypeRef) -> Result<TypeRef, TypecheckError> {
    match ty {
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => Ok(TypeRef::Range(*lo, *hi)),
            Some(TypeDef::Enum { .. } | TypeDef::Struct { .. }) => Ok(ty.clone()),
            None => Err(error(format!(
                "public Kernel cannot resolve type '{}'",
                shown(name)
            ))),
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
        Binder::Typed { type_name, .. } => {
            let name = qualified_name(type_name)?;
            match name.as_str() {
                "Bool" => Ok(TypeRef::Bool),
                "Int" => Ok(TypeRef::Int),
                _ if model.types.contains_key(&name) => Ok(TypeRef::Named(name)),
                _ => Err(error(format!("unknown type '{}'", shown(&name)))),
            }
        }
        Binder::Range { lo, hi, .. } => {
            ensure_assignable_inner(lo, &TypeRef::Int, env, model, None)?;
            ensure_assignable_inner(hi, &TypeRef::Int, env, model, None)?;
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

/// Cycle entry for type inference, and where `recursion::guard` belongs: every
/// arm below recurses back through here for its operands.
///
/// **Not crash-witnessed.** No witness has made this one abort: `check` and
/// `refine` both survive N=1000 (a 2000-long chain, 5x the regression witness)
/// with the guard removed, because [`validate_expression`] is guarded per level
/// and this inference walk rides inside that guarded region. It is guarded
/// because at the deepest validated level it still descends the remaining
/// subtree on its own, which is the same second-order shape as the `serde_json`
/// residual in 4.4 (#622) and differs from the measured sites only by a
/// constant. Treat this note as the evidence a reader needs to tell a measured
/// site from a speculative one (#620); if the distinction is ever enforced by
/// deletion, this is one of the two to delete.
pub(crate) fn infer_type(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    expected: Option<&TypeRef>,
) -> Result<TypeRef, TypecheckError> {
    recursion::guard(|| infer_type_inner(expr, env, model, expected))
}

#[allow(clippy::too_many_lines)]
fn infer_type_inner(
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
            _ => Err(context_error(
                "public Kernel cannot infer uncontextualized none",
            )),
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
            if let Some(expected) = expected {
                match resolve(model, expected)? {
                    TypeRef::Set(_) => return Ok(expected.clone()),
                    TypeRef::Relation(_, _) => {
                        if !items.is_empty() {
                            return Err(error(
                                "relation literals only support the empty initializer Set {}; use r = r.add(a, b) to add pairs",
                            ));
                        }
                        return Ok(expected.clone());
                    }
                    _ => {}
                }
            }
            let first = items
                .first()
                .ok_or_else(|| context_error("public Kernel cannot infer empty Set"))?;
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
                .ok_or_else(|| context_error("public Kernel cannot infer empty Seq"))?;
            Ok(TypeRef::Seq(
                Box::new(infer_type(first, env, model, None)?),
                items.len(),
            ))
        }
        Expr::Struct { name, .. } => Ok(TypeRef::Named(name.clone())),
        Expr::Var(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| unresolved_identifier(name, &enum_member_owners(model, name))),
        Expr::EnumMember {
            type_name, member, ..
        } => match model.types.get(type_name) {
            Some(TypeDef::Enum { members, .. }) if members.contains(member) => {
                Ok(TypeRef::Named(type_name.clone()))
            }
            Some(TypeDef::Enum { .. }) => Err(error(format!(
                "unknown enum member '{}.{member}'",
                shown(type_name)
            ))),
            Some(_) => Err(error(format!("'{}' is not an enum", shown(type_name)))),
            None => Err(error(format!("unknown enum type '{}'", shown(type_name)))),
        },
        Expr::Call { name, .. } => Err(error(format!(
            "unlowered predicate call '{name}' in public Kernel"
        ))),
        Expr::Stage { .. } => Err(error("unlowered stage access in public Kernel")),
        Expr::Index(base, _) => resolve(model, &infer_type(base, env, model, None)?)?
            .index_types(true)
            .map(|(_, value)| value)
            .ok_or_else(|| error("public Kernel index requires Map or Seq")),
        Expr::Field(base, field) => {
            let TypeRef::Named(name) = infer_type(base, env, model, None)? else {
                return Err(error("public Kernel field access requires named struct"));
            };
            match model.types.get(&name) {
                Some(TypeDef::Struct { fields }) => fields
                    .iter()
                    .find_map(|(candidate, ty)| (candidate == field).then(|| ty.clone()))
                    .ok_or_else(|| {
                        error(format!("unknown struct field '{}.{field}'", shown(&name)))
                    }),
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
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let ty = infer_type(then_expr, env, model, expected)?;
            if expected.is_none() && has_int(&ty) {
                let other = infer_type(else_expr, env, model, None)?;
                if matches!(
                    (&ty, &other),
                    (TypeRef::Set(_), TypeRef::Set(_)) | (TypeRef::Seq(_, _), TypeRef::Seq(_, _))
                ) && !has_int(&carrier_type(&other, model)?)
                {
                    return Ok(other);
                }
            }
            Ok(ty)
        }
        Expr::UnaryNamed { name, expr, .. } => match name.as_str() {
            "old" => infer_type(expr, env, model, expected),
            "abs" => Ok(TypeRef::Int),
            "rel_acyclic" | "rel_functional" | "rel_injective" => Ok(TypeRef::Bool),
            "rel_domain" | "rel_range" => {
                let (source, target) = relation_operand_types(name, expr, env, model)?;
                Ok(TypeRef::Set(Box::new(if name == "rel_domain" {
                    source
                } else {
                    target
                })))
            }
            _ => Err(error(format!("unsupported unary expression '{name}'"))),
        },
        Expr::TernaryNamed { name, .. } if name == "rel_reachable" => Ok(TypeRef::Bool),
        Expr::TernaryNamed { name, .. } => {
            Err(error(format!("unsupported ternary expression '{name}'")))
        }
    }
}

/// Container carriers must be equal, not merely scalar-assignable.
pub(crate) fn same_carrier(
    actual: &TypeRef,
    expected: &TypeRef,
    model: &KernelModel,
) -> Result<bool, TypecheckError> {
    Ok(carrier_type(actual, model)? == carrier_type(expected, model)?)
}

fn carrier_type(ty: &TypeRef, model: &KernelModel) -> Result<TypeRef, TypecheckError> {
    let inner = |ty| carrier_type(ty, model).map(Box::new);
    Ok(match resolve(model, ty)? {
        TypeRef::Set(item) => TypeRef::Set(inner(&item)?),
        TypeRef::Seq(item, capacity) => TypeRef::Seq(inner(&item)?, capacity),
        TypeRef::Option(item) => TypeRef::Option(inner(&item)?),
        TypeRef::Map(key, value) => TypeRef::Map(inner(&key)?, inner(&value)?),
        TypeRef::Relation(source, target) => TypeRef::Relation(inner(&source)?, inner(&target)?),
        ty => ty,
    })
}

// Int carries no finite domain, including when inferred from a literal (#1467).
fn has_int(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Int => true,
        TypeRef::Set(item) | TypeRef::Seq(item, _) | TypeRef::Option(item) => has_int(item),
        TypeRef::Map(key, value) | TypeRef::Relation(key, value) => has_int(key) || has_int(value),
        _ => false,
    }
}

/// Whether a value of `actual` may stand where `expected` is written: every answer of
/// [`carrier_coercion`] except `NotAssignable`.
pub(crate) fn types_compatible(
    actual: &TypeRef,
    expected: &TypeRef,
    model: &KernelModel,
) -> Result<bool, TypecheckError> {
    Ok(carrier_coercion(actual, expected, model)? != CarrierCoercion::NotAssignable)
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
        return Err(not_assignable(&ty, expected).with_span(span));
    }
    Ok(ty)
}

fn not_assignable(actual: &TypeRef, expected: &TypeRef) -> TypecheckError {
    error(format!(
        "expression of type {:?} is not assignable to {:?}",
        shown_type(actual),
        shown_type(expected)
    ))
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
    resolve(model, &infer_type(collection, env, model, None)?)?
        .index_types(true)
        .map(|(key, _)| key)
        .ok_or_else(|| error("index expression requires Map or Seq"))
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
        let ty = match infer_type(left, env, model, None) {
            Ok(ty) => ty,
            Err(left_error) => infer_type(right, env, model, None).map_err(|right_error| {
                if right_error.needs_context && !left_error.needs_context {
                    left_error
                } else {
                    right_error
                }
            })?,
        };
        Ok((ty.clone(), ty))
    }
}

pub(crate) fn struct_field_type(
    model: &KernelModel,
    name: &str,
    field: &str,
) -> Result<TypeRef, TypecheckError> {
    let Some(TypeDef::Struct { fields }) = model.types.get(name) else {
        return Err(error(format!("unknown struct '{}'", shown(name))));
    };
    fields
        .iter()
        .find_map(|(candidate, ty)| (candidate == field).then(|| ty.clone()))
        .ok_or_else(|| error(format!("unknown struct field '{}.{field}'", shown(name))))
}

pub(crate) fn ensure_assignable(
    expr: &Expr,
    expected: &TypeRef,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
) -> Result<(), TypecheckError> {
    ensure_assignable_inner(expr, expected, env, model, Some(span))
}

fn ensure_assignable_inner(
    expr: &Expr,
    expected: &TypeRef,
    env: &TypeEnv,
    model: &KernelModel,
    span: Option<Span>,
) -> Result<(), TypecheckError> {
    let resolved = resolve(model, expected).map_err(|error| with_optional_span(error, span))?;
    match (expr, &resolved) {
        (Expr::None, TypeRef::Option(_)) => return Ok(()),
        (Expr::Some(item), TypeRef::Option(expected_item)) => {
            return ensure_assignable_inner(item, expected_item, env, model, span);
        }
        (Expr::Set(items), TypeRef::Set(expected_item))
        | (Expr::Seq(items), TypeRef::Seq(expected_item, _)) => {
            for item in items {
                ensure_assignable_inner(item, expected_item, env, model, span)?;
            }
            return Ok(());
        }
        (Expr::Set(items), TypeRef::Relation(_, _)) => {
            return if items.is_empty() {
                Ok(())
            } else {
                Err(with_optional_span(
                    error(
                        "relation literals only support the empty initializer Set {}; use r = r.add(a, b) to add pairs",
                    ),
                    span,
                ))
            };
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
    let actual =
        infer_type(expr, env, model, None).map_err(|error| with_optional_span(error, span))?;
    if types_compatible(&actual, expected, model)
        .map_err(|error| with_optional_span(error, span))?
    {
        Ok(())
    } else {
        Err(with_optional_span(
            error(format!(
                "expression of type {:?} is not assignable to {:?}",
                shown_type(&actual),
                shown_type(expected)
            )),
            span,
        ))
    }
}

/// What reading a value of one type as another takes. The one carrier rule (#1683):
/// assignability, `check`'s refusal of a Map and symbolic coercion all answer from it.
/// Only `NotAssignable` and `MapMismatch` are refusals, and which of them applies at an
/// expression depends on whether that position reaches coercion, which a type cannot
/// say (`ValidationPhase::symbolic`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CarrierCoercion {
    /// The same carrier, or two scalars of one kind. A value keeps its representation.
    Retag,
    /// `Int` stands for a scalar below an Option, Set or Seq: a literal can infer it, so
    /// the carriers differ and coercion rebuilds the value element by element. An
    /// Option of different scalars also answers this, as its value is always rebuilt.
    RebuildInt,
    /// Two Maps of different carriers, at any depth. They were assignable through `Int`,
    /// but no literal or inference produces a Map and coercion can only retag one.
    MapMismatch,
    /// No rule relates the two.
    NotAssignable,
}

pub(crate) fn carrier_coercion(
    actual: &TypeRef,
    expected: &TypeRef,
    model: &KernelModel,
) -> Result<CarrierCoercion, TypecheckError> {
    use CarrierCoercion::{MapMismatch, NotAssignable, RebuildInt, Retag};
    let (actual, expected) = (resolve(model, actual)?, resolve(model, expected)?);
    let below = match (&actual, &expected) {
        (TypeRef::Int | TypeRef::Range(_, _), TypeRef::Int | TypeRef::Range(_, _))
        | (TypeRef::Bool, TypeRef::Bool) => return Ok(Retag),
        (TypeRef::Named(left), TypeRef::Named(right)) => {
            return Ok(if left == right { Retag } else { NotAssignable });
        }
        (TypeRef::Option(left), TypeRef::Option(right)) => carrier_coercion(left, right, model)?,
        (TypeRef::Set(left), TypeRef::Set(right)) => element_coercion(left, right, model)?,
        (TypeRef::Seq(left, left_capacity), TypeRef::Seq(right, right_capacity))
            if left_capacity == right_capacity || has_int(left) || has_int(right) =>
        {
            element_coercion(left, right, model)?
        }
        (TypeRef::Map(left_key, left_value), TypeRef::Map(right_key, right_value)) => {
            return Ok(
                if element_coercion(left_key, right_key, model)? == NotAssignable
                    || element_coercion(left_value, right_value, model)? == NotAssignable
                {
                    NotAssignable
                } else if same_carrier(&actual, &expected, model)? {
                    Retag
                } else {
                    MapMismatch
                },
            );
        }
        (
            TypeRef::Relation(left_source, left_target),
            TypeRef::Relation(right_source, right_target),
        ) if same_carrier(left_source, right_source, model)?
            && same_carrier(left_target, right_target, model)? =>
        {
            return Ok(Retag);
        }
        _ => return Ok(NotAssignable),
    };
    Ok(match below {
        MapMismatch | NotAssignable => below,
        Retag | RebuildInt if same_carrier(&actual, &expected, model)? => Retag,
        Retag | RebuildInt => RebuildInt,
    })
}

/// A container's element answers the rule only if `Int` is involved. Without `Int` it
/// has a finite carrier, and the whole carrier must agree (#1467).
fn element_coercion(
    actual: &TypeRef,
    expected: &TypeRef,
    model: &KernelModel,
) -> Result<CarrierCoercion, TypecheckError> {
    if has_int(actual) || has_int(expected) {
        carrier_coercion(actual, expected, model)
    } else if same_carrier(actual, expected, model)? {
        Ok(CarrierCoercion::Retag)
    } else {
        Ok(CarrierCoercion::NotAssignable)
    }
}

/// The containers whose every conditional arm symbolic evaluation coerces, where
/// `Int` compatibility does not relate two arms that are each compatible with `Int`.
fn is_coerced_container(ty: &TypeRef, model: &KernelModel) -> Result<bool, TypecheckError> {
    Ok(matches!(
        resolve(model, ty)?,
        TypeRef::Set(_) | TypeRef::Seq(_, _) | TypeRef::Option(_)
    ))
}

/// Extend readable payload bindings from a true expression result.
/// Only conjunctions recurse through both operands. This traversal has its own
/// recursion guard: a nested `and` chain consumes one frame per level without
/// passing back through expression validation. Arithmetic chains are not visited.
pub(crate) fn extend_pattern_binding(
    expression: &Expr,
    env: &mut TypeEnv,
    model: &KernelModel,
) -> Result<(), TypecheckError> {
    recursion::guard(|| extend_pattern_binding_inner(expression, env, model))
}

fn extend_pattern_binding_inner(
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
            // Mirror the evaluators (#1153): a pattern binding never rewrites the
            // type of an action parameter, binder or let already in scope. State
            // variables and constants are not bindings, so they stay shadowable.
            let outer_binding = env.contains_key(binding)
                && !model.state.iter().any(|(name, _)| name == binding)
                && !model.consts.contains_key(binding);
            if !outer_binding {
                env.insert(binding.clone(), *inner);
            }
        }
        Expr::Binary { op, left, right } if crate::pattern_binding_flows(op, false) => {
            extend_pattern_binding(left, env, model)?;
            extend_pattern_binding(right, env, model)?;
        }
        _ => {}
    }
    Ok(())
}

/// `x is some(v)` may only introduce a fresh name (#1157, #1259).
///
/// A colliding `v` is ambiguous between a binding and a reference: the
/// evaluators keep an enclosing binding (#1153) but let a state variable,
/// constant or enum member be shadowed, and only where the match succeeds, so
/// the same text means different things depending on what it collides with and
/// on whether the operand is present. `DESIGN-pattern-binding-scope.md` item 4
/// asks for an explicit rename instead of a silent change of meaning.
fn reject_colliding_pattern_binding(
    binding: &str,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    phase: &ValidationPhase,
) -> Result<(), TypecheckError> {
    // Ordinary validation already judged these names; admission only judges ranges.
    if phase.range_rule.is_some() {
        return Ok(());
    }
    reject_pattern_binding_name(
        phase,
        model.names().pattern_scopes.get(binding),
        binding,
        model.state.iter().map(|(name, _)| name.as_str()),
        model.consts.keys().map(String::as_str),
        model.enum_members.keys().map(String::as_str),
        env.keys().chain(&phase.collisions).map(String::as_str),
        [],
    )
    .map(|_| ())
    .map_err(|error| error.with_span(span))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PatternBinding {
    Fresh,
    Substituted,
}

pub(crate) type PatternScopes = BTreeMap<String, Result<PatternBinding, TypecheckError>>;

/// The same collision rule applies before compose prefixes global names.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reject_pattern_binding_name<'a>(
    phase: &ValidationPhase,
    source_scope: Option<&Result<PatternBinding, TypecheckError>>,
    binding: &str,
    state: impl IntoIterator<Item = &'a str>,
    constants: impl IntoIterator<Item = &'a str>,
    enum_members: impl IntoIterator<Item = &'a str>,
    local: impl IntoIterator<Item = &'a str>,
    fields: impl IntoIterator<Item = &'a str>,
) -> Result<PatternBinding, TypecheckError> {
    if phase.substitutes(binding) {
        return Ok(PatternBinding::Substituted);
    }
    if let Some(decision) = source_scope {
        return decision.clone();
    }
    let is_binding = |name: &str| name == binding;
    let collides_with = if state.into_iter().any(is_binding) {
        "a state variable"
    } else if constants.into_iter().any(is_binding) {
        "a constant"
    } else if enum_members.into_iter().any(is_binding) {
        "an enum member"
    } else if local.into_iter().any(is_binding) {
        "a name already in scope (an action parameter, quantifier binder, let, or earlier pattern binding)"
    } else if fields.into_iter().any(is_binding) {
        "an entity field"
    } else {
        return Ok(PatternBinding::Fresh);
    };
    Err(error(format!(
        "pattern binding '{binding}' in 'is some({binding})' shadows {collides_with} of the same name; \
         rename the pattern variable"
    )))
}

pub(crate) fn lvalue_type(
    target: &LValue,
    _env: &TypeEnv,
    model: &KernelModel,
    span: Span,
) -> Result<TypeRef, TypecheckError> {
    model
        .state_lvalue_type(target)
        .map_err(|model_error| error(model_error.message).with_span(span))
}

enum AggregateValidation {
    Expanded(Expr),
    Deferred,
}

fn aggregate_validation(
    kind: AggregateKind,
    binder: &Binder,
    value: Option<&Expr>,
    env: &TypeEnv,
    model: &KernelModel,
    expand: bool,
) -> Result<AggregateValidation, TypecheckError> {
    let mut deferred = !expand;
    if let Binder::Range { lo, hi, .. } = binder {
        use crate::binder_range::RangeUnknown;

        let lexical = env
            .lexical
            .iter()
            .map(|name| RangeBinding {
                name,
                semantic_ty: &env[name],
                fact: None,
            })
            .collect::<Vec<_>>();
        // Authored bounds/filter/value have passed. Neither a carrier nor a
        // lexical value without its captured fact establishes execution eligibility.
        // Check both endpoints in order even if the first needs deferral.
        for bound in [lo.as_ref(), hi.as_ref()] {
            match model.plan_integer_range(bound, bound, &lexical) {
                RangePlan::Static { .. } => {}
                RangePlan::Unknown(RangeUnknown::ConstantFoldFailed) => {
                    // A closed child's failed fold does not make the whole bound closed.
                    let mut dependent = false;
                    crate::model::visit_free_names(bound, &BTreeSet::new(), &mut |name| {
                        if env.lexical.contains(name)
                            || model.state.iter().any(|(state, _)| state == name)
                        {
                            dependent = true;
                        }
                    });
                    if dependent {
                        deferred = true;
                    } else {
                        // Retain the strict owner's genuine failed-fold diagnostic.
                        static_public_int(bound, model)?;
                    }
                }
                _ => deferred = true,
            }
        }
    }
    if deferred {
        return Ok(AggregateValidation::Deferred);
    }
    normalize_aggregate(kind, binder, value, env, model).map(AggregateValidation::Expanded)
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
                TypeRef::Set(element_ty) => {
                    // Preserve the existing finite-scalar binder boundary;
                    // syntactically finite Set<Int> literals are not admitted.
                    model
                        .domain_values(&element_ty)
                        .map_err(|model_error| error(model_error.to_string()))?;
                    let mut seen = Vec::new();
                    let mut result = Vec::new();
                    for candidate in
                        collection_candidates(collection, None, &element_ty, env, model)?
                    {
                        if seen.contains(&candidate) {
                            continue;
                        }
                        let present = Expr::Method {
                            receiver: collection.clone(),
                            name: "contains".to_owned(),
                            args: vec![candidate.clone()],
                        };
                        let present = seen
                            .iter()
                            .filter(|previous| {
                                !literal_scalar(previous) || !literal_scalar(&candidate)
                            })
                            .fold(present, |present, previous: &Expr| Expr::Binary {
                                op: "and".to_owned(),
                                left: Box::new(present),
                                right: Box::new(Expr::Binary {
                                    op: "!=".to_owned(),
                                    left: Box::new(candidate.clone()),
                                    right: Box::new(previous.clone()),
                                }),
                            });
                        seen.push(candidate.clone());
                        result.push((candidate, present));
                    }
                    result
                }
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

/// Declared state carriers are finite, but expression-local adds can introduce
/// other members. Enumerate those expressions too, then test actual membership
/// and distinctness at the use site. This uses only existing Kernel primitives.
fn collection_candidates(
    collection: &Expr,
    endpoint: Option<usize>,
    element_ty: &TypeRef,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<Vec<Expr>, TypecheckError> {
    match collection {
        Expr::Var(name) if env.collections.contains_key(name) => {
            Ok(env.collections[name][endpoint.unwrap_or(0)].clone())
        }
        Expr::Set(items) => Ok(if endpoint.is_none() {
            items.clone()
        } else {
            Vec::new()
        }),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            let default = value_expression(
                model
                    .default_value(element_ty)
                    .map_err(|model_error| error(model_error.to_string()))?,
            )?;
            let mut candidates = Vec::new();
            for (branch, active_then) in [(then_expr, true), (else_expr, false)] {
                for candidate in collection_candidates(branch, endpoint, element_ty, env, model)? {
                    // Do not evaluate a candidate from an unchosen branch: its
                    // add argument may be a partial read. The harmless default
                    // is still subject to actual membership and deduplication.
                    let (then_expr, else_expr) = if active_then {
                        (candidate, default.clone())
                    } else {
                        (default.clone(), candidate)
                    };
                    candidates.push(Expr::Conditional {
                        condition: condition.clone(),
                        then_expr: Box::new(then_expr),
                        else_expr: Box::new(else_expr),
                        spans: Box::new(ConditionalSpans {
                            condition: unknown_span(),
                            then_expr: unknown_span(),
                            else_expr: unknown_span(),
                        }),
                    });
                }
            }
            Ok(candidates)
        }
        Expr::Method {
            receiver,
            name,
            args,
        } if matches!(name.as_str(), "add" | "remove") => {
            let mut candidates = collection_candidates(receiver, endpoint, element_ty, env, model)?;
            if name == "add" {
                candidates.push(args[endpoint.unwrap_or(0)].clone());
            }
            Ok(candidates)
        }
        Expr::UnaryNamed { name, expr, .. }
            if matches!(name.as_str(), "rel_domain" | "rel_range") =>
        {
            collection_candidates(
                expr,
                Some(usize::from(name == "rel_range")),
                element_ty,
                env,
                model,
            )
        }
        Expr::UnaryNamed { name, expr, span } if name == "old" => Ok(collection_candidates(
            expr, endpoint, element_ty, env, model,
        )?
        .into_iter()
        .map(|candidate| old_candidate(candidate, *span))
        .collect()),
        _ => model
            .domain_values(element_ty)
            .map_err(|model_error| error(model_error.to_string()))?
            .into_iter()
            .map(value_expression)
            .collect(),
    }
}

fn old_candidate(candidate: Expr, span: Span) -> Expr {
    Expr::UnaryNamed {
        name: "old".to_owned(),
        expr: Box::new(candidate),
        span,
    }
}

pub(crate) fn literal_scalar(expr: &Expr) -> bool {
    matches!(expr, Expr::Num(_) | Expr::Bool(_) | Expr::EnumMember { .. })
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
    if let Ok(value) = model.constant_integer(expr) {
        return Ok(value);
    }
    legacy_static_public_int(expr, model)
}

// Do not fold children on the legacy failed-fold path.
fn legacy_static_public_int(expr: &Expr, model: &KernelModel) -> Result<i64, TypecheckError> {
    match expr {
        Expr::Num(value) => Ok(*value),
        Expr::Var(name) => match model.consts.get(name) {
            Some(crate::FslValue::Int(value)) => Ok(*value),
            _ => Err(error(format!("'{}' is not an integer const", shown(name)))),
        },
        Expr::Neg(value) => legacy_static_public_int(value, model)?
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
        crate::FslValue::Enum { type_name, member } => Expr::EnumMember {
            type_name,
            member,
            source_bare: false,
        },
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

#[allow(clippy::struct_excessive_bools)] // Independent facts about one walk, set separately.
#[derive(Clone)]
pub(crate) struct ValidationPhase {
    normalize_aggregates: bool,
    aggregate_bounds_only: bool,
    defer_init_statement_domain: bool,
    substituted: BTreeSet<String>,
    // Reserved by earlier evaluation, without making those names readable.
    pub(crate) collisions: HashSet<String>,
    /// Present only in the checked-admission walk; ordinary validation ignores it.
    range_rule: Option<RangeRule>,
    // The model's own expressions reach symbolic evaluation, so their whole
    // container values must be coercible (#1679). A refinement map, a scenario
    // expectation and a lemma candidate keep the assignable rule. An expectation
    // is evaluated concretely, and so is a map, except that `preserve progress`
    // evaluates a map symbolically: a Map comparison there still reports
    // `cannot coerce`, as a rejected candidate does (see the #1679 amendment).
    // This stays beside `carrier_coercion` because it says where an expression is
    // evaluated, and that rule only sees two types (#1683).
    symbolic: bool,
}

impl ValidationPhase {
    pub(crate) fn substitutes(&self, name: &str) -> bool {
        self.substituted.contains(name)
    }
    pub(crate) fn normalized() -> Self {
        Self {
            normalize_aggregates: true,
            aggregate_bounds_only: false,
            defer_init_statement_domain: false,
            substituted: BTreeSet::new(),
            collisions: HashSet::new(),
            range_rule: None,
            symbolic: true,
        }
    }

    pub(crate) fn init_statements() -> Self {
        Self {
            defer_init_statement_domain: true,
            ..Self::normalized()
        }
    }

    fn concrete_only() -> Self {
        Self {
            symbolic: false,
            ..Self::normalized()
        }
    }

    pub(crate) fn after_patterns(&self, expression: &Expr) -> Self {
        let mut phase = self.clone();
        crate::compose::extend_pattern_names(expression, &mut phase.collisions, true);
        phase
    }

    fn after_where(&self, binder: &Binder) -> Self {
        crate::model::binder_scope(binder)
            .1
            .map_or_else(|| self.clone(), |expr| self.after_patterns(expr))
    }

    fn check(
        &self,
        expr: &Expr,
        env: &TypeEnv,
        model: &KernelModel,
        span: Span,
        expected: Option<&TypeRef>,
        self_relations: &SelfRelations,
    ) -> Result<(), TypecheckError> {
        validate_expression_in_phase(expr, env, model, span, expected, self, self_relations)
    }

    fn expands_aggregate(kind: AggregateKind, binder: &Binder) -> bool {
        matches!(kind, AggregateKind::Count | AggregateKind::Sum)
            && !matches!(binder, Binder::Typed { .. })
    }

    pub(crate) fn under_binder(&self, binder: &Binder, aggregate: Option<AggregateKind>) -> Self {
        let expands = aggregate.is_some_and(|kind| Self::expands_aggregate(kind, binder));
        let mut substituted = self.substituted.clone();
        substituted.remove(binder_name(binder));
        if expands {
            substituted.insert(binder_name(binder).to_owned());
        }
        Self {
            normalize_aggregates: self.normalize_aggregates && !expands,
            aggregate_bounds_only: self.aggregate_bounds_only,
            defer_init_statement_domain: self.defer_init_statement_domain,
            substituted,
            collisions: self.collisions.clone(),
            range_rule: self.range_rule.clone(),
            symbolic: self.symbolic,
        }
    }

    /// The raw walk checked admission uses: no expansion, one scope's carriers.
    fn range_admission(carriers: bool, admission: &Admission) -> Self {
        Self {
            normalize_aggregates: false,
            range_rule: Some(RangeRule {
                carriers,
                symbolic_sets: admission.symbolic_sets.clone(),
                static_aggregates_only: admission.static_aggregates_only,
                ..RangeRule::default()
            }),
            ..Self::normalized()
        }
    }

    /// The phase of one evaluation scope, in ordinary validation or in admission.
    fn for_scope(admission: Option<&Admission>, carriers: bool) -> Self {
        admission.map_or_else(Self::normalized, |admission| {
            Self::range_admission(carriers, admission)
        })
    }

    /// A transition property or postcondition reads `old(...)` from a frame.
    fn with_old_frame(mut self) -> Self {
        if let Some(rule) = &mut self.range_rule {
            rule.old_frame = true;
        }
        self
    }

    /// The engines read `old(...)` from a frame and have none below it, so a second
    /// `old`, or one in a scope with no old frame, stops every engine at evaluation. The
    /// lowered `P unless Q` wraps `P` in `old`, which makes an `old` in `P` the second one.
    fn inside_old(&self) -> Self {
        let mut phase = self.clone();
        if let Some(rule) = &mut phase.range_rule {
            if rule.old_frame {
                rule.old_frame = false;
            } else {
                rule.carriers = false;
                rule.frameless = true;
            }
        }
        phase
    }

    /// A postcondition keeps every lexical fact but has no certified read frame.
    fn for_postcondition(&self) -> Self {
        let mut phase = self.clone();
        if let Some(rule) = &mut phase.range_rule {
            rule.carriers = false;
            rule.after_action = true;
        }
        phase
    }

    /// The scope of a binder's filter and body, with the fact its candidates carry.
    fn bound_by(mut self, binder: &Binder, env: &TypeEnv, model: &KernelModel) -> Self {
        if let Some(rule) = &mut self.range_rule {
            *rule = rule.under_binder(binder, env, model);
        }
        self
    }
}

pub(crate) fn reject_constant_shadow<'a>(
    binding: &str,
    constants: impl IntoIterator<Item = &'a String>,
) -> Result<(), TypecheckError> {
    if constants.into_iter().any(|name| name == binding) {
        return Err(TypecheckError {
            constant_shadow: true,
            ..error(format!(
                "local binding '{binding}' shadows a constant of the same name; rename the local binding"
            ))
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_binder(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    source_phase: &ValidationPhase,
    child_phase: &ValidationPhase,
    site: BinderSite,
    self_relations: &SelfRelations,
) -> Result<TypeRef, TypecheckError> {
    reject_constant_shadow(binder_name(binder), model.consts.keys())
        .map_err(|error| error.with_span(span))?;
    // Check authored bounds before the location-free binder type projection.
    if let Binder::Range { lo, hi, .. } = binder {
        for bound in [lo, hi] {
            validate_expression_in_phase(
                bound,
                env,
                model,
                span,
                Some(&TypeRef::Int),
                source_phase,
                self_relations,
            )?;
        }
        if let Some(rule) = &source_phase.range_rule {
            if rule.static_aggregates_only {
                let substituted = |endpoint: &Expr| {
                    let mut found = false;
                    crate::model::visit_free_names(endpoint, &BTreeSet::new(), &mut |name| {
                        found |= source_phase.substitutes(name);
                    });
                    found
                };
                if matches!(site, BinderSite::Aggregate)
                    && [lo, hi].iter().any(|endpoint| {
                        !substituted(endpoint) && static_public_int(endpoint, model).is_err()
                    })
                {
                    return Err(range_refusal(lo, hi, model, site).with_span(span));
                }
            } else {
                admit_range_binder((lo, hi), env, model, span, rule, site)?;
            }
        }
    }
    let ty = binder_type(binder, env, model).map_err(|error| error.with_span(span))?;
    if source_phase.range_rule.is_some()
        && matches!(binder, Binder::Typed { .. })
        && matches!(resolve(model, &ty), Ok(TypeRef::Int))
        && !declares_finite_int(model)
    {
        return Err(unbounded_typed_refusal(&ty).with_span(span));
    }
    if !source_phase.defer_init_statement_domain
        && let (Binder::Typed { .. }, TypeRef::Named(name)) = (binder, &ty)
        && matches!(model.types.get(name), Some(TypeDef::Struct { .. }))
    {
        model
            .domain_values(&ty)
            .map_err(|failure| error(failure.message).with_span(span))?;
    }
    let (name, where_expr) = match binder {
        Binder::Typed {
            name, where_expr, ..
        }
        | Binder::Range {
            name, where_expr, ..
        } => (name, where_expr.as_deref()),
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => {
            validate_expression_in_phase(
                collection,
                env,
                model,
                span,
                None,
                source_phase,
                self_relations,
            )?;
            (name, where_expr.as_deref())
        }
    };
    if let Some(where_expr) = where_expr {
        let mut local = env.clone();
        local.insert(name.clone(), ty.clone());
        let mut local_relations = self_relations.clone();
        local_relations.remove(name);
        validate_expression_in_phase(
            where_expr,
            &local,
            model,
            span,
            Some(&TypeRef::Bool),
            child_phase,
            &local_relations,
        )?;
    }
    Ok(ty)
}

/// How the engine's refusal of a range binder was spelled before admission.
#[derive(Clone, Copy)]
enum BinderSite {
    Aggregate,
    Quantifier,
    Statement,
}

/// One assigned state's readable init value, as the engine's init recipes hold it.
#[derive(Clone)]
struct InitRecipe {
    ty: TypeRef,
    fact: Option<IntegerEnvelopeFact>,
    literal: Option<i64>,
    known: Option<bool>,
}

/// What one evaluation scope gives the shared range rule. Nothing here calls a solver or
/// evaluates a body; a leadsTo wrapper's bounds are folded one binding at a time, within
/// `WRAPPER_ADMISSION_BINDINGS`, and no binding row outlives its visit.
#[derive(Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
struct RangeRule {
    /// The engines certify this scope's read frame before evaluating it.
    carriers: bool,
    /// No candidate reaches this scope, so the engines never evaluate it.
    dead: bool,
    /// Unconditional values from enumerated binders and action parameters.
    facts: BTreeMap<String, IntegerEnvelopeFact>,
    /// Literal lets: unusable as a bound, but literal once placed in a Seq literal.
    literal_lets: BTreeMap<String, InclusiveI64>,
    /// Lets holding a collection, with the candidates it had where it was bound.
    collection_lets: BTreeMap<String, CollectionCandidates>,
    /// `old(...)` reads a frame in this scope.
    old_frame: bool,
    /// Under an `old(...)` with no frame to read: no name here carries a fact.
    frameless: bool,
    /// State Sets that can hold a key the engines do not read as a solver literal.
    symbolic_sets: SymbolicSets,
    /// A postcondition: it reads the state an action left, and no initial state.
    after_action: bool,
    /// Report only an expanding aggregate whose bound does not fold.
    static_aggregates_only: bool,
    /// Init only: completed assignments, readable as locals.
    recipes: BTreeMap<String, InitRecipe>,
}

/// The candidates a collection binder gets, as far as they stay solver literals.
#[derive(Clone, Copy)]
enum CollectionCandidates {
    Empty,
    Literal(InclusiveI64),
    Opaque,
}

/// How many leadsTo wrapper bindings admission visits, complete or not. It stops walking
/// past this, and is no limit on the language or on what the engines enumerate.
const WRAPPER_ADMISSION_BINDINGS: usize = 1 << 16;

fn join_envelope(left: InclusiveI64, right: InclusiveI64) -> Option<InclusiveI64> {
    InclusiveI64::new(left.lo().min(right.lo()), left.hi().max(right.hi()))
}

impl RangeRule {
    /// Planner input in the shape an engine builds from its bindings.
    fn lexical<'a>(&'a self, env: &'a TypeEnv) -> Vec<RangeBinding<'a>> {
        self.recipes
            .iter()
            .filter(|(name, _)| !env.lexical.contains(*name))
            .map(|(name, recipe)| RangeBinding {
                name,
                semantic_ty: &recipe.ty,
                fact: recipe.fact.as_ref().filter(|_| !self.frameless),
            })
            .chain(env.lexical.iter().map(|name| RangeBinding {
                name,
                semantic_ty: &env[name],
                fact: self.facts.get(name).filter(|_| !self.frameless),
            }))
            .collect()
    }

    fn forget(&mut self, name: &str) {
        self.facts.remove(name);
        self.literal_lets.remove(name);
        self.collection_lets.remove(name);
    }

    fn bind_fact(&mut self, name: &str, envelope: Option<InclusiveI64>) {
        self.forget(name);
        match envelope {
            Some(envelope) => {
                self.facts
                    .insert(name.to_owned(), IntegerEnvelopeFact::new(envelope));
            }
            None => self.dead = true,
        }
    }

    /// An integer item the engines evaluate to a solver literal, not merely in value.
    fn literal_item(
        &self,
        item: &Expr,
        env: &TypeEnv,
        model: &KernelModel,
    ) -> Option<InclusiveI64> {
        match item {
            Expr::Num(value) => InclusiveI64::new(*value, *value),
            Expr::Var(name) if env.lexical.contains(name) => self
                .facts
                .get(name)
                .map(IntegerEnvelopeFact::envelope)
                .or_else(|| self.literal_lets.get(name).copied()),
            Expr::Var(name) if self.recipes.contains_key(name) => self.recipes[name]
                .literal
                .and_then(|value| InclusiveI64::new(value, value)),
            Expr::Var(name) if model.state.iter().any(|(state, _)| state == name) => None,
            Expr::Var(name) => match model.consts.get(name) {
                Some(crate::FslValue::Int(value)) => InclusiveI64::new(*value, *value),
                _ => None,
            },
            _ => None,
        }
    }

    /// The fact a collection binder's candidates carry. A binder over a state Set rests
    /// on the carrier the engines certify on the read frame, the same prerequisite as a
    /// state name in a bound. Elsewhere only a solver literal has a fact.
    fn collection_candidates(
        &self,
        collection: &Expr,
        env: &TypeEnv,
        model: &KernelModel,
    ) -> CollectionCandidates {
        if self.carriers
            && let Some((frame, name)) = crate::binder_range::state_set_source(collection)
            && (frame == crate::binder_range::SetFrame::Current || self.old_frame)
            && !env.lexical.contains(name)
            && let Some(envelope) = crate::binder_range::state_set_element_envelope(model, name)
        {
            return CollectionCandidates::Literal(envelope);
        }
        self.literal_candidates(collection, env, model)
    }

    /// The candidates that stay solver literals, whatever the read frame certifies.
    /// Keys decide it: the engines order known keys without solver terms.
    fn literal_candidates(
        &self,
        collection: &Expr,
        env: &TypeEnv,
        model: &KernelModel,
    ) -> CollectionCandidates {
        use CollectionCandidates::{Empty, Literal, Opaque};
        match collection {
            Expr::Seq(items) | Expr::Set(items) => {
                let mut joined: Option<InclusiveI64> = None;
                for item in items {
                    let Some(envelope) = self.literal_item(item, env, model) else {
                        return Opaque;
                    };
                    joined = match joined {
                        Some(previous) => join_envelope(previous, envelope),
                        None => Some(envelope),
                    };
                }
                joined.map_or(Empty, Literal)
            }
            Expr::Var(name) if env.lexical.contains(name) => {
                self.collection_lets.get(name).copied().unwrap_or(Opaque)
            }
            Expr::Var(name) => match model.state.iter().find(|(state, _)| state == name) {
                Some((_, TypeRef::Seq(_, 0))) => Empty,
                Some((_, TypeRef::Set(_)))
                    if !(if self.after_action {
                        &self.symbolic_sets.after_action
                    } else {
                        &self.symbolic_sets.in_any_state
                    })
                    .contains(name) =>
                {
                    crate::binder_range::state_set_element_envelope(model, name)
                        .map_or(Opaque, Literal)
                }
                _ => Opaque,
            },
            // `old` reads a reframed frame, so no write has added a key there.
            Expr::UnaryNamed { name, expr, .. } if name == "old" && self.old_frame => Self {
                old_frame: false,
                symbolic_sets: SymbolicSets::default(),
                ..self.clone()
            }
            .literal_candidates(expr, env, model),
            Expr::Method {
                receiver,
                name,
                args,
            } if name == "add" || name == "remove" => {
                let Literal(envelope) = self.literal_candidates(receiver, env, model) else {
                    return Opaque;
                };
                match (name.as_str(), args.as_slice()) {
                    ("remove", _) => Literal(envelope),
                    (_, [item]) => self
                        .literal_item(item, env, model)
                        .and_then(|item| join_envelope(envelope, item))
                        .map_or(Opaque, Literal),
                    _ => Opaque,
                }
            }
            _ => Opaque,
        }
    }

    fn under_binder(&self, binder: &Binder, env: &TypeEnv, model: &KernelModel) -> Self {
        let mut child = self.clone();
        let name = binder_name(binder);
        child.forget(name);
        match binder {
            Binder::Range { lo, hi, .. } => {
                let plan = model.plan_integer_range(lo, hi, &self.lexical(env));
                if !matches!(plan, RangePlan::Unknown(_)) {
                    child.bind_fact(name, plan.candidates());
                }
            }
            Binder::Typed { .. } => {
                if let Ok(ty) = binder_type(binder, env, model)
                    && let Ok(TypeRef::Range(lo, hi)) = resolve(model, &ty)
                {
                    child.bind_fact(name, InclusiveI64::new(lo, hi));
                }
            }
            Binder::Collection { collection, .. } => {
                match self.collection_candidates(collection, env, model) {
                    CollectionCandidates::Empty => child.dead = true,
                    CollectionCandidates::Literal(envelope) => {
                        child.bind_fact(name, Some(envelope));
                    }
                    CollectionCandidates::Opaque => {}
                }
            }
        }
        child
    }

    fn bind_param(&mut self, param: &crate::ParamDef, model: &KernelModel) {
        self.forget(param.name());
        if let Ok(TypeRef::Range(lo, hi)) = resolve(model, &param.ty()) {
            self.bind_fact(param.name(), InclusiveI64::new(lo, hi));
        }
    }

    fn bind_let(&mut self, name: &str, value: &Expr, env: &TypeEnv, model: &KernelModel) {
        let literal = self.literal_item(value, env, model);
        let candidates = self.literal_candidates(value, env, model);
        self.forget(name);
        if let Some(literal) = literal {
            self.literal_lets.insert(name.to_owned(), literal);
        }
        self.collection_lets.insert(name.to_owned(), candidates);
    }

    fn known_condition(
        &self,
        condition: &Expr,
        env: &TypeEnv,
        model: &KernelModel,
    ) -> Option<bool> {
        match condition {
            Expr::Bool(value) => return Some(*value),
            Expr::Var(name) if !env.lexical.contains(name) => {
                if let Some(known) = self.recipes.get(name).and_then(|recipe| recipe.known) {
                    return Some(known);
                }
            }
            _ => {}
        }
        if env
            .lexical
            .iter()
            .any(|name| model.consts.contains_key(name))
            || self
                .recipes
                .keys()
                .any(|name| model.consts.contains_key(name))
        {
            return None;
        }
        crate::binder_range::constant_condition(model, condition)
    }

    /// Mirror the engine's recipe after `name = value`, from Core facts alone.
    fn record_init_assignment(
        &mut self,
        name: &str,
        value: &Expr,
        env: &TypeEnv,
        model: &KernelModel,
    ) {
        let Ok(ty) = model.state_lvalue_type(&LValue::Var(name.to_owned())) else {
            return;
        };
        let literal = match self.literal_item(value, env, model) {
            Some(envelope) if envelope.lo() == envelope.hi() => Some(envelope.lo()),
            _ => None,
        };
        // The engines read the assigned term: only a literal or a named constant is known,
        // not a condition they can fold but do not rewrite.
        let known = matches!(value, Expr::Bool(_) | Expr::Var(_))
            .then(|| self.known_condition(value, env, model))
            .flatten();
        let integer = matches!(resolve(model, &ty), Ok(TypeRef::Int | TypeRef::Range(_, _)));
        let fact = {
            let lexical = self.lexical(env);
            let mut selector = |condition: &Expr| self.known_condition(condition, env, model);
            crate::binder_range::init_expression_envelope(
                model,
                value,
                Some(&lexical),
                &mut selector,
            )
        }
        .filter(|envelope| {
            !integer || crate::binder_range::init_envelope_fits(model, &ty, *envelope)
        })
        .map(IntegerEnvelopeFact::new);
        self.recipes.remove(name);
        if !integer || fact.is_some() {
            self.recipes.insert(
                name.to_owned(),
                InitRecipe {
                    ty,
                    fact,
                    literal,
                    known,
                },
            );
        }
    }

    /// Join two init paths when the engine cannot select one of them.
    fn join_recipes(
        left: &BTreeMap<String, InitRecipe>,
        right: &BTreeMap<String, InitRecipe>,
    ) -> BTreeMap<String, InitRecipe> {
        left.iter()
            .filter_map(|(name, recipe)| {
                let other = right.get(name)?;
                let fact = recipe.fact.zip(other.fact).and_then(|(left, right)| {
                    join_envelope(left.envelope(), right.envelope()).map(IntegerEnvelopeFact::new)
                });
                Some((
                    name.clone(),
                    InitRecipe {
                        ty: recipe.ty.clone(),
                        fact,
                        literal: None,
                        known: None,
                    },
                ))
            })
            .collect()
    }
}

/// No engine enumerates an unbounded typed binder, with the engines' spelling.
/// A declared `Int` gives a typed binder its domain, as on the base. A struct so named
/// has none, and the engines refuse it as they refuse the builtin.
fn declares_finite_int(model: &KernelModel) -> bool {
    matches!(
        model.types.get("Int"),
        Some(TypeDef::Domain { .. } | TypeDef::Enum { .. })
    )
}

fn unbounded_typed_refusal(ty: &TypeRef) -> TypecheckError {
    let name = match ty {
        TypeRef::Named(name) => shown(name),
        _ => "Int".to_owned(),
    };
    TypecheckError {
        unprefixed_range_refusal: true,
        ..error(format!("type '{name}' is not a finite scalar"))
    }
}

/// Refuse a range binder that this scope cannot execute, with the existing spelling.
fn admit_range_binder(
    (lo, hi): (&Expr, &Expr),
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    rule: &RangeRule,
    site: BinderSite,
) -> Result<(), TypecheckError> {
    if rule.dead {
        return Ok(());
    }
    let plan = model.plan_integer_range(lo, hi, &rule.lexical(env));
    // Every supplied lexical fact is an unconditional enumerated value here.
    let executable = plan.is_executable(|prerequisite| match prerequisite {
        RangePrerequisite::StateCarrier { .. } => rule.carriers,
        RangePrerequisite::LexicalFact { .. } => true,
    });
    if executable {
        return Ok(());
    }
    Err(range_refusal(lo, hi, model, site).with_span(span))
}

/// The first expanding aggregate in a bound whose own bound is not static.
fn nested_aggregate_refusal(expr: &Expr, model: &KernelModel) -> Option<TypecheckError> {
    recursion::guard(|| {
        if let Expr::Aggregate {
            kind,
            binder: binder @ Binder::Range { lo, hi, .. },
            ..
        } = expr
            && ValidationPhase::expands_aggregate(*kind, binder)
        {
            return [lo.as_ref(), hi.as_ref()].into_iter().find_map(|endpoint| {
                nested_aggregate_refusal(endpoint, model)
                    .or_else(|| static_public_int(endpoint, model).err())
            });
        }
        crate::model::expr_children(expr)
            .into_iter()
            .find_map(|child| nested_aggregate_refusal(child, model))
    })
}

fn range_refusal(lo: &Expr, hi: &Expr, model: &KernelModel, site: BinderSite) -> TypecheckError {
    let endpoints = [lo, hi];
    // A dependent aggregate inside the bound is the refusal the bound always had.
    if let Some(refusal) = endpoints
        .into_iter()
        .find_map(|endpoint| nested_aggregate_refusal(endpoint, model))
    {
        return refusal;
    }
    if matches!(site, BinderSite::Aggregate) {
        return endpoints
            .into_iter()
            .find_map(|endpoint| static_public_int(endpoint, model).err())
            .unwrap_or_else(|| error("public Kernel requires static aggregate range bounds"));
    }
    let message = endpoints
        .into_iter()
        .find_map(|endpoint| {
            model
                .static_binder_bound(endpoint, &mut |name| shown(name))
                .err()
        })
        .unwrap_or_else(|| "binder bound is not a static integer".to_owned());
    TypecheckError {
        unprefixed_range_refusal: true,
        ..error(message)
    }
}

/// Walks a dependent leadsTo wrapper one binding at a time, holding one row only.
struct WrapperWalk<'a> {
    leadsto: &'a crate::LeadsToDef,
    model: &'a KernelModel,
    remaining: usize,
    completed: bool,
    reached: Vec<bool>,
    facts: BTreeMap<String, InclusiveI64>,
    binding: BTreeMap<String, crate::FslValue>,
    outer: BTreeSet<String>,
    locals: Vec<(String, TypeRef)>,
}

impl WrapperWalk<'_> {
    fn note(&mut self, name: &str, value: i64) {
        let Some(point) = InclusiveI64::new(value, value) else {
            return;
        };
        let joined = match self.facts.get(name) {
            Some(fact) => join_envelope(*fact, point),
            None => Some(point),
        };
        if let Some(joined) = joined {
            self.facts.insert(name.to_owned(), joined);
        }
    }

    /// `false` once the walk stops at its budget. Every visit spends it, whether or not
    /// it completes a binding: an always-empty inner binder completes none.
    fn visit(&mut self, index: usize) -> Result<bool, TypecheckError> {
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining == 0 {
            return Ok(false);
        }
        let leadsto = self.leadsto;
        let Some(binder) = leadsto.binders.get(index) else {
            self.completed = true;
            return Ok(true);
        };
        self.reached[index] = true;
        let name = binder_name(binder).to_owned();
        let (lo, hi) = match binder {
            Binder::Range { lo, hi, .. } => {
                let mut no_check = |_: usize, _: &Expr, _: Option<&BTreeMap<_, _>>| Ok(());
                let mut fold = |endpoint: &Expr, walk: &Self| {
                    crate::model::scoped_leadsto_int::<crate::ModelError>(
                        endpoint,
                        walk.model,
                        &walk.outer,
                        &walk.locals,
                        &walk.binding,
                        index,
                        &mut no_check,
                    )
                    .map_err(|failure| TypecheckError {
                        unprefixed_range_refusal: true,
                        span: Some(leadsto.span),
                        ..error(failure.message)
                    })
                };
                let lo = fold(lo, self)?;
                let hi = fold(hi, self)?;
                self.locals.push((name.clone(), TypeRef::Int));
                (lo, hi)
            }
            Binder::Typed { type_name, .. } => {
                let ty = self.model.typed_binder_type(&qualified_name(type_name)?);
                self.locals.push((name.clone(), ty.clone()));
                if let Ok(TypeRef::Range(lo, hi)) = resolve(self.model, &ty) {
                    (lo, hi)
                } else {
                    let values = self
                        .model
                        .domain_values(&ty)
                        .map_err(|failure| error(failure.message))?;
                    let inserted = self.outer.insert(name.clone());
                    for value in values {
                        self.binding.insert(name.clone(), value);
                        if !self.visit(index + 1)? {
                            return Ok(false);
                        }
                    }
                    if inserted {
                        self.outer.remove(&name);
                    }
                    self.locals.pop();
                    self.binding.remove(&name);
                    return Ok(true);
                }
            }
            Binder::Collection { .. } => return Ok(true),
        };
        let inserted = self.outer.insert(name.clone());
        for value in lo..=hi {
            self.note(&name, value);
            self.binding
                .insert(name.clone(), crate::FslValue::Int(value));
            if !self.visit(index + 1)? {
                return Ok(false);
            }
        }
        if inserted {
            self.outer.remove(&name);
        }
        self.locals.pop();
        self.binding.remove(&name);
        Ok(true)
    }
}

/// Bounds and values of a leadsTo wrapper, with the engines' scoping and refusals.
///
/// Facts come from folding each binding's bounds, as the engines do, with no row kept.
/// Past [`WRAPPER_ADMISSION_BINDINGS`] the walk stops, and later binders take the
/// planner's interval over what is known: admission must not enumerate the product.
#[allow(clippy::too_many_lines)]
fn admit_leadsto_binders(
    leadsto: &crate::LeadsToDef,
    model: &KernelModel,
) -> Result<RangeRule, TypecheckError> {
    let mut rule = RangeRule::default();
    let refused = |lo: &Expr, hi: &Expr| {
        let mut refusal = range_refusal(lo, hi, model, BinderSite::Quantifier);
        refusal.span = Some(leadsto.span);
        refusal
    };
    // Bounds without an outer binder are checked once, as the engines' prepass does.
    let mut outer = BTreeSet::new();
    let mut locals = Vec::new();
    for binder in &leadsto.binders {
        match binder {
            Binder::Range { lo, hi, .. } => {
                for endpoint in [lo, hi] {
                    if crate::model::leadsto_uses_outer(endpoint, &outer) {
                        crate::typecheck::validate_planner_integer_endpoint(
                            endpoint, &locals, model,
                        )
                        .map_err(|failure| TypecheckError {
                            unprefixed_range_refusal: true,
                            span: Some(leadsto.span),
                            ..error(failure.message)
                        })?;
                    } else if !model
                        .plan_integer_range(endpoint, endpoint, &[])
                        .is_executable(|_| false)
                    {
                        return Err(refused(lo, hi));
                    }
                }
                locals.push((binder_name(binder).to_owned(), TypeRef::Int));
            }
            Binder::Typed { type_name, .. } => {
                let written = qualified_name(type_name)?;
                let ty = model.typed_binder_type(&written);
                let undeclared_int = written == "Int" && !declares_finite_int(model);
                if undeclared_int || matches!(resolve(model, &ty), Ok(TypeRef::Int)) {
                    return Err(unbounded_typed_refusal(&ty).with_span(leadsto.span));
                }
                locals.push((binder_name(binder).to_owned(), ty));
            }
            Binder::Collection { .. } => {}
        }
        outer.insert(binder_name(binder).to_owned());
    }
    if !crate::model::leadsto_has_dependent_bounds(leadsto) {
        // Independent bounds need no enumeration of the wrapper's product.
        for binder in &leadsto.binders {
            match binder {
                Binder::Range { name, lo, hi, .. } => {
                    rule.bind_fact(name, model.plan_integer_range(lo, hi, &[]).candidates());
                }
                Binder::Typed {
                    name, type_name, ..
                } => {
                    let ty = model.typed_binder_type(&qualified_name(type_name)?);
                    if let Ok(TypeRef::Range(lo, hi)) = resolve(model, &ty) {
                        rule.bind_fact(name, InclusiveI64::new(lo, hi));
                    }
                }
                Binder::Collection { .. } => {}
            }
        }
        return Ok(rule);
    }
    let mut walk = WrapperWalk {
        leadsto,
        model,
        remaining: WRAPPER_ADMISSION_BINDINGS,
        completed: false,
        reached: vec![false; leadsto.binders.len()],
        facts: BTreeMap::new(),
        binding: BTreeMap::new(),
        outer: BTreeSet::new(),
        locals: Vec::new(),
    };
    let complete = walk.visit(0)?;
    if complete {
        rule.dead = !walk.completed;
    } else {
        for binder in &leadsto.binders {
            if let Binder::Typed {
                name, type_name, ..
            } = binder
            {
                let ty = model.typed_binder_type(&qualified_name(type_name)?);
                if let Ok(TypeRef::Range(lo, hi)) = resolve(model, &ty)
                    && let Some(envelope) = InclusiveI64::new(lo, hi)
                {
                    walk.facts.insert(name.clone(), envelope);
                }
            }
            if let Binder::Range { name, lo, hi, .. } = binder {
                let lexical: Vec<_> = leadsto
                    .binders
                    .iter()
                    .take_while(|prior| binder_name(prior) != name)
                    .filter_map(|prior| {
                        let name = binder_name(prior);
                        Some((name, IntegerEnvelopeFact::new(*walk.facts.get(name)?)))
                    })
                    .collect();
                let bindings: Vec<_> = lexical
                    .iter()
                    .map(|(name, fact)| crate::binder_range::RangeBinding {
                        name,
                        semantic_ty: &TypeRef::Int,
                        fact: Some(fact),
                    })
                    .collect();
                // A partial walk's values do not bound the rest: no plan, no fact.
                match model.plan_integer_range(lo, hi, &bindings).candidates() {
                    Some(envelope) => walk.facts.insert(name.clone(), envelope),
                    None => walk.facts.remove(name),
                };
            }
        }
    }
    for (name, envelope) in walk.facts {
        rule.facts.insert(name, IntegerEnvelopeFact::new(envelope));
    }
    Ok(rule)
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}

/// Cycle entry for expression validation, and where `recursion::guard` belongs.
/// Separate from [`infer_type`]'s cycle: this one descends the same tree again
/// to check operand well-formedness, so guarding inference alone leaves it
/// unguarded.
///
/// Measured: `refine` on the N=400 witness, with this function as the innermost
/// frame at ~26 KiB per level.
fn validate_expression(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    expected: Option<&TypeRef>,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    ValidationPhase::normalized().check(expr, env, model, span, expected, self_relations)
}

/// Check an original root actual with its authored fallback from the start.
/// Expanded definitions can carry their own ranges; only an inner range inside
/// this actual belongs to this diagnostic. Root file identity stays with the
/// private obligation, separate from the bare presentation origin.
pub(crate) fn validate_authored_actual(
    expr: &Expr,
    model: &KernelModel,
    bindings: &[(String, TypeRef)],
    expected: &TypeRef,
    authored: Span,
) -> Result<(), TypecheckError> {
    let env = expression_env(model, bindings);
    validate_expression(
        expr,
        &env,
        model,
        authored,
        Some(expected),
        &SelfRelations::new(),
    )
    .map_err(|mut error| {
        error.span = Some(
            error
                .span
                .filter(|inner| {
                    authored.start.offset <= inner.start.offset
                        && inner.start.offset <= inner.end.offset
                        && inner.end.offset <= authored.end.offset
                })
                .unwrap_or(authored),
        );
        error
    })
}

fn validate_expression_in_phase(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    expected: Option<&TypeRef>,
    phase: &ValidationPhase,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    // Statement-domain deferral never extends into RHS/filter expressions.
    let expression_phase = phase.defer_init_statement_domain.then(|| ValidationPhase {
        defer_init_statement_domain: false,
        ..phase.clone()
    });
    let phase = expression_phase.as_ref().unwrap_or(phase);
    recursion::guard(|| {
        validate_expression_inner(expr, env, model, span, expected, phase, self_relations)
    })
    .map_err(|error| error.with_span(span))
}

fn validate_relation_endpoint(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    expected: &TypeRef,
    phase: &ValidationPhase,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    expression_type(expr, env, model, span, Some(expected)).map_err(|mut error| {
        error.message.push_str(
            "; hint: use endpoints matching the relation's declared source and target types",
        );
        error
    })?;
    validate_expression_in_phase(
        expr,
        env,
        model,
        span,
        Some(expected),
        phase,
        self_relations,
    )
}

fn relation_operand_types(
    name: &str,
    operand: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
) -> Result<(TypeRef, TypeRef), TypecheckError> {
    let public_name = name.strip_prefix("rel_").unwrap_or(name);
    match resolve(model, &infer_type(operand, env, model, None)?)? {
        TypeRef::Relation(source, target) => Ok((*source, *target)),
        _ => Err(error(format!(
            "{public_name}() requires a relation; hint: use a relation A -> B operand"
        ))),
    }
}

fn validate_self_relation(
    name: &str,
    source: &TypeRef,
    target: &TypeRef,
    operand: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    let ty = TypeRef::Relation(Box::new(source.clone()), Box::new(target.clone()));
    if !relation_is_always_self(operand, env, model, &ty, self_relations)? {
        let public_name = name.strip_prefix("rel_").unwrap_or(name);
        return Err(error(format!(
            "{public_name}() requires a self-relation (relation T -> T); hint: use a relation with the same declared source and target type"
        )));
    }
    Ok(())
}

fn relation_is_always_self(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    ty: &TypeRef,
    self_relations: &SelfRelations,
) -> Result<bool, TypecheckError> {
    recursion::guard(|| match expr {
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let then_ty = infer_type(then_expr, env, model, Some(ty))?;
            let else_ty = infer_type(else_expr, env, model, Some(ty))?;
            Ok(
                relation_is_always_self(then_expr, env, model, &then_ty, self_relations)?
                    && relation_is_always_self(else_expr, env, model, &else_ty, self_relations)?,
            )
        }
        Expr::UnaryNamed { name, expr, .. } if name == "old" => {
            relation_is_always_self(expr, env, model, ty, self_relations)
        }
        Expr::Method { receiver, name, .. } if matches!(name.as_str(), "add" | "remove") => {
            let receiver_ty = infer_type(receiver, env, model, None)?;
            relation_is_always_self(receiver, env, model, &receiver_ty, self_relations)
        }
        Expr::Var(name) if self_relations.contains_key(name) => Ok(self_relations[name]),
        _ => match resolve(model, ty)? {
            TypeRef::Relation(source, target) => Ok(source == target),
            _ => Err(error("self-relation operand requires a relation")),
        },
    })
}

fn validate_method_signature(
    receiver: &TypeRef,
    name: &str,
    arity: usize,
) -> Result<(), TypecheckError> {
    match (receiver, name, arity) {
        (TypeRef::Set(_) | TypeRef::Seq(_, _), "contains", 1)
        | (TypeRef::Set(_), "add" | "remove", 1)
        | (TypeRef::Set(_) | TypeRef::Seq(_, _), "size", 0)
        | (TypeRef::Seq(_, _), "push" | "at", 1)
        | (TypeRef::Seq(_, _), "pop" | "head", 0)
        | (TypeRef::Relation(_, _), "contains" | "add" | "remove", 2) => Ok(()),
        (TypeRef::Set(_), _, _) => Err(error(format!("invalid Set method '{name}'/{arity}"))),
        (TypeRef::Seq(_, _), _, _) => Err(error(format!("invalid Seq method '{name}'/{arity}"))),
        (TypeRef::Relation(_, _), _, _) => {
            let hint = if matches!(name, "contains" | "add" | "remove") {
                format!("; hint: use r.{name}(source, target)")
            } else {
                String::new()
            };
            Err(error(format!(
                "invalid relation method '{name}'/{arity}{hint}"
            )))
        }
        _ => Err(error("method receiver has no supported collection methods")),
    }
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn validate_expression_inner(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    expected: Option<&TypeRef>,
    phase: &ValidationPhase,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    let validate = |expr: &Expr, env: &TypeEnv, span: Span, expected: Option<&TypeRef>| {
        validate_expression_in_phase(expr, env, model, span, expected, phase, self_relations)
    };
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
            validate(item, env, span, Some(&inner))?;
        }
        Expr::Set(items) if matches!(resolve(model, &ty)?, TypeRef::Relation(_, _)) => {
            // Relation literals only support the empty initializer `Set {}`;
            // emptiness is already enforced by `infer_type`/`ensure_assignable`
            // (there is no per-element type to validate against).
            debug_assert!(items.is_empty());
        }
        Expr::Set(items) | Expr::Seq(items) => {
            let item_ty = collection_item_type(&ty, model)?;
            for item in items {
                validate(item, env, span, Some(&item_ty))?;
            }
        }
        Expr::Struct { name, fields } => {
            for (field, value) in fields {
                let expected = struct_field_type(model, name, field)?;
                validate(value, env, span, Some(&expected))?;
            }
        }
        Expr::Index(collection, index) => {
            let key_ty = index_key_type(collection, env, model)?;
            validate(collection, env, span, None)?;
            validate(index, env, span, Some(&key_ty))?;
        }
        Expr::Field(value, _) => validate(value, env, span, None)?,
        Expr::Method {
            receiver,
            name,
            args,
        } => {
            let receiver_ty = resolve(model, &infer_type(receiver, env, model, None)?)?;
            let argument_type = method_argument_type(name, &receiver_ty);
            validate(receiver, env, span, None)?;
            validate_method_signature(&receiver_ty, name, args.len())
                .map_err(|error| error.with_span(span))?;
            for (index, argument) in args.iter().enumerate() {
                let endpoint_type = match (name.as_str(), &receiver_ty, index) {
                    ("contains" | "add" | "remove", TypeRef::Relation(source, _), 0) => {
                        Some(source.as_ref())
                    }
                    ("contains" | "add" | "remove", TypeRef::Relation(_, target), 1) => {
                        Some(target.as_ref())
                    }
                    _ => None,
                };
                if let Some(endpoint_type) = endpoint_type {
                    validate_relation_endpoint(
                        argument,
                        env,
                        model,
                        span,
                        endpoint_type,
                        phase,
                        self_relations,
                    )?;
                } else {
                    validate(argument, env, span, argument_type.as_ref())?;
                }
            }
        }
        Expr::Binary { op, left, right } => {
            let (left_ty, right_ty) = binary_operand_types(op, left, right, env, model)?;
            validate(left, env, span, Some(&left_ty))?;
            let mut right_env = env.clone();
            if crate::pattern_binding_flows(op, true) {
                extend_pattern_binding(left, &mut right_env, model)?;
            }
            phase.after_patterns(left).check(
                right,
                &right_env,
                model,
                span,
                Some(&right_ty),
                self_relations,
            )?;
        }
        Expr::Neg(operand) => {
            validate(operand, env, span, Some(&TypeRef::Int))?;
        }
        Expr::Not(operand) => {
            validate(operand, env, span, Some(&TypeRef::Bool))?;
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => {
            validate(condition, env, spans.condition, Some(&TypeRef::Bool))?;
            let then_phase = phase.after_patterns(condition);
            let else_phase = then_phase.after_patterns(then_expr);
            // Admission requires mutually compatible arms. Compatibility is an
            // equivalence only over scalars: with `Int` inside a container, an
            // arm can match the expectation but not its sibling, so a container
            // conditional keeps the first arm's carrier. A Bool, ranged or named
            // expectation admits the same arms and lets a diagnostic name the
            // supplied type; `Int` would only hide a more specific first arm.
            let branch_type = match expected {
                Some(expected)
                    if matches!(
                        resolve(model, expected)?,
                        TypeRef::Bool | TypeRef::Range(_, _) | TypeRef::Named(_)
                    ) =>
                {
                    expected
                }
                _ => &ty,
            };
            for (expr, span, phase) in [
                (then_expr, spans.then_expr, &then_phase),
                (else_expr, spans.else_expr, &else_phase),
            ] {
                phase.check(expr, env, model, span, Some(branch_type), self_relations)?;
                // An `Int` first arm is compatible with both its sibling and the
                // expectation without relating them, and symbolic evaluation
                // coerces every arm to the expectation (#1679).
                if phase.symbolic
                    && let Some(expected) = expected
                    && is_coerced_container(expected, model)?
                {
                    ensure_assignable(expr, expected, env, model, span)?;
                }
            }
        }
        Expr::Is { expr, pattern } => {
            if let Pattern::Some(binding) = pattern {
                reject_colliding_pattern_binding(binding, env, model, span, phase)?;
            }
            validate(expr, env, span, None)?;
            if !matches!(
                resolve(model, &infer_type(expr, env, model, None)?)?,
                TypeRef::Option(_)
            ) {
                return Err(error("'is none/some' requires an Option expression").with_span(span));
            }
        }
        Expr::UnaryNamed { name, expr, .. } if name == "old" => {
            validate_expression_in_phase(
                expr,
                env,
                model,
                span,
                None,
                &phase.inside_old(),
                self_relations,
            )?;
            // `old` keeps its operand's type, and symbolic evaluation coerces the
            // operand's arms to what surrounds it (#1679). This is the arm rule above.
            if phase.symbolic
                && let Some(expected) = expected
                && is_coerced_container(expected, model)?
            {
                ensure_assignable(expr, expected, env, model, span)?;
            }
            // Evaluation types the operand from what surrounds it too (#1683), so a Map
            // a literal holds there answers to that, where `infer_type` only sees the
            // literal take the expectation. This does not admit the literal it needs to
            // type: the walk above still decides that.
            if phase.symbolic && expected.is_some() {
                validate_expression_in_phase(
                    expr,
                    env,
                    model,
                    span,
                    expected,
                    &phase.inside_old(),
                    self_relations,
                )?;
            }
        }
        Expr::UnaryNamed { name, expr, .. } => {
            let inner_expected = (name == "abs").then_some(TypeRef::Int);
            validate(expr, env, span, inner_expected.as_ref())?;
            if matches!(
                name.as_str(),
                "rel_acyclic" | "rel_functional" | "rel_injective" | "rel_domain" | "rel_range"
            ) {
                let (source, target) = relation_operand_types(name, expr, env, model)
                    .map_err(|error| error.with_span(span))?;
                if name == "rel_acyclic" {
                    validate_self_relation(
                        name,
                        &source,
                        &target,
                        expr,
                        env,
                        model,
                        self_relations,
                    )
                    .map_err(|error| error.with_span(span))?;
                }
            }
        }
        Expr::Quantified { binder, body, .. } => {
            let child_phase = phase
                .under_binder(binder, None)
                .bound_by(binder, env, model);
            let binder_ty = validate_binder(
                binder,
                env,
                model,
                span,
                phase,
                &child_phase,
                BinderSite::Quantifier,
                self_relations,
            )?;
            let mut local = env.clone();
            local.insert(binder_name(binder).to_owned(), binder_ty);
            let mut local_relations = self_relations.clone();
            local_relations.remove(binder_name(binder));
            validate_expression_in_phase(
                body,
                &local,
                model,
                span,
                Some(&TypeRef::Bool),
                &child_phase.after_where(binder),
                &local_relations,
            )?;
        }
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => {
            let expands = ValidationPhase::expands_aggregate(*kind, binder);
            // Raw descendants share all typing rules, but must not enumerate
            // ranges containing locals before the outer substitution happens.
            let child_phase = phase
                .under_binder(binder, Some(*kind))
                .bound_by(binder, env, model);
            let binder_ty = validate_binder(
                binder,
                env,
                model,
                span,
                phase,
                &child_phase,
                if expands {
                    BinderSite::Aggregate
                } else {
                    BinderSite::Quantifier
                },
                self_relations,
            )?;
            let mut local = env.clone();
            local.insert(binder_name(binder).to_owned(), binder_ty);
            let mut local_relations = self_relations.clone();
            local_relations.remove(binder_name(binder));
            if let Some(value) = value {
                validate_expression_in_phase(
                    value,
                    &local,
                    model,
                    span,
                    Some(&TypeRef::Int),
                    &child_phase.after_where(binder),
                    &local_relations,
                )?;
            }
            if expands && (phase.normalize_aggregates || phase.aggregate_bounds_only) {
                match aggregate_validation(
                    *kind,
                    binder,
                    value.as_deref(),
                    env,
                    model,
                    phase.normalize_aggregates,
                )
                .map_err(|error| error.with_span(span))?
                {
                    AggregateValidation::Expanded(normalized) => {
                        // Generated copies have already passed authored binding checks.
                        let mut generated = phase.clone();
                        crate::collect_pattern_names(&normalized, &mut generated.collisions);
                        generated.substituted.extend(generated.collisions.drain());
                        generated.check(&normalized, env, model, span, expected, self_relations)?;
                    }
                    AggregateValidation::Deferred if !phase.aggregate_bounds_only => {
                        // The complete authored typing pass has succeeded. Revisit
                        // nested bounds without expansion or ordinary value folding;
                        // this phase cannot request another authored-tree replay.
                        let mut bounds = phase.clone();
                        bounds.normalize_aggregates = false;
                        bounds.aggregate_bounds_only = true;
                        validate_expression_in_phase(
                            expr,
                            env,
                            model,
                            span,
                            expected,
                            &bounds,
                            self_relations,
                        )?;
                    }
                    AggregateValidation::Deferred => {}
                }
            }
        }
        Expr::BinaryNamed { left, right, .. } => {
            validate(left, env, span, Some(&TypeRef::Int))?;
            validate(right, env, span, Some(&TypeRef::Int))?;
        }
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => {
            validate(first, env, span, None)?;
            if name == "rel_reachable" {
                let (source, target) = relation_operand_types(name, first, env, model)
                    .map_err(|error| error.with_span(span))?;
                validate_self_relation(name, &source, &target, first, env, model, self_relations)
                    .map_err(|error| error.with_span(span))?;
                validate_relation_endpoint(
                    second,
                    env,
                    model,
                    span,
                    &source,
                    phase,
                    self_relations,
                )?;
                validate_relation_endpoint(
                    third,
                    env,
                    model,
                    span,
                    &target,
                    phase,
                    self_relations,
                )?;
            } else {
                validate(second, env, span, None)?;
                validate(third, env, span, None)?;
            }
        }
    }
    // After its parts, so that a conditional reports the arm at fault.
    if phase.symbolic
        && let Some(expected) = expected
        && carrier_coercion(&ty, expected, model).map_err(|error| error.with_span(span))?
            == CarrierCoercion::MapMismatch
    {
        return Err(not_assignable(&ty, expected).with_span(span));
    }
    Ok(())
}

fn validate_statements(
    statements: &[Statement],
    env: &TypeEnv,
    model: &KernelModel,
    phase: &ValidationPhase,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    let mut phase = phase.clone();
    for statement in statements {
        validate_statement(statement, env, model, &phase, self_relations)?;
        crate::compose::extend_statement_pattern_names(statement, &mut phase.collisions);
    }
    Ok(())
}

fn validate_statement(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
    phase: &ValidationPhase,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => {
            let ty = lvalue_type(target, env, model, *span)?;
            ensure_assignable(value, &ty, env, model, *span)?;
            validate_lvalue(target, env, model, *span, phase, self_relations)?;
            phase.check(value, env, model, *span, Some(&ty), self_relations)
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            phase.check(
                condition,
                env,
                model,
                *span,
                Some(&TypeRef::Bool),
                self_relations,
            )?;
            let phase = phase.after_patterns(condition);
            validate_statements(then_statements, env, model, &phase, self_relations)?;
            validate_statements(else_statements, env, model, &phase, self_relations)
        }
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let child = phase.clone().bound_by(binder, env, model);
            let ty = validate_binder(
                binder,
                env,
                model,
                *span,
                phase,
                &child,
                BinderSite::Statement,
                self_relations,
            )?;
            let mut local = env.clone();
            local.insert(binder_name(binder).to_owned(), ty);
            let mut local_relations = self_relations.clone();
            local_relations.remove(binder_name(binder));
            let phase = child.after_where(binder);
            validate_statements(statements, &local, model, &phase, &local_relations)
        }
    }
}

fn validate_lvalue(
    target: &LValue,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    phase: &ValidationPhase,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    match target {
        LValue::Var(_) => Ok(()),
        LValue::Index(name, index) => {
            let Some((key, _)) = resolve(
                model,
                env.get(name)
                    .ok_or_else(|| error(format!("unknown update target '{}'", shown(name))))?,
            )?
            .index_types(false) else {
                return Err(error("indexed update target requires Map"));
            };
            phase.check(index, env, model, span, Some(&key), self_relations)
        }
        LValue::Field(base, _) => validate_lvalue(base, env, model, span, phase, self_relations),
    }
}

fn validate_map_key_const_shadowing(
    target: &LValue,
    env: &TypeEnv,
    model: &KernelModel,
    visible_consts: &BTreeSet<String>,
    span: Span,
) -> Result<(), TypecheckError> {
    match target {
        LValue::Var(_) => Ok(()),
        LValue::Index(name, index) => {
            let Some((key, _)) = resolve(
                model,
                env.get(name)
                    .ok_or_else(|| error(format!("unknown update target '{}'", shown(name))))?,
            )?
            .index_types(false) else {
                return Ok(());
            };
            let (Expr::Var(member), TypeRef::Named(enum_name)) = (index, &key) else {
                return Ok(());
            };
            let Some(TypeDef::Enum { members, .. }) = model.types.get(enum_name) else {
                return Ok(());
            };
            let Some(value) = model.consts.get(member) else {
                return Ok(());
            };
            if !visible_consts.contains(member) || !members.contains(member) {
                return Ok(());
            }
            let (value, actual) = match value {
                crate::FslValue::Int(value) => (value.to_string(), TypeRef::Int),
                crate::FslValue::Bool(value) => (value.to_string(), TypeRef::Bool),
                _ => return Ok(()),
            };
            Err(error(format!(
                "expression of type {actual:?} is not assignable to {key:?}; cause: map key '{member}' resolves to const {member} = {value} ({actual:?}), not to member {member} of enum {enum_name} because a const shadows an enum member of the same name; hint: rename the const or enum member to make the map key unambiguous"
            ))
            .with_span(span))
        }
        LValue::Field(base, _) => {
            validate_map_key_const_shadowing(base, env, model, visible_consts, span)
        }
    }
}

fn validate_statement_assignments(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
    visible_consts: &BTreeSet<String>,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    match statement {
        Statement::Assign {
            target,
            value,
            span,
        } => {
            let ty = lvalue_type(target, env, model, *span)?;
            ensure_assignable(value, &ty, env, model, *span)?;
            validate_map_key_const_shadowing(target, env, model, visible_consts, *span)?;
            validate_lvalue(
                target,
                env,
                model,
                *span,
                &ValidationPhase::normalized(),
                self_relations,
            )?;
            validate_expression(value, env, model, *span, Some(&ty), self_relations)
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            validate_expression(
                condition,
                env,
                model,
                *span,
                Some(&TypeRef::Bool),
                self_relations,
            )?;
            then_statements
                .iter()
                .chain(else_statements)
                .try_for_each(|item| {
                    validate_statement_assignments(item, env, model, visible_consts, self_relations)
                })
        }
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            let name = binder_name(binder);
            let phase = ValidationPhase::init_statements();
            let ty = validate_binder(
                binder,
                env,
                model,
                *span,
                &phase,
                &phase,
                BinderSite::Statement,
                self_relations,
            )?;
            let mut local = env.clone();
            local.insert(name.to_owned(), ty);
            let mut local_relations = self_relations.clone();
            local_relations.remove(name);
            let mut local_visible_consts = visible_consts.clone();
            local_visible_consts.remove(name);
            statements.iter().try_for_each(|item| {
                validate_statement_assignments(
                    item,
                    &local,
                    model,
                    &local_visible_consts,
                    &local_relations,
                )
            })
        }
    }
}

pub(crate) fn validate_expression_type(
    expr: &Expr,
    expected: &TypeRef,
    bindings: &[(String, TypeRef)],
    model: &KernelModel,
) -> Result<(), TypecheckError> {
    validate_expression_type_between(expr, expected, bindings, model, model)
}

pub(crate) fn validate_expression_type_between(
    expr: &Expr,
    expected: &TypeRef,
    bindings: &[(String, TypeRef)],
    model: &KernelModel,
    expected_model: &KernelModel,
) -> Result<(), TypecheckError> {
    validate_expression_type_in_phase(
        expr,
        expected,
        bindings,
        model,
        expected_model,
        &ValidationPhase::concrete_only(),
    )
}

/// Full authored typing for the pure planner, without candidate enumeration.
pub(crate) fn validate_planner_integer_endpoint(
    expr: &Expr,
    bindings: &[(String, TypeRef)],
    model: &KernelModel,
) -> Result<(), TypecheckError> {
    // The planner evaluates an endpoint itself and reaches no symbolic coercion, and it
    // serves concretely evaluated positions too. A model's own binder answers to the
    // carrier rule in its ordinary walk (#1679).
    let mut phase = ValidationPhase::concrete_only();
    phase.normalize_aggregates = false;
    validate_expression_type_in_phase(expr, &TypeRef::Int, bindings, model, model, &phase)
}

fn validate_expression_type_in_phase(
    expr: &Expr,
    expected: &TypeRef,
    bindings: &[(String, TypeRef)],
    model: &KernelModel,
    expected_model: &KernelModel,
    phase: &ValidationPhase,
) -> Result<(), TypecheckError> {
    let mut env = base_env(model);
    env.extend_lexical(bindings);
    let span = unknown_span();
    let resolved = carrier_type(expected, expected_model).map_err(|error| error.with_span(span))?;
    let actual =
        infer_type(expr, &env, model, Some(&resolved)).map_err(|error| error.with_span(span))?;
    if !types_compatible(&actual, &resolved, model).map_err(|error| error.with_span(span))? {
        return Err(not_assignable(&actual, expected).with_span(span));
    }
    phase
        .check(
            expr,
            &env,
            model,
            span,
            Some(&resolved),
            &SelfRelations::new(),
        )
        .map_err(|error| error.with_span(span))
}

/// Recursively type-check a standalone predicate against a built model, using
/// the same expected Boolean type as an invariant but not its #1679 carrier rule:
/// a mismatch that evaluation reaches is rejected at the proof stage with
/// `cannot coerce`, and one it never reaches can still be proved and used. Returns
/// the checker message without a source location: callers may be validating an
/// argv expression.
///
/// # Errors
/// Returns the recursive checker's diagnostic if the expression is ill-typed
/// or cannot be assigned to `Bool` in the model's environment.
pub fn validate_boolean_expression(expr: &Expr, model: &KernelModel) -> Result<(), String> {
    validate_public_expression(expr, model, unknown_span(), Some(&TypeRef::Bool))
        .map_err(|error| error.message)
}

pub(crate) fn validate_public_expression(
    expr: &Expr,
    model: &KernelModel,
    span: Span,
    expected: Option<&TypeRef>,
) -> Result<(), TypecheckError> {
    ValidationPhase::concrete_only().check(
        expr,
        &base_env(model),
        model,
        span,
        expected,
        &SelfRelations::new(),
    )
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
    phase: &mut ValidationPhase,
) -> Result<(), TypecheckError> {
    let self_relations = SelfRelations::new();
    let visible_consts = model
        .consts
        .keys()
        .filter(|name| {
            !model
                .state
                .iter()
                .any(|(state_name, _)| state_name == *name)
        })
        .cloned()
        .collect();
    validate_statement_assignments(
        statement,
        &base_env(model),
        model,
        &visible_consts,
        &self_relations,
    )?;
    validate_statement(statement, &base_env(model), model, phase, &self_relations)?;
    crate::compose::extend_statement_pattern_names(statement, &mut phase.collisions);
    Ok(())
}

pub(crate) fn validate_checked_model_types(model: &KernelModel) -> Result<(), TypecheckError> {
    let env = base_env(model);
    let self_relations = SelfRelations::new();
    let mut phase = ValidationPhase::init_statements();
    for statement in &model.init {
        validate_statement(statement, &env, model, &phase, &self_relations)?;
        crate::compose::extend_statement_pattern_names(statement, &mut phase.collisions);
    }
    validate_model_expression_types(model)?;
    for projection in &model.projections {
        validate_expression(
            &projection.expr,
            &env,
            model,
            projection.span,
            Some(&TypeRef::Int),
            &self_relations,
        )?;
    }
    Ok(())
}

fn validate_leadsto_expression_types(
    leadsto: &crate::LeadsToDef,
    env: &TypeEnv,
    model: &KernelModel,
    admission: Option<&Admission>,
) -> Result<(), TypecheckError> {
    if leadsto.within.is_some_and(|within| within < 0) {
        return Err(error(format!(
            "leadsTo '{}' within must be non-negative",
            shown(&leadsto.name)
        ))
        .with_span(leadsto.span));
    }
    let mut local = env.clone();
    let self_relations = SelfRelations::new();
    let mut body_phase = ValidationPhase::normalized();
    if let Some(admission) = admission {
        let mut rule = admit_leadsto_binders(leadsto, model)?;
        rule.symbolic_sets.clone_from(&admission.symbolic_sets);
        rule.static_aggregates_only = admission.static_aggregates_only;
        body_phase = ValidationPhase::range_admission(false, admission);
        body_phase.range_rule = Some(rule);
    }
    for binder in &leadsto.binders {
        let phase = ValidationPhase::normalized();
        let ty = validate_binder(
            binder,
            &local,
            model,
            leadsto.span,
            &phase,
            &phase,
            BinderSite::Quantifier,
            &self_relations,
        )?;
        local.insert(binder_name(binder).to_owned(), ty);
    }
    for expr in [&leadsto.before, &leadsto.after] {
        body_phase.check(
            expr,
            &local,
            model,
            leadsto.span,
            Some(&TypeRef::Bool),
            &self_relations,
        )?;
    }
    if let Some(expr) = &leadsto.decreases {
        body_phase.check(
            expr,
            &local,
            model,
            leadsto.span,
            Some(&TypeRef::Int),
            &self_relations,
        )?;
    }
    for helper in &leadsto.helpful {
        let Some(action) = model
            .actions
            .iter()
            .find(|action| action.name == helper.action)
        else {
            return Err(error(format!(
                "leadsTo '{}' helpful action '{}' is not declared",
                shown(&leadsto.name),
                shown(&helper.action)
            ))
            .with_span(helper.span));
        };
        if helper.args.len() != action.params.len() {
            return Err(error(format!(
                "leadsTo '{}' helpful action '{}' expects {} argument(s), got {}",
                shown(&leadsto.name),
                shown(&helper.action),
                action.params.len(),
                helper.args.len()
            ))
            .with_span(helper.span));
        }
        for (argument, param) in helper.args.iter().zip(&action.params) {
            body_phase.check(
                argument,
                &local,
                model,
                helper.span,
                Some(&param.ty()),
                &self_relations,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(crate) fn validate_model_expression_types(model: &KernelModel) -> Result<(), TypecheckError> {
    validate_model_expressions(model, None)
}

/// Apply the shared range rule to every action and property scope.
pub(crate) fn admit_model_ranges(model: &KernelModel) -> Result<(), TypecheckError> {
    validate_model_expressions(model, Some(&Admission::new(model, false)))
}

/// Where a projection stops on an aggregate bound it cannot fold, if it does.
pub(crate) fn locate_unfolded_aggregate(model: &KernelModel) -> Result<(), TypecheckError> {
    validate_model_expressions(model, Some(&Admission::new(model, true)))
}

/// State Sets that can hold a key the engines do not read as a solver literal.
#[derive(Clone, Default)]
struct SymbolicSets {
    /// In some state, the initial one included.
    in_any_state: BTreeSet<String>,
    /// In the state an action leaves, which is all a postcondition reads.
    after_action: BTreeSet<String>,
}

/// What one admission walk judges.
struct Admission {
    symbolic_sets: SymbolicSets,
    static_aggregates_only: bool,
}

impl Admission {
    fn new(model: &KernelModel, static_aggregates_only: bool) -> Self {
        Self {
            symbolic_sets: state_sets_with_symbolic_keys(model),
            static_aggregates_only,
        }
    }
}

/// One assignment to a state Set, with the local names that hold solver literals there.
struct SetWrite<'a> {
    target: &'a str,
    value: &'a Expr,
    literals: BTreeSet<String>,
    aliases: BTreeMap<String, &'a Expr>,
}

fn collect_set_writes<'a>(
    statements: &'a [Statement],
    scope: &(BTreeSet<String>, BTreeMap<String, &'a Expr>),
    model: &KernelModel,
    writes: &mut Vec<SetWrite<'a>>,
) {
    for statement in statements {
        match statement {
            Statement::Assign {
                target: LValue::Var(name),
                value,
                ..
            } if model
                .state
                .iter()
                .any(|(state, ty)| state == name && matches!(ty, TypeRef::Set(_))) =>
            {
                writes.push(SetWrite {
                    target: name,
                    value,
                    literals: scope.0.clone(),
                    aliases: scope.1.clone(),
                });
            }
            Statement::Assign { .. } => {}
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                collect_set_writes(then_statements, scope, model, writes);
                collect_set_writes(else_statements, scope, model, writes);
            }
            Statement::ForAll {
                binder, statements, ..
            } => {
                let mut inner = scope.clone();
                inner.1.remove(binder_name(binder));
                if matches!(binder, Binder::Collection { .. }) {
                    inner.0.remove(binder_name(binder));
                } else {
                    inner.0.insert(binder_name(binder).to_owned());
                }
                collect_set_writes(statements, &inner, model, writes);
            }
        }
    }
}

/// Whether every key of an assigned value is a solver literal, given the Sets known not to be.
fn writes_literal_keys(
    value: &Expr,
    write: &SetWrite<'_>,
    symbolic: &BTreeSet<String>,
    model: &KernelModel,
) -> bool {
    recursion::guard(|| {
        let literal = |item: &Expr| match item {
            Expr::Num(_) => true,
            Expr::Var(name) if write.literals.contains(name) => true,
            Expr::Var(name) => {
                !model.state.iter().any(|(state, _)| state == name)
                    && matches!(model.consts.get(name), Some(crate::FslValue::Int(_)))
            }
            _ => false,
        };
        match value {
            Expr::Set(items) => items.iter().all(literal),
            Expr::Var(name) => write.aliases.get(name).map_or_else(
                || !symbolic.contains(name),
                |alias| writes_literal_keys(alias, write, symbolic, model),
            ),
            Expr::UnaryNamed { name, expr, .. } if name == "old" => {
                writes_literal_keys(expr, write, symbolic, model)
            }
            Expr::Method {
                receiver,
                name,
                args,
            } if name == "add" || name == "remove" => {
                writes_literal_keys(receiver, write, symbolic, model)
                    && (name == "remove" || args.iter().all(literal))
            }
            Expr::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                writes_literal_keys(then_expr, write, symbolic, model)
                    && writes_literal_keys(else_expr, write, symbolic, model)
            }
            _ => false,
        }
    })
}

/// The state Sets a write can give a key the engines do not read as a solver literal.
/// Init is a fixed point, because an init copy carries what init assigned. An action
/// reads every state from a reframed frame, whose keys are the element domain's, so a
/// copy there carries nothing and only the action's own expression decides.
fn state_sets_with_symbolic_keys(model: &KernelModel) -> SymbolicSets {
    let mut init_writes = Vec::new();
    // Init rewrites a state read after a literal assignment to that literal.
    let mut init_scope: (BTreeSet<String>, BTreeMap<String, &Expr>) = Default::default();
    for statement in &model.init {
        collect_set_writes(
            std::slice::from_ref(statement),
            &init_scope,
            model,
            &mut init_writes,
        );
        if let Statement::Assign {
            target: LValue::Var(name),
            value,
            ..
        } = statement
        {
            if matches!(value, Expr::Num(_)) {
                init_scope.0.insert(name.clone());
            } else {
                init_scope.0.remove(name);
            }
        }
    }
    let mut action_writes = Vec::new();
    for action in &model.actions {
        let mut scope: (BTreeSet<String>, BTreeMap<String, &Expr>) = Default::default();
        scope
            .0
            .extend(action.params.iter().map(|param| param.name().to_owned()));
        for guard in &action.guards {
            if let crate::ActionGuard::Let(name, value) = guard {
                scope.0.remove(name);
                scope.1.insert(name.clone(), value);
                if matches!(value, Expr::Num(_)) {
                    scope.0.insert(name.clone());
                }
            }
        }
        collect_set_writes(&action.statements, &scope, model, &mut action_writes);
    }
    let mut in_any_state = BTreeSet::new();
    loop {
        let before = in_any_state.len();
        for write in &init_writes {
            if !writes_literal_keys(write.value, write, &in_any_state, model) {
                in_any_state.insert(write.target.to_owned());
            }
        }
        if in_any_state.len() == before {
            break;
        }
    }
    let after_action: BTreeSet<String> = action_writes
        .iter()
        .filter(|write| !writes_literal_keys(write.value, write, &BTreeSet::new(), model))
        .map(|write| write.target.to_owned())
        .collect();
    in_any_state.extend(after_action.iter().cloned());
    SymbolicSets {
        in_any_state,
        after_action,
    }
}

#[allow(clippy::too_many_lines)]
fn validate_model_expressions(
    model: &KernelModel,
    admission: Option<&Admission>,
) -> Result<(), TypecheckError> {
    let env = base_env(model);
    let self_relations = SelfRelations::new();
    for action in &model.actions {
        let checked = (|| {
            let mut local = env.clone();
            let mut local_relations = SelfRelations::new();
            let mut phase = ValidationPhase::for_scope(admission, true);
            for param in &action.params {
                reject_constant_shadow(param.name(), model.consts.keys())?;
                local.insert(param.name().to_owned(), param.ty());
                if let Some(rule) = &mut phase.range_rule {
                    rule.bind_param(param, model);
                }
            }
            for (index, guard) in action.guards.iter().enumerate() {
                let target = crate::action_guard_target(&action.name, index);
                let span = model.component_item_span(&target).unwrap_or(action.span);
                let checked = (|| {
                    match guard {
                        ActionGuard::Requires(expr) => {
                            phase.check(
                                expr,
                                &local,
                                model,
                                span,
                                Some(&TypeRef::Bool),
                                &local_relations,
                            )?;
                            extend_pattern_binding(expr, &mut local, model)?;
                            phase = phase.after_patterns(expr);
                        }
                        ActionGuard::Let(name, expr) => {
                            reject_constant_shadow(name, model.consts.keys())?;
                            phase.check(expr, &local, model, span, None, &local_relations)?;
                            let ty = expression_type(expr, &local, model, span, None)?;
                            if matches!(
                                resolve(model, &ty).map_err(|error| error.with_span(span))?,
                                TypeRef::Relation(_, _)
                            ) {
                                let is_self = relation_is_always_self(
                                    expr,
                                    &local,
                                    model,
                                    &ty,
                                    &local_relations,
                                )
                                .map_err(|error| error.with_span(span))?;
                                local_relations.insert(name.clone(), is_self);
                            } else {
                                local_relations.remove(name);
                            }
                            if let Some(rule) = &mut phase.range_rule {
                                rule.bind_let(name, expr, &local, model);
                            }
                            local.insert(name.clone(), ty);
                        }
                    }
                    Ok(())
                })();
                checked.map_err(|error: TypecheckError| error.in_component_item(model, &target))?;
            }
            let mut order = phase.clone();
            for (index, statement) in action.statements.iter().enumerate() {
                validate_statement(statement, &local, model, &order, &local_relations).map_err(
                    |error| {
                        error.in_component_item(
                            model,
                            &crate::action_statement_target(&action.name, index),
                        )
                    },
                )?;
                crate::compose::extend_statement_pattern_names(statement, &mut order.collisions);
            }
            for (index, expr) in action.ensures.iter().enumerate() {
                let target = crate::origin::action_ensure_target(&action.name, index);
                let span = model.component_item_span(&target).unwrap_or(action.span);
                phase
                    .for_postcondition()
                    .with_old_frame()
                    .check(
                        expr,
                        &local,
                        model,
                        span,
                        Some(&TypeRef::Bool),
                        &local_relations,
                    )
                    .map_err(|error| error.in_component_item(model, &target))?;
            }
            Ok(())
        })();
        checked.map_err(|error: TypecheckError| {
            error.in_target(model, &crate::action_target(&action.name), action.span)
        })?;
    }
    for (kind, properties, carriers) in [
        ("invariant", &model.invariants, true),
        ("trans", &model.transitions, true),
        ("reachable", &model.reachables, false),
    ] {
        for property in properties {
            let phase = ValidationPhase::for_scope(admission, carriers);
            let phase = if kind == "trans" {
                phase.with_old_frame()
            } else {
                phase
            };
            phase
                .check(
                    &property.expr,
                    &env,
                    model,
                    property.span,
                    Some(&TypeRef::Bool),
                    &self_relations,
                )
                .map_err(|error| {
                    error.in_target(
                        model,
                        &crate::property_target(kind, &property.name),
                        property.span,
                    )
                })?;
        }
    }
    for leadsto in &model.leadstos {
        validate_leadsto_expression_types(leadsto, &env, model, admission).map_err(|error| {
            error.in_target(
                model,
                &crate::property_target("leadsTo", &leadsto.name),
                leadsto.span,
            )
        })?;
    }
    if let Some(expr) = &model.terminal {
        let span = model.terminal_span.unwrap_or_else(unknown_span);
        ValidationPhase::for_scope(admission, false)
            .check(
                expr,
                &env,
                model,
                span,
                Some(&TypeRef::Bool),
                &self_relations,
            )
            .map_err(|error| error.in_target(model, crate::TERMINAL_TARGET, span))?;
    }
    Ok(())
}

/// Apply the shared range rule to one init statement, updating completed assignments.
fn admit_init_statement(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
    phase: &mut ValidationPhase,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    recursion::guard(|| admit_init_statement_inner(statement, env, model, phase, self_relations))
}

fn admit_init_statement_inner(
    statement: &Statement,
    env: &TypeEnv,
    model: &KernelModel,
    phase: &mut ValidationPhase,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
    let rule = |phase: &ValidationPhase| phase.range_rule.clone().unwrap_or_default();
    match statement {
        Statement::Assign { target, value, .. } => {
            validate_statement(statement, env, model, phase, self_relations)?;
            if let (LValue::Var(name), Some(rule)) = (target, &mut phase.range_rule) {
                rule.record_init_assignment(name, value, env, model);
            }
        }
        Statement::If {
            condition,
            then_statements,
            else_statements,
            span,
        } => {
            phase.check(
                condition,
                env,
                model,
                *span,
                Some(&TypeRef::Bool),
                self_relations,
            )?;
            let known = rule(phase).known_condition(condition, env, model);
            let mut then_phase = phase.after_patterns(condition);
            let mut else_phase = then_phase.clone();
            for statement in then_statements {
                admit_init_statement(statement, env, model, &mut then_phase, self_relations)?;
            }
            for statement in else_statements {
                admit_init_statement(statement, env, model, &mut else_phase, self_relations)?;
            }
            let then_recipes = rule(&then_phase).recipes;
            let else_recipes = rule(&else_phase).recipes;
            if let Some(rule) = &mut phase.range_rule {
                rule.recipes = match known {
                    Some(true) => then_recipes,
                    Some(false) => else_recipes,
                    None => RangeRule::join_recipes(&then_recipes, &else_recipes),
                };
            }
        }
        Statement::ForAll {
            binder,
            statements,
            span,
        } => {
            // Statement binder sources read no completed assignment; the filter and body do.
            let mut source = phase.clone();
            if let Some(rule) = &mut source.range_rule {
                rule.recipes.clear();
            }
            let mut child = source.clone().bound_by(binder, env, model);
            if let (Some(rule), Some(outer)) = (&mut child.range_rule, &phase.range_rule) {
                rule.recipes.clone_from(&outer.recipes);
            }
            let ty = validate_binder(
                binder,
                env,
                model,
                *span,
                &source,
                &child,
                BinderSite::Statement,
                self_relations,
            )?;
            let mut local = env.clone();
            local.insert(binder_name(binder).to_owned(), ty);
            let mut local_relations = self_relations.clone();
            local_relations.remove(binder_name(binder));
            let dead = rule(&child).dead;
            let mut body = child.after_where(binder);
            for statement in statements {
                admit_init_statement(statement, &local, model, &mut body, &local_relations)?;
            }
            let unconditional = binder_scope_is_unconditional(binder, &rule(&source), env, model);
            let body_recipes = rule(&body).recipes;
            if !dead && let Some(rule) = &mut phase.range_rule {
                rule.recipes = if unconditional {
                    body_recipes
                } else {
                    RangeRule::join_recipes(&body_recipes, &rule.recipes)
                };
            }
        }
    }
    crate::compose::extend_statement_pattern_names(statement, &mut phase.collisions);
    Ok(())
}

/// Whether every candidate of a statement binder reaches its body without a guard.
fn binder_scope_is_unconditional(
    binder: &Binder,
    rule: &RangeRule,
    env: &TypeEnv,
    model: &KernelModel,
) -> bool {
    if crate::model::binder_scope(binder).1.is_some() {
        return false;
    }
    match binder {
        Binder::Typed { .. } => true,
        Binder::Range { lo, hi, .. } => matches!(
            model.plan_integer_range(lo, hi, &rule.lexical(env)),
            RangePlan::Static { .. }
        ),
        Binder::Collection { collection, .. } => {
            // A Set candidate is guarded by its membership, a Seq literal item is not.
            matches!(collection.as_ref(), Expr::Seq(_))
                && matches!(
                    rule.collection_candidates(collection, env, model),
                    CollectionCandidates::Literal(_)
                )
        }
    }
}

/// Apply the shared range rule to the init statements in execution order.
///
/// # Errors
/// Returns the first refusal with the statement index that owns it.
pub(crate) fn admit_init_ranges(model: &KernelModel) -> Result<(), (usize, TypecheckError)> {
    let env = base_env(model);
    let self_relations = SelfRelations::new();
    let mut phase = ValidationPhase::range_admission(false, &Admission::new(model, false));
    for (index, statement) in model.init.iter().enumerate() {
        admit_init_statement(statement, &env, model, &mut phase, &self_relations)
            .map_err(|error| (index, error))?;
    }
    Ok(())
}

// SPDX-License-Identifier: Apache-2.0

//! Checked-model expression and statement type validation.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use fsl_syntax::{
    AggregateKind, Binder, ConditionalSpans, Expr, LValue, Pattern, SourcePos, Span, Statement,
};

use crate::recursion;
use crate::{ActionGuard, KernelModel, TypeDef, TypeRef};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypecheckError {
    pub(crate) message: String,
    pub(crate) span: Option<Span>,
    needs_context: bool,
    pub(crate) constant_shadow: bool,
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
        self.types.insert(name, ty)
    }

    pub(crate) fn remove(&mut self, name: &str) -> Option<TypeRef> {
        self.collections.remove(name);
        self.types.remove(name)
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
        Expr::Conditional { then_expr, .. } => infer_type(then_expr, env, model, expected),
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

fn compatible_carrier(
    actual: &TypeRef,
    expected: &TypeRef,
    model: &KernelModel,
) -> Result<bool, TypecheckError> {
    if has_int(actual) || has_int(expected) {
        types_compatible(actual, expected, model)
    } else {
        same_carrier(actual, expected, model)
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
        (TypeRef::Option(actual), TypeRef::Option(expected)) => {
            types_compatible(actual, expected, model)?
        }
        (TypeRef::Set(actual), TypeRef::Set(expected)) => {
            compatible_carrier(actual, expected, model)?
        }
        (TypeRef::Seq(actual, actual_capacity), TypeRef::Seq(expected, expected_capacity)) => {
            (actual_capacity == expected_capacity || has_int(actual) || has_int(expected))
                && compatible_carrier(actual, expected, model)?
        }
        (TypeRef::Map(actual_key, actual_value), TypeRef::Map(expected_key, expected_value)) => {
            compatible_carrier(actual_key, expected_key, model)?
                && compatible_carrier(actual_value, expected_value, model)?
        }
        (
            TypeRef::Relation(actual_key, actual_value),
            TypeRef::Relation(expected_key, expected_value),
        ) => {
            same_carrier(actual_key, expected_key, model)?
                && same_carrier(actual_value, expected_value, model)?
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
    reject_pattern_binding_name(
        phase,
        model.names().pattern_scopes.get(binding),
        binding,
        model.state.iter().map(|(name, _)| name.as_str()),
        model.consts.keys().map(String::as_str),
        model.enum_members.keys().map(String::as_str),
        env.keys().chain(&phase.collisions).map(String::as_str),
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
pub(crate) fn reject_pattern_binding_name<'a>(
    phase: &ValidationPhase,
    source_scope: Option<&Result<PatternBinding, TypecheckError>>,
    binding: &str,
    state: impl IntoIterator<Item = &'a str>,
    constants: impl IntoIterator<Item = &'a str>,
    enum_members: impl IntoIterator<Item = &'a str>,
    local: impl IntoIterator<Item = &'a str>,
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
    match expr {
        Expr::Num(value) => Ok(*value),
        Expr::Var(name) => match model.consts.get(name) {
            Some(crate::FslValue::Int(value)) => Ok(*value),
            _ => Err(error(format!("'{}' is not an integer const", shown(name)))),
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

#[derive(Clone)]
pub(crate) struct ValidationPhase {
    normalize_aggregates: bool,
    substituted: BTreeSet<String>,
    // Reserved by earlier evaluation, without making those names readable.
    pub(crate) collisions: HashSet<String>,
}

impl ValidationPhase {
    pub(crate) fn substitutes(&self, name: &str) -> bool {
        self.substituted.contains(name)
    }
    pub(crate) fn normalized() -> Self {
        Self {
            normalize_aggregates: true,
            substituted: BTreeSet::new(),
            collisions: HashSet::new(),
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
            substituted,
            collisions: self.collisions.clone(),
        }
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

fn validate_binder(
    binder: &Binder,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    source_phase: &ValidationPhase,
    child_phase: &ValidationPhase,
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
    }
    let ty = binder_type(binder, env, model).map_err(|error| error.with_span(span))?;
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

fn validate_expression_in_phase(
    expr: &Expr,
    env: &TypeEnv,
    model: &KernelModel,
    span: Span,
    expected: Option<&TypeRef>,
    phase: &ValidationPhase,
    self_relations: &SelfRelations,
) -> Result<(), TypecheckError> {
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
            for (expr, span, phase) in [
                (then_expr, spans.then_expr, &then_phase),
                (else_expr, spans.else_expr, &else_phase),
            ] {
                phase.check(expr, env, model, span, Some(&ty), self_relations)?;
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
        Expr::UnaryNamed { name, expr, .. } => {
            let expected = (name == "abs").then_some(TypeRef::Int);
            validate(expr, env, span, expected.as_ref())?;
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
            let child_phase = phase.under_binder(binder, None);
            let binder_ty = validate_binder(
                binder,
                env,
                model,
                span,
                phase,
                &child_phase,
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
            let child_phase = phase.under_binder(binder, Some(*kind));
            let binder_ty = validate_binder(
                binder,
                env,
                model,
                span,
                phase,
                &child_phase,
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
            if phase.normalize_aggregates && expands {
                let normalized = normalize_aggregate(*kind, binder, value.as_deref(), env, model)
                    .map_err(|error| error.with_span(span))?;
                // Generated copies have already passed authored binding checks.
                let mut generated = phase.clone();
                crate::collect_pattern_names(&normalized, &mut generated.collisions);
                generated.substituted.extend(generated.collisions.drain());
                generated.check(&normalized, env, model, span, expected, self_relations)?;
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
            let ty = validate_binder(binder, env, model, *span, phase, phase, self_relations)?;
            let mut local = env.clone();
            local.insert(binder_name(binder).to_owned(), ty);
            let mut local_relations = self_relations.clone();
            local_relations.remove(binder_name(binder));
            let phase = phase.after_where(binder);
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
            let phase = ValidationPhase::normalized();
            let ty = validate_binder(binder, env, model, *span, &phase, &phase, self_relations)?;
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
    let mut env = base_env(model);
    env.extend(bindings.iter().cloned());
    let span = unknown_span();
    let resolved = carrier_type(expected, expected_model).map_err(|error| error.with_span(span))?;
    let actual =
        infer_type(expr, &env, model, Some(&resolved)).map_err(|error| error.with_span(span))?;
    if !types_compatible(&actual, &resolved, model).map_err(|error| error.with_span(span))? {
        return Err(not_assignable(&actual, expected).with_span(span));
    }
    validate_expression(
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
/// the same expected Boolean type as an invariant. Returns the checker message
/// without a source location: callers may be validating an argv expression.
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
    validate_expression(
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
    let mut phase = ValidationPhase::normalized();
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
) -> Result<(), TypecheckError> {
    let mut local = env.clone();
    let self_relations = SelfRelations::new();
    for binder in &leadsto.binders {
        let phase = ValidationPhase::normalized();
        let ty = validate_binder(
            binder,
            &local,
            model,
            leadsto.span,
            &phase,
            &phase,
            &self_relations,
        )?;
        local.insert(binder_name(binder).to_owned(), ty);
    }
    for expr in [&leadsto.before, &leadsto.after] {
        validate_expression(
            expr,
            &local,
            model,
            leadsto.span,
            Some(&TypeRef::Bool),
            &self_relations,
        )?;
    }
    if let Some(expr) = &leadsto.decreases {
        validate_expression(
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
            validate_expression(
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
    let env = base_env(model);
    let self_relations = SelfRelations::new();
    for action in &model.actions {
        let checked = (|| {
            let mut local = env.clone();
            let mut local_relations = SelfRelations::new();
            for param in &action.params {
                reject_constant_shadow(param.name(), model.consts.keys())?;
                local.insert(param.name().to_owned(), param.ty());
            }
            let mut phase = ValidationPhase::normalized();
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
    for (kind, properties) in [
        ("invariant", &model.invariants),
        ("trans", &model.transitions),
        ("reachable", &model.reachables),
    ] {
        for property in properties {
            validate_expression(
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
        validate_leadsto_expression_types(leadsto, &env, model).map_err(|error| {
            error.in_target(
                model,
                &crate::property_target("leadsTo", &leadsto.name),
                leadsto.span,
            )
        })?;
    }
    if let Some(expr) = &model.terminal {
        let span = model.terminal_span.unwrap_or_else(unknown_span);
        validate_expression(
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

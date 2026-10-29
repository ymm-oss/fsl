// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fsl_syntax::{
    ActionItem, Annotation, AnnotationRegistry, Annotations, Binder, Expr, HelpfulAction, LValue,
    MetaTag, Param, RequirementLink, SourcePos, Span, SpecItem, Statement, SurfaceSpec, TypeExpr,
};

use crate::{
    INIT_TARGET, KernelSpec, LoweringStep, OriginChain, OriginId, OriginRegistry, OriginSite,
    ProjectionDef, SPEC_TARGET, TERMINAL_TARGET, TraceabilityRegistry, action_target, expr_text,
    init_statement_target, property_target, state_target, type_target,
};

/// Stable semantic diagnostic code for a conservative write-alias rejection where
/// index distinctness across `forall` iterations could not be proved (issue #698).
pub const WRITE_DISTINCTNESS_UNPROVED_CODE: &str = "FSL-SEMANTIC-WRITE-DISTINCTNESS-UNPROVED";

/// Context shared by ordinary model validation and pre-prefix compose errors.
pub(crate) enum ExpressionErrorContext<'a> {
    Model,
    Init,
    InlineInitializer(&'a str),
}

impl ExpressionErrorContext<'_> {
    pub(crate) fn message(self, mut message: String) -> String {
        let prefix = match self {
            Self::Model => "invalid model expression".to_owned(),
            Self::Init => "invalid init statement".to_owned(),
            Self::InlineInitializer(name) => {
                format!("invalid inline initializer for '{}'", shown(name))
            }
        };
        message.insert_str(0, ": ");
        message.insert_str(0, &prefix);
        message
    }
}

/// A machine-applicable source edit carried by a typed model diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticEdit {
    pub span: Span,
    pub replacement: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Enum {
        type_name: String,
        member: String,
    },
    None,
    Some(Box<Self>),
    Struct {
        type_name: String,
        fields: BTreeMap<String, Self>,
    },
    Map(BTreeMap<Self, Self>),
    Set(BTreeSet<Self>),
    Seq(Vec<Self>),
    Relation(BTreeSet<(Self, Self)>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeRef {
    Int,
    Bool,
    Named(String),
    Range(i64, i64),
    Map(Box<Self>, Box<Self>),
    Relation(Box<Self>, Box<Self>),
    Set(Box<Self>),
    Seq(Box<Self>, usize),
    Option(Box<Self>),
}

impl TypeRef {
    /// Index key/value types. Sequence writes retain their existing rejection.
    pub(crate) fn index_types(&self, allow_sequence: bool) -> Option<(Self, Self)> {
        match self {
            Self::Map(key, value) => Some((key.as_ref().clone(), value.as_ref().clone())),
            Self::Seq(item, _) if allow_sequence => Some((Self::Int, item.as_ref().clone())),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeDef {
    Domain {
        lo: i64,
        hi: i64,
        symmetric: bool,
    },
    Enum {
        members: Vec<String>,
        symmetric: bool,
    },
    Struct {
        fields: Vec<(String, TypeRef)>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParamDef {
    Typed { name: String, ty: TypeRef },
    Range { name: String, lo: i64, hi: i64 },
}

impl ParamDef {
    pub(crate) fn ty(&self) -> TypeRef {
        match self {
            Self::Typed { ty, .. } => ty.clone(),
            Self::Range { lo, hi, .. } => TypeRef::Range(*lo, *hi),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Typed { name, .. } | Self::Range { name, .. } => name,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionDef {
    pub name: String,
    pub span: Span,
    pub params: Vec<ParamDef>,
    pub requires: Vec<Expr>,
    pub require_spans: Vec<Span>,
    pub lets: Vec<(String, Expr)>,
    pub guards: Vec<ActionGuard>,
    pub statements: Vec<Statement>,
    pub ensures: Vec<Expr>,
    pub ensure_spans: Vec<Span>,
    pub fair: bool,
    /// Set by compose expansion for a synchronized action. Its `requires` are
    /// inherited copies from several components, so per-clause redundancy
    /// diagnostics must not treat a duplicated component guard as removable
    /// (`docs/design/DESIGN-vacuity.md` §2).
    pub sync: bool,
    pub meta: Option<MetaTag>,
    pub annotations: Annotations,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionGuard {
    Requires(Expr),
    Let(String, Expr),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyDef {
    pub name: String,
    pub expr: Expr,
    pub span: Span,
    pub meta: Option<MetaTag>,
    pub annotations: Annotations,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeadsToDef {
    pub name: String,
    pub span: Span,
    pub binders: Vec<Binder>,
    pub before: Expr,
    pub after: Expr,
    pub meta: Option<MetaTag>,
    pub annotations: Annotations,
    pub decreases: Option<Expr>,
    pub within: Option<i64>,
    pub helpful: Vec<HelpfulAction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelModel {
    pub name: String,
    pub consts: BTreeMap<String, Value>,
    pub types: BTreeMap<String, TypeDef>,
    pub enum_members: BTreeMap<String, Value>,
    pub state: Vec<(String, TypeRef)>,
    pub init: Vec<Statement>,
    pub init_meta: Option<MetaTag>,
    pub init_annotations: Annotations,
    pub actions: Vec<ActionDef>,
    pub invariants: Vec<PropertyDef>,
    pub transitions: Vec<PropertyDef>,
    pub reachables: Vec<PropertyDef>,
    pub leadstos: Vec<LeadsToDef>,
    pub terminal: Option<Expr>,
    pub terminal_span: Option<Span>,
    pub projections: Vec<ProjectionDef>,
    origins: OriginRegistry,
    annotations: AnnotationRegistry,
    traceability: TraceabilityRegistry,
    names: crate::NameMap,
}

impl KernelModel {
    /// Whether a state type contributes an implicit verifier bound property.
    #[must_use]
    pub fn type_has_bounds(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Int | TypeRef::Bool => false,
            TypeRef::Range(_, _)
            | TypeRef::Set(_)
            | TypeRef::Seq(_, _)
            | TypeRef::Relation(_, _) => true,
            TypeRef::Option(inner) => self.type_has_bounds(inner),
            TypeRef::Map(_, value) => self.type_has_bounds(value),
            TypeRef::Named(name) => match self.types.get(name) {
                Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) => true,
                Some(TypeDef::Struct { fields }) => {
                    fields.iter().any(|(_, ty)| self.type_has_bounds(ty))
                }
                None => false,
            },
        }
    }

    pub(crate) fn component_item_span(&self, target: &str) -> Option<Span> {
        self.origins
            .primary_for(&crate::origin::component_check_target(target))?
            .secondary
            .first()?
            .span
    }

    pub(crate) fn error_at_target(&self, message: String, span: Span, target: &str) -> ModelError {
        model_error(message).at(span).with_origin(
            self.origins
                .diagnostic_origin(&crate::origin::component_check_target(target))
                .or_else(|| self.origins.diagnostic_origin(target)),
        )
    }
    pub(crate) fn resolve_surface_type(&self, ty: &TypeExpr) -> Result<TypeRef, ModelError> {
        resolve_type(ty, &self.types, &self.consts)
    }

    /// Enumerate a finite scalar domain.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] for unbounded or non-scalar types.
    pub fn domain_values(&self, ty: &TypeRef) -> Result<Vec<Value>, ModelError> {
        match ty {
            TypeRef::Bool => Ok(vec![Value::Bool(false), Value::Bool(true)]),
            TypeRef::Range(lo, hi) => Ok((*lo..=*hi).map(Value::Int).collect()),
            TypeRef::Named(name) => match self.types.get(name) {
                Some(TypeDef::Domain { lo, hi, .. }) => Ok((*lo..=*hi).map(Value::Int).collect()),
                Some(TypeDef::Enum { members, .. }) => Ok(members
                    .iter()
                    .map(|member| Value::Enum {
                        type_name: name.clone(),
                        member: member.clone(),
                    })
                    .collect()),
                _ => Err(model_error(format!(
                    "type '{}' is not a finite scalar",
                    shown(name)
                ))),
            },
            _ => Err(model_error("type is not a finite scalar")),
        }
    }

    /// Whether `value` is one of [`KernelModel::domain_values`]`(ty)`,
    /// decided without listing the domain.
    ///
    /// # Errors
    ///
    /// Returns the [`ModelError`] [`KernelModel::domain_values`] returns for
    /// `ty`.
    pub fn domain_contains(&self, ty: &TypeRef, value: &Value) -> Result<bool, ModelError> {
        let in_range =
            |lo: i64, hi: i64| matches!(value, Value::Int(value) if lo <= *value && *value <= hi);
        match ty {
            TypeRef::Bool => Ok(matches!(value, Value::Bool(_))),
            TypeRef::Range(lo, hi) => Ok(in_range(*lo, *hi)),
            TypeRef::Named(name) => match self.types.get(name) {
                Some(TypeDef::Domain { lo, hi, .. }) => Ok(in_range(*lo, *hi)),
                Some(TypeDef::Enum { members, .. }) => Ok(matches!(
                    value,
                    Value::Enum { type_name, member }
                        if type_name == name && members.contains(member)
                )),
                // Not a finite scalar: `domain_values`' own error.
                _ => self.domain_values(ty).map(|_| false),
            },
            _ => self.domain_values(ty).map(|_| false),
        }
    }

    /// Enumerate the concrete Monitor compatibility domain for map keys.
    ///
    /// Legacy `Map<Int, _>` uses `0..max(consts)` (or `0..1` without integer
    /// constants); named/range/Bool keys use their ordinary finite domain.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] when the key type is unsupported.
    pub fn map_key_values(&self, ty: &TypeRef) -> Result<Vec<Value>, ModelError> {
        if matches!(ty, TypeRef::Int) {
            let hi = self
                .consts
                .values()
                .filter_map(|value| match value {
                    Value::Int(value) => Some(*value),
                    _ => None,
                })
                .max()
                .unwrap_or(1);
            return Ok((0..=hi.max(0)).map(Value::Int).collect());
        }
        self.domain_values(ty)
    }

    /// Construct the concrete default used before sequential init execution.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] for unresolved or invalid types.
    pub fn default_value(&self, ty: &TypeRef) -> Result<Value, ModelError> {
        match ty {
            TypeRef::Int => Ok(Value::Int(0)),
            TypeRef::Bool => Ok(Value::Bool(false)),
            TypeRef::Range(lo, _) => Ok(Value::Int(*lo)),
            TypeRef::Named(name) => match self.types.get(name) {
                Some(TypeDef::Domain { lo, .. }) => Ok(Value::Int(*lo)),
                Some(TypeDef::Enum { members, .. }) => members
                    .first()
                    .map(|member| Value::Enum {
                        type_name: name.clone(),
                        member: member.clone(),
                    })
                    .ok_or_else(|| model_error(format!("enum '{}' has no members", shown(name)))),
                Some(TypeDef::Struct { fields }) => Ok(Value::Struct {
                    type_name: name.clone(),
                    fields: fields
                        .iter()
                        .map(|(field, ty)| Ok((field.clone(), self.default_value(ty)?)))
                        .collect::<Result<_, ModelError>>()?,
                }),
                None => Err(model_error(format!("unknown type '{}'", shown(name)))),
            },
            TypeRef::Map(key, value) => Ok(Value::Map(
                self.map_key_values(key)?
                    .into_iter()
                    .map(|key| Ok((key, self.default_value(value)?)))
                    .collect::<Result<_, ModelError>>()?,
            )),
            TypeRef::Relation(_, _) => Ok(Value::Relation(BTreeSet::new())),
            TypeRef::Set(_) => Ok(Value::Set(BTreeSet::new())),
            TypeRef::Seq(_, _) => Ok(Value::Seq(Vec::new())),
            TypeRef::Option(_) => Ok(Value::None),
        }
    }

    #[must_use]
    pub fn state_type(&self, name: &str) -> Option<&TypeRef> {
        self.state
            .iter()
            .find_map(|(state_name, ty)| (state_name == name).then_some(ty))
    }

    /// Return the resolved type of a state update target.
    ///
    /// The target's index expression is checked separately. This helper owns
    /// only the state-root/Map/struct-field lookup shared by the
    /// typechecker and symbolic transition evaluator.
    ///
    /// # Errors
    ///
    /// Returns a [`ModelError`] when the target does not name a state location
    /// with a resolvable Map or struct-field type.
    pub fn state_lvalue_type(&self, target: &LValue) -> Result<TypeRef, ModelError> {
        match target {
            LValue::Var(name) => self
                .state_type(name)
                .cloned()
                .ok_or_else(|| unknown_update_target(name)),
            LValue::Index(name, _) => match self.state_type(name) {
                Some(ty) => ty
                    .index_types(false)
                    .map(|(_, value)| value)
                    .ok_or_else(|| model_error("indexed update target requires Map")),
                None => Err(unknown_update_target(name)),
            },
            LValue::Field(base, field) => {
                let TypeRef::Named(name) = self.state_lvalue_type(base)? else {
                    return Err(model_error("field update target requires named struct"));
                };
                match self.types.get(&name) {
                    Some(TypeDef::Struct { fields }) => fields
                        .iter()
                        .find_map(|(candidate, ty)| (candidate == field).then(|| ty.clone()))
                        .ok_or_else(|| {
                            model_error(format!("unknown struct field '{name}.{field}'"))
                        }),
                    Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) | None => {
                        Err(model_error("field update target requires struct"))
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn struct_fields(&self, name: &str) -> Option<&[(String, TypeRef)]> {
        match self.types.get(name) {
            Some(TypeDef::Struct { fields }) => Some(fields),
            _ => None,
        }
    }

    #[must_use]
    pub fn action_origin(&self, name: &str) -> Option<&crate::OriginChain> {
        self.origins.primary_for(&action_target(name))
    }

    /// The authored location for an action coverage finding: prefer its
    /// primary origin, then its kernel declaration. Generated-only scaffolding
    /// with zero source positions has no public finding location.
    #[must_use]
    pub fn action_source_span(&self, action: &ActionDef) -> Option<Span> {
        self.action_origin(&action.name)
            .and_then(|origin| origin.primary.as_ref().and_then(|site| site.span))
            .filter(|span| span.start.line > 0 && span.start.column > 0)
            .or_else(|| {
                (action.span.start.line > 0 && action.span.start.column > 0).then_some(action.span)
            })
    }

    /// The owner of this model's internal-to-public name mapping.
    #[must_use]
    pub fn names(&self) -> &crate::NameMap {
        &self.names
    }

    /// Structural compose identity of an action, or `None` when the action is
    /// not a compose component action (a direct spec action or a sync action).
    #[must_use]
    pub fn compose_action(&self, name: &str) -> Option<&crate::ComposeName> {
        self.names.compose_action(name)
    }

    /// See [`crate::NameMap::component_public_name`].
    #[must_use]
    pub fn component_public_name(&self, name: &str, action: &str) -> Option<String> {
        self.names
            .component_public_name(name, action)
            .map(crate::PublicName::into_string)
    }

    /// See [`crate::NameMap::public_action`].
    #[must_use]
    pub fn action_display_name(&self, name: &str) -> String {
        self.names.public_action(name).into_string()
    }

    /// See [`crate::NameMap::public_violation`].
    #[must_use]
    pub fn action_scoped_display_name(&self, name: &str, action: Option<&str>) -> String {
        self.names.public_violation(name, action).into_string()
    }

    /// Give a model rebuilt from this model's lowered surface spec (a mutant
    /// or a counterfactual candidate) the compose component names of
    /// `original`; the surface spec does not carry them.
    pub fn inherit_compose_names(&mut self, original: &Self) {
        self.names.clone_from(&original.names);
    }

    /// Resolve an action name written in a v1 replay trace (see
    /// [`crate::NameMap::resolve_action`]).
    ///
    /// # Errors
    ///
    /// Returns a message naming every candidate when the spelling matches more
    /// than one action, so the caller fails closed instead of picking one.
    pub fn resolve_replay_action(&self, spelling: &str) -> Result<Option<&ActionDef>, String> {
        let resolved = self
            .names
            .resolve_action(
                spelling,
                self.actions.iter().map(|action| action.name.as_str()),
            )
            .map_err(|ambiguous| {
                format!(
                    "action '{spelling}' is ambiguous: it matches {}",
                    ambiguous
                        .candidates
                        .iter()
                        .map(|name| format!(
                            "'{}' (internal '{name}')",
                            self.action_display_name(name)
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
        Ok(resolved.and_then(|name| self.actions.iter().find(|action| action.name == name)))
    }

    #[must_use]
    pub fn origins(&self) -> &OriginRegistry {
        &self.origins
    }

    #[must_use]
    pub fn requirement_for(&self, target: &str) -> Option<&MetaTag> {
        self.traceability.requirement_for(target)
    }

    #[must_use]
    pub fn requirements_for(&self, target: &str) -> Vec<RequirementLink> {
        self.annotations
            .annotations_for(target)
            .requirements()
            .unwrap_or_default()
    }

    /// Return every executable Kernel target carrying a requirement relation.
    #[must_use]
    pub fn requirement_targets(&self) -> BTreeMap<String, Vec<RequirementLink>> {
        let mut targets = BTreeMap::new();
        let mut insert = |target: String| {
            let requirements = self.requirements_for(&target);
            if !requirements.is_empty() {
                targets.insert(target, requirements);
            }
        };
        insert(INIT_TARGET.to_owned());
        for action in &self.actions {
            insert(action_target(&action.name));
        }
        for (kind, properties) in [
            ("invariant", &self.invariants),
            ("trans", &self.transitions),
            ("reachable", &self.reachables),
        ] {
            for property in properties {
                insert(property_target(kind, &property.name));
            }
        }
        for property in &self.leadstos {
            insert(property_target("leadsTo", &property.name));
        }
        targets
    }

    #[must_use]
    pub fn annotations_for(&self, target: &str) -> &Annotations {
        self.annotations.annotations_for(target)
    }

    #[must_use]
    pub fn property_origin(&self, kind: &str, name: &str) -> Option<&crate::OriginChain> {
        self.origins.primary_for(&property_target(kind, name))
    }

    #[must_use]
    pub fn terminal_origin(&self) -> Option<&crate::OriginChain> {
        self.origins.primary_for(TERMINAL_TARGET)
    }

    #[must_use]
    pub fn state_origin(&self, name: &str) -> Option<&crate::OriginChain> {
        self.origins.primary_for(&crate::state_target(name))
    }
}

/// Enumerate the solver-independent static bindings of a `leadsTo` property.
///
/// # Errors
///
/// Returns [`ModelError`] for dynamic/filtering binders or unsupported
/// collection domains.
pub fn static_leadsto_bindings(
    model: &KernelModel,
    property: &LeadsToDef,
) -> Result<Vec<BTreeMap<String, Value>>, ModelError> {
    let mut expanded = vec![BTreeMap::new()];
    for binder in &property.binders {
        check_leadsto_binder(binder)?;
        let (name, values) = match binder {
            Binder::Typed {
                name, type_name, ..
            } => (
                name,
                model.domain_values(&TypeRef::Named(type_name.name.clone()))?,
            ),
            Binder::Range { name, lo, hi, .. } => {
                let lo = static_leadsto_int(lo, model)?;
                let hi = static_leadsto_int(hi, model)?;
                (name, (lo..=hi).map(Value::Int).collect())
            }
            Binder::Collection { .. } => unreachable!("leadsTo binder was checked"),
        };
        let mut next = Vec::new();
        for binding in expanded {
            for value in &values {
                let mut candidate = binding.clone();
                candidate.insert(name.clone(), value.clone());
                next.push(candidate);
            }
        }
        expanded = next;
    }
    Ok(expanded)
}

fn check_leadsto_binder(binder: &Binder) -> Result<(), ModelError> {
    if binder_scope(binder).1.is_some() {
        return Err(model_error(
            "leadsTo wrapper binders do not support where filters",
        ));
    }
    if matches!(binder, Binder::Collection { .. }) {
        return Err(model_error(
            "leadsTo wrapper binders do not support collection domains",
        ));
    }
    Ok(())
}

fn static_leadsto_int(expr: &Expr, model: &KernelModel) -> Result<i64, ModelError> {
    match expr {
        Expr::Num(value) => Ok(*value),
        Expr::Var(name) => match model.consts.get(name) {
            Some(Value::Int(value)) => Ok(*value),
            _ => Err(model_error(format!(
                "'{}' is not an integer constant",
                shown(name)
            ))),
        },
        Expr::Neg(value) => static_leadsto_int(value, model)?
            .checked_neg()
            .ok_or_else(|| model_error("integer overflow in leadsTo binder")),
        _ => Err(model_error(
            "leadsTo range binder must use static integer bounds",
        )),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelError {
    pub message: String,
    pub origin: Option<Box<crate::OriginChain>>,
    /// The diagnostic's own source location, for constructs that carry no
    /// lowering origin — a kernel `spec` registers none at all.
    ///
    /// Deliberately not read by [`fmt::Display`]: the rendered message already
    /// embeds the origin's location where one exists, and duplicating it here
    /// would change every existing message. The CLI reports this as the `loc`
    /// that `docs/design/DESIGN-v1.md` §7.2 guarantees (issue 555).
    pub span: Option<Span>,
    /// Whether this is a name-resolution failure. See
    /// [`crate::CoreErrorKind::NameResolution`] (issue 565).
    pub name_resolution: bool,
    /// Stable machine-readable classification when the frontend knows it.
    pub diagnostic_code: Option<&'static str>,
    /// Actionable repair guidance for delivery surfaces that render hints.
    pub hint: Option<Box<String>>,
    /// A machine-applicable edit for LSP quick-fix surfaces.
    pub quick_fix: Option<Box<DiagnosticEdit>>,
}

impl ModelError {
    /// Classify this diagnostic as a name-resolution failure.
    #[must_use]
    fn into_name_resolution(mut self) -> Self {
        self.name_resolution = true;
        self
    }

    fn with_origin(mut self, origin: Option<crate::OriginChain>) -> Self {
        if let (Some(origin), Some(cause_span)) = (&origin, self.span)
            && origin.dialect == "compose"
            && let Some(site) = &origin.primary
            && let (Some(use_span), [kind, path, declaration]) =
                (site.span, site.declaration_path.as_slice())
            && kind == "use"
        {
            let empty_enum =
                self.message.starts_with("enum '") && self.message.ends_with(" has no members");
            let resolved_path = if empty_enum {
                site.source_file
                    .as_deref()
                    .and_then(|file| std::path::Path::new(file).parent())
                    .map(|parent| parent.join(path).to_string_lossy().into_owned())
            } else {
                None
            };
            let cause = crate::CoreError {
                message: if empty_enum {
                    format!("enum '{declaration}' has no members")
                } else {
                    format!("{declaration}: {}", self.message)
                },
                line: cause_span.start.line,
                column: cause_span.start.column,
                origin: None,
                kind: crate::CoreErrorKind::Semantics,
                diagnostic_code: self.diagnostic_code,
            };
            let mut anchored = crate::compose::component_error(
                resolved_path.as_deref().unwrap_or(path),
                use_span,
                &cause,
                "failed to check",
            );
            if let (Some(file), Some(anchor_origin)) = (&site.source_file, &mut anchored.origin) {
                anchor_origin.set_source_file(file);
            }
            self.message = anchored.message;
            self.span = Some(use_span);
            self.origin = anchored.origin;
            self.name_resolution = false;
            return self;
        }
        self.origin = origin.map(Box::new);
        self
    }

    /// Record where the diagnostic happened, keeping a more precise location
    /// an inner step already recorded.
    #[must_use]
    fn at(mut self, span: Span) -> Self {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }

    #[must_use]
    fn with_diagnostic_code(mut self, code: &'static str) -> Self {
        self.diagnostic_code = Some(code);
        self
    }

    #[must_use]
    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(Box::new(hint.into()));
        self
    }

    #[must_use]
    fn with_quick_fix(mut self, edit: DiagnosticEdit) -> Self {
        self.quick_fix = Some(Box::new(edit));
        self
    }
}

fn with_type_diagnostic_origin(mut error: ModelError, span: Span) -> ModelError {
    if error.origin.is_none() {
        error.origin = Some(Box::new(type_diagnostic_origin(span)));
    }
    error
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let location = self
            .origin
            .as_ref()
            .and_then(|origin| origin.primary.as_ref())
            .and_then(|site| site.span.map(|span| (site.source_file.as_deref(), span)));
        match location {
            Some((Some(file), span)) => write!(
                formatter,
                "{} at {}:{}:{}",
                self.message, file, span.start.line, span.start.column
            ),
            Some((None, span)) => write!(
                formatter,
                "{} at {}:{}",
                self.message, span.start.line, span.start.column
            ),
            None => formatter.write_str(&self.message),
        }?;
        if let Some(secondary) = self
            .origin
            .as_ref()
            .and_then(|origin| origin.secondary.first())
            .and_then(|site| site.span.map(|span| (site.source_file.as_deref(), span)))
        {
            match secondary {
                (Some(file), span) => write!(
                    formatter,
                    "; conflicting assignment at {}:{}:{}",
                    file, span.start.line, span.start.column
                ),
                (None, span) => write!(
                    formatter,
                    "; conflicting assignment at {}:{}",
                    span.start.line, span.start.column
                ),
            }?;
        }
        Ok(())
    }
}

impl std::error::Error for ModelError {}

/// Construct the typed semantic kernel model used by runtime and verifier.
///
/// # Errors
///
/// Returns [`ModelError`] for duplicate declarations, unresolved types,
/// non-constant bounds, invalid capacities, or a missing state block.
pub fn build_model(kernel: KernelSpec) -> Result<KernelModel, ModelError> {
    ModelBuilder::new(kernel).build()
}

struct ModelBuilder {
    spec: SurfaceSpec,
    origins: OriginRegistry,
    annotations: AnnotationRegistry,
    projections: Vec<ProjectionDef>,
    names: crate::NameMap,
    consts: BTreeMap<String, Value>,
    types: BTreeMap<String, TypeDef>,
    enum_members: BTreeMap<String, Value>,
}

impl ModelBuilder {
    fn new(kernel: KernelSpec) -> Self {
        let KernelSpec {
            spec,
            origins,
            annotations,
            projections,
            diagnostics: _,
            names,
        } = kernel;
        Self {
            spec,
            origins,
            annotations,
            projections,
            names,
            consts: BTreeMap::new(),
            types: BTreeMap::new(),
            enum_members: BTreeMap::new(),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn build(mut self) -> Result<KernelModel, ModelError> {
        // Before anything reads a name: a declaration named `true`/`false`/
        // `none` is unreadable from every expression, and the misreading is
        // silent rather than an error (issue #570).
        crate::reserved::check_reserved_names(&self.spec)?;
        reject_duplicate_property_names(&self.spec)?;
        self.collect_consts()?;
        self.collect_types()?;
        self.collect_declaration_annotations();
        self.annotations.validate().map_err(|error| ModelError {
            message: error.message,
            origin: Some(Box::new(source_origin("annotation", error.span, None))),
            span: Some(error.span),
            name_resolution: false,
            diagnostic_code: None,
            hint: None,
            quick_fix: None,
        })?;
        let state_names = self
            .spec
            .items
            .iter()
            .filter_map(|item| match item {
                SpecItem::State(fields) => Some(fields.iter().map(|field| field.name.clone())),
                _ => None,
            })
            .flatten()
            .collect::<BTreeSet<_>>();
        let explicit_init_writes = self
            .spec
            .items
            .iter()
            .filter_map(|item| match item {
                SpecItem::Init { statements, .. } => Some(statements),
                _ => None,
            })
            .flat_map(|statements| statement_root_spans(statements))
            .collect::<BTreeMap<_, _>>();
        let mut state = Vec::new();
        let mut inline_init = Vec::new();
        let mut init = Vec::new();
        let mut inline_initializers = Vec::new();
        let mut init_meta = None;
        let mut actions = Vec::new();
        let mut action_names = BTreeSet::new();
        let mut invariants = Vec::new();
        let mut transitions = Vec::new();
        let mut reachables = Vec::new();
        let mut leadstos = Vec::new();
        let mut terminal = None;
        let mut terminal_span = None;
        let mut traceability = TraceabilityRegistry::default();
        for (target, annotations) in self.annotations.targets() {
            for requirement in annotations
                .requirements()
                .expect("annotations were validated before model construction")
            {
                traceability.bind(
                    target.to_owned(),
                    MetaTag {
                        id: requirement.id,
                        text: requirement.text,
                        span: Some(requirement.span),
                    },
                );
            }
        }
        for item in &self.spec.items {
            match item {
                SpecItem::State(fields) => {
                    for field in fields {
                        if state.iter().any(|(existing, _)| existing == &field.name) {
                            return Err(model_error(format!(
                                "duplicate state variable '{}'",
                                shown(&field.name)
                            ))
                            .with_origin(self.origins.diagnostic_origin(&state_target(&field.name)))
                            // The redeclaration, not the first binding: the
                            // registry origin and the message-derived fallback
                            // both name the innocent earlier `field.name`
                            // (issues 555, 565).
                            .at(field.span)
                            .into_name_resolution());
                        }
                        reject_map_int_key(&field.name, &field.ty, field.span)?;
                        let origin = self.origins.diagnostic_origin(&state_target(&field.name));
                        let resolved = self
                            .resolve_type(&field.ty)
                            .map_err(|error| error.with_origin(origin.clone()).at(field.span))?;
                        if !self.legal_state_type(&resolved) {
                            return Err(model_error(format!(
                                "state variable '{}' has unsupported state type",
                                shown(&field.name)
                            ))
                            .with_origin(origin)
                            .at(field.span));
                        }
                        if let Some(initializer) = &field.initializer {
                            if let Some(form) = unsupported_inline_form(initializer) {
                                return Err(model_error(format!(
                                    "inline initializer for '{}' does not allow {form}; use init",
                                    shown(&field.name)
                                ))
                                .with_origin(Some(source_origin(
                                    &field.name,
                                    field.initializer_span.unwrap_or(field.span),
                                    None,
                                ))));
                            }
                            if let Some(name) = first_state_reference(initializer, &state_names) {
                                return Err(model_error(format!(
                                    "inline initializer for '{}' must not read state root '{}'",
                                    shown(&field.name),
                                    shown(&name)
                                ))
                                .with_origin(Some(source_origin(
                                    &field.name,
                                    field.initializer_span.unwrap_or(field.span),
                                    None,
                                ))));
                            }
                            if let Some(conflict_span) = explicit_init_writes.get(&field.name) {
                                return Err(model_error(format!(
                                    "state root '{}' is assigned by both an inline initializer and init",
                                    shown(&field.name)
                                ))
                                .with_origin(Some(source_origin(
                                    &field.name,
                                    field.span,
                                    Some(*conflict_span),
                                ))));
                            }
                            let initializer_span = field.initializer_span.unwrap_or(field.span);
                            inline_initializers.push((
                                inline_init.len(),
                                field.name.clone(),
                                initializer_span,
                            ));
                            inline_init.push(Statement::Assign {
                                target: LValue::Var(field.name.clone()),
                                value: initializer.clone(),
                                span: initializer_span,
                            });
                        }
                        state.push((field.name.clone(), resolved));
                    }
                }
                // Dialect lowering can append generated init fragments (for
                // example an NFR age counter) after the user's init block.
                // Every fragment is part of the same logical initialization;
                // replacing the earlier fragment leaves user state unconstrained.
                SpecItem::Init {
                    statements, meta, ..
                } => {
                    init.extend(statements.clone());
                    if init_meta.is_none() {
                        init_meta.clone_from(meta);
                    }
                }
                SpecItem::Action {
                    name,
                    params,
                    items,
                    fair,
                    meta,
                    span,
                    sync,
                    ..
                } => {
                    let origin = self.origins.diagnostic_origin(&action_target(name));
                    if !action_names.insert(name) {
                        return Err(model_error(format!("duplicate action '{}'", shown(name)))
                            .with_origin(origin));
                    }
                    let mut action = self
                        .action(name, params, items, *span, *fair, meta.clone())
                        .map_err(|error| error.with_origin(origin))?;
                    // Carried outside `action` so its argument list stays
                    // within the workspace Clippy budget; `sync` is a compose
                    // marker, not part of the action's own lowering.
                    action.sync = *sync;
                    actions.push(action);
                }
                SpecItem::Invariant {
                    name,
                    expr,
                    span,
                    meta,
                    ..
                } => {
                    invariants.push(PropertyDef {
                        name: name.clone(),
                        expr: expr.as_ref().clone(),
                        span: *span,
                        meta: meta.clone(),
                        annotations: self
                            .annotations
                            .annotations_for(&property_target("invariant", name))
                            .clone(),
                    });
                }
                SpecItem::Trans {
                    name,
                    expr,
                    span,
                    meta,
                    ..
                } => {
                    transitions.push(PropertyDef {
                        name: name.clone(),
                        expr: expr.as_ref().clone(),
                        span: *span,
                        meta: meta.clone(),
                        annotations: self
                            .annotations
                            .annotations_for(&property_target("trans", name))
                            .clone(),
                    });
                }
                SpecItem::Reachable {
                    name,
                    expr,
                    span,
                    meta,
                    ..
                } => {
                    reachables.push(PropertyDef {
                        name: name.clone(),
                        expr: expr.as_ref().clone(),
                        span: *span,
                        meta: meta.clone(),
                        annotations: self
                            .annotations
                            .annotations_for(&property_target("reachable", name))
                            .clone(),
                    });
                }
                SpecItem::Terminal { expr, span } => {
                    terminal = Some(expr.as_ref().clone());
                    terminal_span = Some(*span);
                }
                SpecItem::Unless {
                    name,
                    before,
                    after,
                    span,
                    meta,
                    ..
                } => transitions.push(PropertyDef {
                    name: name.clone(),
                    expr: unless_expr(before, after),
                    span: *span,
                    meta: meta.clone(),
                    annotations: self
                        .annotations
                        .annotations_for(&property_target("trans", name))
                        .clone(),
                }),
                SpecItem::Until {
                    name,
                    before,
                    after,
                    span,
                    meta,
                    ..
                } => {
                    transitions.push(PropertyDef {
                        name: format!("{name}_until_safety"),
                        expr: unless_expr(before, after),
                        span: *span,
                        meta: meta.clone(),
                        annotations: self
                            .annotations
                            .annotations_for(&property_target(
                                "trans",
                                &format!("{name}_until_safety"),
                            ))
                            .clone(),
                    });
                    leadstos.push(LeadsToDef {
                        name: name.clone(),
                        span: *span,
                        binders: Vec::new(),
                        before: before.as_ref().clone(),
                        after: after.as_ref().clone(),
                        meta: meta.clone(),
                        annotations: self
                            .annotations
                            .annotations_for(&property_target("leadsTo", name))
                            .clone(),
                        decreases: None,
                        within: None,
                        helpful: Vec::new(),
                    });
                }
                SpecItem::LeadsTo {
                    name,
                    binders,
                    before,
                    after,
                    span,
                    meta,
                    decreases,
                    within,
                    helpful,
                    ..
                } => {
                    let origin = self
                        .origins
                        .diagnostic_origin(&property_target("leadsTo", name));
                    leadstos.push(LeadsToDef {
                        name: name.clone(),
                        span: *span,
                        binders: binders.clone(),
                        before: before.as_ref().clone(),
                        after: after.as_ref().clone(),
                        meta: meta.clone(),
                        annotations: self
                            .annotations
                            .annotations_for(&property_target("leadsTo", name))
                            .clone(),
                        decreases: decreases.as_deref().cloned(),
                        within: within
                            .as_deref()
                            .map(|expr| self.const_int(expr))
                            .transpose()
                            .map_err(|error| error.with_origin(origin))?,
                        helpful: helpful.clone(),
                    });
                }
                _ => {}
            }
        }
        if state.is_empty() {
            return Err(model_error("spec has no state block")
                .with_origin(self.origins.diagnostic_origin(SPEC_TARGET)));
        }
        inline_init.extend(init);
        let init = inline_init;
        let mut model = KernelModel {
            name: self.spec.name,
            consts: self.consts,
            types: self.types,
            enum_members: self.enum_members,
            state,
            init,
            init_meta,
            init_annotations: self.annotations.annotations_for(INIT_TARGET).clone(),
            actions,
            invariants,
            transitions,
            reachables,
            leadstos,
            terminal,
            terminal_span,
            projections: self.projections,
            origins: self.origins,
            annotations: self.annotations,
            traceability,
            names: self.names,
        };
        let inline_initializers = inline_initializers
            .into_iter()
            .map(|(index, name, span)| (index, (name, span)))
            .collect::<BTreeMap<_, _>>();
        let mut init_phase = crate::typecheck::ValidationPhase::normalized();
        for (index, statement) in model.init.iter().enumerate() {
            let checked =
                crate::typecheck::validate_statement_types(statement, &model, &mut init_phase);
            checked.map_err(|error| {
                let origin = error.span.map(type_diagnostic_origin);
                let context = if let Some((name, _)) = inline_initializers.get(&index) {
                    ExpressionErrorContext::InlineInitializer(name)
                } else {
                    ExpressionErrorContext::Init
                };
                let mut failure = model_error(context.message(error.message)).with_origin(origin);
                if let Some(span) = error.span {
                    failure = failure.at(span);
                }
                if let Some(component) =
                    model
                        .origins
                        .diagnostic_origin(&crate::origin::component_check_target(
                            &init_statement_target(index),
                        ))
                {
                    failure = failure.with_origin(Some(component));
                }
                failure
            })?;
        }
        crate::typecheck::validate_model_expression_types(&model).map_err(|error| {
            let origin = error.span.map(type_diagnostic_origin);
            model_error(ExpressionErrorContext::Model.message(error.message)).with_origin(origin)
        })?;
        for (index, statement) in model.init.iter().enumerate() {
            check_init_collection_binders(
                std::slice::from_ref(statement),
                &model,
                &BTreeSet::new(),
            )
            .map_err(|error| {
                error.with_origin(model.origins.diagnostic_origin(
                    &crate::origin::component_check_target(&init_statement_target(index)),
                ))
            })?;
        }
        for property in &model.leadstos {
            for binder in &property.binders {
                check_leadsto_binder(binder).map_err(|error| {
                    model.error_at_target(
                        error.message,
                        property.span,
                        &property_target("leadsTo", &property.name),
                    )
                })?;
            }
        }
        for projection in &model.projections {
            crate::typecheck::validate_expression_type(
                &projection.expr,
                &TypeRef::Int,
                &[],
                &model,
            )
            .map_err(|error| {
                model_error(format!(
                    "invalid KPI projection '{}': {}",
                    projection.name, error.message
                ))
                .with_origin(Some(type_diagnostic_origin(projection.span)))
            })?;
        }
        model.origins.discard_component_check_targets();
        model.names.pattern_scopes.clear();
        Ok(model)
    }

    fn collect_consts(&mut self) -> Result<(), ModelError> {
        for item in &self.spec.items {
            if let SpecItem::Const { name, value } = item {
                if self.consts.contains_key(name) {
                    return Err(model_error(format!("duplicate const '{}'", shown(name))));
                }
                let value = eval_const(value, &self.consts)?;
                self.consts.insert(name.clone(), value);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn collect_types(&mut self) -> Result<(), ModelError> {
        let items = self.spec.items.clone();
        for item in &items {
            match item {
                SpecItem::Type {
                    name,
                    lo,
                    hi,
                    symmetric,
                } => {
                    let origin = self.origins.diagnostic_origin(&type_target(name));
                    self.insert_type(
                        name,
                        TypeDef::Domain {
                            lo: self
                                .const_int(lo)
                                .map_err(|error| error.with_origin(origin.clone()))?,
                            hi: self
                                .const_int(hi)
                                .map_err(|error| error.with_origin(origin.clone()))?,
                            symmetric: *symmetric,
                        },
                    )
                    .map_err(|error| error.with_origin(origin))?;
                }
                SpecItem::Enum {
                    name,
                    members,
                    member_spans,
                    span,
                    symmetric,
                } => {
                    let origin = self.origins.diagnostic_origin(&type_target(name));
                    if members.is_empty() {
                        let mut error =
                            model_error(format!("enum '{}' has no members", shown(name)));
                        if let Some(span) = span {
                            error = error.at(*span);
                        }
                        let origin = self
                            .origins
                            .diagnostic_origin(&crate::origin::component_check_target(
                                &type_target(name),
                            ))
                            .or(origin);
                        return Err(error.with_origin(origin));
                    }
                    self.insert_type(
                        name,
                        TypeDef::Enum {
                            members: members.clone(),
                            symmetric: *symmetric,
                        },
                    )
                    .map_err(|error| error.with_origin(origin.clone()))?;
                    for (index, member) in members.iter().enumerate() {
                        if self.enum_members.contains_key(member) {
                            let mut duplicate =
                                model_error(format!("duplicate enum member '{member}'"))
                                    .with_origin(origin)
                                    .into_name_resolution();
                            // The repeated occurrence, not the first one. Left
                            // unlocated the report falls back to the first
                            // token matching the quoted name, which for a
                            // duplicate is the earlier, innocent declaration by
                            // construction (issue 576).
                            if let Some(span) = member_spans.get(index) {
                                duplicate = duplicate.at(*span);
                            }
                            return Err(duplicate);
                        }
                        self.enum_members.insert(
                            member.clone(),
                            Value::Enum {
                                type_name: name.clone(),
                                member: member.clone(),
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        for item in &items {
            if let SpecItem::Struct { name, fields, span } = item {
                let origin = self.origins.diagnostic_origin(&type_target(name));
                // `TypeExpr` carries no span of its own, so the declaration's
                // span is the closest true location a field-level type
                // diagnostic can report (issue 555).
                for (field, ty) in fields {
                    reject_map_int_key(&format!("{name}.{field}"), ty, *span)?;
                }
                let resolved = fields
                    .iter()
                    .map(|(field, ty)| Ok((field.clone(), self.resolve_type(ty)?)))
                    .collect::<Result<Vec<_>, ModelError>>()
                    .map_err(|error| error.with_origin(origin.clone()).at(*span))?;
                for (field, ty) in &resolved {
                    if !self.optional_scalar(ty) {
                        return Err(model_error(format!(
                            "struct field '{name}.{field}' has non-scalar type"
                        ))
                        .with_origin(origin)
                        .at(*span));
                    }
                }
                self.insert_type(name, TypeDef::Struct { fields: resolved })
                    .map_err(|error| error.with_origin(origin).at(*span))?;
            }
        }
        Ok(())
    }

    fn scalar(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Int | TypeRef::Bool | TypeRef::Range(_, _) => true,
            TypeRef::Named(name) => match self.types.get(name) {
                Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) => true,
                Some(TypeDef::Struct { .. }) | None => false,
            },
            TypeRef::Map(_, _)
            | TypeRef::Relation(_, _)
            | TypeRef::Set(_)
            | TypeRef::Seq(_, _)
            | TypeRef::Option(_) => false,
        }
    }

    fn bounded_scalar(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Bool | TypeRef::Range(_, _) => true,
            TypeRef::Named(name) => match self.types.get(name) {
                Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) => true,
                Some(TypeDef::Struct { .. }) | None => false,
            },
            TypeRef::Int
            | TypeRef::Map(_, _)
            | TypeRef::Relation(_, _)
            | TypeRef::Set(_)
            | TypeRef::Seq(_, _)
            | TypeRef::Option(_) => false,
        }
    }

    fn optional_scalar(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Int | TypeRef::Bool | TypeRef::Range(_, _) | TypeRef::Named(_) => {
                self.scalar(ty)
            }
            TypeRef::Option(inner) => self.optional_scalar(inner),
            TypeRef::Map(_, _) | TypeRef::Relation(_, _) | TypeRef::Set(_) | TypeRef::Seq(_, _) => {
                false
            }
        }
    }

    fn struct_value(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Named(name) => match self.types.get(name) {
                Some(TypeDef::Struct { fields }) => fields
                    .iter()
                    .all(|(_, field_ty)| self.optional_scalar(field_ty)),
                Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) | None => false,
            },
            TypeRef::Int
            | TypeRef::Bool
            | TypeRef::Range(_, _)
            | TypeRef::Map(_, _)
            | TypeRef::Relation(_, _)
            | TypeRef::Set(_)
            | TypeRef::Seq(_, _)
            | TypeRef::Option(_) => false,
        }
    }

    fn legal_state_type(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Int | TypeRef::Bool | TypeRef::Range(_, _) | TypeRef::Named(_) => {
                self.optional_scalar(ty) || self.struct_value(ty)
            }
            TypeRef::Option(_) => self.optional_scalar(ty),
            TypeRef::Map(key, value) => {
                self.bounded_scalar(key)
                    && (self.optional_scalar(value) || self.struct_value(value))
            }
            TypeRef::Relation(source, target) => {
                self.bounded_scalar(source) && self.bounded_scalar(target)
            }
            TypeRef::Set(element) => self.bounded_scalar(element),
            TypeRef::Seq(element, _) => self.scalar(element),
        }
    }

    fn insert_type(&mut self, name: &str, definition: TypeDef) -> Result<(), ModelError> {
        if self.types.insert(name.to_owned(), definition).is_some() {
            Err(model_error(format!("duplicate type '{}'", shown(name))))
        } else {
            Ok(())
        }
    }

    fn resolve_type(&self, ty: &TypeExpr) -> Result<TypeRef, ModelError> {
        resolve_type(ty, &self.types, &self.consts)
    }

    fn action(
        &self,
        name: &str,
        params: &[Param],
        items: &[ActionItem],
        span: Span,
        fair: bool,
        meta: Option<MetaTag>,
    ) -> Result<ActionDef, ModelError> {
        let mut parameter_names = BTreeSet::new();
        let params = params
            .iter()
            .map(|param| {
                let parameter_name = match param {
                    Param::Typed(name, _) | Param::Range(name, _, _) => name,
                };
                if !parameter_names.insert(parameter_name) {
                    return Err(model_error(format!(
                        "duplicate action parameter '{parameter_name}'"
                    )));
                }
                self.action_param(param)
            })
            .collect::<Result<_, ModelError>>()?;
        let mut requires = Vec::new();
        let mut require_spans = Vec::new();
        let mut lets = Vec::new();
        let mut statements = Vec::new();
        let mut ensures = Vec::new();
        let mut ensure_spans = Vec::new();
        let mut guards = Vec::new();
        for item in items {
            match item {
                ActionItem::Requires(expr, item_span) => {
                    requires.push(expr.clone());
                    require_spans.push(*item_span);
                    guards.push(ActionGuard::Requires(expr.clone()));
                }
                ActionItem::Ensures(expr, item_span) => {
                    ensures.push(expr.clone());
                    ensure_spans.push(*item_span);
                }
                ActionItem::Let(name, expr, _) => {
                    lets.push((name.clone(), expr.clone()));
                    guards.push(ActionGuard::Let(name.clone(), expr.clone()));
                }
                ActionItem::Statement(statement) => statements.push(statement.clone()),
            }
        }
        let mut index_constants = self.consts.clone();
        index_constants.extend(self.enum_members.clone());
        if let Some(conflict) = duplicate_statement_write(&statements, &index_constants) {
            return Err(match conflict {
                WriteAliasConflict::Duplicate((_, duplicate_span)) => {
                    model_error("an action may not assign the same state location more than once")
                        .with_origin(self.origins.diagnostic_origin(&action_target(name)))
                        .at(duplicate_span)
                }
                WriteAliasConflict::DistinctnessUnproved(conflict) => self
                    .write_distinctness_unproved_error(
                        name,
                        &conflict.target,
                        &conflict.value,
                        conflict.span,
                        &conflict.binder,
                        conflict.forall_span,
                    ),
            });
        }
        Ok(ActionDef {
            name: name.to_owned(),
            span,
            params,
            requires,
            require_spans,
            lets,
            guards,
            statements,
            ensures,
            ensure_spans,
            fair,
            sync: false,
            meta,
            annotations: self
                .annotations
                .annotations_for(&action_target(name))
                .clone(),
        })
    }

    fn action_param(&self, param: &Param) -> Result<ParamDef, ModelError> {
        match param {
            Param::Typed(name, qualified) => {
                if qualified.namespace.is_some() {
                    return Err(model_error("qualified type remained after kernel lowering"));
                }
                let ty = match qualified.name.as_str() {
                    "Bool" => TypeExpr::Bool,
                    "Int" => TypeExpr::Int,
                    _ => TypeExpr::Name(qualified.name.clone()),
                };
                let ty = self.resolve_type(&ty)?;
                let finite_scalar = matches!(ty, TypeRef::Bool | TypeRef::Range(_, _))
                    || matches!(
                        &ty,
                        TypeRef::Named(name)
                            if matches!(
                                self.types.get(name),
                                Some(TypeDef::Domain { .. } | TypeDef::Enum { .. })
                            )
                    );
                if !finite_scalar {
                    return Err(model_error("action parameter type is not a finite scalar"));
                }
                Ok(ParamDef::Typed {
                    name: name.clone(),
                    ty,
                })
            }
            Param::Range(name, lo, hi) => {
                let lo = self.const_int(lo)?;
                let hi = self.const_int(hi)?;
                if lo > hi {
                    return Err(model_error("action parameter range is empty"));
                }
                Ok(ParamDef::Range {
                    name: name.clone(),
                    lo,
                    hi,
                })
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn collect_declaration_annotations(&mut self) {
        if let Some(meta) = &self.spec.meta {
            self.annotations.bind(
                SPEC_TARGET,
                Annotation::from_legacy_kind(
                    meta.id.clone(),
                    meta.text.clone(),
                    meta.span.unwrap_or_else(unknown_span),
                ),
            );
        }
        for item in &self.spec.items {
            let (target, meta, annotations, span) = match item {
                SpecItem::Init {
                    meta, annotations, ..
                } => (
                    Some(INIT_TARGET.to_owned()),
                    meta.as_ref(),
                    Some(annotations),
                    unknown_span(),
                ),
                SpecItem::Action {
                    name,
                    meta,
                    span,
                    annotations,
                    ..
                } => (
                    Some(action_target(name)),
                    meta.as_ref(),
                    Some(annotations),
                    *span,
                ),
                SpecItem::Invariant {
                    name,
                    meta,
                    span,
                    annotations,
                    ..
                } => (
                    Some(property_target("invariant", name)),
                    meta.as_ref(),
                    Some(annotations),
                    *span,
                ),
                SpecItem::Trans {
                    name,
                    meta,
                    span,
                    annotations,
                    ..
                }
                | SpecItem::Unless {
                    name,
                    meta,
                    span,
                    annotations,
                    ..
                } => (
                    Some(property_target("trans", name)),
                    meta.as_ref(),
                    Some(annotations),
                    *span,
                ),
                SpecItem::Reachable {
                    name,
                    meta,
                    span,
                    annotations,
                    ..
                } => (
                    Some(property_target("reachable", name)),
                    meta.as_ref(),
                    Some(annotations),
                    *span,
                ),
                SpecItem::Until {
                    name,
                    meta,
                    span,
                    annotations,
                    ..
                } => {
                    let trans_target = property_target("trans", &format!("{name}_until_safety"));
                    let leadsto_target = property_target("leadsTo", name);
                    self.annotations
                        .extend(trans_target.clone(), annotations.clone());
                    self.annotations
                        .extend(leadsto_target.clone(), annotations.clone());
                    if let Some(meta) = meta {
                        let annotation = Annotation::from_legacy(
                            meta.id.clone(),
                            meta.text.clone(),
                            meta.span.unwrap_or(*span),
                        );
                        self.annotations.bind(trans_target, annotation.clone());
                        self.annotations.bind(leadsto_target, annotation);
                    }
                    (None, None, None, *span)
                }
                SpecItem::LeadsTo {
                    name,
                    meta,
                    span,
                    annotations,
                    ..
                } => (
                    Some(property_target("leadsTo", name)),
                    meta.as_ref(),
                    Some(annotations),
                    *span,
                ),
                _ => (None, None, None, unknown_span()),
            };
            let Some(target) = target else { continue };
            if let Some(annotations) = annotations {
                self.annotations.extend(target.clone(), annotations.clone());
            }
            if let Some(meta) = meta {
                self.annotations.bind(
                    target,
                    Annotation::from_legacy(
                        meta.id.clone(),
                        meta.text.clone(),
                        meta.span.unwrap_or(span),
                    ),
                );
            }
        }
    }

    fn const_int(&self, expr: &Expr) -> Result<i64, ModelError> {
        match eval_const(expr, &self.consts)? {
            Value::Int(value) => Ok(value),
            _ => Err(model_error("constant expression must be an integer")),
        }
    }

    fn write_distinctness_unproved_error(
        &self,
        action_name: &str,
        target: &LValue,
        value: &Expr,
        span: Span,
        binder: &Binder,
        forall_span: Span,
    ) -> ModelError {
        let mut error =
            model_error("cannot prove write-index distinctness across forall iterations")
                .with_diagnostic_code(WRITE_DISTINCTNESS_UNPROVED_CODE)
                .with_origin(self.origins.diagnostic_origin(&action_target(action_name)))
                .at(span);
        if let Some((hint, replacement)) =
            self.try_distinctness_hint(target, value, binder, forall_span)
        {
            error = error.with_hint(hint).with_quick_fix(DiagnosticEdit {
                span: forall_span,
                replacement,
            });
        }
        error
    }

    fn try_distinctness_hint(
        &self,
        target: &LValue,
        value: &Expr,
        binder: &Binder,
        forall_span: Span,
    ) -> Option<(String, String)> {
        let LValue::Index(map_name, index) = target else {
            return None;
        };
        let binder_name = match binder {
            Binder::Typed { name, .. } | Binder::Range { name, .. } => name,
            Binder::Collection { .. } => return None,
        };
        if expr_contains_var(value, binder_name) {
            return None;
        }
        let key_type = self.map_key_type_name(map_name)?;
        let offset = affine_binder_offset(index, binder_name)?;
        // The binder's real bounds, not its width: `Off = 2..5` indexed as
        // `BASE + c` covers `BASE+2..=BASE+5`, and a width-only rewrite writes
        // `BASE..BASE+4`. `binder_domain_bounds` also refuses a filtered binder,
        // whose subset this rewrite cannot reproduce.
        let (lo, hi) = self.binder_domain_bounds(binder)?;
        // The replacement introduces `k`. If the RHS or the offset already
        // refers to a `k` from an enclosing scope, the new binder captures it
        // and the rewrite changes meaning. Refuse instead of renaming: the
        // classification, code and location still reach the reader.
        if expr_contains_var(value, "k") || expr_contains_var(&offset, "k") || map_name == "k" {
            return None;
        }
        // `hi + 1` is the exclusive upper bound; refuse rather than wrap if the
        // domain reaches i64::MAX.
        let exclusive_hi = hi.checked_add(1)?;
        // The offset is spliced into `k >= <offset>` and `k < <offset> + n`.
        // `expr_text` does not parenthesize a conditional or a binary operand,
        // so anything that is not already atomic must be wrapped or the emitted
        // comparison reassociates.
        let offset_text = match &offset {
            Expr::Var(_) | Expr::Num(_) | Expr::Field(..) => expr_text(&offset),
            other => format!("({})", expr_text(other)),
        };
        let rhs = expr_text(value);
        // The written set is `offset + lo ..= offset + hi`, so the exclusive
        // upper bound is `offset + hi + 1`. Emitting `offset + width` instead
        // silently shifts the whole interval whenever `lo != 0`.
        let low_text = if lo == 0 {
            offset_text.clone()
        } else {
            format!("{offset_text} + {lo}")
        };
        let high_text = format!("{offset_text} + {exclusive_hi}");
        let replacement = format!(
            "forall k: {key_type} {{ if k >= {low_text} and k < {high_text} {{ {map_name}[k] = {rhs} }} }}"
        );
        let _ = forall_span;
        let hint = replacement.clone();
        Some((hint, replacement))
    }

    fn map_key_type_name(&self, map_name: &str) -> Option<String> {
        self.spec.items.iter().find_map(|item| match item {
            SpecItem::State(fields) => fields.iter().find_map(|field| {
                if field.name != map_name {
                    return None;
                }
                match &field.ty {
                    TypeExpr::Map(key, _) => type_expr_display_name(key),
                    _ => None,
                }
            }),
            _ => None,
        })
    }

    /// The binder's inclusive `(lo, hi)`, or `None` when it is not a bounded
    /// integer domain. #698's quick-fix needs the lower bound, not just the
    /// width: `Off = 2..5` indexed as `BASE + c` covers `BASE+2..=BASE+5`, and
    /// a width-only rewrite writes `BASE..BASE+4` -- a different set.
    fn binder_domain_bounds(&self, binder: &Binder) -> Option<(i64, i64)> {
        match binder {
            Binder::Typed {
                type_name,
                where_expr,
                ..
            } => {
                // A filtered binder ranges over a subset this rewrite cannot
                // reproduce, so refuse rather than widen it.
                if where_expr.is_some() {
                    return None;
                }
                match self.types.get(&type_name.name) {
                    Some(TypeDef::Domain { lo, hi, .. }) => Some((*lo, *hi)),
                    _ => None,
                }
            }
            Binder::Range {
                lo, hi, where_expr, ..
            } => {
                if where_expr.is_some() {
                    return None;
                }
                Some((self.const_int(lo).ok()?, self.const_int(hi).ok()?))
            }
            Binder::Collection { .. } => None,
        }
    }
}

fn resolve_type(
    ty: &TypeExpr,
    types: &BTreeMap<String, TypeDef>,
    consts: &BTreeMap<String, Value>,
) -> Result<TypeRef, ModelError> {
    Ok(match ty {
        TypeExpr::Int => TypeRef::Int,
        TypeExpr::Bool => TypeRef::Bool,
        TypeExpr::Name(name) => {
            if !types.contains_key(name) {
                return Err(model_error(format!("unknown type '{}'", shown(name))));
            }
            TypeRef::Named(name.clone())
        }
        TypeExpr::Range(lo, hi) => TypeRef::Range(const_int(lo, consts)?, const_int(hi, consts)?),
        TypeExpr::Map(key, value) => TypeRef::Map(
            Box::new(resolve_type(key, types, consts)?),
            Box::new(resolve_type(value, types, consts)?),
        ),
        TypeExpr::Relation(source, target) => TypeRef::Relation(
            Box::new(resolve_type(source, types, consts)?),
            Box::new(resolve_type(target, types, consts)?),
        ),
        TypeExpr::Set(inner) => TypeRef::Set(Box::new(resolve_type(inner, types, consts)?)),
        TypeExpr::Seq(inner, cap) => {
            let cap = usize::try_from(const_int(cap, consts)?)
                .map_err(|_| model_error("sequence capacity must be non-negative"))?;
            TypeRef::Seq(Box::new(resolve_type(inner, types, consts)?), cap)
        }
        TypeExpr::Option(inner) => TypeRef::Option(Box::new(resolve_type(inner, types, consts)?)),
    })
}

fn const_int(expr: &Expr, consts: &BTreeMap<String, Value>) -> Result<i64, ModelError> {
    match eval_const(expr, consts)? {
        Value::Int(value) => Ok(value),
        _ => Err(model_error("constant expression must be an integer")),
    }
}

/// One write to a state location, with the span of the statement performing it.
type StateWrite = (LValue, Span);

#[derive(Clone, Debug, Eq, PartialEq)]
struct DistinctnessUnprovedWrite {
    target: LValue,
    value: Expr,
    span: Span,
    binder: Binder,
    forall_span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum WriteAliasConflict {
    Duplicate(StateWrite),
    DistinctnessUnproved(Box<DistinctnessUnprovedWrite>),
}

/// Find a state location an action writes twice, with the span of the write
/// that repeats it.
///
/// The span travels with the target so the diagnostic can point at the offending
/// assignment rather than at the action as a whole (issue 555).
fn duplicate_statement_write(
    statements: &[Statement],
    constants: &BTreeMap<String, Value>,
) -> Option<WriteAliasConflict> {
    fn writes(
        statements: &[Statement],
        constants: &BTreeMap<String, Value>,
    ) -> Result<Vec<StateWrite>, Box<WriteAliasConflict>> {
        let mut seen: Vec<(LValue, Span)> = Vec::new();
        for statement in statements {
            let candidates = match statement {
                Statement::Assign { target, span, .. } => vec![(target.clone(), *span)],
                Statement::If {
                    then_statements,
                    else_statements,
                    ..
                } => {
                    let mut branch = writes(then_statements, constants)?;
                    branch.extend(writes(else_statements, constants)?);
                    branch
                }
                Statement::ForAll {
                    binder,
                    statements,
                    span: forall_span,
                    ..
                } => {
                    let repeated = writes(statements, constants)?;
                    if let Some((target, write_span)) = repeated
                        .iter()
                        .find(|(target, _)| !write_is_injective_for_binder(target, binder))
                    {
                        let (_, index, _) = lvalue_path(target);
                        if index.is_some_and(|idx| static_index_identity(idx, constants).is_some())
                        {
                            return Err(Box::new(WriteAliasConflict::Duplicate((
                                target.clone(),
                                *write_span,
                            ))));
                        }
                        let Some((value, _)) = assignment_for_target(statements, target) else {
                            return Err(Box::new(WriteAliasConflict::Duplicate((
                                target.clone(),
                                *write_span,
                            ))));
                        };
                        return Err(Box::new(WriteAliasConflict::DistinctnessUnproved(
                            Box::new(DistinctnessUnprovedWrite {
                                target: target.clone(),
                                value,
                                span: *write_span,
                                binder: binder.clone(),
                                forall_span: *forall_span,
                            }),
                        )));
                    }
                    repeated
                }
            };
            if let Some(write) = candidates.iter().find(|(target, _)| {
                seen.iter()
                    .any(|(previous, _)| lvalues_may_alias(previous, target, constants))
            }) {
                return Err(Box::new(WriteAliasConflict::Duplicate(write.clone())));
            }
            seen.extend(candidates);
        }
        Ok(seen)
    }
    writes(statements, constants).err().map(|write| *write)
}

fn assignment_for_target(statements: &[Statement], target: &LValue) -> Option<(Expr, Span)> {
    statements.iter().find_map(|statement| match statement {
        Statement::Assign {
            target: candidate,
            value,
            span,
        } if candidate == target => Some((value.clone(), *span)),
        _ => None,
    })
}

/// The key type as it may appear after `forall k:`.
///
/// Deliberately named types only. An inline `Map<0..7, _>` key would render as
/// `forall k: 0..7`, but the grammar only accepts a qualified name after `:`
/// (`fsl-syntax`'s `parser.rs`, the `qualified_name()` call in the binder rule);
/// the range form is spelled `forall k in 0..7`. Emitting the former produces a
/// hint that does not parse, which is a worse outcome than no hint: #698's
/// contract is that the repair it names actually checks.
fn type_expr_display_name(ty: &TypeExpr) -> Option<String> {
    match ty {
        TypeExpr::Name(name) => Some(name.clone()),
        _ => None,
    }
}

fn affine_binder_offset(index: &Expr, binder_name: &str) -> Option<Expr> {
    match index {
        Expr::Binary { op, left, right } if op == "+" => {
            if matches!(&**right, Expr::Var(name) if name == binder_name)
                && !expr_contains_var(left, binder_name)
            {
                return Some(left.as_ref().clone());
            }
            if matches!(&**left, Expr::Var(name) if name == binder_name)
                && !expr_contains_var(right, binder_name)
            {
                return Some(right.as_ref().clone());
            }
        }
        _ => {}
    }
    None
}

fn expr_contains_var(expr: &Expr, name: &str) -> bool {
    // Built on `expr_children`/`binder_exprs` rather than a second hand-written
    // match: the hand-written one silently missed `Expr::Method`'s `args` and a
    // quantifier's own range/filter, and #698's quick-fix guard reads this. A
    // walk that drifts from the AST here does not merely miss a case, it emits
    // an edit that moves an RHS out of the scope that bound it. Any new `Expr`
    // variant is covered the moment `expr_children` covers it.
    if let Expr::Var(candidate) = expr
        && candidate == name
    {
        return true;
    }
    // A binder's own range and filter are evaluated in the outer scope, and its
    // body may shadow `name`. Fail closed on the whole construct rather than
    // modelling that: refusing a hint is safe, offering a captured one is not.
    if matches!(expr, Expr::Quantified { .. } | Expr::Aggregate { .. }) {
        return true;
    }
    expr_children(expr)
        .into_iter()
        .any(|child| expr_contains_var(child, name))
}

fn write_is_injective_for_binder(target: &LValue, binder: &Binder) -> bool {
    let name = match binder {
        Binder::Typed { name, .. } | Binder::Range { name, .. } => name,
        Binder::Collection { .. } => return false,
    };
    let (_, index, _) = lvalue_path(target);
    matches!(index, Some(Expr::Var(index)) if index == name)
}

/// Resolve an index expression to a comparable static identity, if it has one.
///
/// This is used only for the write-aliasing check in [`lvalues_may_alias`]. It
/// recognizes enum-member indices (both the typed `Expr::EnumMember` literal and
/// a local constant bound to an enum value) in addition to the `Int`/`Bool`
/// constants that [`eval_const`] already handles, so two writes indexed by
/// distinct enum members are provably distinct. It must not be used to widen
/// what counts as a constant expression elsewhere (`eval_const`/`ConstType` are
/// shared with `const_int`, domain-type bounds, `Seq` capacity, and `within`
/// deadlines).
fn static_index_identity(expr: &Expr, constants: &BTreeMap<String, Value>) -> Option<Value> {
    match expr {
        Expr::EnumMember {
            type_name, member, ..
        } => Some(Value::Enum {
            type_name: type_name.clone(),
            member: member.clone(),
        }),
        Expr::Var(name) => match constants.get(name) {
            Some(value @ Value::Enum { .. }) => Some(value.clone()),
            _ => eval_const(expr, constants).ok(),
        },
        _ => eval_const(expr, constants).ok(),
    }
}

fn lvalues_may_alias(left: &LValue, right: &LValue, constants: &BTreeMap<String, Value>) -> bool {
    let (left_root, left_index, left_fields) = lvalue_path(left);
    let (right_root, right_index, right_fields) = lvalue_path(right);
    if left_root != right_root {
        return false;
    }
    if let (Some(left), Some(right)) = (left_index, right_index)
        && let (Some(left), Some(right)) = (
            static_index_identity(left, constants),
            static_index_identity(right, constants),
        )
        && left != right
    {
        return false;
    }
    left_fields
        .iter()
        .zip(&right_fields)
        .all(|(left, right)| left == right)
}

fn lvalue_path(target: &LValue) -> (&str, Option<&Expr>, Vec<&str>) {
    match target {
        LValue::Var(name) => (name, None, Vec::new()),
        LValue::Index(name, index) => (name, Some(index), Vec::new()),
        LValue::Field(base, field) => {
            let (root, index, mut fields) = lvalue_path(base);
            fields.push(field);
            (root, index, fields)
        }
    }
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

fn source_origin(name: &str, primary: Span, secondary: Option<Span>) -> OriginChain {
    OriginChain {
        id: OriginId(format!(
            "kernel:inline-initializer:{name}:{}:{}",
            primary.start.offset, primary.end.offset
        )),
        dialect: "kernel".to_owned(),
        primary: Some(OriginSite {
            source_file: None,
            span: Some(primary),
            dialect: "kernel".to_owned(),
            declaration_path: vec!["state".to_owned(), name.to_owned()],
        }),
        secondary: secondary
            .map(|span| OriginSite {
                source_file: None,
                span: Some(span),
                dialect: "kernel".to_owned(),
                declaration_path: vec!["init".to_owned(), name.to_owned()],
            })
            .into_iter()
            .collect(),
        lowering_steps: vec![LoweringStep {
            kind: "inline_initializer".to_owned(),
            detail: Some("normalized to init assignment".to_owned()),
        }],
        generated: false,
    }
}

pub(crate) fn type_diagnostic_origin(span: Span) -> OriginChain {
    OriginChain {
        id: OriginId(format!(
            "kernel:type-diagnostic:{}:{}",
            span.start.offset, span.end.offset
        )),
        dialect: "kernel".to_owned(),
        primary: Some(OriginSite {
            source_file: None,
            span: Some(span),
            dialect: "kernel".to_owned(),
            declaration_path: Vec::new(),
        }),
        secondary: Vec::new(),
        lowering_steps: Vec::new(),
        generated: false,
    }
}

fn lvalue_root(target: &LValue) -> &str {
    match target {
        LValue::Var(name) | LValue::Index(name, _) => name,
        LValue::Field(base, _) => lvalue_root(base),
    }
}

fn statement_root_spans(statements: &[Statement]) -> Vec<(String, Span)> {
    let mut roots = Vec::new();
    for statement in statements {
        match statement {
            Statement::Assign { target, span, .. } => {
                roots.push((lvalue_root(target).to_owned(), *span));
            }
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                roots.extend(statement_root_spans(then_statements));
                roots.extend(statement_root_spans(else_statements));
            }
            Statement::ForAll { statements, .. } => {
                roots.extend(statement_root_spans(statements));
            }
        }
    }
    roots
}

fn first_state_reference(expr: &Expr, state_names: &BTreeSet<String>) -> Option<String> {
    let mut references = Vec::new();
    collect_state_references(expr, state_names, &mut references);
    references.into_iter().next()
}

fn collect_state_references(
    expr: &Expr,
    state_names: &BTreeSet<String>,
    references: &mut Vec<String>,
) {
    visit_free_names(expr, &BTreeSet::new(), &mut |name| {
        if state_names.contains(name) {
            references.push(name.to_owned());
        }
    });
}

/// Visit free variable occurrences in source order, including duplicates.
pub(crate) fn visit_free_names(
    expr: &Expr,
    bound: &BTreeSet<String>,
    visit: &mut impl FnMut(&str),
) {
    crate::recursion::guard(|| visit_free_names_inner(expr, bound, visit));
}

fn visit_free_names_inner(expr: &Expr, bound: &BTreeSet<String>, visit: &mut impl FnMut(&str)) {
    if let Expr::Binary { op, left, right } = expr {
        visit_free_names(left, bound, visit);
        let local = crate::compose::pattern_scope(
            left,
            &bound.iter().cloned().collect(),
            &std::collections::HashSet::new(),
            Some(op),
        );
        visit_free_names(right, &local.into_iter().collect(), visit);
        return;
    }
    if let Expr::Var(name) = expr
        && !bound.contains(name)
    {
        visit(name);
    }
    let scoped = match expr {
        Expr::Quantified { binder, body, .. } => Some((binder, Some(body.as_ref()))),
        Expr::Aggregate { binder, value, .. } => Some((binder, value.as_deref())),
        _ => None,
    };
    if let Some((binder, body)) = scoped {
        let local = visit_free_binder_names(binder, bound, visit);
        if let Some(body) = body {
            visit_free_names(body, &local, visit);
        }
    } else {
        for child in expr_children(expr) {
            visit_free_names(child, bound, visit);
        }
    }
}

pub(crate) fn visit_free_binder_names(
    binder: &Binder,
    bound: &BTreeSet<String>,
    visit: &mut impl FnMut(&str),
) -> BTreeSet<String> {
    let (name, filter) = binder_scope(binder);
    let mut sources = binder_exprs(binder);
    if filter.is_some() {
        sources.pop();
    }
    for source in sources {
        visit_free_names(source, bound, visit);
    }
    let mut local = bound.clone();
    local.insert(name.to_owned());
    if let Some(filter) = filter {
        visit_free_names(filter, &local, visit);
    }
    local
}

/// Free state references, excluding enclosing locals, in lexicographic name order.
#[must_use]
pub fn state_references<'a>(
    expr: &Expr,
    model: &KernelModel,
    bound_names: impl IntoIterator<Item = &'a str>,
) -> BTreeSet<String> {
    let mut state_names = model
        .state
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    for name in bound_names {
        state_names.remove(name);
    }
    let mut references = Vec::new();
    collect_state_references(expr, &state_names, &mut references);
    references.into_iter().collect()
}

/// Reject state-dependent collection domains in an init statement's `forall`.
///
/// Model construction and unchecked-model verifier/runtime entries share this rule.
///
/// # Errors
/// Returns the shared diagnostic for the first state name in lexicographic order.
pub fn check_init_collection_binder<'a>(
    model: &KernelModel,
    binder: &Binder,
    bound_names: impl IntoIterator<Item = &'a str>,
) -> Result<(), ModelError> {
    let Binder::Collection { collection, .. } = binder else {
        return Ok(());
    };
    if let Some(name) = state_references(collection, model, bound_names).first() {
        return Err(model_error(format!(
            "init forall over a state collection is not supported; state variable '{}' is not allowed",
            shown(name)
        )));
    }
    Ok(())
}

fn check_init_collection_binders(
    statements: &[Statement],
    model: &KernelModel,
    bound_names: &BTreeSet<String>,
) -> Result<(), ModelError> {
    for statement in statements {
        match statement {
            Statement::ForAll {
                binder,
                statements,
                span,
            } => {
                check_init_collection_binder(model, binder, bound_names.iter().map(String::as_str))
                    .map_err(|error| error.at(*span))?;
                let mut local = bound_names.clone();
                local.insert(binder_scope(binder).0.to_owned());
                check_init_collection_binders(statements, model, &local)?;
            }
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                check_init_collection_binders(then_statements, model, bound_names)?;
                check_init_collection_binders(else_statements, model, bound_names)?;
            }
            Statement::Assign { .. } => {}
        }
    }
    Ok(())
}

pub(crate) fn unsupported_inline_form(expr: &Expr) -> Option<&'static str> {
    let unsupported = match expr {
        Expr::Quantified { .. } => Some("a quantified expression"),
        Expr::Aggregate { .. } => Some("a binder or aggregate expression"),
        Expr::UnaryNamed { name, .. }
            if name == "old" || name == "stage" || name.starts_with("rel_") =>
        {
            Some("a temporal or relational expression")
        }
        Expr::TernaryNamed { name, .. } if name.starts_with("rel_") => {
            Some("a relational expression")
        }
        _ => None,
    };
    unsupported.or_else(|| {
        expr_children(expr)
            .into_iter()
            .find_map(unsupported_inline_form)
    })
}

fn expr_children(expr: &Expr) -> Vec<&Expr> {
    match expr {
        Expr::Some(expr)
        | Expr::Neg(expr)
        | Expr::Not(expr)
        | Expr::Stage { entity: expr, .. }
        | Expr::UnaryNamed { expr, .. }
        | Expr::Is { expr, .. }
        | Expr::Field(expr, _) => vec![expr],
        Expr::Set(items) | Expr::Seq(items) => items.iter().collect(),
        Expr::Struct { fields, .. } => fields.iter().map(|(_, expr)| expr).collect(),
        Expr::Call { args, .. } => args.iter().collect(),
        Expr::Index(left, right)
        | Expr::Binary { left, right, .. }
        | Expr::BinaryNamed { left, right, .. } => vec![left, right],
        Expr::Method { receiver, args, .. } => std::iter::once(receiver.as_ref())
            .chain(args.iter())
            .collect(),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => vec![condition, then_expr, else_expr],
        Expr::Quantified { binder, body, .. } => binder_exprs(binder)
            .into_iter()
            .chain(std::iter::once(body.as_ref()))
            .collect(),
        Expr::Aggregate { binder, value, .. } => binder_exprs(binder)
            .into_iter()
            .chain(value.as_deref())
            .collect(),
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => vec![first, second, third],
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) | Expr::EnumMember { .. } => {
            Vec::new()
        }
    }
}

fn binder_exprs(binder: &Binder) -> Vec<&Expr> {
    match binder {
        Binder::Typed { where_expr, .. } => where_expr.iter().map(AsRef::as_ref).collect(),
        Binder::Range {
            lo, hi, where_expr, ..
        } => std::iter::once(lo.as_ref())
            .chain(std::iter::once(hi.as_ref()))
            .chain(where_expr.as_deref())
            .collect(),
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => std::iter::once(collection.as_ref())
            .chain(where_expr.iter().map(AsRef::as_ref))
            .collect(),
    }
}

pub(crate) fn binder_scope(binder: &Binder) -> (&str, Option<&Expr>) {
    match binder {
        Binder::Typed {
            name, where_expr, ..
        }
        | Binder::Range {
            name, where_expr, ..
        }
        | Binder::Collection {
            name, where_expr, ..
        } => (name, where_expr.as_deref()),
    }
}

fn unless_expr(before: &Expr, after: &Expr) -> Expr {
    Expr::Binary {
        op: "=>".to_owned(),
        left: Box::new(Expr::Binary {
            op: "and".to_owned(),
            left: Box::new(old(before.clone())),
            right: Box::new(Expr::Not(Box::new(old(after.clone())))),
        }),
        right: Box::new(Expr::Binary {
            op: "or".to_owned(),
            left: Box::new(before.clone()),
            right: Box::new(after.clone()),
        }),
    }
}

fn old(expr: Expr) -> Expr {
    Expr::UnaryNamed {
        name: "old".to_owned(),
        expr: Box::new(expr),
        span: synthetic_span(),
    }
}

fn synthetic_span() -> fsl_syntax::Span {
    let pos = fsl_syntax::SourcePos {
        offset: 0,
        line: 0,
        column: 0,
    };
    fsl_syntax::Span {
        start: pos,
        end: pos,
    }
}

fn eval_const(expr: &Expr, consts: &BTreeMap<String, Value>) -> Result<Value, ModelError> {
    eval_const_typed(expr, consts, true)?
        .value
        .ok_or_else(|| model_error("constant expression produced no value"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConstType {
    Int,
    Bool,
}

struct TypedConst {
    ty: ConstType,
    value: Option<Value>,
}

fn require_const_type(actual: ConstType, expected: ConstType) -> Result<(), ModelError> {
    if actual == expected {
        Ok(())
    } else {
        Err(model_error("constant expression type mismatch"))
    }
}

#[allow(clippy::too_many_lines)]
fn eval_const_typed(
    expr: &Expr,
    consts: &BTreeMap<String, Value>,
    evaluate: bool,
) -> Result<TypedConst, ModelError> {
    let typed = |ty, value| TypedConst {
        ty,
        value: evaluate.then_some(value),
    };
    match expr {
        Expr::Num(value) => Ok(typed(ConstType::Int, Value::Int(*value))),
        Expr::Bool(value) => Ok(typed(ConstType::Bool, Value::Bool(*value))),
        Expr::Var(name) => match consts.get(name).cloned() {
            Some(value @ Value::Int(_)) => Ok(typed(ConstType::Int, value)),
            Some(value @ Value::Bool(_)) => Ok(typed(ConstType::Bool, value)),
            Some(_) => Err(model_error("unsupported constant value type")),
            None => Err(model_error(format!("unknown constant '{}'", shown(name)))),
        },
        Expr::Neg(value) => {
            let value = eval_const_typed(value, consts, evaluate)?;
            require_const_type(value.ty, ConstType::Int)?;
            let value = value
                .value
                .map(|value| checked_neg(as_int(&value)?).map(Value::Int))
                .transpose()?;
            Ok(TypedConst {
                ty: ConstType::Int,
                value,
            })
        }
        Expr::Not(value) => {
            let value = eval_const_typed(value, consts, evaluate)?;
            require_const_type(value.ty, ConstType::Bool)?;
            let value = value
                .value
                .map(|value| as_bool(&value).map(|value| Value::Bool(!value)))
                .transpose()?;
            Ok(TypedConst {
                ty: ConstType::Bool,
                value,
            })
        }
        Expr::Binary { op, left, right } => {
            let left = eval_const_typed(left, consts, evaluate)?;
            let right = eval_const_typed(right, consts, evaluate)?;
            let result_type = match op.as_str() {
                "+" | "-" | "*" | "/" | "%" => {
                    require_const_type(left.ty, ConstType::Int)?;
                    require_const_type(right.ty, ConstType::Int)?;
                    ConstType::Int
                }
                "<" | "<=" | ">" | ">=" => {
                    require_const_type(left.ty, ConstType::Int)?;
                    require_const_type(right.ty, ConstType::Int)?;
                    ConstType::Bool
                }
                "and" | "or" | "=>" => {
                    require_const_type(left.ty, ConstType::Bool)?;
                    require_const_type(right.ty, ConstType::Bool)?;
                    ConstType::Bool
                }
                "==" | "!=" => {
                    require_const_type(left.ty, right.ty)?;
                    ConstType::Bool
                }
                _ => return Err(model_error(format!("unsupported constant operator '{op}'"))),
            };
            let value = left
                .value
                .zip(right.value)
                .map(|(left, right)| eval_const_binary(op, &left, &right))
                .transpose()?;
            Ok(TypedConst {
                ty: result_type,
                value,
            })
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => {
            let condition = eval_const_typed(condition, consts, evaluate)
                .map_err(|error| with_type_diagnostic_origin(error, spans.condition))?;
            require_const_type(condition.ty, ConstType::Bool)
                .map_err(|error| with_type_diagnostic_origin(error, spans.condition))?;
            let selected = condition.value.as_ref().map(as_bool).transpose()?;
            let then_expr = eval_const_typed(then_expr, consts, evaluate && selected == Some(true))
                .map_err(|error| with_type_diagnostic_origin(error, spans.then_expr))?;
            let else_expr =
                eval_const_typed(else_expr, consts, evaluate && selected == Some(false))
                    .map_err(|error| with_type_diagnostic_origin(error, spans.else_expr))?;
            require_const_type(else_expr.ty, then_expr.ty)
                .map_err(|error| with_type_diagnostic_origin(error, spans.else_expr))?;
            Ok(TypedConst {
                ty: then_expr.ty,
                value: if selected == Some(true) {
                    then_expr.value
                } else {
                    else_expr.value
                },
            })
        }
        Expr::BinaryNamed { name, left, right } if name == "min" || name == "max" => {
            let left = eval_const_typed(left, consts, evaluate)?;
            let right = eval_const_typed(right, consts, evaluate)?;
            require_const_type(left.ty, ConstType::Int)?;
            require_const_type(right.ty, ConstType::Int)?;
            let value = left
                .value
                .zip(right.value)
                .map(|(left, right)| {
                    let left = as_int(&left)?;
                    let right = as_int(&right)?;
                    Ok(Value::Int(if name == "min" {
                        left.min(right)
                    } else {
                        left.max(right)
                    }))
                })
                .transpose()?;
            Ok(TypedConst {
                ty: ConstType::Int,
                value,
            })
        }
        Expr::UnaryNamed { name, expr, .. } if name == "abs" => {
            let value = eval_const_typed(expr, consts, evaluate)?;
            require_const_type(value.ty, ConstType::Int)?;
            let value = value
                .value
                .map(|value| {
                    as_int(&value)?
                        .checked_abs()
                        .map(Value::Int)
                        .ok_or_else(|| model_error("integer overflow in abs"))
                })
                .transpose()?;
            Ok(TypedConst {
                ty: ConstType::Int,
                value,
            })
        }
        _ => Err(model_error("expression is not constant")),
    }
}

fn eval_const_binary(op: &str, left: &Value, right: &Value) -> Result<Value, ModelError> {
    match op {
        "+" => checked_int(left, right, i64::checked_add, "addition"),
        "-" => checked_int(left, right, i64::checked_sub, "subtraction"),
        "*" => checked_int(left, right, i64::checked_mul, "multiplication"),
        "/" => {
            let left = as_int(left)?;
            let right = as_int(right)?;
            if right == 0 {
                return Err(model_error("division by zero"));
            }
            left.checked_div_euclid(right)
                .map(Value::Int)
                .ok_or_else(|| model_error("integer overflow in division"))
        }
        "%" => {
            let left = as_int(left)?;
            let right = as_int(right)?;
            if right == 0 {
                return Err(model_error("remainder by zero"));
            }
            left.checked_rem_euclid(right)
                .map(Value::Int)
                .ok_or_else(|| model_error("integer overflow in remainder"))
        }
        "==" => Ok(Value::Bool(left == right)),
        "!=" => Ok(Value::Bool(left != right)),
        "<" | "<=" | ">" | ">=" => {
            let left = as_int(left)?;
            let right = as_int(right)?;
            Ok(Value::Bool(match op {
                "<" => left < right,
                "<=" => left <= right,
                ">" => left > right,
                ">=" => left >= right,
                _ => unreachable!(),
            }))
        }
        "and" => Ok(Value::Bool(as_bool(left)? && as_bool(right)?)),
        "or" => Ok(Value::Bool(as_bool(left)? || as_bool(right)?)),
        "=>" => Ok(Value::Bool(!as_bool(left)? || as_bool(right)?)),
        _ => Err(model_error(format!("unsupported constant operator '{op}'"))),
    }
}

fn checked_int(
    left: &Value,
    right: &Value,
    operation: fn(i64, i64) -> Option<i64>,
    label: &str,
) -> Result<Value, ModelError> {
    operation(as_int(left)?, as_int(right)?)
        .map(Value::Int)
        .ok_or_else(|| model_error(format!("integer overflow in {label}")))
}

fn checked_neg(value: i64) -> Result<i64, ModelError> {
    value
        .checked_neg()
        .ok_or_else(|| model_error("integer overflow in negation"))
}

fn as_int(value: &Value) -> Result<i64, ModelError> {
    match value {
        Value::Int(value) => Ok(*value),
        _ => Err(model_error("expected integer constant")),
    }
}

fn as_bool(value: &Value) -> Result<bool, ModelError> {
    match value {
        Value::Bool(value) => Ok(*value),
        _ => Err(model_error("expected Boolean constant")),
    }
}

/// Reject two properties that share a name (issue #1192).
///
/// Property names form one namespace across `invariant`, `trans`, `unless`,
/// `reachable`, `leadsTo`, and `until` (which also declares the
/// `<name>_until_safety` trans it lowers to). Results are keyed by that name —
/// the `leads_to`/`reachables` envelope maps, `--property` selection, origin
/// and annotation lookup, and Public Kernel `source_kind` — so a second
/// declaration silently collapses into the first: the induction engine
/// reported a false `leadsTo` as `proved`. The error points at the later
/// declaration and names the earlier one's location.
fn reject_duplicate_property_names(spec: &SurfaceSpec) -> Result<(), ModelError> {
    let mut seen: BTreeMap<String, (&'static str, Span)> = BTreeMap::new();
    for item in &spec.items {
        let (kind, span, names) = match item {
            SpecItem::Invariant { name, span, .. } => ("invariant", *span, vec![name.clone()]),
            SpecItem::Trans { name, span, .. } => ("trans", *span, vec![name.clone()]),
            SpecItem::Unless { name, span, .. } => ("unless", *span, vec![name.clone()]),
            SpecItem::Reachable { name, span, .. } => ("reachable", *span, vec![name.clone()]),
            SpecItem::LeadsTo { name, span, .. } => ("leadsTo", *span, vec![name.clone()]),
            SpecItem::Until { name, span, .. } => (
                "until",
                *span,
                vec![name.clone(), format!("{name}_until_safety")],
            ),
            _ => continue,
        };
        for name in names {
            if let Some((first_kind, first_span)) = seen.get(&name) {
                return Err(model_error(format!(
                    "duplicate property name '{}': {kind} reuses the name of the {first_kind} declared at {}:{}",
                    shown(&name),
                    first_span.start.line,
                    first_span.start.column
                ))
                .at(span));
            }
            seen.insert(name, (kind, span));
        }
    }
    Ok(())
}

/// A Kernel name as a diagnostic shows it: the public spelling every output
/// uses (`acct.bal` for the compose component state `acct__bal`).
fn shown(name: &str) -> String {
    crate::unmapped_public_spelling(name)
}

pub(crate) fn unknown_update_target(name: &str) -> ModelError {
    model_error(format!("unknown update target '{}'", shown(name)))
}

fn model_error(message: impl Into<String>) -> ModelError {
    ModelError {
        message: message.into(),
        origin: None,
        span: None,
        name_resolution: false,
        diagnostic_code: None,
        hint: None,
        quick_fix: None,
    }
}

fn reject_map_int_key(name: &str, ty: &TypeExpr, span: Span) -> Result<(), ModelError> {
    if contains_map_int_key(ty) {
        return Err(model_error(format!(
            "Map<Int, ...> on '{}' is rejected; use a bounded domain type as key; declare `type K = 0..<max>` and use `Map<K, ...>`",
            shown(name)
        ))
        .at(span));
    }
    Ok(())
}

fn contains_map_int_key(ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Int | TypeExpr::Bool | TypeExpr::Range(_, _) | TypeExpr::Name(_) => false,
        TypeExpr::Map(key, value) => {
            matches!(key.as_ref(), TypeExpr::Int)
                || contains_map_int_key(key)
                || contains_map_int_key(value)
        }
        TypeExpr::Relation(source, target) => {
            contains_map_int_key(source) || contains_map_int_key(target)
        }
        TypeExpr::Set(inner) | TypeExpr::Seq(inner, _) | TypeExpr::Option(inner) => {
            contains_map_int_key(inner)
        }
    }
}

#[cfg(test)]
mod action_source_span_tests {
    use super::*;
    use crate::{FsResolver, build_model, parse_kernel_source};

    fn model() -> KernelModel {
        build_model(
            parse_kernel_source(
                "spec Source { state { x: Bool } init { x = false } action step() { x = true } }",
                &FsResolver::new("."),
            )
            .expect("parse kernel"),
        )
        .expect("build model")
    }

    fn span(line: u32, column: u32) -> Span {
        let position = SourcePos {
            offset: 0,
            line,
            column,
        };
        Span {
            start: position,
            end: position,
        }
    }

    fn set_origin(model: &mut KernelModel, primary: Option<Span>) {
        let mut origin = OriginChain::generated_only("test-action", "test");
        origin.primary = primary.map(|span| OriginSite {
            source_file: None,
            span: Some(span),
            dialect: "test".to_owned(),
            declaration_path: vec!["step".to_owned()],
        });
        model.origins = OriginRegistry::default();
        model
            .origins
            .bind(action_target(&model.actions[0].name), origin);
    }

    #[test]
    fn authored_origin_wins_over_kernel_and_zero_spans() {
        let mut model = model();
        let authored = span(7, 3);
        set_origin(&mut model, Some(authored));
        assert_eq!(model.action_source_span(&model.actions[0]), Some(authored));
        model.actions[0].span = span(0, 0);
        assert_eq!(model.action_source_span(&model.actions[0]), Some(authored));
    }

    #[test]
    fn generated_only_sentinel_has_no_source_span() {
        let mut model = model();
        set_origin(&mut model, None);
        for position in [span(0, 0), span(0, 3), span(7, 0)] {
            model.actions[0].span = position;
            assert_eq!(model.action_source_span(&model.actions[0]), None);
        }
    }

    #[test]
    fn kernel_declaration_is_used_without_a_positive_origin() {
        let mut model = model();
        let declaration = model.actions[0].span;
        assert_eq!(
            model.action_source_span(&model.actions[0]),
            Some(declaration)
        );
        for primary in [None, Some(span(0, 0)), Some(span(0, 3)), Some(span(7, 0))] {
            set_origin(&mut model, primary);
            assert_eq!(
                model.action_source_span(&model.actions[0]),
                Some(declaration)
            );
        }
    }
}

#[cfg(test)]
mod compose_name_tests {
    use crate::{ComposeName, CoreError, FileResolver, build_model, parse_kernel_source};

    struct Components;

    impl FileResolver for Components {
        fn read(&self, path: &str) -> Result<String, CoreError> {
            let name = path.trim_end_matches(".fsl");
            Ok(format!(
                "spec {name} {{ state {{ n: 0..1 }} init {{ n = 0 }} action go() {{ requires n < 1  n = 1 }} }}"
            ))
        }
    }

    fn model() -> super::KernelModel {
        let source = "compose Pair { use Left as left from \"Left.fsl\"  use Right as right from \"Right.fsl\" }";
        build_model(parse_kernel_source(source, &Components).expect("lower compose"))
            .expect("build compose model")
    }

    fn resolved(model: &super::KernelModel, spelling: &str) -> Option<String> {
        model
            .resolve_replay_action(spelling)
            .expect("unambiguous spelling")
            .map(|action| action.name.clone())
    }

    #[test]
    fn replay_spellings_resolve_through_the_structural_table() {
        let model = model();
        assert_eq!(resolved(&model, "left.go").as_deref(), Some("left__go"));
        assert_eq!(resolved(&model, "left__go").as_deref(), Some("left__go"));
        assert_eq!(resolved(&model, "right.go").as_deref(), Some("right__go"));
        assert_eq!(resolved(&model, "go"), None);
        assert_eq!(resolved(&model, "left.nosuch"), None);
        assert_eq!(model.action_display_name("left__go"), "left.go");
    }

    #[test]
    fn a_spelling_matching_two_actions_fails_closed() {
        let mut model = model();
        // A checked model cannot hold this table (the internal names would be
        // duplicate actions); inject it to pin the fail-closed branch.
        model.names.insert_action_identity(
            "right__go",
            ComposeName {
                alias: "left".to_owned(),
                name: "go".to_owned(),
            },
        );
        let error = model
            .resolve_replay_action("left.go")
            .expect_err("ambiguous spelling must not pick an action");
        assert_eq!(
            error,
            "action 'left.go' is ambiguous: it matches 'left.go' (internal 'left__go'), 'left.go' (internal 'right__go')"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsl_syntax::{AggregateKind, ConditionalSpans, Pattern, QualifiedName};

    #[test]
    fn diagnostic_only_component_targets_do_not_change_public_provenance() {
        let mut kernel = crate::parse_direct_kernel_spec(
            "spec P { state { done: Bool } init { done = false } action go() {} }",
        )
        .expect("parse");
        let model = build_model(kernel.clone()).expect("model");
        let before = crate::public_kernel_contract_for_version(
            &kernel,
            &model,
            "root.fsl",
            "kernel",
            crate::PublicKernelVersion::V2,
        )
        .expect("v2");
        for target in [
            "property:invariant:part__Capture".to_owned(),
            crate::action_guard_target("go", 0),
            crate::action_statement_target("go", 0),
            crate::origin::action_ensure_target("go", 0),
        ] {
            kernel.origins.bind(
                crate::origin::component_check_target(&target),
                OriginChain::generated_only("diagnostic-context", "compose"),
            );
        }
        let model = build_model(kernel.clone()).expect("checked model");
        assert!(
            model
                .origins
                .targets()
                .all(|(target, _)| !crate::origin::is_component_check_target(target))
        );
        let after = crate::public_kernel_contract_for_version(
            &kernel,
            &model,
            "root.fsl",
            "kernel",
            crate::PublicKernelVersion::V2,
        )
        .expect("v2");
        assert_eq!(
            before, after,
            "diagnostic ownership must not promote public provenance assurance"
        );
    }

    fn var(name: &str) -> Expr {
        Expr::Var(name.to_owned())
    }

    fn collection(name: &str, source: Expr, filter: Option<Expr>) -> Binder {
        Binder::Collection {
            name: name.to_owned(),
            collection: Box::new(source),
            where_expr: filter.map(Box::new),
        }
    }

    fn references(expr: &Expr) -> Vec<String> {
        let mut found = Vec::new();
        collect_state_references(expr, &BTreeSet::from(["a".into(), "z".into()]), &mut found);
        found
    }

    fn model() -> KernelModel {
        let resolver = crate::FsResolver::new(".");
        build_model(crate::parse_kernel_source(
            "spec References { type K = 0..1 state { z: K, a: K } init { z = 0 a = 0 } action go() {} }",
            &resolver,
        ).expect("parse")).expect("model")
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Explicit inventory of all 24 AST variants and their edges.
    fn state_reference_edges_cover_every_expression_variant() {
        // These are AST traversal probes, not necessarily well-typed evaluation inputs.
        let z = || Box::new(var("z"));
        let a = || Box::new(var("a"));
        let span = synthetic_span();
        let cases = [
            (Expr::Num(1), vec![]),
            (Expr::Bool(true), vec![]),
            (Expr::None, vec![]),
            (Expr::Some(z()), vec!["z"]),
            (Expr::Set(vec![var("z"), var("a")]), vec!["z", "a"]),
            (Expr::Seq(vec![var("z"), var("a")]), vec!["z", "a"]),
            (
                Expr::Struct {
                    name: "R".into(),
                    fields: vec![("x".into(), var("z")), ("y".into(), var("a"))],
                },
                vec!["z", "a"],
            ),
            (var("z"), vec!["z"]),
            (
                Expr::EnumMember {
                    type_name: "E".into(),
                    member: "One".into(),
                    source_bare: false,
                },
                vec![],
            ),
            (
                Expr::Call {
                    name: "p".into(),
                    args: vec![var("z"), var("a")],
                    span,
                },
                vec!["z", "a"],
            ),
            (Expr::Index(z(), a()), vec!["z", "a"]),
            (Expr::Field(z(), "x".into()), vec!["z"]),
            (
                Expr::Method {
                    receiver: z(),
                    name: "contains".into(),
                    args: vec![var("a")],
                },
                vec!["z", "a"],
            ),
            (
                Expr::Binary {
                    op: "and".into(),
                    left: z(),
                    right: a(),
                },
                vec!["z", "a"],
            ),
            (Expr::Neg(z()), vec!["z"]),
            (Expr::Not(z()), vec!["z"]),
            (
                Expr::Conditional {
                    condition: z(),
                    then_expr: a(),
                    else_expr: z(),
                    spans: Box::new(ConditionalSpans {
                        condition: span,
                        then_expr: span,
                        else_expr: span,
                    }),
                },
                vec!["z", "a", "z"],
            ),
            (
                Expr::Is {
                    expr: z(),
                    pattern: Pattern::Some("fresh".into()),
                },
                vec!["z"],
            ),
            (
                Expr::Quantified {
                    quantifier: "forall".into(),
                    binder: collection("z", Expr::Seq(vec![var("z"), var("a")]), Some(var("z"))),
                    body: z(),
                },
                vec!["z", "a"],
            ),
            (
                Expr::Aggregate {
                    kind: AggregateKind::Sum,
                    binder: collection("z", Expr::Seq(vec![var("z"), var("a")]), Some(var("z"))),
                    value: Some(a()),
                },
                vec!["z", "a", "a"],
            ),
            (
                Expr::Stage {
                    process: None,
                    entity: z(),
                    entity_span: span,
                    span,
                },
                vec!["z"],
            ),
            (
                Expr::UnaryNamed {
                    name: "old".into(),
                    expr: z(),
                    span,
                },
                vec!["z"],
            ),
            (
                Expr::BinaryNamed {
                    name: "min".into(),
                    left: z(),
                    right: a(),
                },
                vec!["z", "a"],
            ),
            (
                Expr::TernaryNamed {
                    name: "rel_reachable".into(),
                    first: z(),
                    second: a(),
                    third: z(),
                },
                vec!["z", "a", "z"],
            ),
        ];
        assert_eq!(cases.len(), 24);
        for (expr, expected) in cases {
            assert_eq!(references(&expr), expected, "{expr:?}");
            assert_eq!(
                first_state_reference(&expr, &BTreeSet::from(["a".into(), "z".into()])).as_deref(),
                expected.first().copied()
            );
        }
    }

    #[test]
    fn state_reference_scope_covers_typed_range_and_all_aggregates() {
        let binders = [
            Binder::Typed {
                name: "z".into(),
                type_name: QualifiedName {
                    namespace: None,
                    name: "K".into(),
                },
                where_expr: Some(Box::new(var("z"))),
            },
            Binder::Range {
                name: "z".into(),
                lo: Box::new(var("z")),
                hi: Box::new(var("a")),
                where_expr: Some(Box::new(var("z"))),
            },
            collection("z", Expr::Seq(vec![var("z"), var("a")]), Some(var("z"))),
        ];
        for (index, binder) in binders.into_iter().enumerate() {
            for kind in [
                AggregateKind::Count,
                AggregateKind::Sum,
                AggregateKind::Unique,
                AggregateKind::ExactlyOne,
            ] {
                let expr = Expr::Aggregate {
                    kind,
                    binder: binder.clone(),
                    value: (kind == AggregateKind::Sum).then(|| Box::new(var("z"))),
                };
                assert_eq!(
                    references(&expr),
                    if index == 0 { vec![] } else { vec!["z", "a"] }
                );
            }
        }
    }

    #[test]
    fn state_reference_scope_restores_names_after_nested_shadowing() {
        let inner = Expr::Quantified {
            quantifier: "exists".into(),
            binder: collection("z", Expr::Seq(vec![var("z")]), Some(var("z"))),
            body: Box::new(var("z")),
        };
        let outer = Expr::Quantified {
            quantifier: "forall".into(),
            binder: collection("z", Expr::Seq(vec![var("z")]), Some(var("z"))),
            body: Box::new(Expr::Binary {
                op: "and".into(),
                left: Box::new(inner),
                right: Box::new(var("a")),
            }),
        };
        let expr = Expr::Seq(vec![outer, var("z"), var("a")]);
        assert_eq!(references(&expr), ["z", "a", "z", "a"]);
        let model = model();
        assert_eq!(
            state_references(&expr, &model, [])
                .into_iter()
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert_eq!(
            state_references(&expr, &model, ["z"])
                .into_iter()
                .collect::<Vec<_>>(),
            ["a"]
        );
        assert!(state_references(&expr, &model, ["a", "z"]).is_empty());
    }

    #[test]
    fn init_collection_guard_distinguishes_local_and_free_state_names() {
        let model = model();
        let binder = collection("item", Expr::Seq(vec![var("z"), var("a")]), None);
        assert!(
            check_init_collection_binder(&model, &binder, [])
                .unwrap_err()
                .message
                .contains("'a'")
        );
        assert!(
            check_init_collection_binder(&model, &binder, ["a"])
                .unwrap_err()
                .message
                .contains("'z'")
        );
        assert!(check_init_collection_binder(&model, &binder, ["a", "z"]).is_ok());
    }

    #[test]
    fn unchecked_leadsto_models_share_the_source_support_rule() {
        let model = model();
        let binders = [
            collection("item", Expr::Seq(vec![Expr::Num(1)]), None),
            Binder::Range {
                name: "item".into(),
                lo: Box::new(Expr::Num(0)),
                hi: Box::new(Expr::Num(1)),
                where_expr: Some(Box::new(Expr::Bool(false))),
            },
        ];
        for binder in binders {
            let property = LeadsToDef {
                name: "Capture".into(),
                span: synthetic_span(),
                binders: vec![binder.clone()],
                before: Expr::Bool(true),
                after: Expr::Bool(true),
                meta: None,
                annotations: Annotations::default(),
                decreases: None,
                within: Some(1),
                helpful: vec![],
            };
            assert_eq!(
                static_leadsto_bindings(&model, &property).unwrap_err(),
                check_leadsto_binder(&binder).unwrap_err()
            );
        }
    }

    #[test]
    fn shared_index_types_preserve_map_and_sequence_capabilities() {
        let map = TypeRef::Map(Box::new(TypeRef::Bool), Box::new(TypeRef::Int));
        let relation = TypeRef::Relation(Box::new(TypeRef::Bool), Box::new(TypeRef::Int));
        let seq = TypeRef::Seq(Box::new(TypeRef::Bool), 2);
        for allow_sequence in [false, true] {
            assert_eq!(
                map.index_types(allow_sequence),
                Some((TypeRef::Bool, TypeRef::Int))
            );
            assert!(relation.index_types(allow_sequence).is_none());
        }
        assert_eq!(seq.index_types(true), Some((TypeRef::Int, TypeRef::Bool)));
        assert!(seq.index_types(false).is_none());
        let mut model = model();
        model.state.push(("h".into(), relation));
        assert!(
            model
                .state_lvalue_type(&LValue::Index("h".into(), Expr::Bool(false)))
                .is_err()
        );
    }
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fsl_syntax::{
    ActionItem, Annotation, AnnotationRegistry, Annotations, Binder, Expr, LValue, MetaTag, Param,
    RequirementLink, SourcePos, Span, SpecItem, Statement, SurfaceSpec, TypeExpr,
};

use crate::{
    INIT_TARGET, KernelSpec, LoweringStep, OriginChain, OriginId, OriginRegistry, OriginSite,
    ProjectionDef, SPEC_TARGET, TraceabilityRegistry, action_target, property_target, state_target,
    type_target,
};

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
    pub projections: Vec<ProjectionDef>,
    origins: OriginRegistry,
    annotations: AnnotationRegistry,
    traceability: TraceabilityRegistry,
}

impl KernelModel {
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
                _ => Err(model_error(format!("type '{name}' is not a finite scalar"))),
            },
            _ => Err(model_error("type is not a finite scalar")),
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
                    .ok_or_else(|| model_error(format!("enum '{name}' has no members"))),
                Some(TypeDef::Struct { fields }) => Ok(Value::Struct {
                    type_name: name.clone(),
                    fields: fields
                        .iter()
                        .map(|(field, ty)| Ok((field.clone(), self.default_value(ty)?)))
                        .collect::<Result<_, ModelError>>()?,
                }),
                None => Err(model_error(format!("unknown type '{name}'"))),
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

    #[must_use]
    pub fn annotations_for(&self, target: &str) -> &Annotations {
        self.annotations.annotations_for(target)
    }

    #[must_use]
    pub fn property_origin(&self, kind: &str, name: &str) -> Option<&crate::OriginChain> {
        self.origins.primary_for(&property_target(kind, name))
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
        if match binder {
            Binder::Typed { where_expr, .. }
            | Binder::Range { where_expr, .. }
            | Binder::Collection { where_expr, .. } => where_expr.is_some(),
        } {
            return Err(model_error(
                "where filters are not implemented in bounded leadsTo",
            ));
        }
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
            Binder::Collection { .. } => {
                return Err(model_error(
                    "collection binders are not implemented in bounded leadsTo",
                ));
            }
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

fn static_leadsto_int(expr: &Expr, model: &KernelModel) -> Result<i64, ModelError> {
    match expr {
        Expr::Num(value) => Ok(*value),
        Expr::Var(name) => match model.consts.get(name) {
            Some(Value::Int(value)) => Ok(*value),
            _ => Err(model_error(format!("'{name}' is not an integer constant"))),
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
}

impl ModelError {
    fn with_origin(mut self, origin: Option<crate::OriginChain>) -> Self {
        self.origin = origin.map(Box::new);
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
        } = kernel;
        Self {
            spec,
            origins,
            annotations,
            projections,
            consts: BTreeMap::new(),
            types: BTreeMap::new(),
            enum_members: BTreeMap::new(),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn build(mut self) -> Result<KernelModel, ModelError> {
        self.collect_consts()?;
        self.collect_types()?;
        self.collect_declaration_annotations();
        self.annotations.validate().map_err(|error| ModelError {
            message: error.message,
            origin: Some(Box::new(source_origin("annotation", error.span, None))),
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
        let mut invariants = Vec::new();
        let mut transitions = Vec::new();
        let mut reachables = Vec::new();
        let mut leadstos = Vec::new();
        let mut terminal = None;
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
                                field.name
                            ))
                            .with_origin(
                                self.origins.diagnostic_origin(&state_target(&field.name)),
                            ));
                        }
                        if let Some(initializer) = &field.initializer {
                            if let Some(form) = unsupported_inline_form(initializer) {
                                return Err(model_error(format!(
                                    "inline initializer for '{}' does not allow {form}; use init",
                                    field.name
                                ))
                                .with_origin(Some(source_origin(
                                    &field.name,
                                    field.initializer_span.unwrap_or(field.span),
                                    None,
                                ))));
                            }
                            if let Some(name) = first_state_reference(initializer, &state_names) {
                                return Err(model_error(format!(
                                    "inline initializer for '{}' must not read state root '{name}'",
                                    field.name
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
                                    field.name
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
                        let origin = self.origins.diagnostic_origin(&state_target(&field.name));
                        state.push((
                            field.name.clone(),
                            self.resolve_type(&field.ty)
                                .map_err(|error| error.with_origin(origin))?,
                        ));
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
                    ..
                } => {
                    let origin = self.origins.diagnostic_origin(&action_target(name));
                    actions.push(
                        self.action(name, params, items, *span, *fair, meta.clone())
                            .map_err(|error| error.with_origin(origin))?,
                    );
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
                SpecItem::Terminal { expr, .. } => terminal = Some(expr.as_ref().clone()),
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
        let model = KernelModel {
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
            projections: self.projections,
            origins: self.origins,
            annotations: self.annotations,
            traceability,
        };
        let inline_initializers = inline_initializers
            .into_iter()
            .map(|(index, name, span)| (index, (name, span)))
            .collect::<BTreeMap<_, _>>();
        for (index, statement) in model.init.iter().enumerate() {
            crate::public_kernel::validate_statement_types(statement, &model).map_err(|error| {
                let origin = error.span.map(type_diagnostic_origin);
                if let Some((name, _)) = inline_initializers.get(&index) {
                    model_error(format!(
                        "invalid inline initializer for '{name}': {}",
                        error.message
                    ))
                    .with_origin(origin)
                } else {
                    model_error(format!("invalid init statement: {}", error.message))
                        .with_origin(origin)
                }
            })?;
        }
        crate::public_kernel::validate_model_expression_types(&model).map_err(|error| {
            let origin = error.span.map(type_diagnostic_origin);
            model_error(format!("invalid model expression: {}", error.message)).with_origin(origin)
        })?;
        for projection in &model.projections {
            crate::public_kernel::validate_expression_type(
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
        Ok(model)
    }

    fn collect_consts(&mut self) -> Result<(), ModelError> {
        for item in &self.spec.items {
            if let SpecItem::Const { name, value } = item {
                if self.consts.contains_key(name) {
                    return Err(model_error(format!("duplicate const '{name}'")));
                }
                let value = eval_const(value, &self.consts)?;
                self.consts.insert(name.clone(), value);
            }
        }
        Ok(())
    }

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
                    symmetric,
                } => {
                    let origin = self.origins.diagnostic_origin(&type_target(name));
                    self.insert_type(
                        name,
                        TypeDef::Enum {
                            members: members.clone(),
                            symmetric: *symmetric,
                        },
                    )
                    .map_err(|error| error.with_origin(origin.clone()))?;
                    for member in members {
                        if self.enum_members.contains_key(member) {
                            return Err(model_error(format!("duplicate enum member '{member}'"))
                                .with_origin(origin));
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
            if let SpecItem::Struct { name, fields } = item {
                let origin = self.origins.diagnostic_origin(&type_target(name));
                let resolved = fields
                    .iter()
                    .map(|(field, ty)| Ok((field.clone(), self.resolve_type(ty)?)))
                    .collect::<Result<Vec<_>, ModelError>>()
                    .map_err(|error| error.with_origin(origin.clone()))?;
                for (field, ty) in &resolved {
                    if !self.is_scalar_struct_field(ty) {
                        return Err(model_error(format!(
                            "struct field '{name}.{field}' has non-scalar type"
                        ))
                        .with_origin(origin));
                    }
                }
                self.insert_type(name, TypeDef::Struct { fields: resolved })
                    .map_err(|error| error.with_origin(origin))?;
            }
        }
        Ok(())
    }

    fn is_scalar_struct_field(&self, ty: &TypeRef) -> bool {
        match ty {
            TypeRef::Int | TypeRef::Bool | TypeRef::Range(_, _) => true,
            TypeRef::Named(name) => matches!(
                self.types.get(name),
                Some(TypeDef::Domain { .. } | TypeDef::Enum { .. })
            ),
            TypeRef::Option(inner) => self.is_scalar_struct_field(inner),
            TypeRef::Map(_, _) | TypeRef::Relation(_, _) | TypeRef::Set(_) | TypeRef::Seq(_, _) => {
                false
            }
        }
    }

    fn insert_type(&mut self, name: &str, definition: TypeDef) -> Result<(), ModelError> {
        if self.types.insert(name.to_owned(), definition).is_some() {
            Err(model_error(format!("duplicate type '{name}'")))
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
        let params = params
            .iter()
            .map(|param| match param {
                Param::Typed(name, qualified) => {
                    if qualified.namespace.is_some() {
                        return Err(model_error("qualified type remained after kernel lowering"));
                    }
                    let ty = TypeExpr::Name(qualified.name.clone());
                    Ok(ParamDef::Typed {
                        name: name.clone(),
                        ty: self.resolve_type(&ty)?,
                    })
                }
                Param::Range(name, lo, hi) => Ok(ParamDef::Range {
                    name: name.clone(),
                    lo: self.const_int(lo)?,
                    hi: self.const_int(hi)?,
                }),
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
        if duplicate_statement_write(&statements, &index_constants).is_some() {
            return Err(model_error(
                "an action may not assign the same state location more than once",
            )
            .with_origin(self.origins.diagnostic_origin(&action_target(name))));
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
            meta,
            annotations: self
                .annotations
                .annotations_for(&action_target(name))
                .clone(),
        })
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
                return Err(model_error(format!("unknown type '{name}'")));
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

fn duplicate_statement_write(
    statements: &[Statement],
    constants: &BTreeMap<String, Value>,
) -> Option<LValue> {
    fn writes(
        statements: &[Statement],
        constants: &BTreeMap<String, Value>,
    ) -> Result<Vec<LValue>, Box<LValue>> {
        let mut seen = Vec::new();
        for statement in statements {
            let candidates = match statement {
                Statement::Assign { target, .. } => vec![target.clone()],
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
                    binder, statements, ..
                } => {
                    let repeated = writes(statements, constants)?;
                    if let Some(target) = repeated
                        .iter()
                        .find(|target| !write_is_injective_for_binder(target, binder))
                    {
                        return Err(Box::new(target.clone()));
                    }
                    repeated
                }
            };
            if let Some(target) = candidates.iter().find(|target| {
                seen.iter()
                    .any(|previous| lvalues_may_alias(previous, target, constants))
            }) {
                return Err(Box::new(target.clone()));
            }
            seen.extend(candidates);
        }
        Ok(seen)
    }
    writes(statements, constants).err().map(|target| *target)
}

fn write_is_injective_for_binder(target: &LValue, binder: &Binder) -> bool {
    let name = match binder {
        Binder::Typed { name, .. } | Binder::Range { name, .. } => name,
        Binder::Collection { .. } => return false,
    };
    let (_, index, _) = lvalue_path(target);
    matches!(index, Some(Expr::Var(index)) if index == name)
}

fn lvalues_may_alias(left: &LValue, right: &LValue, constants: &BTreeMap<String, Value>) -> bool {
    let (left_root, left_index, left_fields) = lvalue_path(left);
    let (right_root, right_index, right_fields) = lvalue_path(right);
    if left_root != right_root {
        return false;
    }
    if let (Some(left), Some(right)) = (left_index, right_index)
        && let (Ok(left), Ok(right)) = (eval_const(left, constants), eval_const(right, constants))
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

fn type_diagnostic_origin(span: Span) -> OriginChain {
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
    if let Expr::Var(name) = expr
        && state_names.contains(name)
    {
        return Some(name.clone());
    }
    expr_children(expr)
        .into_iter()
        .find_map(|child| first_state_reference(child, state_names))
}

fn unsupported_inline_form(expr: &Expr) -> Option<&'static str> {
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
        Expr::Num(_) | Expr::Bool(_) | Expr::None | Expr::Var(_) => Vec::new(),
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
            None => Err(model_error(format!("unknown constant '{name}'"))),
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
            Ok(Value::Int(left.div_euclid(right)))
        }
        "%" => {
            let left = as_int(left)?;
            let right = as_int(right)?;
            if right == 0 {
                return Err(model_error("remainder by zero"));
            }
            Ok(Value::Int(left.rem_euclid(right)))
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

fn model_error(message: impl Into<String>) -> ModelError {
    ModelError {
        message: message.into(),
        origin: None,
    }
}

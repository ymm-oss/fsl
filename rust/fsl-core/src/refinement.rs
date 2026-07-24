// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fsl_syntax::{
    ActionTarget, Binder, ConditionalSpans, CorrespondenceOrigin, Expr, MetaTag, QualifiedName,
    RefinementItem, RefinementParam, RequirementAction, RequirementActionItem,
    RequirementBlockItem, RequirementsItem, Span, SurfaceDocument, SurfaceRefinement,
};

use crate::{
    ActionDef, FileResolver, KernelModel, ParamDef, TypeDef, TypeRef, build_model,
    parse_kernel_source,
};

#[derive(Clone, Debug)]
struct EnumMapping {
    source: String,
    target: String,
    members: Vec<(String, String)>,
    span: Span,
    assurance: EnumMappingAssurance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EnumMappingAssurance {
    Bijection,
    SourceTotal,
}

impl EnumMappingAssurance {
    const fn declaration_name(self) -> &'static str {
        match self {
            Self::Bijection => "enum conversion",
            Self::SourceTotal => "enum abstraction",
        }
    }

    const fn call_name(self) -> &'static str {
        match self {
            Self::Bijection => "convert",
            Self::SourceTotal => "abstract",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateMap {
    pub name: String,
    pub binder: Option<Binder>,
    pub expr: Expr,
    pub span: Option<Span>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionRef(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionCorrespondenceTarget {
    Stutter,
    Action { action: ActionRef, args: Vec<Expr> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionCorrespondence {
    pub impl_action: ActionRef,
    pub impl_params: Vec<ParamDef>,
    pub target: ActionCorrespondenceTarget,
    pub origin: CorrespondenceOrigin,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressMap {
    pub leads_to: String,
    pub actions: Vec<ActionRef>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refinement {
    pub name: String,
    pub impl_name: String,
    pub abs_name: String,
    pub state_maps: BTreeMap<String, StateMap>,
    pub action_correspondences: BTreeMap<String, ActionCorrespondence>,
    pub progress: Vec<ProgressMap>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementsContract {
    pub abstraction: KernelModel,
    pub refinement: Refinement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefinementError {
    pub message: String,
    pub span: Option<Span>,
}

impl fmt::Display for RefinementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RefinementError {}

/// Parse and validate a refinement mapping against its implementation and
/// abstract kernel models.
///
/// # Errors
///
/// Returns [`RefinementError`] for syntax errors, mismatched model names,
/// duplicate/missing maps, or unknown action correspondences.
pub fn parse_refinement(
    source: &str,
    implementation: &KernelModel,
    abstraction: &KernelModel,
) -> Result<Refinement, RefinementError> {
    let document = fsl_syntax::parse_surface_document(source).map_err(|error| RefinementError {
        message: error.message,
        span: Some(error.span),
    })?;
    let SurfaceDocument::Refinement(surface) = document else {
        return Err(refinement_error("expected refinement document", None));
    };
    build_refinement(surface, implementation, abstraction, None)
}

#[allow(clippy::too_many_lines)]
fn build_refinement(
    surface: SurfaceRefinement,
    implementation: &KernelModel,
    abstraction: &KernelModel,
    implicit_actions: Option<Span>,
) -> Result<Refinement, RefinementError> {
    let mut impl_name = None;
    let mut abs_name = None;
    let mut maps_auto = None;
    let mut state_maps = BTreeMap::new();
    let mut enum_mapping_items = Vec::new();
    let mut action_correspondences = BTreeMap::new();
    let mut action_sources = Vec::new();
    let mut progress = Vec::new();
    let type_context = refinement_type_context(implementation, abstraction);
    for item in surface.items {
        match item {
            RefinementItem::Impl(name) => impl_name = Some(name),
            RefinementItem::Abs(name) => abs_name = Some(name),
            RefinementItem::MapsAuto(span) => maps_auto = Some(span),
            RefinementItem::EnumConversion {
                name,
                source,
                target,
                members,
                span,
            } => enum_mapping_items.push((
                EnumMappingAssurance::Bijection,
                name,
                source,
                target,
                members,
                span,
            )),
            RefinementItem::EnumAbstraction {
                name,
                source,
                target,
                members,
                span,
            } => enum_mapping_items.push((
                EnumMappingAssurance::SourceTotal,
                name,
                source,
                target,
                members,
                span,
            )),
            RefinementItem::Map {
                name,
                binder,
                expr,
                span,
            } => {
                if abstraction.state_type(&name).is_none() {
                    return Err(refinement_error(
                        format!("unknown abstract state variable '{name}'"),
                        Some(span),
                    ));
                }
                if state_maps
                    .insert(
                        name.clone(),
                        StateMap {
                            name: name.clone(),
                            binder,
                            expr: *expr,
                            span: Some(span),
                        },
                    )
                    .is_some()
                {
                    return Err(refinement_error(
                        format!("duplicate map for '{name}'"),
                        Some(span),
                    ));
                }
            }
            RefinementItem::Action {
                name,
                params,
                target,
                origin,
                span,
            } => action_sources.push(ActionCorrespondenceSource {
                impl_action: name,
                impl_params: params,
                target,
                origin,
                span,
            }),
            RefinementItem::PreserveProgress { responses, .. } => {
                progress.extend(responses.into_iter().map(|(leads_to, actions, span)| {
                    ProgressMap {
                        leads_to,
                        actions: actions.into_iter().map(ActionRef).collect(),
                        span,
                    }
                }));
            }
        }
    }
    let impl_name =
        impl_name.ok_or_else(|| refinement_error("refinement missing impl spec name", None))?;
    let abs_name =
        abs_name.ok_or_else(|| refinement_error("refinement missing abs spec name", None))?;
    if impl_name != implementation.name {
        return Err(refinement_error(
            format!(
                "impl name '{impl_name}' does not match impl spec '{}'",
                implementation.name
            ),
            None,
        ));
    }
    if abs_name != abstraction.name {
        return Err(refinement_error(
            format!(
                "abs name '{abs_name}' does not match abs spec '{}'",
                abstraction.name
            ),
            None,
        ));
    }
    let enum_mappings = build_enum_mappings(enum_mapping_items, &type_context)?;
    for state_map in state_maps.values_mut() {
        state_map.expr = elaborate_enum_conversions(state_map.expr.clone(), &enum_mappings)?;
    }
    for source in &mut action_sources {
        if let ActionTarget::Action(_, args) = &mut source.target {
            for argument in args {
                *argument = elaborate_enum_conversions(argument.clone(), &enum_mappings)?;
            }
        }
    }
    validate_correspondence_duplicates(&action_sources)?;
    for source in action_sources {
        insert_action_correspondence(
            &mut action_correspondences,
            source,
            implementation,
            abstraction,
            &type_context,
        )?;
    }
    if let Some(auto_span) = maps_auto {
        apply_auto_state_maps(implementation, abstraction, &mut state_maps)?;
        apply_auto_action_correspondences(
            implementation,
            abstraction,
            &mut action_correspondences,
            &type_context,
            auto_span,
            false,
        )?;
    }
    if let Some(span) = implicit_actions {
        apply_auto_action_correspondences(
            implementation,
            abstraction,
            &mut action_correspondences,
            &type_context,
            span,
            true,
        )?;
    }
    validate_refinement_expressions(abstraction, &state_maps, &type_context)?;
    for (name, _) in &abstraction.state {
        if !state_maps.contains_key(name) {
            return Err(refinement_error(
                format!("missing map for abstract state variable '{name}'"),
                None,
            ));
        }
    }
    for action in &implementation.actions {
        if !action_correspondences.contains_key(&action.name) {
            return Err(refinement_error(
                format!(
                    "missing action correspondence for impl action '{}'",
                    action.name
                ),
                None,
            ));
        }
    }
    for declaration in &progress {
        if !abstraction
            .leadstos
            .iter()
            .any(|property| property.name == declaration.leads_to)
        {
            return Err(refinement_error(
                format!("unknown abstract leadsTo '{}'", declaration.leads_to),
                Some(declaration.span),
            ));
        }
        for action in &declaration.actions {
            if !action_correspondences.contains_key(&action.0) {
                return Err(refinement_error(
                    format!("unknown impl progress action '{}'", action.0),
                    Some(declaration.span),
                ));
            }
        }
    }
    Ok(Refinement {
        name: surface.name,
        impl_name,
        abs_name,
        state_maps,
        action_correspondences,
        progress,
    })
}

type SurfaceEnumMapping = (
    EnumMappingAssurance,
    String,
    String,
    String,
    Vec<(String, String, Span)>,
    Span,
);

fn build_enum_mappings(
    items: Vec<SurfaceEnumMapping>,
    context: &KernelModel,
) -> Result<BTreeMap<String, EnumMapping>, RefinementError> {
    let mut mappings: BTreeMap<String, EnumMapping> = BTreeMap::new();
    for (assurance, name, source, target, rows, span) in items {
        let declaration = assurance.declaration_name();
        let source_members = enum_type_members(context, declaration, &source, span)?;
        let target_members = enum_type_members(context, declaration, &target, span)?;
        let mut seen_source = BTreeSet::new();
        let mut seen_target = BTreeSet::new();
        let mut members = Vec::new();
        for (source_member, target_member, row_span) in rows {
            if !source_members.contains(&source_member) {
                return Err(refinement_error(
                    format!("unknown enum member '{source}.{source_member}'"),
                    Some(row_span),
                ));
            }
            if !target_members.contains(&target_member) {
                return Err(refinement_error(
                    format!("unknown enum member '{target}.{target_member}'"),
                    Some(row_span),
                ));
            }
            if !seen_source.insert(source_member.clone()) {
                return Err(refinement_error(
                    format!(
                        "{declaration} '{name}' maps source member '{source_member}' more than once"
                    ),
                    Some(row_span),
                ));
            }
            let first_target_mapping = seen_target.insert(target_member.clone());
            if assurance == EnumMappingAssurance::Bijection && !first_target_mapping {
                return Err(refinement_error(
                    format!(
                        "enum conversion '{name}' maps target member '{target_member}' more than once"
                    ),
                    Some(row_span),
                ));
            }
            members.push((source_member, target_member));
        }
        let missing_source = source_members
            .iter()
            .filter(|member| !seen_source.contains(*member))
            .cloned()
            .collect::<Vec<_>>();
        let missing_target = target_members
            .iter()
            .filter(|member| !seen_target.contains(*member))
            .cloned()
            .collect::<Vec<_>>();
        if !missing_source.is_empty()
            || (assurance == EnumMappingAssurance::Bijection && !missing_target.is_empty())
        {
            let message = if assurance == EnumMappingAssurance::Bijection {
                format!(
                    "enum conversion '{name}' must cover every source and target member exactly once; missing source: [{}]; missing target: [{}]",
                    missing_source.join(", "),
                    missing_target.join(", ")
                )
            } else {
                format!(
                    "enum abstraction '{name}' must cover every source member exactly once; missing source: [{}]",
                    missing_source.join(", ")
                )
            };
            return Err(refinement_error(message, Some(span)));
        }
        if let Some(existing) = mappings.get(&name) {
            let message = if existing.assurance == assurance {
                format!("duplicate {declaration} '{name}'")
            } else {
                format!("duplicate enum mapping '{name}'")
            };
            return Err(refinement_error(message, Some(span)));
        }
        mappings.insert(
            name,
            EnumMapping {
                source,
                target,
                members,
                span,
                assurance,
            },
        );
    }
    Ok(mappings)
}

fn enum_type_members(
    context: &KernelModel,
    declaration: &str,
    type_name: &str,
    span: Span,
) -> Result<Vec<String>, RefinementError> {
    match context.types.get(type_name) {
        Some(TypeDef::Enum { members, .. }) if members.is_empty() => Err(refinement_error(
            format!("{declaration} endpoint '{type_name}' has no members"),
            Some(span),
        )),
        Some(TypeDef::Enum { members, .. }) => Ok(members.clone()),
        Some(_) => Err(refinement_error(
            format!("{declaration} endpoint '{type_name}' is not an enum"),
            Some(span),
        )),
        None => Err(refinement_error(
            format!("unknown {declaration} type '{type_name}'"),
            Some(span),
        )),
    }
}

#[allow(clippy::too_many_lines)]
fn elaborate_enum_conversions(
    expr: Expr,
    conversions: &BTreeMap<String, EnumMapping>,
) -> Result<Expr, RefinementError> {
    Ok(match expr {
        Expr::Call { name, args, span } if name == "convert" || name == "abstract" => {
            let declaration = if name == "convert" {
                "enum conversion"
            } else {
                "enum abstraction"
            };
            let [conversion_name, argument] = args.as_slice() else {
                return Err(refinement_error(
                    format!("{name} expects exactly two arguments: {name}(name, expression)"),
                    Some(span),
                ));
            };
            let Expr::Var(conversion_name) = conversion_name else {
                return Err(refinement_error(
                    format!("{name} first argument must be an {declaration} name"),
                    Some(span),
                ));
            };
            let conversion = conversions.get(conversion_name).ok_or_else(|| {
                refinement_error(
                    format!("unknown {declaration} '{conversion_name}'"),
                    Some(span),
                )
            })?;
            if conversion.assurance.call_name() != name {
                return Err(refinement_error(
                    format!(
                        "{} '{conversion_name}' must be invoked with {}",
                        conversion.assurance.declaration_name(),
                        conversion.assurance.call_name()
                    ),
                    Some(span),
                ));
            }
            let argument = elaborate_enum_conversions(argument.clone(), conversions)?;
            let (_, fallback_member) = conversion
                .members
                .last()
                .expect("validated enum conversions are non-empty");
            let mut expanded = Expr::EnumMember {
                type_name: conversion.target.clone(),
                member: fallback_member.clone(),
            };
            for (source_member, target_member) in conversion.members.iter().rev() {
                expanded = Expr::Conditional {
                    condition: Box::new(Expr::Binary {
                        op: "==".to_owned(),
                        left: Box::new(argument.clone()),
                        right: Box::new(Expr::EnumMember {
                            type_name: conversion.source.clone(),
                            member: source_member.clone(),
                        }),
                    }),
                    then_expr: Box::new(Expr::EnumMember {
                        type_name: conversion.target.clone(),
                        member: target_member.clone(),
                    }),
                    else_expr: Box::new(expanded),
                    spans: Box::new(ConditionalSpans {
                        condition: span,
                        then_expr: span,
                        else_expr: conversion.span,
                    }),
                };
            }
            expanded
        }
        Expr::Call { name, args, span } => Expr::Call {
            name,
            args: args
                .into_iter()
                .map(|expr| elaborate_enum_conversions(expr, conversions))
                .collect::<Result<_, _>>()?,
            span,
        },
        Expr::Some(expr) => Expr::Some(Box::new(elaborate_enum_conversions(*expr, conversions)?)),
        Expr::Set(items) => Expr::Set(
            items
                .into_iter()
                .map(|expr| elaborate_enum_conversions(expr, conversions))
                .collect::<Result<_, _>>()?,
        ),
        Expr::Seq(items) => Expr::Seq(
            items
                .into_iter()
                .map(|expr| elaborate_enum_conversions(expr, conversions))
                .collect::<Result<_, _>>()?,
        ),
        Expr::Struct { name, fields } => Expr::Struct {
            name,
            fields: fields
                .into_iter()
                .map(|(name, expr)| Ok((name, elaborate_enum_conversions(expr, conversions)?)))
                .collect::<Result<_, RefinementError>>()?,
        },
        Expr::Index(base, index) => Expr::Index(
            Box::new(elaborate_enum_conversions(*base, conversions)?),
            Box::new(elaborate_enum_conversions(*index, conversions)?),
        ),
        Expr::Field(base, field) => Expr::Field(
            Box::new(elaborate_enum_conversions(*base, conversions)?),
            field,
        ),
        Expr::Method {
            receiver,
            name,
            args,
        } => Expr::Method {
            receiver: Box::new(elaborate_enum_conversions(*receiver, conversions)?),
            name,
            args: args
                .into_iter()
                .map(|expr| elaborate_enum_conversions(expr, conversions))
                .collect::<Result<_, _>>()?,
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op,
            left: Box::new(elaborate_enum_conversions(*left, conversions)?),
            right: Box::new(elaborate_enum_conversions(*right, conversions)?),
        },
        Expr::Neg(expr) => Expr::Neg(Box::new(elaborate_enum_conversions(*expr, conversions)?)),
        Expr::Not(expr) => Expr::Not(Box::new(elaborate_enum_conversions(*expr, conversions)?)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            spans,
        } => Expr::Conditional {
            condition: Box::new(elaborate_enum_conversions(*condition, conversions)?),
            then_expr: Box::new(elaborate_enum_conversions(*then_expr, conversions)?),
            else_expr: Box::new(elaborate_enum_conversions(*else_expr, conversions)?),
            spans,
        },
        Expr::Is { expr, pattern } => Expr::Is {
            expr: Box::new(elaborate_enum_conversions(*expr, conversions)?),
            pattern,
        },
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => Expr::Quantified {
            quantifier,
            binder: elaborate_conversion_binder(binder, conversions)?,
            body: Box::new(elaborate_enum_conversions(*body, conversions)?),
        },
        Expr::Aggregate {
            kind,
            binder,
            value,
        } => Expr::Aggregate {
            kind,
            binder: elaborate_conversion_binder(binder, conversions)?,
            value: value
                .map(|expr| elaborate_enum_conversions(*expr, conversions).map(Box::new))
                .transpose()?,
        },
        Expr::Stage {
            process,
            entity,
            entity_span,
            span,
        } => Expr::Stage {
            process,
            entity: Box::new(elaborate_enum_conversions(*entity, conversions)?),
            entity_span,
            span,
        },
        Expr::UnaryNamed { name, expr, span } => Expr::UnaryNamed {
            name,
            expr: Box::new(elaborate_enum_conversions(*expr, conversions)?),
            span,
        },
        Expr::BinaryNamed { name, left, right } => Expr::BinaryNamed {
            name,
            left: Box::new(elaborate_enum_conversions(*left, conversions)?),
            right: Box::new(elaborate_enum_conversions(*right, conversions)?),
        },
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => Expr::TernaryNamed {
            name,
            first: Box::new(elaborate_enum_conversions(*first, conversions)?),
            second: Box::new(elaborate_enum_conversions(*second, conversions)?),
            third: Box::new(elaborate_enum_conversions(*third, conversions)?),
        },
        expr @ (Expr::Num(_)
        | Expr::Bool(_)
        | Expr::None
        | Expr::Var(_)
        | Expr::EnumMember { .. }) => expr,
    })
}

fn elaborate_conversion_binder(
    binder: Binder,
    conversions: &BTreeMap<String, EnumMapping>,
) -> Result<Binder, RefinementError> {
    Ok(match binder {
        Binder::Typed {
            name,
            type_name,
            where_expr,
        } => Binder::Typed {
            name,
            type_name,
            where_expr: where_expr
                .map(|expr| elaborate_enum_conversions(*expr, conversions).map(Box::new))
                .transpose()?,
        },
        Binder::Range {
            name,
            lo,
            hi,
            where_expr,
        } => Binder::Range {
            name,
            lo: Box::new(elaborate_enum_conversions(*lo, conversions)?),
            hi: Box::new(elaborate_enum_conversions(*hi, conversions)?),
            where_expr: where_expr
                .map(|expr| elaborate_enum_conversions(*expr, conversions).map(Box::new))
                .transpose()?,
        },
        Binder::Collection {
            name,
            collection,
            where_expr,
        } => Binder::Collection {
            name,
            collection: Box::new(elaborate_enum_conversions(*collection, conversions)?),
            where_expr: where_expr
                .map(|expr| elaborate_enum_conversions(*expr, conversions).map(Box::new))
                .transpose()?,
        },
    })
}

fn validate_refinement_expressions(
    abstraction: &KernelModel,
    state_maps: &BTreeMap<String, StateMap>,
    context: &KernelModel,
) -> Result<(), RefinementError> {
    for state_map in state_maps.values() {
        validate_state_map(state_map, abstraction, context)?;
    }
    Ok(())
}

fn refinement_type_context(implementation: &KernelModel, abstraction: &KernelModel) -> KernelModel {
    let mut context = implementation.clone();
    for (name, definition) in &abstraction.types {
        context
            .types
            .entry(name.clone())
            .or_insert(definition.clone());
    }
    for (name, value) in &abstraction.enum_members {
        context
            .enum_members
            .entry(name.clone())
            .or_insert(value.clone());
    }
    context
}

fn validate_state_map(
    state_map: &StateMap,
    abstraction: &KernelModel,
    context: &KernelModel,
) -> Result<(), RefinementError> {
    let mut expected = abstraction
        .state_type(&state_map.name)
        .expect("state maps were checked against abstraction state")
        .clone();
    let mut bindings = Vec::new();
    if let Some(binder) = &state_map.binder {
        let binder_ty = crate::typecheck::expression_binder_type(binder, context)
            .map_err(|error| invalid_state_map_at_map(state_map, "binder", &error.message))?;
        let TypeRef::Map(key, value) = expected else {
            return Err(refinement_error(
                format!(
                    "map '{}' has a binder but its abstract state is not a Map",
                    state_map.name
                ),
                state_map.span,
            ));
        };
        let binder_name = match binder {
            Binder::Typed { name, .. }
            | Binder::Range { name, .. }
            | Binder::Collection { name, .. } => name.clone(),
        };
        crate::typecheck::validate_expression_type(
            &Expr::Var(binder_name.clone()),
            &key,
            &[(binder_name.clone(), binder_ty.clone())],
            context,
        )
        .map_err(|error| invalid_state_map_at_map(state_map, "binder", &error.message))?;
        bindings.push((binder_name, binder_ty));
        expected = *value;
    }
    crate::typecheck::validate_expression_type(&state_map.expr, &expected, &bindings, context)
        .map_err(|error| {
            invalid_state_map(
                state_map,
                "expression",
                &error.message,
                error
                    .span
                    .expect("mapped expression type errors carry source spans"),
            )
        })
}

fn invalid_state_map_at_map(state_map: &StateMap, part: &str, message: &str) -> RefinementError {
    refinement_error(
        format!("invalid map {part} for '{}': {message}", state_map.name),
        state_map.span,
    )
}

fn invalid_state_map(
    state_map: &StateMap,
    part: &str,
    message: &str,
    span: Span,
) -> RefinementError {
    refinement_error(
        format!("invalid map {part} for '{}': {message}", state_map.name),
        Some(span),
    )
}

fn parameter_type(parameter: &ParamDef) -> TypeRef {
    match parameter {
        ParamDef::Typed { ty, .. } => ty.clone(),
        ParamDef::Range { lo, hi, .. } => TypeRef::Range(*lo, *hi),
    }
}

fn apply_auto_state_maps(
    implementation: &KernelModel,
    abstraction: &KernelModel,
    state_maps: &mut BTreeMap<String, StateMap>,
) -> Result<(), RefinementError> {
    for (name, abs_ty) in &abstraction.state {
        if state_maps.contains_key(name) || implementation.state_type(name).is_none() {
            continue;
        }
        let (binder, expr) = match abs_ty {
            TypeRef::Map(key, _) => {
                let TypeRef::Named(type_name) = key.as_ref() else {
                    return Err(refinement_error(
                        "Map keys need a named bounded type for per-key identity mapping",
                        None,
                    ));
                };
                let binder = Binder::Typed {
                    name: "_k".to_owned(),
                    type_name: QualifiedName {
                        namespace: None,
                        name: type_name.clone(),
                    },
                    where_expr: None,
                };
                (
                    Some(binder),
                    Expr::Index(
                        Box::new(Expr::Var(name.clone())),
                        Box::new(Expr::Var("_k".to_owned())),
                    ),
                )
            }
            _ => (None, Expr::Var(name.clone())),
        };
        state_maps.insert(
            name.clone(),
            StateMap {
                name: name.clone(),
                binder,
                expr,
                span: None,
            },
        );
    }
    Ok(())
}

fn apply_auto_action_correspondences(
    implementation: &KernelModel,
    abstraction: &KernelModel,
    action_correspondences: &mut BTreeMap<String, ActionCorrespondence>,
    context: &KernelModel,
    span: Span,
    stutter_unmatched: bool,
) -> Result<(), RefinementError> {
    for impl_action in &implementation.actions {
        if action_correspondences.contains_key(&impl_action.name) {
            continue;
        }
        let abs_action = abstraction
            .actions
            .iter()
            .find(|action| action.name == impl_action.name);
        if abs_action.is_none() && !stutter_unmatched {
            continue;
        }
        let params = impl_action
            .params
            .iter()
            .map(|param| RefinementParam {
                name: param.name().to_owned(),
                ty: None,
            })
            .collect::<Vec<_>>();
        insert_action_correspondence(
            action_correspondences,
            ActionCorrespondenceSource {
                impl_action: impl_action.name.clone(),
                impl_params: params,
                target: abs_action.map_or(ActionTarget::Stutter, |abs_action| {
                    ActionTarget::Action(
                        abs_action.name.clone(),
                        abs_action
                            .params
                            .iter()
                            .enumerate()
                            .filter_map(|(index, parameter)| {
                                impl_action
                                    .params
                                    .iter()
                                    .find(|candidate| candidate.name() == parameter.name())
                                    .or_else(|| impl_action.params.get(index))
                                    .map(|candidate| Expr::Var(candidate.name().to_owned()))
                            })
                            .collect(),
                    )
                }),
                origin: CorrespondenceOrigin::Auto,
                span,
            },
            implementation,
            abstraction,
            context,
        )?;
    }
    Ok(())
}

struct ActionCorrespondenceSource {
    impl_action: String,
    impl_params: Vec<RefinementParam>,
    target: ActionTarget,
    origin: CorrespondenceOrigin,
    span: Span,
}

fn validate_correspondence_duplicates(
    sources: &[ActionCorrespondenceSource],
) -> Result<(), RefinementError> {
    let mut first_by_action = BTreeMap::new();
    for source in sources {
        if let Some(previous) = first_by_action.insert(source.impl_action.as_str(), source) {
            return Err(refinement_error(
                format!(
                    "duplicate action correspondence for '{}': {} at {}:{} conflicts with {} at {}:{}",
                    source.impl_action,
                    previous.origin.as_str(),
                    previous.span.start.line,
                    previous.span.start.column,
                    source.origin.as_str(),
                    source.span.start.line,
                    source.span.start.column,
                ),
                Some(source.span),
            ));
        }
    }
    Ok(())
}

fn insert_action_correspondence(
    correspondences: &mut BTreeMap<String, ActionCorrespondence>,
    source: ActionCorrespondenceSource,
    implementation: &KernelModel,
    abstraction: &KernelModel,
    context: &KernelModel,
) -> Result<(), RefinementError> {
    let implementation_action = implementation
        .actions
        .iter()
        .find(|action| action.name == source.impl_action)
        .ok_or_else(|| {
            refinement_error(
                format!("unknown impl action '{}'", source.impl_action),
                Some(source.span),
            )
        })?;
    validate_impl_params(&source, implementation_action, context)?;
    let target = lower_action_target(&source, implementation_action, abstraction, context)?;
    correspondences.insert(
        source.impl_action.clone(),
        ActionCorrespondence {
            impl_action: ActionRef(source.impl_action),
            impl_params: implementation_action.params.clone(),
            target,
            origin: source.origin,
            span: source.span,
        },
    );
    Ok(())
}

fn validate_impl_params(
    source: &ActionCorrespondenceSource,
    action: &ActionDef,
    context: &KernelModel,
) -> Result<(), RefinementError> {
    if action.params.len() != source.impl_params.len()
        || action
            .params
            .iter()
            .zip(&source.impl_params)
            .any(|(actual, declared)| actual.name() != declared.name)
    {
        return Err(refinement_error(
            format!(
                "action map parameters for '{}' must match the impl action parameter names and order",
                source.impl_action
            ),
            Some(source.span),
        ));
    }
    for (actual, declared) in action.params.iter().zip(&source.impl_params) {
        let Some(annotation) = &declared.ty else {
            continue;
        };
        let resolved = context
            .resolve_surface_type(annotation)
            .map_err(|error| refinement_error(error.message, Some(source.span)))?;
        if resolved != parameter_type(actual) {
            return Err(refinement_error(
                format!(
                    "action map parameter '{}.{}' type does not match the impl action",
                    source.impl_action, declared.name
                ),
                Some(source.span),
            ));
        }
    }
    Ok(())
}

fn lower_action_target(
    source: &ActionCorrespondenceSource,
    implementation_action: &ActionDef,
    abstraction: &KernelModel,
    context: &KernelModel,
) -> Result<ActionCorrespondenceTarget, RefinementError> {
    let target = match &source.target {
        ActionTarget::Stutter => ActionCorrespondenceTarget::Stutter,
        ActionTarget::Action(name, args) => {
            let abstraction_action = abstraction
                .actions
                .iter()
                .find(|action| action.name == *name)
                .ok_or_else(|| {
                    refinement_error(
                        format!("unknown abstract action '{name}'"),
                        Some(source.span),
                    )
                })?;
            if abstraction_action.params.len() != args.len() {
                return Err(refinement_error(
                    format!(
                        "action map '{}' -> '{}' has {} arguments, expected {}",
                        source.impl_action,
                        name,
                        args.len(),
                        abstraction_action.params.len()
                    ),
                    Some(source.span),
                ));
            }
            let bindings = implementation_action
                .params
                .iter()
                .map(|param| (param.name().to_owned(), parameter_type(param)))
                .collect::<Vec<_>>();
            for (index, (argument, parameter)) in
                args.iter().zip(&abstraction_action.params).enumerate()
            {
                crate::typecheck::validate_expression_type(
                    argument,
                    &parameter_type(parameter),
                    &bindings,
                    context,
                )
                .map_err(|error| {
                    refinement_error(
                        format!(
                            "invalid argument {} for action '{}' -> '{}': {}",
                            index + 1,
                            source.impl_action,
                            name,
                            error.message
                        ),
                        error.span.or(Some(source.span)),
                    )
                })?;
            }
            if source.origin == CorrespondenceOrigin::Auto
                && let (Some(impl_actor), Some(abs_actor)) = (
                    action_actor(implementation_action.meta.as_ref()),
                    action_actor(abstraction_action.meta.as_ref()),
                )
                && impl_actor != abs_actor
            {
                return Err(refinement_error(
                    format!(
                        "maps auto actor mismatch for action '{}': impl actor '{}' != abstract actor '{}'",
                        source.impl_action, impl_actor, abs_actor
                    ),
                    Some(source.span),
                ));
            }
            ActionCorrespondenceTarget::Action {
                action: ActionRef(name.clone()),
                args: args.clone(),
            }
        }
    };
    Ok(target)
}

fn action_actor(meta: Option<&MetaTag>) -> Option<&str> {
    meta.and_then(|tag| tag.text.as_deref())
        .and_then(|text| text.strip_prefix("by "))
        .map(str::trim)
        .filter(|actor| !actor.is_empty())
}

fn refinement_error(message: impl Into<String>, span: Option<Span>) -> RefinementError {
    RefinementError {
        message: message.into(),
        span,
    }
}

fn append_requirement_action_maps(action: &RequirementAction, items: &mut Vec<RefinementItem>) {
    let params = action
        .params
        .iter()
        .map(|param| RefinementParam {
            name: match param {
                fsl_syntax::Param::Typed(name, _) | fsl_syntax::Param::Range(name, ..) => {
                    name.clone()
                }
            },
            ty: None,
        })
        .collect::<Vec<_>>();
    if let Some(branches) = action.items.iter().find_map(|item| match item {
        RequirementActionItem::Branches { branches, .. } => Some(branches),
        RequirementActionItem::Action(_) => None,
    }) {
        for (index, branch) in branches.iter().enumerate() {
            items.push(RefinementItem::Action {
                name: format!("{}__b{}", action.name, index + 1),
                params: params.clone(),
                target: branch.maps.target.clone(),
                origin: CorrespondenceOrigin::InlineMapsClause,
                span: branch.maps.span,
            });
        }
    } else if let Some(mapping) = &action.maps {
        items.push(RefinementItem::Action {
            name: action.name.clone(),
            params,
            target: mapping.target.clone(),
            origin: CorrespondenceOrigin::InlineMapsClause,
            span: mapping.span,
        });
    }
}

/// Resolve a requirements-layer `implements` declaration into the abstract
/// model and its generated refinement mapping.
///
/// # Errors
///
/// Returns [`RefinementError`] for dependency, lowering, or mapping failures.
#[allow(clippy::too_many_lines)]
pub fn requirements_implements(
    source: &str,
    resolver: &dyn FileResolver,
    implementation: &KernelModel,
) -> Result<Option<ImplementsContract>, RefinementError> {
    let document = fsl_syntax::parse_surface_document(source).map_err(|error| RefinementError {
        message: error.message,
        span: Some(error.span),
    })?;
    let SurfaceDocument::Requirements(requirements) = document else {
        return Ok(None);
    };
    let implements = requirements.items.iter().find_map(|item| match item {
        RequirementsItem::Implements {
            name,
            path,
            items,
            span,
        } => Some((name, path, items, *span)),
        _ => None,
    });
    let Some((abs_name, path, explicit, span)) = implements else {
        return Ok(None);
    };
    let abs_source = resolver.read(path).map_err(|error| RefinementError {
        message: error.message,
        span: Some(span),
    })?;
    let kernel = parse_kernel_source(&abs_source, resolver).map_err(|error| RefinementError {
        message: error.message,
        span: Some(span),
    })?;
    let abstraction = build_model(kernel).map_err(|error| RefinementError {
        message: error.message,
        span: Some(span),
    })?;
    if abstraction.name != *abs_name {
        return Err(refinement_error(
            format!(
                "spec name mismatch: expected '{abs_name}', got '{}'",
                abstraction.name
            ),
            Some(span),
        ));
    }
    let mut items = vec![
        RefinementItem::Impl(implementation.name.clone()),
        RefinementItem::Abs(abstraction.name.clone()),
    ];
    items.extend(explicit.iter().cloned());
    for item in &requirements.items {
        match item {
            RequirementsItem::Requirement {
                items: declarations,
                ..
            } => {
                for declaration in declarations {
                    if let RequirementBlockItem::Action(action) = declaration {
                        append_requirement_action_maps(action, &mut items);
                    }
                }
            }
            RequirementsItem::Action(action) => {
                append_requirement_action_maps(action, &mut items);
            }
            _ => {}
        }
    }
    let explicit_maps = explicit
        .iter()
        .filter_map(|item| match item {
            RefinementItem::Map { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    for (name, ty) in &abstraction.state {
        if explicit_maps.contains(name.as_str()) || implementation.state_type(name).is_none() {
            continue;
        }
        let (binder, expr) = match ty {
            TypeRef::Map(key, _) => {
                let TypeRef::Named(type_name) = key.as_ref() else {
                    return Err(refinement_error(
                        "auto state map requires a named Map key type",
                        Some(span),
                    ));
                };
                (
                    Some(Binder::Typed {
                        name: "c".to_owned(),
                        type_name: QualifiedName {
                            namespace: None,
                            name: type_name.clone(),
                        },
                        where_expr: None,
                    }),
                    Expr::Index(
                        Box::new(Expr::Var(name.clone())),
                        Box::new(Expr::Var("c".to_owned())),
                    ),
                )
            }
            _ => (None, Expr::Var(name.clone())),
        };
        items.push(RefinementItem::Map {
            name: name.clone(),
            binder,
            expr: Box::new(expr),
            span,
        });
    }
    let refinement = build_refinement(
        SurfaceRefinement {
            name: format!("{}Implements{}", implementation.name, abstraction.name),
            items,
        },
        implementation,
        &abstraction,
        Some(span),
    )?;
    Ok(Some(ImplementsContract {
        abstraction,
        refinement,
    }))
}

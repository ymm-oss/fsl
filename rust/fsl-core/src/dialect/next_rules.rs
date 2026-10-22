// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Lowering of the guards and properties next-edition rules declare (fsl 5
//! plan §3 S6, design draft B.3–B.6).
//!
//! A [`RuleGuard`] becomes one `requires` of its event's action, after the
//! stage guard, with an origin whose lowering step is the statement kind
//! (`allow`, `never`, `after`). A [`RuleProperty`] becomes an invariant
//! (`always`) or a transition property (`enters`) quantified over the
//! entity, named by [`crate::naming::rule_property_name`]. Both carry the
//! rule's requirement.
//!
//! The stage guard itself, an event's first `requires`, has an origin whose
//! step is `move` (`Submitted -> Approved`, or `in Submitted` for an event
//! that keeps its stage), so every `requires` of an event's action names
//! where it came from (`fslc explain`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use fsl_syntax::{
    AggregateKind, Annotation, Binder, EventForm, Expr, MeasureDeclaration, ProcessTransition,
    RequirementBlockItem, RequirementsItem, RuleGuard, RuleProperty, SpecItem,
};

use super::{
    GENERATED_NAME_COLLISION, Process, RequirementsProcess, core_error, meta, process_field_state,
    qualified, stage_access, typed_binder,
};
use crate::{
    CoreError, LoweringStep, OriginChain, OriginId, OriginSite, ProjectionDef, property_target,
};

/// The replacement that reads the stage of `process`'s instance `binder`
/// where a rule condition spells it [`fsl_syntax::stage_reference`].
pub(super) fn stage_replacement(process: &Process, binder: &str) -> (String, Expr) {
    (
        fsl_syntax::stage_reference(&process.entity),
        stage_access(process, Expr::Var(binder.to_owned())),
    )
}

fn rule_origin(
    id: String,
    span: fsl_syntax::Span,
    requirement: &str,
    kind: &str,
    detail: &str,
) -> OriginChain {
    OriginChain {
        id: OriginId(id),
        dialect: "requirements".to_owned(),
        primary: Some(OriginSite {
            source_file: None,
            span: Some(span),
            dialect: "requirements".to_owned(),
            declaration_path: vec![requirement.to_owned()],
        }),
        secondary: Vec::new(),
        lowering_steps: vec![LoweringStep {
            kind: kind.to_owned(),
            detail: Some(format!("{requirement} {detail}")),
        }],
        generated: true,
    }
}

/// The origin of one rule guard of `transition`'s action.
pub(super) fn guard_origin(transition: &ProcessTransition, guard: &RuleGuard) -> OriginChain {
    rule_origin(
        format!(
            "requirements:{}:{}:{}",
            guard.kind, transition.name, guard.span.start.offset
        ),
        guard.span,
        &guard.requirement,
        guard.kind,
        &guard.detail,
    )
}

/// The origin of the stage guard of `transition`'s action: the event's
/// move, `Source -> Target` or `in Source`.
pub(super) fn move_origin(transition: &ProcessTransition, event: &EventForm) -> OriginChain {
    OriginChain {
        id: OriginId(format!(
            "requirements:move:{}:{}",
            transition.name, transition.span.start.offset
        )),
        dialect: "requirements".to_owned(),
        primary: Some(OriginSite {
            source_file: None,
            span: Some(transition.span),
            dialect: "requirements".to_owned(),
            declaration_path: vec![transition.name.clone()],
        }),
        secondary: Vec::new(),
        lowering_steps: vec![LoweringStep {
            kind: "move".to_owned(),
            detail: Some(if event.moves {
                format!("{} -> {}", transition.source, transition.target)
            } else {
                format!("in {}", transition.source)
            }),
        }],
        generated: true,
    }
}

/// A name written for a rule property that a Kernel property cannot take.
pub const INVALID_RULE_PROPERTY_NAME: &str = "invalid_rule_property_name";
/// A name written for a rule property that another property already has.
pub const RULE_PROPERTY_NAME_COLLISION: &str = "rule_property_name_collision";

/// One lowered rule property with what the requirements lowering binds to
/// it.
pub(super) struct LoweredProperty {
    pub item: SpecItem,
    pub target: String,
    pub annotation: Annotation,
    pub origin: OriginChain,
}

/// The property names the document declares.
fn declared_property_names(items: &[RequirementsItem]) -> BTreeSet<String> {
    items
        .iter()
        .flat_map(|item| match item {
            RequirementsItem::Common(property) => vec![property],
            RequirementsItem::Requirement { items, .. } => items
                .iter()
                .filter_map(|item| match item {
                    RequirementBlockItem::Property(property) => Some(property),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        })
        .filter_map(|property| match property {
            SpecItem::Invariant { name, .. }
            | SpecItem::Trans { name, .. }
            | SpecItem::Reachable { name, .. }
            | SpecItem::LeadsTo { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

/// How a rule property reads instance `binder` of `process`: its fields,
/// bare or spelled `Entity.field`, and its stage.
fn property_replacements(process: &RequirementsProcess, binder: &str) -> HashMap<String, Expr> {
    let mut replacements = HashMap::new();
    for field in &process.fields {
        let read = Expr::Index(
            Box::new(Expr::Var(process_field_state(
                &process.process.name,
                &field.name,
            ))),
            Box::new(Expr::Var(binder.to_owned())),
        );
        replacements.insert(field.name.clone(), read.clone());
        replacements.insert(format!("{}.{}", process.process.entity, field.name), read);
    }
    let (stage, read) = stage_replacement(&process.process, binder);
    replacements.insert(stage, read);
    replacements
}

/// The Kernel property name of each of `properties` (a requirements
/// document's rule properties, in source order), as the lowering names them:
/// the written name, or the one [`crate::naming::rule_property_name`]
/// generates. A reader (`fslc document`) finds each property's lowered
/// expression by it.
#[must_use]
pub fn rule_property_names(properties: &[RuleProperty]) -> Vec<String> {
    let mut ordinals = BTreeMap::<(&str, &str), usize>::new();
    properties
        .iter()
        .map(|property| {
            property.name.clone().unwrap_or_else(|| {
                let ordinal = ordinals
                    .entry((property.requirement.as_str(), property.kind))
                    .or_default();
                *ordinal += 1;
                crate::naming::rule_property_name(&property.requirement, property.kind, *ordinal)
            })
        })
        .collect()
}

/// The Kernel name of `property`: the written one, checked, or the one
/// [`crate::naming::rule_property_name`] generates, numbered among the
/// requirement's unnamed statements of its kind. The name joins `declared`.
fn property_name<'a>(
    property: &'a RuleProperty,
    ordinals: &mut BTreeMap<(&'a str, &'a str), usize>,
    declared: &mut BTreeSet<String>,
) -> Result<String, CoreError> {
    let name = if let Some(written) = &property.name {
        if let Some(reason) = crate::naming::invalid_rule_property_name(written) {
            return Err(CoreError {
                diagnostic_code: Some(INVALID_RULE_PROPERTY_NAME),
                ..core_error(
                    format!(
                        "rule {} names its property '{written}', which {reason}",
                        property.requirement
                    ),
                    property.span,
                )
            });
        }
        written.clone()
    } else {
        let ordinal = ordinals
            .entry((property.requirement.as_str(), property.kind))
            .or_default();
        *ordinal += 1;
        crate::naming::rule_property_name(&property.requirement, property.kind, *ordinal)
    };
    if !declared.insert(name.clone()) {
        let (code, verb) = if property.name.is_some() {
            (RULE_PROPERTY_NAME_COLLISION, "names")
        } else {
            (GENERATED_NAME_COLLISION, "generates")
        };
        return Err(CoreError {
            diagnostic_code: Some(code),
            ..core_error(
                format!(
                    "rule {} `{}` {verb} the property '{name}', which is already \
                     declared; rename one of them",
                    property.requirement, property.written
                ),
                property.span,
            )
        });
    }
    Ok(name)
}

/// Lower `properties` over the processes their entities desugared into;
/// `items` are the document's items, whose property names a generated name
/// may not take.
///
/// # Errors
///
/// Returns [`INVALID_RULE_PROPERTY_NAME`] for a written name no property can
/// take, [`RULE_PROPERTY_NAME_COLLISION`] when a written name is taken, and
/// [`GENERATED_NAME_COLLISION`] when a generated one is.
pub(super) fn lower_rule_properties(
    properties: &[RuleProperty],
    processes: &[RequirementsProcess],
    binders: &[String],
    items: &[RequirementsItem],
) -> Result<Vec<LoweredProperty>, CoreError> {
    let mut declared = declared_property_names(items);
    let mut ordinals = BTreeMap::<(&str, &str), usize>::new();
    let mut lowered = Vec::new();
    for property in properties {
        let Some((process, binder)) = processes
            .iter()
            .zip(binders)
            .find(|(process, _)| process.process.entity == property.entity)
        else {
            return Err(core_error(
                format!("entity '{}' has no lifecycle", property.entity),
                property.span,
            ));
        };
        let replacements = property_replacements(process, binder);
        let name = property_name(property, &mut ordinals, &mut declared)?;
        let expr = Box::new(Expr::Quantified {
            quantifier: "forall".to_owned(),
            binder: typed_binder(binder, &property.entity),
            body: Box::new(crate::substitute_expr(
                property.condition.clone(),
                &replacements,
            )),
        });
        let metadata = meta(&property.requirement, property.requirement_text.clone());
        let (item, kind) = if property.kind == "enters" {
            (
                SpecItem::Trans {
                    name: name.clone(),
                    expr,
                    span: property.span,
                    meta: metadata,
                    annotations: fsl_syntax::Annotations::default(),
                },
                "trans",
            )
        } else {
            (
                SpecItem::Invariant {
                    name: name.clone(),
                    expr,
                    span: property.span,
                    meta: metadata,
                    annotations: fsl_syntax::Annotations::default(),
                },
                "invariant",
            )
        };
        lowered.push(LoweredProperty {
            item,
            target: property_target(kind, &name),
            annotation: Annotation::Requirement {
                id: property.requirement.clone(),
                text: Some(property.requirement_text.clone()),
                span: property.requirement_span,
            },
            origin: rule_origin(
                format!(
                    "requirements:{}:{name}:{}",
                    property.kind, property.span.start.offset
                ),
                property.span,
                &property.requirement,
                property.kind,
                &property.detail,
            ),
        });
    }
    Ok(lowered)
}

/// The projections `measures` declare: each counts the instances of its
/// entity, bound as the entity's binder, that satisfy its condition (the
/// concept's stages and `where`). Like a `kpi`'s, a measure's projection
/// adds no state.
///
/// # Errors
///
/// Returns an error for a measure over an entity without a lifecycle.
pub(super) fn measure_projections(
    measures: &[MeasureDeclaration],
    processes: &[RequirementsProcess],
    binders: &[String],
) -> Result<Vec<ProjectionDef>, CoreError> {
    measures
        .iter()
        .map(|measure| {
            let Some((process, binder)) = processes
                .iter()
                .zip(binders)
                .find(|(process, _)| process.process.entity == measure.entity)
            else {
                return Err(core_error(
                    format!(
                        "measure '{}': entity '{}' has no lifecycle",
                        measure.name, measure.entity
                    ),
                    measure.span,
                ));
            };
            let condition = crate::substitute_expr(
                measure.condition.clone(),
                &property_replacements(process, binder),
            );
            Ok(ProjectionDef {
                name: measure.name.clone(),
                entity: measure.entity.clone(),
                stage: measure.counted.clone(),
                expr: Expr::Aggregate {
                    kind: AggregateKind::Count,
                    binder: Binder::Typed {
                        name: binder.clone(),
                        type_name: qualified(&measure.entity),
                        where_expr: Some(Box::new(condition)),
                    },
                    value: None,
                },
                span: measure.span,
            })
        })
        .collect()
}

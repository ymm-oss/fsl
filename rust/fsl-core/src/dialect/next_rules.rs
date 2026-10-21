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

use std::collections::{BTreeMap, BTreeSet, HashMap};

use fsl_syntax::{
    Annotation, Expr, ProcessTransition, RequirementBlockItem, RequirementsItem, RuleGuard,
    RuleProperty, SpecItem,
};

use super::{
    GENERATED_NAME_COLLISION, Process, RequirementsProcess, core_error, meta, process_field_state,
    stage_access, typed_binder,
};
use crate::rule_guards::{
    ENUMERATION_BOUND, GuardDecision, RULE_CONFLICT_UNSATISFIABLE_GUARD, rule_guard_problems,
};
use crate::{CoreError, LoweringStep, OriginChain, OriginId, OriginSite, property_target};

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

/// Refuse an event whose rule guards cannot hold together, where the
/// enumeration of the type domains decides it (decisions/048 J3). A problem
/// beyond [`ENUMERATION_BOUND`] is left to `fslc check`.
///
/// # Errors
///
/// Returns [`RULE_CONFLICT_UNSATISFIABLE_GUARD`] with the conflicting rules.
pub(super) fn check_rule_guards(
    requirements: &fsl_syntax::SurfaceRequirements,
) -> Result<(), CoreError> {
    for problem in rule_guard_problems(requirements) {
        if let GuardDecision::Unsatisfiable(core) = problem.decide_by_enumeration(ENUMERATION_BOUND)
        {
            return Err(CoreError {
                diagnostic_code: Some(RULE_CONFLICT_UNSATISFIABLE_GUARD),
                ..core_error(problem.unsatisfiable_message(&core), problem.span)
            });
        }
    }
    Ok(())
}

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

/// Lower `properties` over the processes their entities desugared into;
/// `items` are the document's items, whose property names a generated name
/// may not take.
///
/// # Errors
///
/// Returns [`GENERATED_NAME_COLLISION`] when a generated name is declared.
pub(super) fn lower_rule_properties(
    properties: &[RuleProperty],
    processes: &[RequirementsProcess],
    binders: &[String],
    items: &[RequirementsItem],
) -> Result<Vec<LoweredProperty>, CoreError> {
    let declared = declared_property_names(items);
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
        let ordinal = ordinals
            .entry((property.requirement.as_str(), property.kind))
            .or_default();
        *ordinal += 1;
        let name =
            crate::naming::rule_property_name(&property.requirement, property.kind, *ordinal);
        if declared.contains(&name) {
            return Err(CoreError {
                diagnostic_code: Some(GENERATED_NAME_COLLISION),
                ..core_error(
                    format!(
                        "rule {} `{}` generates the property '{name}', which is already \
                         declared; rename it",
                        property.requirement, property.detail
                    ),
                    property.span,
                )
            });
        }
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

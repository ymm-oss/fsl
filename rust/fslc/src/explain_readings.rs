// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The readings a next-edition requirements document leaves implicit, as
//! `fslc explain` shows them (#1817): per event, where each input is stored,
//! whether the phrase names a verb, whether the event is fair and that its
//! guards all must hold; per entity, the initial stage and the stage order
//! its lifecycle lines give.
//!
//! The desugaring records the event and lifecycle readings on the surface
//! ([`fsl_syntax::NextEditionRequirements`]); fairness and the guards are the
//! Kernel action's, which an event's origin names. Nothing here reaches the
//! Kernel, and a document of any other form has no readings.

use fsl_core::{ActionDef, KernelModel, action_target};
use fsl_syntax::{EventReading, InputReading, InputStore, NextEditionRequirements};
use serde_json::{Value, json};

/// The readings of `source`, or `None` unless it is a next-edition
/// requirements document.
pub(crate) fn of_source(source: &str) -> Option<NextEditionRequirements> {
    match fsl_syntax::parse_surface_document_with(source, crate::process::parse_options()) {
        Ok(fsl_syntax::SurfaceDocument::Requirements(requirements)) => requirements.next,
        _ => None,
    }
}

/// The action `event` lowers to: the one whose `event` origin is the
/// event's declaration.
fn event_action<'m>(model: &'m KernelModel, event: &EventReading) -> Option<&'m ActionDef> {
    model.actions.iter().find(|action| {
        model
            .origins()
            .origins_for(&action_target(&action.name))
            .iter()
            .any(|origin| {
                origin
                    .lowering_steps
                    .iter()
                    .any(|step| step.kind == "event")
                    && origin
                        .primary
                        .as_ref()
                        .and_then(|site| site.span)
                        .is_some_and(|span| span.start.offset == event.span.start.offset)
            })
    })
}

fn input_json(input: &InputReading) -> Value {
    json!({
        "name": input.name,
        "written": input.written,
        "stored": input
            .stored
            .iter()
            .map(|(field, store)| json!({"field": field, "by": store.as_str()}))
            .collect::<Vec<_>>(),
    })
}

/// `explain`'s `readings`: `entities[]` (`entity`, `initial`, `stages`,
/// `lifecycle_lines`) and `events[]` (`event`, `entity`, `verb`,
/// `verb_omitted`, `fair`, `inputs[]` with where each is `stored`, and
/// `guards[]`: each `requires` of the event's action, `conjunctive`, with
/// its origins as `explain --lowering` lists them).
pub(crate) fn json(model: &KernelModel, next: &NextEditionRequirements) -> Value {
    let entities = next
        .lifecycle_readings
        .iter()
        .map(|lifecycle| {
            json!({
                "entity": lifecycle.entity,
                "initial": lifecycle.initial,
                "stages": lifecycle.stages,
                "lifecycle_lines": lifecycle.lines,
            })
        })
        .collect::<Vec<_>>();
    let events = next
        .event_readings
        .iter()
        .filter_map(|event| {
            let action = event_action(model, event)?;
            let guards = action
                .requires
                .iter()
                .enumerate()
                .map(|(index, requirement)| {
                    json!({
                        "index": index,
                        "expr": fslc_rust::expr_text(requirement),
                        "conjunctive": true,
                        "origins": crate::explain_lowering::guard_origins(model, &action.name, index),
                    })
                })
                .collect::<Vec<_>>();
            Some(json!({
                "event": event.event,
                "entity": event.entity,
                "verb": event.verb,
                "verb_omitted": event.verb_omitted,
                "fair": action.fair,
                "inputs": event.inputs.iter().map(input_json).collect::<Vec<_>>(),
                "guards": guards,
            }))
        })
        .collect::<Vec<_>>();
    json!({"entities": entities, "events": events})
}

/// The readable lines of the `Lifecycles:` section, one per entity:
/// `Order: initial Empty; stages Empty, Placed (1 lifecycle line)`.
pub(crate) fn lifecycle_lines(next: &NextEditionRequirements) -> Vec<String> {
    next.lifecycle_readings
        .iter()
        .map(|lifecycle| {
            let source = if lifecycle.lines == 1 {
                "1 lifecycle line".to_owned()
            } else {
                format!(
                    "{} lifecycle lines: the first stage of the first line is the initial one; \
                     the stages are ordered as they first appear",
                    lifecycle.lines
                )
            };
            format!(
                "{}: initial {}; stages {} ({source})",
                lifecycle.entity,
                lifecycle.initial,
                lifecycle.stages.join(", ")
            )
        })
        .collect()
}

fn reading_of<'n>(
    model: &KernelModel,
    next: &'n NextEditionRequirements,
    action: &ActionDef,
) -> Option<&'n EventReading> {
    next.event_readings
        .iter()
        .find(|event| event_action(model, event).is_some_and(|lowered| lowered.name == action.name))
}

/// The readable lines after an event's `event:` line: the verb when the
/// phrase leaves it out, fairness, and where each input is stored. Empty for
/// an action of no event.
pub(crate) fn action_lines(
    model: &KernelModel,
    next: Option<&NextEditionRequirements>,
    action: &ActionDef,
) -> Vec<String> {
    let Some(event) = next.and_then(|next| reading_of(model, next, action)) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if event.verb_omitted {
        lines.push(format!(
            "verb: none written; the event name {} is the verb",
            event.verb
        ));
    }
    lines.push(if action.fair {
        "fair: yes (every event but a fault event)".to_owned()
    } else {
        "fair: no (fault event)".to_owned()
    });
    for input in &event.inputs {
        let stored = input
            .stored
            .iter()
            .map(|(field, store)| {
                let why = match store {
                    InputStore::SameName => "the field of the same name".to_owned(),
                    InputStore::Set => format!("set {field} = input {}", input.name),
                    InputStore::Relation => "the relation's actor instance".to_owned(),
                };
                format!("stored in {field} ({why})")
            })
            .collect::<Vec<_>>();
        lines.push(format!(
            "input {}: {}",
            input.name,
            if stored.is_empty() {
                "not stored".to_owned()
            } else {
                stored.join(", ")
            }
        ));
    }
    lines
}

/// The readable line before an event's `requires:` block: every guard must
/// hold, and an `allow … only when` is one more guard.
pub(crate) fn guards_line(
    model: &KernelModel,
    next: Option<&NextEditionRequirements>,
    action: &ActionDef,
) -> Option<String> {
    next.and_then(|next| reading_of(model, next, action))?;
    let count = action.requires.len();
    Some(format!(
        "guards: {} below must hold (an allow … only when adds one; it allows nothing by itself)",
        if count == 1 {
            "the 1 requires".to_owned()
        } else {
            format!("all {count} requires")
        }
    ))
}

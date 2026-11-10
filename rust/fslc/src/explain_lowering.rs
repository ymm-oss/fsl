// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Where each lowered `requires` and property of a next-edition requirements
//! document came from, as `fslc explain` shows it (`--readable`, and
//! `--lowering` as JSON).
//!
//! The lowering records it on the Kernel's origins: an event's action has an
//! `event` step, its stage guard a `move` step, each guard a decision table
//! or a rule adds a `decide` / `allow` / `never` / `after` step, and each
//! property a rule or a table's `assuming` adds an `always` / `enters` /
//! `assuming` step. The details are in canonical names and English keywords;
//! `explain` respells them for the reader afterwards like the rest of its
//! output ([`crate::name_display`]). This module reads the steps and nothing
//! else, so a document in the earlier forms, whose origins carry none of
//! these steps, explains exactly as before.

use std::fmt::Write as _;
use std::path::Path;

use fsl_core::{
    KernelModel, LoweringStep, OriginChain, action_guard_target, action_target, property_target,
};
use serde_json::{Value, json};

/// The lowering steps of the next-edition requirements forms.
const STEPS: &[&str] = &[
    "event", "move", "decide", "allow", "never", "after", "always", "enters", "assuming",
];

/// The steps whose detail is a rule statement (`REQ-1 allow …`).
const RULE_STEPS: &[&str] = &["allow", "never", "after", "always", "enters"];

/// The steps a rule or a decision table adds to an event's guard.
const GUARD_STEPS: &[&str] = &["decide", "allow", "never", "after"];

/// One next-edition lowering step, with the origin site it was read from.
struct Step<'a> {
    lowering: &'a LoweringStep,
    detail: &'a str,
    file: Option<&'a str>,
    line: Option<u32>,
}

impl Step<'_> {
    /// `rule REQ-1 allow …`, `decide REQ-2-3 for …`, `move A -> B`.
    fn label(&self) -> String {
        let kind = if RULE_STEPS.contains(&self.lowering.kind.as_str()) {
            "rule"
        } else {
            self.lowering.kind.as_str()
        };
        format!("{kind} {}", self.detail)
    }

    /// ` (file.fsl:12)`, or nothing without a source line.
    fn location(&self) -> String {
        match (self.file, self.line) {
            (Some(file), Some(line)) => format!(
                " ({}:{line})",
                Path::new(file)
                    .file_name()
                    .map_or(file.into(), |name| name.to_string_lossy())
            ),
            (None, Some(line)) => format!(" (line {line})"),
            _ => String::new(),
        }
    }

    fn json(&self) -> Value {
        json!({
            "kind": self.lowering.kind,
            "detail": self.detail,
            "line": self.line,
        })
    }
}

/// The next-edition steps of the origins bound to `target`, in binding and
/// step order; `kinds` narrows them.
fn steps<'m>(model: &'m KernelModel, target: &str, kinds: &[&str]) -> Vec<Step<'m>> {
    model
        .origins()
        .origins_for(target)
        .iter()
        .flat_map(|origin: &OriginChain| {
            let site = origin.primary.as_ref();
            origin
                .lowering_steps
                .iter()
                .filter(|step| kinds.contains(&step.kind.as_str()))
                .filter_map(move |step| {
                    Some(Step {
                        lowering: step,
                        detail: step.detail.as_deref()?,
                        file: site.and_then(|site| site.source_file.as_deref()),
                        line: site.and_then(|site| site.span).map(|span| span.start.line),
                    })
                })
        })
        .collect()
}

/// The IDs of the rules and decision tables the `index`-th `requires` of
/// `action` came from: each such guard's origin is declared under the ID of
/// its rule or table. Empty for a guard of any other origin, so a document in
/// the earlier forms has none.
pub(crate) fn guard_requirements(model: &KernelModel, action: &str, index: usize) -> Vec<String> {
    model
        .origins()
        .origins_for(&action_guard_target(action, index))
        .iter()
        .filter(|origin| {
            origin
                .lowering_steps
                .iter()
                .any(|step| GUARD_STEPS.contains(&step.kind.as_str()))
        })
        .filter_map(|origin| origin.primary.as_ref()?.declaration_path.first().cloned())
        .collect()
}

/// The readable lines naming the event an action lowers from:
/// `event: System approves Claim Submitted -> Approved (claims.fsl:27)`.
pub(crate) fn action_lines(model: &KernelModel, action: &str) -> Vec<String> {
    steps(model, &action_target(action), &["event"])
        .iter()
        .map(|step| format!("event: {}{}", step.detail, step.location()))
        .collect()
}

/// The readable suffix of the action's `index`-th `requires`:
/// `  ← decide REQ-2-3 for Submitted Claim row 1: … (claims.fsl:37)`, one
/// `←` per origin; empty when it has none from the next-edition forms.
pub(crate) fn requires_suffix(model: &KernelModel, action: &str, index: usize) -> String {
    let mut suffix = String::new();
    for step in steps(model, &action_guard_target(action, index), STEPS) {
        let _ = write!(suffix, "  ← {}{}", step.label(), step.location());
    }
    suffix
}

/// The readable lines naming where the property `kind` `name` came from:
/// `← rule REQ-3 always Order in Shipped has qty > 0 (orders.fsl:29)`.
pub(crate) fn property_lines(model: &KernelModel, kind: &str, name: &str) -> Vec<String> {
    steps(model, &property_target(kind, name), STEPS)
        .iter()
        .map(|step| format!("← {}{}", step.label(), step.location()))
        .collect()
}

/// `explain --lowering`: per action, its event and the origins of each
/// `requires`; per property, its origins. Only the actions and properties
/// with a next-edition lowering step are listed.
pub(crate) fn lowering(model: &KernelModel) -> Value {
    let actions = model
        .actions
        .iter()
        .filter_map(|action| {
            let event = steps(model, &action_target(&action.name), &["event"]);
            let requires = action
                .requires
                .iter()
                .enumerate()
                .map(|(index, requirement)| {
                    json!({
                        "index": index,
                        "expr": fslc_rust::expr_text(requirement),
                        "origins": steps(model, &action_guard_target(&action.name, index), STEPS)
                            .iter()
                            .map(Step::json)
                            .collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();
            let lowered = !event.is_empty()
                || requires
                    .iter()
                    .any(|item| item["origins"].as_array().is_some_and(|o| !o.is_empty()));
            lowered.then(|| {
                json!({
                    "action": model.action_display_name(&action.name),
                    "origins": event.iter().map(Step::json).collect::<Vec<_>>(),
                    "requires": requires,
                })
            })
        })
        .collect::<Vec<_>>();
    let mut named = Vec::new();
    named.extend(model.invariants.iter().map(|p| ("invariant", &p.name)));
    named.extend(model.transitions.iter().map(|p| ("trans", &p.name)));
    named.extend(model.reachables.iter().map(|p| ("reachable", &p.name)));
    named.extend(model.leadstos.iter().map(|p| ("leadsTo", &p.name)));
    let properties = named
        .into_iter()
        .filter_map(|(kind, name)| {
            let origins = steps(model, &property_target(kind, name), STEPS);
            (!origins.is_empty()).then(|| {
                json!({
                    "kind": kind,
                    "name": model.names().public_property(name).into_string(),
                    "origins": origins.iter().map(Step::json).collect::<Vec<_>>(),
                })
            })
        })
        .collect::<Vec<_>>();
    json!({"actions": actions, "properties": properties})
}

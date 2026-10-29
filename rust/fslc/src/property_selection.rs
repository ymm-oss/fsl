// SPDX-License-Identifier: Apache-2.0

//! Which properties a verification run checks (`--property`,
//! `--exclude-property`), the bounds they imply, and the `--vacuity` mode.

use fsl_core::KernelModel;
use serde_json::{Value, json};

use crate::cli_output::envelope;

pub(crate) fn is_bounds_name(name: &str) -> bool {
    fsl_core::SyntheticName::parse_internal(name)
        .is_some_and(|synthetic| synthetic.bounds_state().is_some())
}

pub(crate) fn select_properties(
    model: &mut KernelModel,
    selected: Option<&str>,
    excluded: &[String],
) -> Result<(), String> {
    let names = model.names().clone();
    let public = |name: &str| names.public_property(name).into_string();
    let available = model
        .state
        .iter()
        .filter(|(_, ty)| model.type_has_bounds(ty))
        .map(|(name, _)| {
            let bound = fsl_core::SyntheticName::Bounds { state: name };
            names.public_synthetic(&bound).into_string()
        })
        .chain(model.invariants.iter().map(|item| public(&item.name)))
        .chain(model.transitions.iter().map(|item| public(&item.name)))
        .chain(model.leadstos.iter().map(|item| public(&item.name)))
        .chain(model.reachables.iter().map(|item| public(&item.name)))
        .collect::<Vec<_>>();
    if let Some(name) = selected {
        if !available.iter().any(|candidate| candidate == name) {
            let mut display_available = available.clone();
            display_available.sort();
            display_available.dedup();
            return Err(format!(
                "no such property: {name} (available: {})",
                display_available.join(", ")
            ));
        }
        model.invariants.retain(|item| public(&item.name) == name);
        model.transitions.retain(|item| public(&item.name) == name);
        model.leadstos.retain(|item| public(&item.name) == name);
        model.reachables.retain(|item| public(&item.name) == name);
    }
    if !excluded.is_empty() {
        let selected_available = model
            .invariants
            .iter()
            .map(|item| public(&item.name))
            .chain(model.transitions.iter().map(|item| public(&item.name)))
            .chain(model.leadstos.iter().map(|item| public(&item.name)))
            .chain(model.reachables.iter().map(|item| public(&item.name)))
            .collect::<Vec<_>>();
        let mut missing = excluded
            .iter()
            .filter(|name| {
                !selected_available
                    .iter()
                    .any(|candidate| candidate == *name)
            })
            .cloned()
            .collect::<Vec<_>>();
        missing.sort();
        missing.dedup();
        if !missing.is_empty() {
            return Err(format!(
                "no such property: {} (available: {})",
                missing.join(", "),
                selected_available.join(", ")
            ));
        }
        model
            .invariants
            .retain(|item| !excluded.contains(&public(&item.name)));
        model
            .transitions
            .retain(|item| !excluded.contains(&public(&item.name)));
        model
            .leadstos
            .retain(|item| !excluded.contains(&public(&item.name)));
        model
            .reachables
            .retain(|item| !excluded.contains(&public(&item.name)));
    }
    Ok(())
}

pub(crate) fn selected_implicit_bounds(
    model: &KernelModel,
    selected: Option<&str>,
    excluded: &[String],
) -> Option<std::collections::BTreeSet<String>> {
    if selected.is_none() && !excluded.iter().any(|name| is_bounds_name(name)) {
        return None;
    }
    let mut bounds = model
        .state
        .iter()
        .filter(|(_, ty)| model.type_has_bounds(ty))
        .map(|(name, _)| fsl_core::SyntheticName::Bounds { state: name }.internal())
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(selected) = selected {
        bounds.retain(|name| model.names().public_violation(name, None).into_string() == selected);
    }
    bounds.retain(|name| {
        !excluded.contains(&model.names().public_violation(name, None).into_string())
    });
    Some(bounds)
}

pub(crate) fn apply_vacuity_mode(output: &mut Value, mode: &str) -> Option<i32> {
    let findings = output
        .get("warnings")
        .and_then(Value::as_array)
        .map(|warnings| {
            warnings
                .iter()
                .filter(|warning| {
                    warning
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_some_and(fsl_core::is_vacuity_kind)
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if mode == "ignore" {
        if let Some(warnings) = output.get_mut("warnings").and_then(Value::as_array_mut) {
            warnings.retain(|warning| {
                !warning
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(fsl_core::is_vacuity_kind)
            });
        }
        return None;
    }
    if mode != "error" || findings.is_empty() {
        return None;
    }
    let mut error = envelope();
    error.insert("result".to_owned(), json!("error"));
    if let Some(spec) = output.get("spec") {
        error.insert("spec".to_owned(), spec.clone());
    }
    error.insert(
        "kind".to_owned(),
        findings[0]
            .get("kind")
            .cloned()
            .unwrap_or_else(|| json!("vacuous")),
    );
    error.insert("findings".to_owned(), Value::Array(findings));
    if let Some(checked) = output.get("checked_to_depth") {
        error.insert("checked_to_depth".to_owned(), checked.clone());
    }
    if let Some(cost) = output.get("cost") {
        error.insert("cost".to_owned(), cost.clone());
    }
    error.insert("trace_type".to_owned(), json!("vacuity"));
    *output = Value::Object(error);
    Some(2)
}

pub(crate) fn invariant_names(model: &KernelModel) -> Vec<String> {
    invariant_names_selected(model, None)
}

pub(crate) use fslc_rust::verification_output::invariant_names_selected;

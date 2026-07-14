// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_syntax::{Annotations, MetaTag};
use serde_json::{Map, Value, json};

use crate::{KernelModel, TypeRef, display_name};

#[must_use]
pub fn model_warnings(model: &KernelModel) -> Vec<Value> {
    let mut warnings = model
        .state
        .iter()
        .filter(|(_, ty)| {
            matches!(ty, TypeRef::Map(key, _) if matches!(key.as_ref(), TypeRef::Int))
        })
        .map(|(name, _)| {
            json!({
                "message": format!("Map<Int, ...> on '{}' is deprecated; use a bounded domain type as key", display_name(name)),
                "hint": "declare `type K = 0..<max>` and use `Map<K, ...>`",
            })
        })
        .collect::<Vec<_>>();
    if model.invariants.is_empty()
        && model.transitions.is_empty()
        && model.reachables.is_empty()
        && model.leadstos.is_empty()
    {
        warnings.push(json!({
            "message": "spec declares no user invariants (only implicit type bounds are checked)",
        }));
    }
    warnings
}

/// Return the deterministic requirement projection for checked annotations.
///
/// # Panics
///
/// Panics only when passed annotations that bypassed checked-model validation.
#[must_use]
pub fn requirement_metadata(annotations: &Annotations, legacy: Option<&MetaTag>) -> Vec<Value> {
    let mut requirements = annotations
        .requirements()
        .expect("checked model annotations are valid")
        .into_iter()
        .map(|requirement| json!({"id":requirement.id,"text":requirement.text}))
        .collect::<Vec<_>>();
    if requirements.is_empty()
        && let Some(meta) = legacy.filter(|meta| !meta.id.eq_ignore_ascii_case("undecided"))
    {
        requirements.push(json!({"id":meta.id,"text":meta.text}));
    }
    requirements
}

/// Add singular and plural requirement projections to a diagnostic object.
///
/// # Panics
///
/// Panics only when passed annotations that bypassed checked-model validation.
pub fn insert_requirement_metadata(
    output: &mut Map<String, Value>,
    annotations: &Annotations,
    legacy: Option<&MetaTag>,
) {
    let requirements = requirement_metadata(annotations, legacy);
    if let Some(first) = requirements.first() {
        output.insert("requirement".to_owned(), first.clone());
        output.insert("requirements".to_owned(), Value::Array(requirements));
    }
}

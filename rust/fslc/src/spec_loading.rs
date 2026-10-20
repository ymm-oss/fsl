// SPDX-License-Identifier: Apache-2.0

//! Reading a spec into a checked model, the state snapshots that seed a run,
//! and the checked-document validations that run on the same source.

use std::path::Path;

use fsl_core::{FslValue, KernelModel, KernelSpec, TypeDef, TypeRef};
use fslc_rust::spec_load::{SemanticDiagnostic, SpecLoadError, kernel_load_error};
use fslc_rust::verification_output::{DiagnosticClass, TraceGate};
use serde_json::{Map, Value};

use crate::cli_options::ScopeBounds;
use crate::cli_output::envelope;

pub(crate) fn validate_specialized_document_from_source(
    path: &Path,
    source: &str,
) -> Result<(), String> {
    fslc_rust::check_stages::validate_specialized_document(
        source,
        &path.display().to_string(),
        crate::process::parse_options(),
    )
}

pub(crate) fn validate_requirement_traces_from_source(
    _path: &Path,
    source: &str,
    model: &KernelModel,
) -> Result<TraceGate, String> {
    validate_requirement_trace_source(source, model)
}

pub(crate) fn validate_requirement_trace_source(
    source: &str,
    model: &KernelModel,
) -> Result<TraceGate, String> {
    fslc_rust::verification_output::validate_requirement_trace_source(
        &envelope(),
        source,
        model,
        crate::process::parse_options(),
    )
}

pub(crate) fn load_model(path: &Path) -> Result<KernelModel, SpecLoadError> {
    load_kernel_model(path).map(|(_, _, model)| model)
}

pub(crate) fn load_model_from_source(
    path: &Path,
    source: &str,
) -> Result<KernelModel, SpecLoadError> {
    load_kernel_model_from_source(path, source).map(|(_, model)| model)
}

pub(crate) fn load_kernel_model(
    path: &Path,
) -> Result<(String, KernelSpec, KernelModel), SpecLoadError> {
    let source = read_spec_source(path)?;
    let (kernel, model) = load_kernel_model_from_source(path, &source)?;
    Ok((source, kernel, model))
}

/// Build a checked Kernel/model from a caller-owned root-source snapshot.
///
/// `load_kernel_model` remains the path-reading compatibility entry point;
/// callers that already captured their source — whether to avoid a second
/// root-path read or because they also need a specialized surface projection
/// — use this variant. The resolver can still read imported dependencies.
pub(crate) fn load_kernel_model_from_source(
    path: &Path,
    source: &str,
) -> Result<(KernelSpec, KernelModel), SpecLoadError> {
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    load_kernel_model_from_source_with_resolver(path, source, &resolver)
}

/// `load_kernel_model_from_source` with a caller-owned resolver, so a
/// cache-key computation can observe the dependency reads (issue #1023) by
/// passing a recording resolver instead of a plain `FsResolver`.
pub(crate) fn load_kernel_model_from_source_with_resolver(
    path: &Path,
    source: &str,
    resolver: &dyn fsl_core::FileResolver,
) -> Result<(KernelSpec, KernelModel), SpecLoadError> {
    fslc_rust::check_stages::load_kernel_model(
        source,
        &path.to_string_lossy(),
        resolver,
        crate::process::parse_options(),
    )
}

/// Read a spec file, classifying a read failure as `io` rather than letting the
/// message-string classifier fall through to `semantics` (issue 497: a missing
/// single analyze input reported `semantics` while the same missing file in a
/// batch reported `io`).
pub(crate) fn read_spec_source(path: &Path) -> Result<String, SpecLoadError> {
    std::fs::read_to_string(path)
        .map_err(|error| SpecLoadError::Io(format!("{}: {error}", path.display())))
}

#[allow(clippy::too_many_lines)]
pub(crate) fn snapshot_value(
    model: &KernelModel,
    ty: &TypeRef,
    value: &Value,
    path: &str,
) -> Result<FslValue, String> {
    let type_error = || format!("{path} has a value incompatible with {ty:?}");
    match ty {
        TypeRef::Bool => value.as_bool().map(FslValue::Bool).ok_or_else(type_error),
        TypeRef::Int => value.as_i64().map(FslValue::Int).ok_or_else(type_error),
        TypeRef::Range(lo, hi) => {
            let number = value.as_i64().ok_or_else(type_error)?;
            if number < *lo || number > *hi {
                return Err(format!(
                    "{path} value {number} is out of range [{lo}..{hi}]"
                ));
            }
            Ok(FslValue::Int(number))
        }
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => {
                snapshot_value(model, &TypeRef::Range(*lo, *hi), value, path)
            }
            Some(TypeDef::Enum { members, .. }) => {
                let member = value.as_str().ok_or_else(type_error)?;
                if !members.iter().any(|candidate| candidate == member) {
                    return Err(format!(
                        "{path} enum member '{member}' is not one of {}",
                        members.join(", ")
                    ));
                }
                Ok(FslValue::Enum {
                    type_name: name.clone(),
                    member: member.to_owned(),
                })
            }
            Some(TypeDef::Struct { fields }) => {
                let object = value.as_object().ok_or_else(type_error)?;
                for key in object.keys() {
                    if !fields.iter().any(|(field, _)| field == key) {
                        return Err(format!("unknown struct field '{path}.{key}'"));
                    }
                }
                let parsed = fields
                    .iter()
                    .map(|(field, field_ty)| {
                        let field_value = object
                            .get(field)
                            .ok_or_else(|| format!("missing struct field '{path}.{field}'"))?;
                        Ok((
                            field.clone(),
                            snapshot_value(
                                model,
                                field_ty,
                                field_value,
                                &format!("{path}.{field}"),
                            )?,
                        ))
                    })
                    .collect::<Result<_, String>>()?;
                Ok(FslValue::Struct {
                    type_name: name.clone(),
                    fields: parsed,
                })
            }
            None => Err(format!("unknown type '{name}'")),
        },
        TypeRef::Option(inner) => {
            if value.is_null() {
                Ok(FslValue::None)
            } else if matches!(inner.as_ref(), TypeRef::Option(_)) {
                let object = value.as_object().ok_or_else(|| {
                    format!(
                        "{path} nested Option must be null or {{\"kind\":\"some\",\"value\":...}}"
                    )
                })?;
                if object.len() != 2
                    || object.get("kind") != Some(&Value::String("some".to_owned()))
                    || !object.contains_key("value")
                {
                    return Err(format!(
                        "{path} nested Option must be null or {{\"kind\":\"some\",\"value\":...}}"
                    ));
                }
                Ok(FslValue::Some(Box::new(snapshot_value(
                    model,
                    inner,
                    object.get("value").expect("checked value key"),
                    path,
                )?)))
            } else {
                Ok(FslValue::Some(Box::new(snapshot_value(
                    model, inner, value, path,
                )?)))
            }
        }
        TypeRef::Map(key_ty, value_ty) => {
            let object = value.as_object().ok_or_else(type_error)?;
            let keys = model
                .map_key_values(key_ty)
                .map_err(|error| error.to_string())?;
            let mut entries = std::collections::BTreeMap::new();
            for key in keys {
                let rendered = match &key {
                    FslValue::Int(value) => value.to_string(),
                    FslValue::Bool(value) => value.to_string(),
                    FslValue::Enum { member, .. } => member.clone(),
                    _ => return Err(format!("{path} has a non-scalar map key")),
                };
                let entry = object
                    .get(&rendered)
                    .ok_or_else(|| format!("missing map key '{path}[{rendered}]'"))?;
                entries.insert(
                    key,
                    snapshot_value(model, value_ty, entry, &format!("{path}[{rendered}]"))?,
                );
            }
            if object.len() != entries.len() {
                return Err(format!("{path} contains an unknown map key"));
            }
            Ok(FslValue::Map(entries))
        }
        TypeRef::Set(element_ty) => {
            let items = value.as_array().ok_or_else(type_error)?;
            Ok(FslValue::Set(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        snapshot_value(model, element_ty, item, &format!("{path}[{index}]"))
                    })
                    .collect::<Result<_, _>>()?,
            ))
        }
        TypeRef::Seq(element_ty, capacity) => {
            let items = value.as_array().ok_or_else(type_error)?;
            if items.len() > *capacity {
                return Err(format!(
                    "{path} sequence length {} exceeds capacity {capacity}",
                    items.len()
                ));
            }
            Ok(FslValue::Seq(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        snapshot_value(model, element_ty, item, &format!("{path}[{index}]"))
                    })
                    .collect::<Result<_, _>>()?,
            ))
        }
        TypeRef::Relation(source_ty, target_ty) => {
            let items = value.as_array().ok_or_else(type_error)?;
            Ok(FslValue::Relation(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let pair = item
                            .as_array()
                            .filter(|pair| pair.len() == 2)
                            .ok_or_else(|| format!("{path}[{index}] must be a pair"))?;
                        Ok((
                            snapshot_value(
                                model,
                                source_ty,
                                &pair[0],
                                &format!("{path}[{index}][0]"),
                            )?,
                            snapshot_value(
                                model,
                                target_ty,
                                &pair[1],
                                &format!("{path}[{index}][1]"),
                            )?,
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            ))
        }
    }
}

pub(crate) fn load_state_snapshot(
    path: &Path,
    model: &KernelModel,
) -> Result<std::collections::BTreeMap<String, FslValue>, (String, String)> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        (
            "io".to_owned(),
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("file not found: {}", path.display())
            } else {
                error.to_string()
            },
        )
    })?;
    let value: Value = serde_json::from_str(&source).map_err(|error| {
        (
            "io".to_owned(),
            format!(
                "invalid state JSON at line {}, column {}: {}",
                error.line(),
                error.column(),
                error
            ),
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        (
            "type".to_owned(),
            "state snapshot must be a JSON object".to_owned(),
        )
    })?;
    load_snapshot_value_object(object, model).map_err(|error| ("type".to_owned(), error))
}

pub(crate) fn load_snapshot_value_object(
    object: &Map<String, Value>,
    model: &KernelModel,
) -> Result<std::collections::BTreeMap<String, FslValue>, String> {
    for key in object.keys() {
        if !model
            .state
            .iter()
            .any(|(name, _)| model.names().public_state(name).into_string() == *key)
        {
            return Err(format!("unknown state variable '{key}'"));
        }
    }
    model
        .state
        .iter()
        .map(|(name, ty)| {
            let display_name = model.names().public_state(name).into_string();
            let value = object
                .get(&display_name)
                .ok_or_else(|| format!("missing state variable '{display_name}'"))?;
            Ok((
                name.clone(),
                snapshot_value(model, ty, value, &format!("state.{display_name}"))?,
            ))
        })
        .collect()
}

pub(crate) fn load_model_scoped(
    path: &Path,
    scope: &ScopeBounds,
) -> Result<KernelModel, SpecLoadError> {
    let source = read_spec_source(path)?;
    load_model_scoped_from_source(path, &source, scope)
}

pub(crate) fn load_model_scoped_from_source(
    _path: &Path,
    source: &str,
    scope: &ScopeBounds,
) -> Result<KernelModel, SpecLoadError> {
    let kernel = match fsl_core::parse_kernel_source_with_bounds_with_options(
        source,
        &scope.instances,
        &scope.values,
        crate::process::parse_options(),
    ) {
        Ok(kernel) => kernel,
        // A rejected `--instances`/`--values` bound is a CLI argument
        // defect, not a construct in the spec, so it owns no location.
        Err(error) if error.kind == fsl_core::CoreErrorKind::ScopeOverrideWithoutBounds => {
            return Err(SpecLoadError::Semantic(Box::new(SemanticDiagnostic {
                class: DiagnosticClass::Core(error.kind),
                diagnostic_code: error.diagnostic_code,
                ..SemanticDiagnostic::unlocated(error.message)
            })));
        }
        Err(error) => {
            return Err(kernel_load_error(
                source,
                &error,
                crate::process::parse_options(),
            ));
        }
    };
    fsl_core::build_model(kernel).map_err(|error| {
        SpecLoadError::Semantic(Box::new(SemanticDiagnostic::from_model_error(&error)))
    })
}

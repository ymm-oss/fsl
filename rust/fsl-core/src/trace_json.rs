// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};

use crate::{FslValue, KernelModel, OriginChain, OriginSite, TraceStep, TypeRef};

#[must_use]
pub fn display_name(name: &str) -> String {
    name.replacen("__", ".", 1).replace("QqDbSepqQ", "__")
}

fn origin_site_json(site: &OriginSite) -> Value {
    let span = site.span.map(|span| {
        json!({
            "start": {
                "offset": span.start.offset,
                "line": span.start.line,
                "column": span.start.column,
            },
            "end": {
                "offset": span.end.offset,
                "line": span.end.line,
                "column": span.end.column,
            },
        })
    });
    json!({
        "source_file": site.source_file,
        "span": span,
        "dialect": site.dialect,
        "declaration_path": site.declaration_path,
    })
}

#[must_use]
pub fn internal_origin_json(origin: &OriginChain) -> Value {
    json!({
        "identity": origin.id.0,
        "dialect": origin.dialect,
        "primary": origin.primary.as_ref().map(origin_site_json),
        "secondary": origin.secondary.iter().map(origin_site_json).collect::<Vec<_>>(),
        "lowering_steps": origin.lowering_steps.iter().map(|step| json!({
            "kind": step.kind,
            "detail": step.detail,
        })).collect::<Vec<_>>(),
        "generated": origin.generated,
    })
}

#[must_use]
pub fn origin_display_name(origin: &OriginChain) -> Option<&str> {
    // Database lowering now has truthful source origins, but its public action
    // and property names predate source-backed display-name substitution. Keep
    // those executable/replay identities stable while still publishing the
    // origin chain and authored locations.
    if origin.dialect == "dbsystem" {
        return None;
    }
    origin
        .primary
        .as_ref()
        .and_then(|site| site.declaration_path.last())
        .map(String::as_str)
}

fn map_key(value: &FslValue) -> String {
    match value {
        FslValue::Int(value) => value.to_string(),
        FslValue::Bool(value) => value.to_string(),
        FslValue::Enum { member, .. } => member.clone(),
        _ => format!("{value:?}"),
    }
}

#[must_use]
pub fn fsl_value_json(value: &FslValue) -> Value {
    match value {
        FslValue::Int(value) => json!(value),
        FslValue::Bool(value) => json!(value),
        FslValue::Enum { member, .. } => json!(member),
        FslValue::None => Value::Null,
        // State values cannot contain an Option around a struct or collection,
        // so an untyped value can identify a nested Option from its payload.
        // Keep the legacy scalar bytes, but retain the outer presence bit when
        // that payload is itself an Option.
        FslValue::Some(value) if matches!(value.as_ref(), FslValue::None | FslValue::Some(_)) => {
            json!({"kind":"some", "value":fsl_value_json(value)})
        }
        FslValue::Some(value) => fsl_value_json(value),
        FslValue::Struct { fields, .. } => Value::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), fsl_value_json(value)))
                .collect(),
        ),
        FslValue::Map(entries) => Value::Object(
            entries
                .iter()
                .map(|(key, value)| (map_key(key), fsl_value_json(value)))
                .collect(),
        ),
        FslValue::Set(values) => Value::Array(values.iter().map(fsl_value_json).collect()),
        FslValue::Seq(values) => Value::Array(values.iter().map(fsl_value_json).collect()),
        FslValue::Relation(values) => Value::Array(
            values
                .iter()
                .map(|(source, target)| json!([fsl_value_json(source), fsl_value_json(target)]))
                .collect(),
        ),
    }
}

#[must_use]
pub fn state_json(state: &BTreeMap<String, FslValue>) -> Value {
    Value::Object(
        state
            .iter()
            .map(|(name, value)| (display_name(name), fsl_value_json(value)))
            .collect(),
    )
}

#[must_use]
pub fn state_summary(model: &KernelModel, state: &BTreeMap<String, FslValue>) -> String {
    model
        .state
        .iter()
        .filter_map(|(name, ty)| {
            state
                .get(name)
                .map(|value| format!("{}={}", display_name(name), format_value(model, value, ty)))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_value(model: &KernelModel, value: &FslValue, ty: &TypeRef) -> String {
    match value {
        FslValue::Int(value) => value.to_string(),
        FslValue::Bool(value) => value.to_string(),
        FslValue::Enum { member, .. } => member.clone(),
        FslValue::None => "none".to_owned(),
        FslValue::Some(value) => format!(
            "some({})",
            format_value(
                model,
                value,
                match ty {
                    TypeRef::Option(inner) => inner,
                    _ => ty,
                },
            )
        ),
        FslValue::Struct { type_name, fields } => {
            let declared = model.struct_fields(type_name).unwrap_or(&[]);
            format!(
                "{{{}}}",
                declared
                    .iter()
                    .filter_map(|(name, field_ty)| fields
                        .get(name)
                        .map(|value| format!("{name}: {}", format_value(model, value, field_ty))))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        FslValue::Map(entries) => {
            let (key_ty, value_ty) = match ty {
                TypeRef::Map(key, value) => (key.as_ref(), value.as_ref()),
                _ => (ty, ty),
            };
            format!(
                "{{{}}}",
                entries
                    .iter()
                    .map(|(key, value)| format!(
                        "{}: {}",
                        format_value(model, key, key_ty),
                        format_value(model, value, value_ty)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        FslValue::Set(values) => {
            let inner = match ty {
                TypeRef::Set(inner) => inner.as_ref(),
                _ => ty,
            };
            format!(
                "[{}]",
                values
                    .iter()
                    .map(|value| format_value(model, value, inner))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        FslValue::Seq(values) => {
            let inner = match ty {
                TypeRef::Seq(inner, _) => inner.as_ref(),
                _ => ty,
            };
            format!(
                "[{}]",
                values
                    .iter()
                    .map(|value| format_value(model, value, inner))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        FslValue::Relation(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|(source, target)| format!(
                    "[{}, {}]",
                    fsl_value_json(source),
                    fsl_value_json(target)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[must_use]
pub fn trace_json(model: &KernelModel, trace: &[TraceStep]) -> Value {
    Value::Array(
        trace
            .iter()
            .map(|entry| {
                let mut value = Map::new();
                value.insert("step".to_owned(), json!(entry.step));
                value.insert("state".to_owned(), state_json(&entry.state));
                if let Some(action) = &entry.action {
                    let mut action_json = Map::new();
                    let origin = model.action_origin(&action.name);
                    action_json.insert(
                        "name".to_owned(),
                        json!(
                            origin
                                .and_then(origin_display_name)
                                .map_or_else(|| display_name(&action.name), str::to_owned)
                        ),
                    );
                    if let Some(origin) = origin {
                        action_json.insert(
                            "generated_name".to_owned(),
                            json!(display_name(&action.name)),
                        );
                        action_json.insert("origin".to_owned(), internal_origin_json(origin));
                    }
                    action_json.insert(
                        "params".to_owned(),
                        Value::Object(
                            action
                                .params
                                .iter()
                                .map(|(name, value)| (name.clone(), fsl_value_json(value)))
                                .collect(),
                        ),
                    );
                    if let Some(definition) = model
                        .actions
                        .iter()
                        .find(|definition| definition.name == action.name)
                    {
                        action_json.insert("loc".to_owned(), definition.span.python_loc());
                    }
                    value.insert("action".to_owned(), Value::Object(action_json));
                    value.insert(
                        "changes".to_owned(),
                        trace.get(entry.step.saturating_sub(1)).map_or_else(
                            || Value::Object(Map::new()),
                            |previous| {
                                Value::Object(compute_changes(&previous.state, &entry.state))
                            },
                        ),
                    );
                }
                Value::Object(value)
            })
            .collect(),
    )
}

fn compute_changes(
    previous: &BTreeMap<String, FslValue>,
    current: &BTreeMap<String, FslValue>,
) -> Map<String, Value> {
    fn insert_change(
        path: &str,
        previous: Option<&FslValue>,
        current: Option<&FslValue>,
        out: &mut Map<String, Value>,
    ) {
        out.insert(
            path.to_owned(),
            json!({
                "from": previous.map_or(Value::Null, fsl_value_json),
                "to": current.map_or(Value::Null, fsl_value_json),
            }),
        );
    }

    fn walk(
        path: &str,
        previous: Option<&FslValue>,
        current: Option<&FslValue>,
        out: &mut Map<String, Value>,
    ) {
        if previous == current {
            return;
        }

        // Option values are atomic public values. In particular, do not
        // descend into the canonical {kind,value} encoding: those fields can
        // also occur in a legal struct and are not logical state paths.
        if matches!(previous, Some(FslValue::None | FslValue::Some(_)))
            || matches!(current, Some(FslValue::None | FslValue::Some(_)))
        {
            insert_change(path, previous, current, out);
            return;
        }

        if let (
            Some(FslValue::Struct {
                fields: previous, ..
            }),
            Some(FslValue::Struct {
                fields: current, ..
            }),
        ) = (previous, current)
        {
            let keys = previous
                .keys()
                .chain(current.keys())
                .collect::<BTreeSet<_>>();
            for key in keys {
                walk(
                    &format!("{path}[{key}]"),
                    previous.get(key),
                    current.get(key),
                    out,
                );
            }
            return;
        }

        if let (Some(FslValue::Map(previous)), Some(FslValue::Map(current))) = (previous, current) {
            let keys = previous
                .keys()
                .chain(current.keys())
                .collect::<BTreeSet<_>>();
            for key in keys {
                walk(
                    &format!("{path}[{}]", map_key(key)),
                    previous.get(key),
                    current.get(key),
                    out,
                );
            }
            return;
        }

        insert_change(path, previous, current, out);
    }

    let mut changes = Map::new();
    let keys = previous
        .keys()
        .chain(current.keys())
        .collect::<BTreeSet<_>>();
    for key in keys {
        walk(
            &display_name(key),
            previous.get(key),
            current.get(key),
            &mut changes,
        );
    }
    changes
}

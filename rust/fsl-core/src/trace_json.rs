// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::{FslValue, KernelModel, OriginChain, OriginSite, TraceStep};

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
                                Value::Object(compute_changes(
                                    &state_json(&previous.state),
                                    &state_json(&entry.state),
                                ))
                            },
                        ),
                    );
                }
                Value::Object(value)
            })
            .collect(),
    )
}

fn compute_changes(previous: &Value, current: &Value) -> Map<String, Value> {
    fn walk(path: &str, previous: &Value, current: &Value, out: &mut Map<String, Value>) {
        if previous == current {
            return;
        }
        if let (Value::Object(previous), Value::Object(current)) = (previous, current) {
            let mut keys = previous.keys().chain(current.keys()).collect::<Vec<_>>();
            keys.sort_unstable();
            keys.dedup();
            for key in keys {
                let next = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}[{key}]")
                };
                walk(
                    &next,
                    previous.get(key).unwrap_or(&Value::Null),
                    current.get(key).unwrap_or(&Value::Null),
                    out,
                );
            }
        } else if !path.is_empty() {
            out.insert(path.to_owned(), json!({"from": previous, "to": current}));
        }
    }

    let mut changes = Map::new();
    walk("", previous, current, &mut changes);
    changes
}

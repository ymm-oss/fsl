// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::Read;

use fsl_core::{FslValue, KernelModel, ParamDef, TypeDef, TypeRef};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Event {
    action: String,
    params: BTreeMap<String, Value>,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: fsl-replay-actions SPEC < events.json");
    match run(&path) {
        Ok(value) => println!(
            "{}",
            serde_json::to_string(&value).expect("serialize replay result")
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

fn run(path: &str) -> Result<Value, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())?;
    let events = serde_json::from_str::<Vec<Event>>(&input).map_err(|error| error.to_string())?;
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let base = std::path::Path::new(path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel =
        fsl_core::parse_kernel_source(&source, &resolver).map_err(|error| error.to_string())?;
    let model = fsl_core::build_model(kernel).map_err(|error| error.to_string())?;
    let mut monitor =
        fsl_runtime::Monitor::new(model.clone()).map_err(|error| error.to_string())?;
    let mut violation = monitor
        .current_violation()
        .map_err(|error| error.to_string())?;
    for event in events {
        let action = model
            .actions
            .iter()
            .find(|action| action.name == event.action)
            .ok_or_else(|| format!("unknown action '{}'", event.action))?;
        let params = action
            .params
            .iter()
            .map(|param| {
                let value = event
                    .params
                    .get(param.name())
                    .ok_or_else(|| format!("missing parameter '{}'", param.name()))?;
                Ok((param.name().to_owned(), parse_param(&model, param, value)?))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        if params.len() != event.params.len() {
            return Err(format!(
                "unexpected parameter for action '{}'",
                event.action
            ));
        }
        let enabled = monitor.enabled().map_err(|error| error.to_string())?;
        let instance = enabled
            .iter()
            .find(|instance| instance.action == event.action && instance.params == params)
            .ok_or_else(|| format!("action '{}' is not enabled", event.action))?;
        let result = monitor.step(instance).map_err(|error| error.to_string())?;
        violation = result.violation;
    }
    Ok(json!({
        "state": state_json(&monitor.state),
        "violation": violation.map(|violation| json!({
            "kind": violation.kind,
            "name": violation.name,
            "step": violation.step,
        })),
    }))
}

fn parse_param(model: &KernelModel, param: &ParamDef, value: &Value) -> Result<FslValue, String> {
    match param {
        ParamDef::Range { .. } => value
            .as_i64()
            .map(FslValue::Int)
            .ok_or_else(|| format!("parameter '{}' must be an integer", param.name())),
        ParamDef::Typed { ty, .. } => parse_typed_param(model, ty, value, param.name()),
    }
}

fn parse_typed_param(
    model: &KernelModel,
    ty: &TypeRef,
    value: &Value,
    name: &str,
) -> Result<FslValue, String> {
    match ty {
        TypeRef::Bool => value
            .as_bool()
            .map(FslValue::Bool)
            .or_else(|| value.as_i64().map(|value| FslValue::Bool(value != 0)))
            .ok_or_else(|| format!("parameter '{name}' must be Boolean")),
        TypeRef::Int | TypeRef::Range(_, _) => value
            .as_i64()
            .map(FslValue::Int)
            .ok_or_else(|| format!("parameter '{name}' must be an integer")),
        TypeRef::Named(type_name) => match model.types.get(type_name) {
            Some(TypeDef::Domain { .. }) => value
                .as_i64()
                .map(FslValue::Int)
                .ok_or_else(|| format!("parameter '{name}' must be an integer")),
            Some(TypeDef::Enum { members, .. }) => {
                let member = value
                    .as_str()
                    .ok_or_else(|| format!("parameter '{name}' must be an enum member"))?;
                if !members.iter().any(|candidate| candidate == member) {
                    return Err(format!("unknown enum member '{member}'"));
                }
                Ok(FslValue::Enum {
                    type_name: type_name.clone(),
                    member: member.to_owned(),
                })
            }
            Some(TypeDef::Struct { .. }) | None => {
                Err(format!("parameter '{name}' has unsupported type"))
            }
        },
        _ => Err(format!("parameter '{name}' has non-scalar type")),
    }
}

fn state_json(state: &BTreeMap<String, FslValue>) -> Value {
    Value::Object(
        state
            .iter()
            .map(|(name, value)| (name.clone(), fsl_value_json(value)))
            .collect(),
    )
}

fn fsl_value_json(value: &FslValue) -> Value {
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

fn map_key(value: &FslValue) -> String {
    match value {
        FslValue::Int(value) => value.to_string(),
        FslValue::Bool(value) => value.to_string(),
        FslValue::Enum { member, .. } => member.clone(),
        _ => format!("{value:?}"),
    }
}

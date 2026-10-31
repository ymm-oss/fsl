// SPDX-License-Identifier: Apache-2.0

//! Implementation-log helpers: JSON log mappings, JSONL records, and the
//! action and parameter matching a log replay applies.

use std::path::Path;

use fsl_core::{FslValue, KernelExpr, KernelModel, ParamDef, TypeDef, TypeRef};
use serde_json::{Map, Value, json};

#[allow(clippy::too_many_lines)]
pub(crate) fn mapping_json_expr(
    expr: &KernelExpr,
    raw: &Map<String, Value>,
    bindings: &Map<String, Value>,
    model: &KernelModel,
) -> Result<Value, String> {
    match expr {
        KernelExpr::Num(value) => Ok(json!(value)),
        KernelExpr::Bool(value) => Ok(json!(value)),
        KernelExpr::None => Ok(Value::Null),
        KernelExpr::Some(value) => mapping_json_expr(value, raw, bindings, model),
        KernelExpr::Var(name) => bindings
            .get(name)
            .or_else(|| raw.get(name))
            .cloned()
            .or_else(|| model.enum_members.get(name).map(fslc_rust::fsl_value_json))
            .ok_or_else(|| format!("mapped state is missing '{name}'")),
        KernelExpr::Field(value, field) => mapping_json_expr(value, raw, bindings, model)?
            .as_object()
            .and_then(|object| object.get(field))
            .cloned()
            .ok_or_else(|| format!("mapped state is missing field '{field}'")),
        KernelExpr::Index(value, index) => {
            let value = mapping_json_expr(value, raw, bindings, model)?;
            let index = mapping_json_expr(index, raw, bindings, model)?;
            if let Some(object) = value.as_object() {
                let key = index.as_str().map_or_else(
                    || {
                        index.as_i64().map_or_else(
                            || index.as_bool().map(|value| value.to_string()),
                            |value| Some(value.to_string()),
                        )
                    },
                    |value| Some(value.to_owned()),
                );
                return key
                    .and_then(|key| object.get(&key).cloned())
                    .ok_or_else(|| "mapped state is missing indexed key".to_owned());
            }
            if let (Some(array), Some(index)) = (value.as_array(), index.as_u64()) {
                return array
                    .get(usize::try_from(index).map_err(|error| error.to_string())?)
                    .cloned()
                    .ok_or_else(|| "mapped array index is out of range".to_owned());
            }
            Err("indexed mapping expression requires an object or array".to_owned())
        }
        KernelExpr::Binary { op, left, right } => {
            let left = mapping_json_expr(left, raw, bindings, model)?;
            let right = mapping_json_expr(right, raw, bindings, model)?;
            match op.as_str() {
                "+" | "-" | "*" | "/" | "%" => {
                    let left = left
                        .as_i64()
                        .ok_or_else(|| "mapping arithmetic requires integers".to_owned())?;
                    let right = right
                        .as_i64()
                        .ok_or_else(|| "mapping arithmetic requires integers".to_owned())?;
                    match op.as_str() {
                        "+" => left.checked_add(right),
                        "-" => left.checked_sub(right),
                        "*" => left.checked_mul(right),
                        "/" if right != 0 => left.checked_div(right),
                        "%" if right != 0 => left.checked_rem(right),
                        _ => None,
                    }
                    .map(|value| json!(value))
                    .ok_or_else(|| "invalid mapping arithmetic".to_owned())
                }
                "==" => Ok(json!(left == right)),
                "!=" => Ok(json!(left != right)),
                "<" | "<=" | ">" | ">=" => {
                    let left = left
                        .as_i64()
                        .ok_or_else(|| "mapping comparison requires integers".to_owned())?;
                    let right = right
                        .as_i64()
                        .ok_or_else(|| "mapping comparison requires integers".to_owned())?;
                    Ok(json!(match op.as_str() {
                        "<" => left < right,
                        "<=" => left <= right,
                        ">" => left > right,
                        _ => left >= right,
                    }))
                }
                "and" | "or" | "=>" => {
                    let left = left
                        .as_bool()
                        .ok_or_else(|| "mapping logic requires Booleans".to_owned())?;
                    let right = right
                        .as_bool()
                        .ok_or_else(|| "mapping logic requires Booleans".to_owned())?;
                    Ok(json!(match op.as_str() {
                        "and" => left && right,
                        "or" => left || right,
                        _ => !left || right,
                    }))
                }
                _ => Err(format!("unsupported mapping operator '{op}'")),
            }
        }
        KernelExpr::Not(value) => Ok(json!(
            !mapping_json_expr(value, raw, bindings, model)?
                .as_bool()
                .ok_or_else(|| "mapping not requires Boolean".to_owned())?
        )),
        KernelExpr::Neg(value) => Ok(json!(
            -mapping_json_expr(value, raw, bindings, model)?
                .as_i64()
                .ok_or_else(|| "mapping negation requires integer".to_owned())?
        )),
        KernelExpr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            if mapping_json_expr(condition, raw, bindings, model)?
                .as_bool()
                .ok_or_else(|| "mapping condition requires Boolean".to_owned())?
            {
                mapping_json_expr(then_expr, raw, bindings, model)
            } else {
                mapping_json_expr(else_expr, raw, bindings, model)
            }
        }
        KernelExpr::Set(items) | KernelExpr::Seq(items) => items
            .iter()
            .map(|item| mapping_json_expr(item, raw, bindings, model))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        KernelExpr::Struct { fields, .. } => fields
            .iter()
            .map(|(name, value)| {
                Ok((
                    name.clone(),
                    mapping_json_expr(value, raw, bindings, model)?,
                ))
            })
            .collect::<Result<Map<_, _>, String>>()
            .map(Value::Object),
        _ => Err("unsupported JSON log mapping expression".to_owned()),
    }
}

pub(crate) fn json_mismatches(expected: &Value, observed: &Value, path: &str) -> Vec<Value> {
    if let (Some(expected), Some(observed)) = (expected.as_object(), observed.as_object()) {
        let keys = expected
            .keys()
            .chain(observed.keys())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        return keys
            .into_iter()
            .flat_map(|key| {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match (expected.get(&key), observed.get(&key)) {
                    (Some(left), Some(right)) => json_mismatches(left, right, &child),
                    (left, right) => vec![json!({
                        "path": child,
                        "expected": left.cloned().unwrap_or(Value::Null),
                        "observed": right.cloned().unwrap_or(Value::Null),
                    })],
                }
            })
            .collect();
    }
    if expected == observed {
        Vec::new()
    } else {
        vec![json!({"path":path,"expected":expected,"observed":observed})]
    }
}

pub(crate) fn read_jsonl_records(path: &Path) -> Result<Vec<(usize, Value)>, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    parse_jsonl_records(&source)
}

pub(crate) fn parse_jsonl_records(source: &str) -> Result<Vec<(usize, Value)>, String> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line)
                .map(|record| (index + 1, record))
                .map_err(|error| format!("invalid JSONL at line {}: {error}", index + 1))
        })
        .collect()
}

pub(crate) fn untyped_replay_enum_mapping_span(
    mapping: &fsl_syntax::SurfaceRefinement,
) -> Option<fsl_syntax::Span> {
    for item in &mapping.items {
        match item {
            fsl_syntax::RefinementItem::EnumConversion { span, .. }
            | fsl_syntax::RefinementItem::EnumAbstraction { span, .. } => return Some(*span),
            fsl_syntax::RefinementItem::Map { binder, expr, .. } => {
                if let Some(span) = binder
                    .as_ref()
                    .and_then(untyped_replay_binder_enum_mapping_span)
                    .or_else(|| untyped_replay_expr_enum_mapping_span(expr))
                {
                    return Some(span);
                }
            }
            fsl_syntax::RefinementItem::Action {
                target: fsl_syntax::ActionTarget::Action(_, args),
                ..
            } => {
                if let Some(span) = args.iter().find_map(untyped_replay_expr_enum_mapping_span) {
                    return Some(span);
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn untyped_replay_expr_enum_mapping_span(
    expr: &fsl_syntax::Expr,
) -> Option<fsl_syntax::Span> {
    ["convert", "abstract"]
        .into_iter()
        .find_map(|name| fsl_core::expression_call_span(expr, name))
}

pub(crate) fn untyped_replay_binder_enum_mapping_span(
    binder: &fsl_syntax::Binder,
) -> Option<fsl_syntax::Span> {
    match binder {
        fsl_syntax::Binder::Typed { where_expr, .. } => where_expr
            .as_deref()
            .and_then(untyped_replay_expr_enum_mapping_span),
        fsl_syntax::Binder::Range {
            lo, hi, where_expr, ..
        } => untyped_replay_expr_enum_mapping_span(lo)
            .or_else(|| untyped_replay_expr_enum_mapping_span(hi))
            .or_else(|| {
                where_expr
                    .as_deref()
                    .and_then(untyped_replay_expr_enum_mapping_span)
            }),
        fsl_syntax::Binder::Collection {
            collection,
            where_expr,
            ..
        } => untyped_replay_expr_enum_mapping_span(collection).or_else(|| {
            where_expr
                .as_deref()
                .and_then(untyped_replay_expr_enum_mapping_span)
        }),
    }
}

/// Whether a JSONL log action selects `action` under `maps auto`: its public
/// name, which is also the spelling `maps auto` matched before #1234 now that
/// no declared name contains `__` (#1256).
pub(crate) fn log_action_matches(
    model: &KernelModel,
    action: &fsl_core::ActionDef,
    source: &str,
) -> bool {
    model.action_display_name(&action.name) == source
}

pub(crate) fn parse_params(
    model: &KernelModel,
    action: &fsl_core::ActionDef,
    values: &Map<String, Value>,
) -> Result<std::collections::BTreeMap<String, FslValue>, String> {
    if values.len() != action.params.len() {
        return Err(format!(
            "parameter mismatch for action '{}'",
            model.action_display_name(&action.name)
        ));
    }
    action
        .params
        .iter()
        .map(|param| {
            let value = values
                .get(param.name())
                .ok_or_else(|| format!("missing parameter '{}'", param.name()))?;
            Ok((
                param.name().to_owned(),
                parse_param_value(model, param, value)?,
            ))
        })
        .collect()
}

pub(crate) fn parse_param_value(
    model: &KernelModel,
    param: &ParamDef,
    value: &Value,
) -> Result<FslValue, String> {
    match param {
        ParamDef::Range { .. } => value
            .as_i64()
            .map(FslValue::Int)
            .ok_or_else(|| format!("parameter '{}' must be an integer", param.name())),
        ParamDef::Typed { ty, .. } => match ty {
            TypeRef::Bool => value
                .as_bool()
                .map(FslValue::Bool)
                .or_else(|| value.as_i64().map(|value| FslValue::Bool(value != 0)))
                .ok_or_else(|| format!("parameter '{}' must be Boolean", param.name())),
            TypeRef::Int | TypeRef::Range(_, _) => value
                .as_i64()
                .map(FslValue::Int)
                .ok_or_else(|| format!("parameter '{}' must be an integer", param.name())),
            TypeRef::Named(type_name) => match model.types.get(type_name) {
                Some(TypeDef::Domain { .. }) => value
                    .as_i64()
                    .map(FslValue::Int)
                    .ok_or_else(|| format!("parameter '{}' must be an integer", param.name())),
                Some(TypeDef::Enum { members, .. }) => {
                    let member = value.as_str().ok_or_else(|| {
                        format!("parameter '{}' must be an enum member", param.name())
                    })?;
                    if !members.iter().any(|candidate| candidate == member) {
                        return Err(format!("unknown enum member '{member}'"));
                    }
                    Ok(FslValue::Enum {
                        type_name: type_name.clone(),
                        member: member.to_owned(),
                    })
                }
                Some(TypeDef::Struct { .. }) | None => {
                    Err(format!("parameter '{}' has unsupported type", param.name()))
                }
            },
            _ => Err(format!("parameter '{}' has non-scalar type", param.name())),
        },
    }
}

#[cfg(test)]
mod captured_jsonl_tests {
    use super::{parse_jsonl_records, read_jsonl_records};
    use serde_json::json;

    #[test]
    fn captured_jsonl_preserves_lines_errors_and_path_wrapper() {
        let path = std::env::temp_dir().join(format!("jsonl-wrapper-{}", std::process::id()));
        let raw = "\r\n  \r\n{\"text\":\"日本語\"}\r\n\ntrue\n";
        std::fs::write(&path, raw).unwrap();
        let expected = vec![(3, json!({"text":"日本語"})), (5, json!(true))];
        assert_eq!(parse_jsonl_records(raw).unwrap(), expected);
        assert_eq!(read_jsonl_records(&path).unwrap(), expected);
        let invalid = "\n{}\r\n\nnot json\n";
        std::fs::write(&path, invalid).unwrap();
        let error = parse_jsonl_records(invalid).unwrap_err();
        assert!(error.starts_with("invalid JSONL at line 4:"), "{error}");
        assert_eq!(read_jsonl_records(&path).unwrap_err(), error);
        std::fs::write(&path, [0xff]).unwrap();
        assert_eq!(
            read_jsonl_records(&path).unwrap_err(),
            std::fs::read_to_string(&path).unwrap_err().to_string()
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            read_jsonl_records(&path).unwrap_err(),
            std::fs::read_to_string(&path).unwrap_err().to_string()
        );
        assert!(parse_jsonl_records(" \r\n\n").unwrap().is_empty());
    }
}

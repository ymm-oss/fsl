// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::Write;

use fsl_core::{
    FslValue, KernelBinder as Binder, KernelExpr as Expr, KernelModel, Pattern, TraceStep,
};
use serde_json::{Value, json};

#[must_use]
pub fn display_name(name: &str) -> String {
    name.replacen("__", ".", 1)
}

#[must_use]
pub fn trace_json(model: &KernelModel, trace: &[TraceStep]) -> Value {
    Value::Array(
        trace
            .iter()
            .map(|entry| {
                let mut value = serde_json::Map::new();
                value.insert("step".to_owned(), json!(entry.step));
                value.insert("state".to_owned(), state_json(&entry.state));
                if let Some(action) = &entry.action {
                    let mut action_json = serde_json::Map::new();
                    action_json.insert("name".to_owned(), json!(display_name(&action.name)));
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
                            || Value::Object(serde_json::Map::new()),
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

fn compute_changes(previous: &Value, current: &Value) -> serde_json::Map<String, Value> {
    fn walk(
        path: &str,
        previous: &Value,
        current: &Value,
        out: &mut serde_json::Map<String, Value>,
    ) {
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

    let mut changes = serde_json::Map::new();
    walk("", previous, current, &mut changes);
    changes
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

fn map_key(value: &FslValue) -> String {
    match value {
        FslValue::Int(value) => value.to_string(),
        FslValue::Bool(value) => value.to_string(),
        FslValue::Enum { member, .. } => member.clone(),
        _ => format!("{value:?}"),
    }
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn expr_text(expr: &Expr) -> String {
    fn precedence(expr: &Expr) -> u8 {
        match expr {
            Expr::Quantified { .. } => 0,
            Expr::Binary { op, .. } => match op.as_str() {
                "=>" => 1,
                "or" => 2,
                "and" => 3,
                "==" | "!=" | "<" | "<=" | ">" | ">=" => 6,
                "+" | "-" => 7,
                "*" | "/" | "%" => 8,
                _ => 10,
            },
            Expr::Not(_) => 4,
            Expr::Is { .. } => 5,
            Expr::Neg(_) => 9,
            _ => 10,
        }
    }

    fn operand(expr: &Expr, minimum: u8) -> String {
        let rendered = expr_text(expr);
        if precedence(expr) < minimum {
            format!("({rendered})")
        } else {
            rendered
        }
    }

    fn binder_text(binder: &Binder) -> String {
        match binder {
            Binder::Typed {
                name,
                type_name,
                where_expr,
            } => {
                let mut text = format!("{name}: {}", display_name(&type_name.name));
                if let Some(condition) = where_expr {
                    let _ = write!(text, " where {}", expr_text(condition));
                }
                text
            }
            Binder::Range { name, lo, hi } => {
                format!("{name} in {}..{}", expr_text(lo), expr_text(hi))
            }
            Binder::Collection {
                name,
                collection,
                where_expr,
            } => {
                let mut text = format!("{name} in {}", expr_text(collection));
                if let Some(condition) = where_expr {
                    let _ = write!(text, " where {}", expr_text(condition));
                }
                text
            }
        }
    }

    match expr {
        Expr::Num(value) => value.to_string(),
        Expr::Bool(value) => value.to_string(),
        Expr::None => "none".to_owned(),
        Expr::Some(value) => format!("some({})", expr_text(value)),
        Expr::Set(values) => format!(
            "Set {{{}}}",
            values.iter().map(expr_text).collect::<Vec<_>>().join(", ")
        ),
        Expr::Seq(values) => format!(
            "Seq {{{}}}",
            values.iter().map(expr_text).collect::<Vec<_>>().join(", ")
        ),
        Expr::Struct { name, fields } => format!(
            "{} {{ {} }}",
            display_name(name),
            {
                let mut fields = fields.iter().collect::<Vec<_>>();
                fields.sort_by_key(|(name, _)| name.as_str());
                fields
            }
            .into_iter()
            .map(|(name, value)| format!("{name}: {}", expr_text(value)))
            .collect::<Vec<_>>()
            .join(", ")
        ),
        Expr::Var(name) => display_name(name),
        Expr::Call { name, args, .. } => format!(
            "{}({})",
            display_name(name),
            args.iter().map(expr_text).collect::<Vec<_>>().join(", ")
        ),
        Expr::Index(base, index) => format!("{}[{}]", operand(base, 10), expr_text(index)),
        Expr::Field(base, field) => format!("{}.{field}", operand(base, 10)),
        Expr::Method {
            receiver,
            name,
            args,
        } => format!(
            "{}.{}({})",
            operand(receiver, 10),
            name,
            args.iter().map(expr_text).collect::<Vec<_>>().join(", ")
        ),
        Expr::Binary { op, left, right } => {
            let (left_minimum, right_minimum) = match op.as_str() {
                "=>" => (2, 1),
                "or" => (2, 3),
                "and" => (3, 4),
                "==" | "!=" | "<" | "<=" | ">" | ">=" => (7, 7),
                "+" | "-" => (7, 8),
                "*" | "/" | "%" => (8, 9),
                _ => (0, 0),
            };
            format!(
                "{} {op} {}",
                operand(left, left_minimum),
                operand(right, right_minimum)
            )
        }
        Expr::Neg(value) => format!("-{}", operand(value, 9)),
        Expr::Not(value) => format!("not {}", operand(value, 4)),
        Expr::IfThenElse {
            condition,
            then_expr,
            else_expr,
        } => format!(
            "if {} then {} else {}",
            expr_text(condition),
            expr_text(then_expr),
            expr_text(else_expr)
        ),
        Expr::Is { expr, pattern } => match pattern {
            Pattern::None => format!("{} is none", operand(expr, 6)),
            Pattern::Some(name) => format!("{} is some({name})", operand(expr, 6)),
        },
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => format!("{quantifier} {}: {}", binder_text(binder), expr_text(body)),
        Expr::Count {
            name,
            type_name,
            condition,
        } => format!(
            "count({name}: {} where {})",
            display_name(&type_name.name),
            expr_text(condition)
        ),
        Expr::Sum {
            name,
            type_name,
            body,
            condition,
        } => format!(
            "sum({name}: {} of {}{})",
            display_name(&type_name.name),
            expr_text(body),
            condition
                .as_ref()
                .map_or_else(String::new, |value| format!(" where {}", expr_text(value)))
        ),
        Expr::UnaryNamed { name, expr, .. } => {
            let public = match name.as_str() {
                "rel_acyclic" => "acyclic",
                "rel_functional" => "functional",
                "rel_injective" => "injective",
                "rel_domain" => "domain",
                "rel_range" => "range",
                other => other,
            };
            format!("{public}({})", expr_text(expr))
        }
        Expr::BinaryNamed { name, left, right } => {
            format!("{}({}, {})", name, expr_text(left), expr_text(right))
        }
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => format!(
            "{}({}, {}, {})",
            name,
            expr_text(first),
            expr_text(second),
            expr_text(third)
        ),
        Expr::BinderNamed { name, binder } => format!("{name}({})", binder_text(binder)),
    }
}

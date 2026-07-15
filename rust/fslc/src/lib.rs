// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write;

use fsl_core::{
    FslValue, KernelBinder as Binder, KernelExpr as Expr, KernelModel, ParamDef, Pattern,
};
use serde_json::{Value, json};

pub mod coverage;
pub mod frontend_output;
pub mod origin_coverage;
pub mod verification_output;

pub use fsl_core::{
    display_name, fsl_value_json, internal_origin_json, origin_display_name, state_json, trace_json,
};

/// Diff two already-JSON-rendered values into a native `trace_json`-style
/// nested-path `changes` map, for `conformance_vectors`' before/after pair.
///
/// Kept as a private duplicate of `fsl_core::trace_json`'s internal helper of
/// the same name: conformance vectors are a CLI-only artifact never produced
/// by the browser Worker, so sharing it would widen `fsl-core`'s public
/// surface for no cross-implementation parity benefit.
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

pub const CONFORMANCE_V1_SCHEMA_VERSION: &str = "1.0.0";
pub const CONFORMANCE_V1_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/kernel/conformance.v1.schema.json";
pub const CONFORMANCE_V2_SCHEMA_VERSION: &str = "2.0.0";
pub const CONFORMANCE_V2_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/kernel/conformance.v2.schema.json";
pub const CONFORMANCE_SCHEMA_VERSION: &str = CONFORMANCE_V1_SCHEMA_VERSION;
pub const CONFORMANCE_SCHEMA_ID: &str = CONFORMANCE_V1_SCHEMA_ID;

type ActionCall = (String, BTreeMap<String, FslValue>);

/// Build deterministic, language-neutral concrete transition vectors.
///
/// Every bounded action instance is represented for every explored state.
/// Disabled instances and runtime violations retain the input state, making
/// failure semantics directly testable by an external implementation.
///
/// # Errors
///
/// Returns an error when initialization, bounded parameter enumeration, guard
/// evaluation, or a concrete step cannot be evaluated.
pub fn conformance_vectors(model: &KernelModel, depth: usize) -> Result<Value, String> {
    conformance_vectors_for_version(model, depth, fsl_core::PublicKernelVersion::V1)
}

/// Build conformance vectors corresponding to an explicitly negotiated Kernel major.
///
/// # Errors
///
/// Returns an error when concrete initialization or exploration fails.
pub fn conformance_vectors_for_version(
    model: &KernelModel,
    depth: usize,
    version: fsl_core::PublicKernelVersion,
) -> Result<Value, String> {
    let all_calls = action_calls(model)?;
    let initial = fsl_runtime::Monitor::new(model.clone()).map_err(|error| error.to_string())?;
    let initial_json = conformance_state_json(model, &initial.state)?;
    let initial_key = serde_json::to_string(&initial_json).map_err(|error| error.to_string())?;
    let mut seen = BTreeMap::from([(initial_key, "s0".to_owned())]);
    let mut queue = VecDeque::from([("s0".to_owned(), 0_usize, initial)]);
    let mut states = vec![json!({"id":"s0","depth":0,"state":initial_json})];
    let mut vectors = Vec::new();

    while let Some((state_id, state_depth, monitor)) = queue.pop_front() {
        let before = conformance_state_json(model, &monitor.state)?;
        for (action, params) in &all_calls {
            let action_json = json!({
                "name":display_name(action),
                "params":params.iter().map(|(name,value)|(name.clone(),fsl_value_json(value))).collect::<serde_json::Map<_,_>>()
            });
            let mut successor = monitor.clone();
            let result = successor
                .attempt(action, params)
                .map_err(|error| error.to_string())?;
            let after = conformance_state_json(model, &result.state)?;
            if let Some(violation) = result.violation {
                let attempted = result
                    .attempted_state
                    .as_ref()
                    .map(|state| conformance_state_json(model, state))
                    .transpose()?;
                vectors.push(json!({
                    "state":state_id,"action":action_json,
                    "outcome":{
                        "kind":violation.kind,"name":violation.name,
                        "state_changed":after != before,"state":after,
                        "attempted_state":attempted
                    }
                }));
                continue;
            }

            let changes = compute_changes(&before, &after);
            vectors.push(json!({
                "state":state_id,"action":action_json,
                "outcome":{"kind":"ok","state_changed":after != before,"state":after,"changes":changes}
            }));
            if state_depth >= depth {
                continue;
            }
            let key = serde_json::to_string(&after).map_err(|error| error.to_string())?;
            if let std::collections::btree_map::Entry::Vacant(entry) = seen.entry(key) {
                let id = format!("s{}", states.len());
                entry.insert(id.clone());
                states.push(json!({"id":id,"depth":state_depth+1,"state":after}));
                queue.push_back((id, state_depth + 1, successor));
            }
        }
    }

    let (schema_id, schema_version) = match version {
        fsl_core::PublicKernelVersion::V1 => {
            (CONFORMANCE_V1_SCHEMA_ID, CONFORMANCE_V1_SCHEMA_VERSION)
        }
        fsl_core::PublicKernelVersion::V2 => {
            (CONFORMANCE_V2_SCHEMA_ID, CONFORMANCE_V2_SCHEMA_VERSION)
        }
    };
    Ok(json!({
        "$schema":schema_id,
        "schema_version":schema_version,
        "kernel_schema_version":version.schema_version(),
        "result":"conformance",
        "spec":model.name,
        "depth":depth,
        "states":states,
        "vectors":vectors,
    }))
}

struct PythonRandom {
    state: [u32; 624],
    index: usize,
}

impl PythonRandom {
    fn seeded_zero() -> Self {
        let mut random = Self {
            state: [0; 624],
            index: 624,
        };
        random.state[0] = 19_650_218;
        for index in 1..624 {
            let previous = random.state[index - 1];
            random.state[index] = (previous ^ (previous >> 30))
                .wrapping_mul(1_812_433_253)
                .wrapping_add(u32::try_from(index).expect("MT index fits u32"));
        }
        let mut index = 1_usize;
        let mut key_index = 0_usize;
        for _ in 0..624 {
            let previous = random.state[index - 1];
            random.state[index] = (random.state[index]
                ^ ((previous ^ (previous >> 30)).wrapping_mul(1_664_525)))
            .wrapping_add(0_u32)
            .wrapping_add(u32::try_from(key_index).expect("MT key index fits u32"));
            index += 1;
            key_index += 1;
            if index >= 624 {
                random.state[0] = random.state[623];
                index = 1;
            }
            if key_index >= 1 {
                key_index = 0;
            }
        }
        for _ in 0..623 {
            let previous = random.state[index - 1];
            random.state[index] = (random.state[index]
                ^ ((previous ^ (previous >> 30)).wrapping_mul(1_566_083_941)))
            .wrapping_sub(u32::try_from(index).expect("MT index fits u32"));
            index += 1;
            if index >= 624 {
                random.state[0] = random.state[623];
                index = 1;
            }
        }
        random.state[0] = 0x8000_0000;
        random
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            for index in 0..624 {
                let value = (self.state[index] & 0x8000_0000)
                    | (self.state[(index + 1) % 624] & 0x7fff_ffff);
                self.state[index] = self.state[(index + 397) % 624]
                    ^ (value >> 1)
                    ^ if value & 1 == 0 { 0 } else { 0x9908_b0df };
            }
            self.index = 0;
        }
        let mut value = self.state[self.index];
        self.index += 1;
        value ^= value >> 11;
        value ^= (value << 7) & 0x9d2c_5680;
        value ^= (value << 15) & 0xefc6_0000;
        value ^= value >> 18;
        value
    }

    fn below(&mut self, upper: usize) -> usize {
        let bits = usize::BITS - upper.leading_zeros();
        loop {
            let value = self.next_u32() >> (32 - bits);
            let value = usize::try_from(value).expect("u32 fits usize on supported targets");
            if value < upper {
                return value;
            }
        }
    }
}

fn ordered_object(value: &Value, order: &[String]) -> Value {
    let Some(values) = value.as_object() else {
        return value.clone();
    };
    let mut result = serde_json::Map::new();
    for key in order {
        if let Some(value) = values.get(key) {
            result.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in values {
        if !result.contains_key(key) {
            result.insert(key.clone(), value.clone());
        }
    }
    Value::Object(result)
}

/// Build the versioned fixed-seed conformance trace consumed by native testgen.
///
/// Unlike the exhaustive bounded conformance corpus, this contract records one
/// deterministic path and retains ordinary Monitor JSON values so existing
/// generated harness bytes remain stable.
///
/// # Errors
///
/// Returns an error when concrete initialization, enabled-action enumeration,
/// or stepping fails.
pub fn testgen_trace_vectors(model: &KernelModel) -> Result<Value, String> {
    let mut monitor =
        fsl_runtime::Monitor::new(model.clone()).map_err(|error| error.to_string())?;
    let state_order = model
        .state
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let initial = ordered_object(&state_json(&monitor.state), &state_order);
    let mut steps = Vec::new();
    let mut random = PythonRandom::seeded_zero();
    for _ in 0..100 {
        let enabled = monitor.enabled().map_err(|error| error.to_string())?;
        if enabled.is_empty() {
            break;
        }
        let choice = &enabled[random.below(enabled.len())];
        let action = choice.action.clone();
        let param_order = model
            .actions
            .iter()
            .find(|candidate| candidate.name == action)
            .map(|candidate| {
                candidate
                    .params
                    .iter()
                    .map(|param| param.name().to_owned())
                    .collect::<Vec<_>>()
            })
            .ok_or_else(|| format!("enabled action '{action}' is absent from the model"))?;
        let params = choice
            .params
            .iter()
            .map(|(name, value)| (name.clone(), fsl_value_json(value)))
            .collect::<serde_json::Map<_, _>>();
        let params = ordered_object(&Value::Object(params), &param_order);
        monitor.step(choice).map_err(|error| error.to_string())?;
        steps.push(json!({
            "action": display_name(&action),
            "params": params,
            "expected": ordered_object(&state_json(&monitor.state), &state_order)
        }));
    }
    Ok(json!({
        "$schema": fsl_core::TESTGEN_TRACE_V1_SCHEMA_ID,
        "schema_version": fsl_core::TESTGEN_TRACE_V1_SCHEMA_VERSION,
        "kernel_schema_version": fsl_core::KERNEL_SCHEMA_VERSION,
        "result": "testgen_trace",
        "spec": model.name,
        "initial": initial,
        "steps": steps
    }))
}

fn conformance_state_json(
    model: &KernelModel,
    state: &BTreeMap<String, FslValue>,
) -> Result<Value, String> {
    Ok(Value::Object(
        model
            .state
            .iter()
            .map(|(name, ty)| {
                let value = state
                    .get(name)
                    .ok_or_else(|| format!("missing state variable '{name}'"))?;
                Ok((
                    display_name(name),
                    conformance_value_json(model, ty, value)?,
                ))
            })
            .collect::<Result<_, String>>()?,
    ))
}

fn conformance_value_json(
    model: &KernelModel,
    ty: &fsl_core::TypeRef,
    value: &FslValue,
) -> Result<Value, String> {
    use fsl_core::{TypeDef, TypeRef};
    match (ty, value) {
        (TypeRef::Option(_), FslValue::None) => Ok(json!({"kind":"none"})),
        (TypeRef::Option(inner), FslValue::Some(value)) => Ok(json!({
            "kind":"some",
            "value":conformance_value_json(model, inner, value)?
        })),
        (TypeRef::Seq(inner, _), FslValue::Seq(values)) => Ok(Value::Array(
            values
                .iter()
                .map(|value| conformance_value_json(model, inner, value))
                .collect::<Result<_, _>>()?,
        )),
        (TypeRef::Set(inner), FslValue::Set(values)) => Ok(Value::Array(
            values
                .iter()
                .map(|value| conformance_value_json(model, inner, value))
                .collect::<Result<_, _>>()?,
        )),
        (TypeRef::Map(_, item), FslValue::Map(entries)) => Ok(Value::Object(
            entries
                .iter()
                .map(|(key_value, value)| {
                    Ok((
                        map_key(key_value),
                        conformance_value_json(model, item, value)?,
                    ))
                })
                .collect::<Result<_, String>>()?,
        )),
        (TypeRef::Named(name), FslValue::Struct { fields, .. }) => {
            let Some(TypeDef::Struct {
                fields: definitions,
            }) = model.types.get(name)
            else {
                return Err(format!("unknown struct type '{name}'"));
            };
            Ok(Value::Object(
                definitions
                    .iter()
                    .map(|(field, field_ty)| {
                        let value = fields
                            .get(field)
                            .ok_or_else(|| format!("missing struct field '{name}.{field}'"))?;
                        Ok((
                            field.clone(),
                            conformance_value_json(model, field_ty, value)?,
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            ))
        }
        _ => Ok(fsl_value_json(value)),
    }
}

fn action_calls(model: &KernelModel) -> Result<Vec<ActionCall>, String> {
    let mut calls = Vec::new();
    for action in &model.actions {
        let mut bindings = vec![BTreeMap::new()];
        for parameter in &action.params {
            let values = match parameter {
                ParamDef::Typed { ty, .. } => {
                    model.domain_values(ty).map_err(|error| error.to_string())?
                }
                ParamDef::Range { lo, hi, .. } => (*lo..=*hi).map(FslValue::Int).collect(),
            };
            let mut next = Vec::new();
            for existing in bindings {
                for value in &values {
                    let mut candidate = existing.clone();
                    candidate.insert(parameter.name().to_owned(), value.clone());
                    next.push(candidate);
                }
            }
            bindings = next;
        }
        calls.extend(
            bindings
                .into_iter()
                .map(|params| (action.name.clone(), params)),
        );
    }
    calls.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| format!("{:?}", left.1).cmp(&format!("{:?}", right.1)))
    });
    let mut unique = BTreeSet::new();
    calls.retain(|call| unique.insert(format!("{}:{:?}", call.0, call.1)));
    Ok(calls)
}

/// Map key encoding for `conformance_value_json`'s `Map`-typed fields.
///
/// Kept as a private duplicate of `fsl_core::trace_json`'s internal helper of
/// the same name, for the same reason as `compute_changes` above.
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
    expr_text_with_origins(None, expr)
}

/// Render an expression with source-level names recovered from lowering origins.
#[must_use]
pub fn source_expr_text(model: &KernelModel, expr: &Expr) -> String {
    expr_text_with_origins(Some(model), expr)
}

#[allow(clippy::too_many_lines)]
fn expr_text_with_origins(model: Option<&KernelModel>, expr: &Expr) -> String {
    fn precedence(expr: &Expr) -> u8 {
        match expr {
            Expr::Conditional { .. } | Expr::Quantified { .. } => 0,
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

    fn operand(model: Option<&KernelModel>, expr: &Expr, minimum: u8) -> String {
        let rendered = expr_text_with_origins(model, expr);
        if precedence(expr) < minimum {
            format!("({rendered})")
        } else {
            rendered
        }
    }

    fn binder_text(model: Option<&KernelModel>, binder: &Binder) -> String {
        match binder {
            Binder::Typed {
                name,
                type_name,
                where_expr,
            } => {
                let mut text = format!("{name}: {}", display_name(&type_name.name));
                if let Some(condition) = where_expr {
                    let _ = write!(text, " where {}", expr_text_with_origins(model, condition));
                }
                text
            }
            Binder::Range { name, lo, hi } => {
                format!(
                    "{name} in {}..{}",
                    expr_text_with_origins(model, lo),
                    expr_text_with_origins(model, hi)
                )
            }
            Binder::Collection {
                name,
                collection,
                where_expr,
            } => {
                let mut text = format!("{name} in {}", expr_text_with_origins(model, collection));
                if let Some(condition) = where_expr {
                    let _ = write!(text, " where {}", expr_text_with_origins(model, condition));
                }
                text
            }
        }
    }

    match expr {
        Expr::Num(value) => value.to_string(),
        Expr::Bool(value) => value.to_string(),
        Expr::None => "none".to_owned(),
        Expr::Some(value) => format!("some({})", expr_text_with_origins(model, value)),
        Expr::Set(values) => format!(
            "Set {{{}}}",
            values
                .iter()
                .map(|value| expr_text_with_origins(model, value))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Seq(values) => format!(
            "Seq {{{}}}",
            values
                .iter()
                .map(|value| expr_text_with_origins(model, value))
                .collect::<Vec<_>>()
                .join(", ")
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
            .map(|(name, value)| { format!("{name}: {}", expr_text_with_origins(model, value)) })
            .collect::<Vec<_>>()
            .join(", ")
        ),
        Expr::Var(name) => display_name(name),
        Expr::Call { name, args, .. } => format!(
            "{}({})",
            display_name(name),
            args.iter()
                .map(|argument| expr_text_with_origins(model, argument))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Index(base, index) => {
            let stage_qualifier = model.and_then(|model| {
                let Expr::Var(name) = base.as_ref() else {
                    return None;
                };
                let origin = model.state_origin(name)?;
                origin
                    .lowering_steps
                    .iter()
                    .any(|step| step.kind == "synthesize_stage_map")
                    .then(|| {
                        origin
                            .lowering_steps
                            .iter()
                            .find(|step| step.kind == "qualified_process_path")
                            .and_then(|step| step.detail.as_deref())
                    })
            });
            if let Some(qualifier) = stage_qualifier {
                format!(
                    "{}stage({})",
                    qualifier.map_or_else(String::new, |path| format!("{path}.")),
                    expr_text_with_origins(model, index)
                )
            } else {
                format!(
                    "{}[{}]",
                    operand(model, base, 10),
                    expr_text_with_origins(model, index)
                )
            }
        }
        Expr::Field(base, field) => format!("{}.{field}", operand(model, base, 10)),
        Expr::Method {
            receiver,
            name,
            args,
        } => format!(
            "{}.{}({})",
            operand(model, receiver, 10),
            name,
            args.iter()
                .map(|argument| expr_text_with_origins(model, argument))
                .collect::<Vec<_>>()
                .join(", ")
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
                operand(model, left, left_minimum),
                operand(model, right, right_minimum)
            )
        }
        Expr::Neg(value) => format!("-{}", operand(model, value, 9)),
        Expr::Not(value) => format!("not {}", operand(model, value, 4)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => format!(
            "if {} then {} else {}",
            expr_text_with_origins(model, condition),
            expr_text_with_origins(model, then_expr),
            expr_text_with_origins(model, else_expr)
        ),
        Expr::Is { expr, pattern } => match pattern {
            Pattern::None => format!("{} is none", operand(model, expr, 6)),
            Pattern::Some(name) => format!("{} is some({name})", operand(model, expr, 6)),
        },
        Expr::Quantified {
            quantifier,
            binder,
            body,
        } => format!(
            "{quantifier} {}: {}",
            binder_text(model, binder),
            expr_text_with_origins(model, body)
        ),
        Expr::Count {
            name,
            type_name,
            condition,
        } => format!(
            "count({name}: {} where {})",
            display_name(&type_name.name),
            expr_text_with_origins(model, condition)
        ),
        Expr::Sum {
            name,
            type_name,
            body,
            condition,
        } => format!(
            "sum({name}: {} of {}{})",
            display_name(&type_name.name),
            expr_text_with_origins(model, body),
            condition.as_ref().map_or_else(String::new, |value| {
                format!(" where {}", expr_text_with_origins(model, value))
            })
        ),
        Expr::Stage {
            process, entity, ..
        } => process.as_ref().map_or_else(
            || format!("stage({})", expr_text_with_origins(model, entity)),
            |process| format!("{process}.stage({})", expr_text_with_origins(model, entity)),
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
            format!("{public}({})", expr_text_with_origins(model, expr))
        }
        Expr::BinaryNamed { name, left, right } => {
            format!(
                "{}({}, {})",
                name,
                expr_text_with_origins(model, left),
                expr_text_with_origins(model, right)
            )
        }
        Expr::TernaryNamed {
            name,
            first,
            second,
            third,
        } => format!(
            "{}({}, {}, {})",
            name,
            expr_text_with_origins(model, first),
            expr_text_with_origins(model, second),
            expr_text_with_origins(model, third)
        ),
        Expr::BinderNamed { name, binder } => {
            format!("{name}({})", binder_text(model, binder))
        }
    }
}

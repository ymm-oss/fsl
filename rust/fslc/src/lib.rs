// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use fsl_core::{FslValue, KernelModel, ParamDef};
use serde_json::{Value, json};

pub mod coverage;
pub mod frontend_output;
pub mod migration;
pub mod origin_coverage;
pub mod replay_trace;
pub mod source_diagnostic;
pub mod verification_output;

pub use fsl_core::{
    display_name, expr_text, fsl_value_json, internal_origin_json, origin_display_name,
    source_expr_text, state_json, trace_json,
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

// SPDX-License-Identifier: Apache-2.0

//! WASM entry point used exclusively inside the browser verification Worker.

use std::collections::BTreeMap;

use fsl_core::{
    CoreError, FileResolver, KernelModel, TypeDef, TypeRef, display_name, fsl_value_json,
    trace_json,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use wasm_bindgen::prelude::*;

#[derive(Debug, Deserialize)]
struct Request {
    cmd: String,
    source: String,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    options: Options,
}

#[derive(Debug, Deserialize)]
struct Options {
    #[serde(default = "default_depth")]
    depth: usize,
    #[serde(default = "default_deadlock")]
    deadlock: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            depth: default_depth(),
            deadlock: default_deadlock(),
        }
    }
}

const fn default_depth() -> usize {
    8
}

fn default_deadlock() -> String {
    "warn".to_owned()
}

struct MemoryResolver {
    files: BTreeMap<String, String>,
}

impl FileResolver for MemoryResolver {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        self.files.get(path).cloned().ok_or_else(|| CoreError {
            message: format!("file not found: {path}"),
            line: 1,
            column: 1,
            origin: None,
        })
    }
}

fn envelope() -> Map<String, Value> {
    let mut output = Map::new();
    output.insert("fsl".to_owned(), json!("1.0"));
    output
}

fn error(kind: &str, message: impl AsRef<str>) -> Value {
    let mut output = envelope();
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!(kind));
    output.insert("message".to_owned(), json!(message.as_ref()));
    Value::Object(output)
}

fn build(request: &Request) -> Result<KernelModel, Value> {
    let resolver = MemoryResolver {
        files: request.files.clone(),
    };
    let kernel = fsl_core::parse_kernel_source(&request.source, &resolver)
        .map_err(|failure| error("parse", failure.to_string()))?;
    fsl_core::build_model(kernel).map_err(|failure| error("semantics", failure.to_string()))
}

fn has_bounds(model: &KernelModel, ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Int | TypeRef::Bool | TypeRef::Relation(_, _) => false,
        TypeRef::Range(_, _) | TypeRef::Set(_) | TypeRef::Seq(_, _) => true,
        TypeRef::Option(inner) => has_bounds(model, inner),
        TypeRef::Map(_, value) => has_bounds(model, value),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { .. } | TypeDef::Enum { .. }) => true,
            Some(TypeDef::Struct { fields }) => fields.iter().any(|(_, ty)| has_bounds(model, ty)),
            None => false,
        },
    }
}

fn invariant_names(model: &KernelModel) -> Vec<String> {
    let mut names = model
        .state
        .iter()
        .filter(|(_, ty)| has_bounds(model, ty))
        .map(|(name, _)| format!("_bounds_{name}"))
        .collect::<Vec<_>>();
    names.extend(
        model
            .invariants
            .iter()
            .map(|property| property.name.clone()),
    );
    names
}

fn check(request: &Request) -> Value {
    let model = match build(request) {
        Ok(model) => model,
        Err(error) => return error,
    };
    let mut output = envelope();
    output.insert("result".to_owned(), json!("ok"));
    output.insert("spec".to_owned(), json!(model.name));
    let warnings = if model.invariants.is_empty()
        && model.transitions.is_empty()
        && model.reachables.is_empty()
        && model.leadstos.is_empty()
    {
        json!([{"message": "spec declares no user invariants (only implicit type bounds are checked)"}])
    } else {
        json!([])
    };
    output.insert("warnings".to_owned(), warnings);
    Value::Object(output)
}

async fn verify(request: &Request) -> Value {
    let model = match build(request) {
        Ok(model) => model,
        Err(error) => return error,
    };
    let mut solver = fsl_solver_z3js::Z3JsSolver::new();
    let result =
        match fsl_verifier::verify_bounded(&model, &mut solver, request.options.depth).await {
            Ok(result) => result,
            Err(failure) => return error("semantics", failure.to_string()),
        };
    render_verify(&model, &request.options, result)
}

#[allow(clippy::too_many_lines)]
fn render_verify(model: &KernelModel, options: &Options, result: fsl_verifier::BmcResult) -> Value {
    let mut output = envelope();
    output.insert("spec".to_owned(), json!(model.name));
    if let Some(violation) = result.violation {
        output.insert("result".to_owned(), json!("violated"));
        output.insert("violation_kind".to_owned(), json!(violation.kind));
        output.insert("invariant".to_owned(), json!(violation.name));
        output.insert("violated_at_step".to_owned(), json!(violation.step));
        output.insert(
            "last_action".to_owned(),
            violation
                .last_action
                .map_or(Value::Null, |name| json!({"name": name})),
        );
        output.insert("checked_to_depth".to_owned(), json!(violation.step));
        output.insert("completeness".to_owned(), json!("bounded"));
        output.insert("trace_type".to_owned(), json!(violation.kind));
        output.insert("trace".to_owned(), trace_json(model, &violation.trace));
        return Value::Object(output);
    }
    // Native CLI (rust/fslc/src/verification.rs render_bmc_result) checks
    // deadlock-as-error before leadsto_violation: both can legitimately be
    // `Some` at once (deadlock_step is set inside the per-step loop,
    // leadsto_violation only after it ends), so this order is the contract.
    if options.deadlock == "error"
        && let Some(step) = result.deadlock_step
    {
        output.insert("result".to_owned(), json!("violated"));
        output.insert("violation_kind".to_owned(), json!("deadlock"));
        output.insert("invariant".to_owned(), json!("deadlock"));
        output.insert("violated_at_step".to_owned(), json!(step));
        output.insert("checked_to_depth".to_owned(), json!(step));
        output.insert("completeness".to_owned(), json!("bounded"));
        output.insert("trace_type".to_owned(), json!("deadlock"));
        return Value::Object(output);
    }
    if let Some(violation) = result.leadsto_violation {
        output.insert("result".to_owned(), json!("violated"));
        output.insert("violation_kind".to_owned(), json!("leadsTo"));
        output.insert("invariant".to_owned(), json!(display_name(&violation.name)));
        output.insert("violated_at_step".to_owned(), json!(violation.step));
        if let Some(details) = violation.leads_to {
            output.insert(
                "bindings".to_owned(),
                Value::Object(
                    details
                        .bindings
                        .iter()
                        .map(|(name, value)| (name.clone(), fsl_value_json(value)))
                        .collect(),
                ),
            );
            output.insert("pending_since".to_owned(), json!(details.pending_since));
            if let Some(loop_start) = details.loop_start {
                output.insert("loop_start".to_owned(), json!(loop_start));
            }
            if let Some(deadline) = details.deadline {
                output.insert("deadline".to_owned(), json!(deadline));
            }
            if let Some(within) = details.within {
                output.insert("within".to_owned(), json!(within));
            }
            output.insert("stutter".to_owned(), json!(details.stutter));
            output.insert("hint".to_owned(), json!(details.hint));
        }
        output.insert("checked_to_depth".to_owned(), json!(violation.step));
        output.insert("completeness".to_owned(), json!("bounded"));
        output.insert("trace_type".to_owned(), json!("leadsTo"));
        output.insert("trace".to_owned(), trace_json(model, &violation.trace));
        return Value::Object(output);
    }
    output.insert("result".to_owned(), json!("verified"));
    output.insert("depth".to_owned(), json!(options.depth));
    output.insert("checked_to_depth".to_owned(), json!(options.depth));
    output.insert("completeness".to_owned(), json!("bounded"));
    output.insert(
        "invariants_checked".to_owned(),
        json!(invariant_names(model)),
    );
    output.insert(
        "transitions_checked".to_owned(),
        json!(
            model
                .transitions
                .iter()
                .map(|property| &property.name)
                .collect::<Vec<_>>()
        ),
    );
    output.insert(
        "reachables".to_owned(),
        Value::Object(
            result
                .reachables
                .iter()
                .map(|(name, witness)| {
                    (
                        name.clone(),
                        witness.as_ref().map_or(
                            Value::Null,
                            |witness| json!({"witnessed_at_step": witness.step}),
                        ),
                    )
                })
                .collect(),
        ),
    );
    output.insert(
        "action_coverage".to_owned(),
        Value::Object(
            result
                .action_coverage
                .iter()
                .map(|(name, covered)| (name.clone(), json!(covered)))
                .collect(),
        ),
    );
    output.insert(
        "deadlock".to_owned(),
        if options.deadlock == "ignore" {
            json!({"found": false})
        } else {
            result.deadlock_step.map_or_else(
                || json!({"found": false}),
                |step| json!({"found": true, "at_step": step}),
            )
        },
    );
    output.insert("warnings".to_owned(), json!([]));
    output.insert(
        "note".to_owned(),
        json!(format!(
            "bounded verification: no violation within depth {}",
            options.depth
        )),
    );
    Value::Object(output)
}

/// Execute one Worker request and return the stable JSON envelope as text.
#[wasm_bindgen]
pub async fn run(request_json: String) -> String {
    let request = match serde_json::from_str::<Request>(&request_json) {
        Ok(request) => request,
        Err(failure) => {
            return error("io", format!("invalid request JSON: {failure}")).to_string();
        }
    };
    let output = match request.cmd.as_str() {
        "check" => check(&request),
        "verify" => verify(&request).await,
        command => error(
            "usage",
            format!("command '{command}' is not available in the browser Worker"),
        ),
    };
    serde_json::to_string_pretty(&output).unwrap_or_else(|failure| {
        format!(
            "{{\"fsl\":\"1.0\",\"result\":\"error\",\"kind\":\"internal\",\"message\":{}}}",
            serde_json::to_string(&failure.to_string()).unwrap_or_else(|_| "null".to_owned())
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsl_core::{FslValue, TraceAction, TraceStep};
    use fsl_verifier::{BmcResult, BmcViolation, LeadsToViolation};

    fn model_from(source: &str) -> KernelModel {
        let resolver = MemoryResolver {
            files: BTreeMap::new(),
        };
        let kernel = fsl_core::parse_kernel_source(source, &resolver).expect("parse");
        fsl_core::build_model(kernel).expect("model")
    }

    #[test]
    fn deadlock_as_error_wins_over_leadsto_violation() {
        let model =
            model_from("spec Test { state { x: Int } init { x = 0 } action a() { x = 0 } }");
        let result = BmcResult {
            spec: model.name.clone(),
            depth: 4,
            violation: None,
            leadsto_violation: Some(BmcViolation {
                kind: "leadsTo".to_owned(),
                name: "SomeLeadsTo".to_owned(),
                step: 1,
                last_action: None,
                trace: Vec::new(),
                leads_to: Some(LeadsToViolation {
                    bindings: BTreeMap::new(),
                    pending_since: 0,
                    loop_start: None,
                    deadline: None,
                    within: None,
                    stutter: false,
                    hint: "stuck".to_owned(),
                }),
            }),
            reachables: BTreeMap::new(),
            deadlock_step: Some(1),
            deadlock_trace: Some(Vec::new()),
            action_coverage: BTreeMap::new(),
            frontier_progress: false,
        };
        let options = Options {
            depth: 4,
            deadlock: "error".to_owned(),
        };

        let envelope = render_verify(&model, &options, result);

        assert_eq!(envelope["violation_kind"], json!("deadlock"));
    }

    #[test]
    fn trace_json_diffs_struct_state_by_field_not_whole_value() {
        let model = model_from(
            "spec Test { \
             struct Job { status: Int, priority: Int } \
             state { job: Job } \
             init { job = Job { status: 0, priority: 0 } } \
             action advance() { job.status = 1 } \
             }",
        );

        let before = FslValue::Struct {
            type_name: "Job".to_owned(),
            fields: BTreeMap::from([
                ("status".to_owned(), FslValue::Int(0)),
                ("priority".to_owned(), FslValue::Int(0)),
            ]),
        };
        let after = FslValue::Struct {
            type_name: "Job".to_owned(),
            fields: BTreeMap::from([
                ("status".to_owned(), FslValue::Int(1)),
                ("priority".to_owned(), FslValue::Int(0)),
            ]),
        };
        let trace = vec![
            TraceStep {
                step: 0,
                state: BTreeMap::from([("job".to_owned(), before)]),
                action: None,
                changes: BTreeMap::new(),
            },
            TraceStep {
                step: 1,
                state: BTreeMap::from([("job".to_owned(), after)]),
                action: Some(TraceAction {
                    name: "advance".to_owned(),
                    params: BTreeMap::new(),
                }),
                changes: BTreeMap::new(),
            },
        ];

        let rendered = trace_json(&model, &trace);
        let changes = rendered[1]["changes"]
            .as_object()
            .expect("changes is an object");

        assert!(
            changes.keys().any(|key| key.contains("[status]")),
            "expected a nested-path key like 'job[status]', got {changes:?}"
        );
        assert!(
            !changes.contains_key("job"),
            "whole-struct key must not appear, got {changes:?}"
        );
    }
}

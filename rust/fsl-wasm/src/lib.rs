// SPDX-License-Identifier: Apache-2.0

//! WASM entry point used exclusively inside the browser verification Worker.

use std::collections::BTreeMap;

use fsl_core::{CoreError, FileResolver, KernelModel, TypeDef, TypeRef};
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

#[allow(clippy::too_many_lines)]
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
        return Value::Object(output);
    }
    if request.options.deadlock == "error"
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
    output.insert("result".to_owned(), json!("verified"));
    output.insert("depth".to_owned(), json!(request.options.depth));
    output.insert("checked_to_depth".to_owned(), json!(request.options.depth));
    output.insert("completeness".to_owned(), json!("bounded"));
    output.insert(
        "invariants_checked".to_owned(),
        json!(invariant_names(&model)),
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
        if request.options.deadlock == "ignore" {
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
            request.options.depth
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

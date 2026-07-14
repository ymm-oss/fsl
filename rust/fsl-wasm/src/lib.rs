// SPDX-License-Identifier: Apache-2.0

//! WASM entry point used exclusively inside the browser verification Worker.

use std::collections::BTreeMap;

use fsl_core::{
    CoreError, FileResolver, KernelModel, TypeDef, TypeRef, display_name, fsl_value_json,
    model_warnings, trace_json,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}

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

fn envelope(solver_version: &str) -> Map<String, Value> {
    let mut output = Map::new();
    output.insert("fsl".to_owned(), json!("1.0"));
    output.insert(
        "versions".to_owned(),
        fsl_core::version_metadata(
            "fsl-wasm",
            env!("CARGO_PKG_VERSION"),
            "z3-solver-wasm",
            solver_version,
        ),
    );
    output
}

fn error(solver_version: &str, kind: &str, message: impl AsRef<str>) -> Value {
    let mut output = envelope(solver_version);
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!(kind));
    output.insert("message".to_owned(), json!(message.as_ref()));
    Value::Object(output)
}

fn build(request: &Request, solver_version: &str) -> Result<KernelModel, Value> {
    let resolver = MemoryResolver {
        files: request.files.clone(),
    };
    let kernel = fsl_core::parse_kernel_source(&request.source, &resolver)
        .map_err(|failure| error(solver_version, "parse", failure.to_string()))?;
    fsl_core::build_model(kernel)
        .map_err(|failure| error(solver_version, "semantics", failure.to_string()))
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

fn check(request: &Request, solver_version: &str) -> Value {
    let model = match build(request, solver_version) {
        Ok(model) => model,
        Err(error) => return error,
    };
    let mut output = envelope(solver_version);
    output.insert("result".to_owned(), json!("ok"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("warnings".to_owned(), Value::Array(model_warnings(&model)));
    Value::Object(output)
}

async fn verify(request: &Request, solver_version: &str) -> Value {
    let started = performance_now();
    let model = match build(request, solver_version) {
        Ok(model) => model,
        Err(error) => return error,
    };
    let mut solver = fsl_solver_z3js::Z3JsSolver::new();
    let result =
        match fsl_verifier::verify_bounded(&model, &mut solver, request.options.depth).await {
            Ok(result) => result,
            Err(failure) => return error(solver_version, "semantics", failure.to_string()),
        };
    let statistics = fsl_solver::SmtSolver::statistics(&solver);
    render_verify(
        &model,
        &request.options,
        result,
        solver_version,
        &statistics,
        (performance_now() - started) / 1000.0,
    )
}

#[allow(clippy::too_many_lines)]
fn render_verify(
    model: &KernelModel,
    options: &Options,
    result: fsl_verifier::BmcResult,
    solver_version: &str,
    statistics: &fsl_solver::VerificationStatistics,
    elapsed_s: f64,
) -> Value {
    let mut output = envelope(solver_version);
    output.insert("spec".to_owned(), json!(model.name));
    output.insert(
        "cost".to_owned(),
        serde_json::to_value(statistics.with_elapsed(elapsed_s))
            .expect("verification cost serializes"),
    );
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
    // deadlock-as-error before leadsto_violation: both are populated inside
    // the same per-step loop and can legitimately be `Some` at once, so this
    // order is the contract.
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
    output.insert(
        "warnings".to_owned(),
        Value::Array(fsl_runtime::verification_warnings(
            model,
            options.depth,
            options.deadlock == "warn",
            result.deadlock_step,
            result
                .deadlock_trace
                .as_ref()
                .and_then(|trace| trace.last())
                .map(|entry| &entry.state),
            &result.action_coverage,
        )),
    );
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
///
/// # Panics
///
/// Panics only if an in-memory `serde_json::Value` cannot be serialized.
#[wasm_bindgen]
pub async fn run(request_json: String) -> String {
    let solver_version = fsl_solver_z3js::version();
    let request = match serde_json::from_str::<Request>(&request_json) {
        Ok(request) => request,
        Err(failure) => {
            return error(
                &solver_version,
                "io",
                format!("invalid request JSON: {failure}"),
            )
            .to_string();
        }
    };
    let output = match request.cmd.as_str() {
        "check" => check(&request, &solver_version),
        "verify" => verify(&request, &solver_version).await,
        command => error(
            &solver_version,
            "usage",
            format!("command '{command}' is not available in the browser Worker"),
        ),
    };
    serde_json::to_string_pretty(&output).expect("JSON values serialize")
}

/// Render an internal verifier error after the Worker solver runtime initialized.
///
/// # Panics
///
/// Panics only if an in-memory `serde_json::Value` cannot be serialized.
#[wasm_bindgen]
#[must_use]
pub fn internal_error(message: String) -> String {
    let output = error(&fsl_solver_z3js::version(), "internal", message);
    serde_json::to_string_pretty(&output).expect("JSON values serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsl_core::{FslValue, TraceAction, TraceStep};
    use fsl_verifier::{BmcResult, BmcViolation, LeadsToViolation};

    const TEST_SOLVER_VERSION: &str = "Z3 4.16.0.0";

    fn model_from(source: &str) -> KernelModel {
        let resolver = MemoryResolver {
            files: BTreeMap::new(),
        };
        let kernel = fsl_core::parse_kernel_source(source, &resolver).expect("parse");
        fsl_core::build_model(kernel).expect("model")
    }

    #[test]
    fn build_rejects_duplicate_action_writes() {
        let request = Request {
            cmd: "verify".to_owned(),
            source: "spec Duplicate { state { x: Bool } init { x = false } action write_twice() { x = true x = false } }".to_owned(),
            files: BTreeMap::new(),
            options: Options::default(),
        };

        let error = build(&request, TEST_SOLVER_VERSION)
            .expect_err("duplicate write must fail in Worker build");

        assert_eq!(error["kind"], json!("semantics"));
        assert!(
            error["message"]
                .as_str()
                .is_some_and(|message| message.contains("same state location"))
        );
    }

    #[test]
    fn verified_result_contains_shared_warnings() {
        let model = model_from(
            "spec Warnings { state { x: Bool } init { x = false } \
             action blocked() { requires x x = false } \
             invariant Vacuous \"REQ-WARN: vacuous warning\" { x => x } }",
        );
        let initial = TraceStep {
            step: 0,
            state: BTreeMap::from([("x".to_owned(), FslValue::Bool(false))]),
            action: None,
            changes: BTreeMap::new(),
        };
        let result = BmcResult {
            spec: model.name.clone(),
            depth: 2,
            violation: None,
            leadsto_violation: None,
            reachables: BTreeMap::new(),
            deadlock_step: Some(0),
            deadlock_trace: Some(vec![initial]),
            action_coverage: BTreeMap::from([("blocked".to_owned(), false)]),
            frontier_progress: false,
        };

        let envelope = render_verify(
            &model,
            &Options::default(),
            result,
            TEST_SOLVER_VERSION,
            &fsl_solver::VerificationStatistics::default(),
            0.0,
        );
        let warnings = envelope["warnings"].as_array().expect("warnings array");

        assert_eq!(envelope["versions"]["verifier"]["name"], "fsl-wasm");
        assert_eq!(
            envelope["versions"]["verifier"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(envelope["versions"]["core"]["name"], "fsl-core");
        assert_eq!(envelope["versions"]["solver"]["name"], "z3");
        assert_eq!(envelope["versions"]["solver"]["backend"], "z3-solver-wasm");
        assert!(
            envelope["versions"]["solver"]["version"]
                .as_str()
                .is_some_and(|version| version.starts_with("Z3 4.16.0"))
        );
        assert_eq!(warnings.len(), 3);
        assert_eq!(warnings[0]["kind"], json!("vacuous_implication"));
        assert_eq!(
            warnings[0]["requirement"],
            json!({"id": "REQ-WARN", "text": "vacuous warning"})
        );
        assert!(warnings[0]["loc"].is_object());
        assert_eq!(warnings[1]["kind"], json!("deadlock"));
        assert_eq!(
            warnings[1]["message"],
            json!("deadlock reachable at step 0 (state: x=false)")
        );
        assert!(
            warnings[2]["message"]
                .as_str()
                .is_some_and(|message| message.contains("action 'blocked' is never enabled"))
        );
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

        let envelope = render_verify(
            &model,
            &options,
            result,
            TEST_SOLVER_VERSION,
            &fsl_solver::VerificationStatistics::default(),
            0.0,
        );

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

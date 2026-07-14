// SPDX-License-Identifier: Apache-2.0

//! WASM entry point used exclusively inside the browser verification Worker.

use std::collections::BTreeMap;

use fsl_core::{CoreError, FileResolver, KernelModel, model_warnings};
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
    #[serde(default = "default_source_file")]
    source_file: String,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    options: Options,
}

fn default_source_file() -> String {
    "spec.fsl".to_owned()
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
    let kernel =
        fsl_core::parse_kernel_source_with_file(&request.source, &resolver, &request.source_file)
            .map_err(|failure| error(solver_version, "parse", failure.to_string()))?;
    fsl_core::build_model(kernel).map_err(|failure| {
        fslc_rust::verification_output::render_semantic_error(
            envelope(solver_version),
            &failure.to_string(),
        )
    })
}

fn check(request: &Request, solver_version: &str) -> Value {
    if let Some(output) = fslc_rust::frontend_output::ai_project_check_output(
        &request.source,
        &request.source_file,
        envelope(solver_version),
    ) {
        return output;
    }
    if let Err(failure) = fsl_syntax::parse_document(fsl_syntax::SourceFile::new(&request.source)) {
        return fslc_rust::frontend_output::render_surface_parse_error(
            envelope(solver_version),
            &failure,
        );
    }
    let model = match build(request, solver_version) {
        Ok(model) => model,
        Err(error) => return error,
    };
    let has_trace_contract = match fslc_rust::verification_output::validate_requirement_trace_source(
        &envelope(solver_version),
        &request.source,
        &model,
    ) {
        Ok((Some(failure), _)) => return failure,
        Ok((None, has_contract)) => has_contract,
        Err(failure) => return error(solver_version, "semantics", failure),
    };
    let mut output = envelope(solver_version);
    output.insert("result".to_owned(), json!("ok"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("warnings".to_owned(), Value::Array(model_warnings(&model)));
    let mut output = add_frontend_metadata(
        request,
        solver_version,
        &model,
        has_trace_contract,
        8,
        Value::Object(output),
    );
    match governance_output(request) {
        Ok(Some(governance)) => {
            output
                .as_object_mut()
                .expect("check envelope")
                .insert("governance".to_owned(), governance);
        }
        Ok(None) => {}
        Err(failure) => return error(solver_version, "type", failure),
    }
    output
}

fn governance_output(request: &Request) -> Result<Option<Value>, String> {
    let resolver = MemoryResolver {
        files: request.files.clone(),
    };
    fslc_rust::verification_output::governance_output(&request.source, |preservation| {
        let implementation_source = resolver
            .read(&preservation.after_path)
            .map_err(|failure| failure.to_string())?;
        let abstraction_source = resolver
            .read(&preservation.before_path)
            .map_err(|failure| failure.to_string())?;
        let mapping_source = resolver
            .read(&preservation.refinement_path)
            .map_err(|failure| failure.to_string())?;
        let implementation = fsl_core::build_model(
            fsl_core::parse_kernel_source_with_file(
                &implementation_source,
                &resolver,
                &preservation.after_path,
            )
            .map_err(|failure| failure.to_string())?,
        )
        .map_err(|failure| failure.to_string())?;
        let abstraction = fsl_core::build_model(
            fsl_core::parse_kernel_source_with_file(
                &abstraction_source,
                &resolver,
                &preservation.before_path,
            )
            .map_err(|failure| failure.to_string())?,
        )
        .map_err(|failure| failure.to_string())?;
        let mapping = fsl_core::parse_refinement(&mapping_source, &implementation, &abstraction)
            .map_err(|failure| failure.message)?;
        if !mapping.progress.is_empty() {
            return Err(
                "governance preservation with progress requires browser refinement progress verification"
                    .to_owned(),
            );
        }
        let checked = fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 8)
            .map_err(|failure| failure.to_string())?;
        Ok(json!(if checked.failure.is_some() {
            "refinement_failed"
        } else {
            "refines"
        }))
    })
}

fn remove_generic_invariant_warning(output: &mut Value) {
    if let Some(warnings) = output.get_mut("warnings").and_then(Value::as_array_mut) {
        warnings.retain(|warning| {
            warning.get("message").and_then(Value::as_str)
                != Some("spec declares no user invariants (only implicit type bounds are checked)")
        });
    }
}

fn add_frontend_metadata(
    request: &Request,
    solver_version: &str,
    model: &KernelModel,
    has_trace_contract: bool,
    depth: usize,
    mut output: Value,
) -> Value {
    if has_trace_contract {
        remove_generic_invariant_warning(&mut output);
    }
    let resolver = MemoryResolver {
        files: request.files.clone(),
    };
    match fslc_rust::verification_output::requirements_implements_output(
        &request.source,
        &resolver,
        model,
        depth,
    ) {
        Ok(Some(implements)) => {
            output
                .as_object_mut()
                .expect("verify envelope")
                .insert("implements".to_owned(), implements);
            remove_generic_invariant_warning(&mut output);
        }
        Ok(None) => {}
        Err(failure) => return error(solver_version, "semantics", failure),
    }
    let additions = fslc_rust::frontend_output::implicit_initial_value_warnings(
        &request.source,
        &request.source_file,
    );
    if !additions.is_empty() {
        output
            .as_object_mut()
            .expect("verify envelope")
            .entry("warnings")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("warnings array")
            .extend(additions);
    }
    output
}

async fn verify(request: &Request, solver_version: &str) -> Value {
    let started = performance_now();
    if let Err(failure) = fsl_syntax::parse_surface_document(&request.source) {
        return error(solver_version, "parse", failure.to_string());
    }
    let model = match build(request, solver_version) {
        Ok(model) => model,
        Err(error) => return error,
    };
    let has_trace_contract = match fslc_rust::verification_output::validate_requirement_trace_source(
        &envelope(solver_version),
        &request.source,
        &model,
    ) {
        Ok((Some(failure), _)) => return failure,
        Ok((None, has_contract)) => has_contract,
        Err(failure) => return error(solver_version, "semantics", failure),
    };
    let deadlock =
        match fslc_rust::verification_output::DeadlockMode::parse(&request.options.deadlock) {
            Ok(deadlock) => deadlock,
            Err(message) => return error(solver_version, "usage", message),
        };
    match fsl_runtime::find_boundary_violation(model.clone(), request.options.depth) {
        Ok(Some((violation, trace))) => {
            let statistics = fsl_solver::VerificationStatistics::default();
            return fslc_rust::verification_output::render_boundary_output(
                envelope(solver_version),
                &model,
                &violation,
                &trace,
                &fslc_rust::verification_output::BmcOutputOptions {
                    depth: request.options.depth,
                    deadlock,
                    checked_bounds: None,
                    elapsed_s: (performance_now() - started) / 1000.0,
                    statistics: &statistics,
                },
            )
            .0;
        }
        Ok(None) => {}
        Err(failure) => {
            return fslc_rust::verification_output::render_semantic_error(
                envelope(solver_version),
                &failure.to_string(),
            );
        }
    }
    let mut solver = fsl_solver_z3js::Z3JsSolver::new();
    let result =
        match fsl_verifier::verify_bounded(&model, &mut solver, request.options.depth).await {
            Ok(result) => result,
            Err(failure) => {
                return fslc_rust::verification_output::render_semantic_error(
                    envelope(solver_version),
                    &failure.to_string(),
                );
            }
        };
    if let Err(failure) =
        fslc_rust::verification_output::replay_bmc_witnesses(&model, &result, None)
    {
        return error(solver_version, "internal", failure);
    }
    let statistics = fsl_solver::SmtSolver::statistics(&solver);
    let (output, _) = fslc_rust::verification_output::render_bmc_output(
        envelope(solver_version),
        &model,
        &result,
        fslc_rust::verification_output::BmcOutputOptions {
            depth: request.options.depth,
            deadlock,
            checked_bounds: None,
            elapsed_s: (performance_now() - started) / 1000.0,
            statistics: &statistics,
        },
    );
    add_frontend_metadata(
        request,
        solver_version,
        &model,
        has_trace_contract,
        request.options.depth,
        output,
    )
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
    use fsl_core::{FslValue, TraceAction, TraceStep, trace_json};
    use fsl_verifier::{BmcResult, BmcViolation, LeadsToViolation};

    const TEST_SOLVER_VERSION: &str = "Z3 4.16.0.0";

    fn model_from(source: &str) -> KernelModel {
        let resolver = MemoryResolver {
            files: BTreeMap::new(),
        };
        let kernel = fsl_core::parse_kernel_source(source, &resolver).expect("parse");
        fsl_core::build_model(kernel).expect("model")
    }

    fn render_verify(
        model: &KernelModel,
        options: &Options,
        result: &BmcResult,
        solver_version: &str,
        statistics: &fsl_solver::VerificationStatistics,
        elapsed_s: f64,
    ) -> Value {
        fslc_rust::verification_output::render_bmc_output(
            envelope(solver_version),
            model,
            result,
            fslc_rust::verification_output::BmcOutputOptions {
                depth: options.depth,
                deadlock: fslc_rust::verification_output::DeadlockMode::parse(&options.deadlock)
                    .expect("test deadlock mode is valid"),
                checked_bounds: None,
                elapsed_s,
                statistics,
            },
        )
        .0
    }

    #[test]
    fn build_rejects_duplicate_action_writes() {
        let request = Request {
            cmd: "verify".to_owned(),
            source: "spec Duplicate { state { x: Bool } init { x = false } action write_twice() { x = true x = false } }".to_owned(),
            source_file: "duplicate.fsl".to_owned(),
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
            &result,
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
            &result,
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

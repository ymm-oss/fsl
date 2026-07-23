// SPDX-License-Identifier: Apache-2.0

//! Backend-neutral JSON rendering for bounded verification results.

use std::collections::BTreeSet;
use std::future::Future;

use fsl_core::{
    FslValue, KernelExpr, KernelModel, TypeDef, TypeRef, display_name, fsl_value_json,
    insert_requirement_metadata, internal_origin_json, origin_display_name, state_json, trace_json,
};
use fsl_solver::VerificationStatistics;
use fsl_verifier::{BmcResult, BmcViolation};
use serde_json::{Map, Value, json};

/// Deadlock reporting policy shared by native and browser verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeadlockMode {
    Warn,
    Error,
    Ignore,
}

impl DeadlockMode {
    /// Parse the public CLI/Worker spelling.
    ///
    /// # Errors
    ///
    /// Returns the public usage diagnostic for an unsupported mode.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            "ignore" => Ok(Self::Ignore),
            _ => Err("--deadlock must be warn, error, or ignore".to_owned()),
        }
    }
}

/// Inputs that vary by verification invocation rather than solver result.
#[derive(Clone, Copy)]
pub struct BmcOutputOptions<'a> {
    pub depth: usize,
    pub deadlock: DeadlockMode,
    pub checked_bounds: Option<&'a BTreeSet<String>>,
    pub elapsed_s: f64,
    pub statistics: &'a VerificationStatistics,
}

/// Render a model-construction error using the public semantic classification.
#[must_use]
pub fn render_semantic_error(mut output: Map<String, Value>, message: &str) -> Value {
    let kind = semantic_error_kind(message);
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!(kind));
    output.insert("message".to_owned(), json!(message));
    if message.starts_with("struct field '") && message.ends_with(" has non-scalar type") {
        output.insert(
            "hint".to_owned(),
            json!("struct fields must be scalar (domain type, enum, Bool, Int) or Option<scalar>; use a separate Map for Set/Map/Seq/struct fields"),
        );
    }
    Value::Object(output)
}

/// A governance diagnostic with its source location preserved across delivery
/// surfaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceOutputError {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

impl GovernanceOutputError {
    #[must_use]
    pub fn new(message: impl Into<String>, line: u32, column: u32) -> Self {
        Self {
            message: message.into(),
            line,
            column,
        }
    }
}

impl From<fsl_core::CoreError> for GovernanceOutputError {
    fn from(error: fsl_core::CoreError) -> Self {
        Self::new(error.to_string(), error.line, error.column)
    }
}

/// Render a governance type diagnostic using the public JSON contract.
#[must_use]
pub fn render_governance_error(
    mut output: Map<String, Value>,
    error: &GovernanceOutputError,
) -> Value {
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!("type"));
    output.insert("message".to_owned(), json!(error.message));
    output.insert(
        "loc".to_owned(),
        json!({"line": error.line, "column": error.column}),
    );
    Value::Object(output)
}

/// Classify a typed-model diagnostic identically on every Rust delivery surface.
#[must_use]
pub fn semantic_error_kind(message: &str) -> &'static str {
    if message == "init constraints are unsatisfiable" {
        "vacuous"
    } else if message.starts_with("unknown type '")
        || message.starts_with("cannot coerce symbolic value")
        || message.starts_with("struct field '") && message.ends_with(" has non-scalar type")
    {
        "type"
    } else {
        "semantics"
    }
}

/// Evaluate a requirements-layer `implements` declaration for verify metadata.
///
/// # Errors
///
/// Returns a diagnostic when dependency resolution, lowering, or concrete
/// refinement checking fails.
pub fn requirements_implements_output(
    source: &str,
    resolver: &dyn fsl_core::FileResolver,
    model: &KernelModel,
    depth: usize,
) -> Result<Option<Value>, String> {
    let Some(contract) = fsl_core::requirements_implements(source, resolver, model)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let checked =
        fsl_runtime::check_refinement(model, &contract.abstraction, &contract.refinement, depth)
            .map_err(|error| error.to_string())?;
    Ok(Some(if let Some(failure) = checked.failure {
        json!({
            "abs": contract.abstraction.name,
            "result": "refinement_failed",
            "violation": {"result": "refinement_failed", "kind": failure.kind},
        })
    } else {
        json!({"abs": contract.abstraction.name, "result": "refines"})
    }))
}

/// Render governance relationships while delegating preservation verification
/// to the delivery surface that owns the refinement backend.
///
/// # Errors
///
/// Returns a diagnostic when the governance document is malformed.
pub fn governance_output(
    source: &str,
    resolver: &dyn fsl_core::FileResolver,
    mut preservation_result: impl FnMut(
        &fsl_core::GovernancePreservation,
    ) -> Result<Value, GovernanceOutputError>,
) -> Result<Option<Value>, GovernanceOutputError> {
    let Some(contract) =
        fsl_core::governance_contract(source, resolver).map_err(GovernanceOutputError::from)?
    else {
        return Ok(None);
    };
    let preservation_results = contract
        .preservations
        .iter()
        .map(&mut preservation_result)
        .collect::<Result<Vec<_>, GovernanceOutputError>>()?;
    Ok(Some(render_governance_contract(
        &contract,
        preservation_results,
    )))
}

/// Async counterpart of [`governance_output`] for browser solver backends.
///
/// # Errors
///
/// Returns a diagnostic when the governance document or a preservation is invalid.
pub async fn governance_output_async<F, Fut>(
    source: &str,
    resolver: &dyn fsl_core::FileResolver,
    mut preservation_result: F,
) -> Result<Option<Value>, GovernanceOutputError>
where
    F: FnMut(&fsl_core::GovernancePreservation) -> Fut,
    Fut: Future<Output = Result<Value, GovernanceOutputError>>,
{
    let Some(contract) =
        fsl_core::governance_contract(source, resolver).map_err(GovernanceOutputError::from)?
    else {
        return Ok(None);
    };
    let mut preservation_results = Vec::with_capacity(contract.preservations.len());
    for preservation in &contract.preservations {
        preservation_results.push(preservation_result(preservation).await?);
    }
    Ok(Some(render_governance_contract(
        &contract,
        preservation_results,
    )))
}

fn render_governance_contract(
    contract: &fsl_core::GovernanceContract,
    preservation_results: Vec<Value>,
) -> Value {
    let delegates = contract
        .delegates
        .iter()
        .map(|delegate| {
            let satisfied = delegate
                .satisfied
                .iter()
                .map(|(control, artifacts)| {
                    (
                        control.clone(),
                        Value::Array(
                            artifacts
                                .iter()
                                .map(|(kind, id)| json!({"kind": kind, "id": id}))
                                .collect(),
                        ),
                    )
                })
                .collect::<Map<_, _>>();
            json!({
                "business": delegate.business,
                "required": delegate.required,
                "satisfied": satisfied,
            })
        })
        .collect::<Vec<_>>();
    let preservations = contract
        .preservations
        .iter()
        .zip(preservation_results)
        .map(|(preservation, result)| {
            json!({
                "name": preservation.name,
                "before": preservation.before_name,
                "after": preservation.after_name,
                "preserve": preservation.preserve,
                "result": result,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "name": contract.name,
        "controls": contract.controls,
        "delegates": delegates,
        "preservations": preservations,
    })
}

/// Resolve one requirements trace step against the Monitor's enabled actions.
///
/// # Errors
///
/// Returns a diagnostic when arguments or enabled actions cannot be evaluated.
pub fn requirement_step_match(
    monitor: &fsl_runtime::Monitor,
    step: &fsl_core::RequirementsTraceStep,
) -> Result<(Vec<FslValue>, Option<fsl_runtime::EnabledAction>), String> {
    let mut arguments = Vec::new();
    for argument in &step.args {
        arguments.push(
            fsl_runtime::eval(
                argument,
                &monitor.state,
                &mut std::collections::BTreeMap::new(),
                &monitor.model,
                None,
            )
            .map_err(|error| error.to_string())?,
        );
    }
    let enabled = monitor.enabled().map_err(|error| error.to_string())?;
    let branch_prefix = format!("{}__b", step.name);
    for action in &monitor.model.actions {
        if action.name != step.name
            && !action.name.starts_with(&branch_prefix)
            && display_name(&action.name) != step.name
        {
            continue;
        }
        if action.params.len() != arguments.len() {
            continue;
        }
        let params = action
            .params
            .iter()
            .zip(&arguments)
            .map(|(param, value)| (param.name().to_owned(), value.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        if let Some(instance) = enabled
            .iter()
            .find(|instance| instance.action == action.name && instance.params == params)
        {
            return Ok((arguments, Some(instance.clone())));
        }
    }
    Ok((arguments, None))
}

fn requirement_step_json(step: &fsl_core::RequirementsTraceStep, arguments: &[FslValue]) -> Value {
    json!({
        "action": step.name,
        "args": arguments.iter().map(fsl_value_json).collect::<Vec<_>>(),
    })
}

fn requirement_failure_base(
    envelope: &Map<String, Value>,
    kind: &str,
    case: &fsl_core::RequirementsTraceCase,
) -> Map<String, Value> {
    let mut output = envelope.clone();
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!(kind));
    output.insert("id".to_owned(), json!(case.id));
    output.insert("text".to_owned(), json!(case.text));
    output
}

/// Validate requirements-layer acceptance and forbidden traces before proof.
///
/// # Errors
///
/// Returns a diagnostic when the trace contract cannot be parsed or executed.
///
/// # Panics
///
/// Panics only if a requirements contract bypasses parser validation and carries
/// an acceptance case without its required expectation.
#[allow(clippy::too_many_lines)]
pub fn validate_requirement_trace_source(
    envelope: &Map<String, Value>,
    source: &str,
    model: &KernelModel,
) -> Result<(Option<Value>, bool), String> {
    let Some(contract) =
        fsl_core::requirements_trace_contract(source).map_err(|error| error.to_string())?
    else {
        return Ok((None, false));
    };
    let has_contract = !contract.acceptance.is_empty() || !contract.forbidden.is_empty();
    for case in &contract.acceptance {
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).map_err(|e| e.to_string())?;
        for (index, step) in case.steps.iter().enumerate() {
            let (arguments, instance) = requirement_step_match(&monitor, step)?;
            let Some(instance) = instance else {
                let mut output = requirement_failure_base(envelope, "acceptance", case);
                output.insert("failed_step".to_owned(), json!(index));
                output.insert("step".to_owned(), requirement_step_json(step, &arguments));
                output.insert("step_results".to_owned(), json!([]));
                output.insert(
                    "loc".to_owned(),
                    json!({"line": step.line, "column": step.column}),
                );
                output.insert("trace_type".to_owned(), json!("acceptance"));
                return Ok((Some(Value::Object(output)), has_contract));
            };
            let result = monitor.step(&instance).map_err(|error| error.to_string())?;
            if result.violation.is_some() {
                let mut output = requirement_failure_base(envelope, "acceptance", case);
                output.insert("failed_step".to_owned(), json!(index));
                output.insert("step".to_owned(), requirement_step_json(step, &arguments));
                output.insert("step_results".to_owned(), json!([]));
                output.insert(
                    "loc".to_owned(),
                    json!({"line": step.line, "column": step.column}),
                );
                output.insert("trace_type".to_owned(), json!("acceptance"));
                return Ok((Some(Value::Object(output)), has_contract));
            }
        }
        let expectation = case
            .expectation
            .as_ref()
            .expect("acceptance contract always has an expectation");
        let expression = match expectation {
            fsl_core::RequirementsTraceExpectation::Expr(expression) => expression.clone(),
            fsl_core::RequirementsTraceExpectation::Stage {
                entity,
                instance,
                stage,
            } => KernelExpr::Binary {
                op: "==".to_owned(),
                left: Box::new(KernelExpr::Index(
                    Box::new(KernelExpr::Var(format!("{}_stage", entity.to_lowercase()))),
                    Box::new(KernelExpr::Num(*instance)),
                )),
                right: Box::new(KernelExpr::Var(stage.clone())),
            },
        };
        let value = fsl_runtime::eval(
            &expression,
            &monitor.state,
            &mut std::collections::BTreeMap::new(),
            &monitor.model,
            None,
        )
        .map_err(|error| error.to_string())?;
        if value != FslValue::Bool(true) {
            let mut output = requirement_failure_base(envelope, "acceptance", case);
            output.insert("failed_step".to_owned(), json!(case.steps.len()));
            output.insert("expect".to_owned(), expression.python_ast());
            output.insert("state".to_owned(), state_json(&monitor.state));
            output.insert(
                "loc".to_owned(),
                json!({"line": case.line, "column": case.column}),
            );
            output.insert("trace_type".to_owned(), json!("acceptance"));
            return Ok((Some(Value::Object(output)), has_contract));
        }
    }
    for case in &contract.forbidden {
        if case.steps.is_empty() {
            return Err(format!(
                "forbidden '{}' must have at least one step",
                case.id
            ));
        }
        let mut monitor = fsl_runtime::Monitor::new(model.clone()).map_err(|e| e.to_string())?;
        let mut accepted_trace = Vec::new();
        for (index, step) in case.steps.iter().enumerate() {
            let (arguments, instance) = requirement_step_match(&monitor, step)?;
            let is_final = index + 1 == case.steps.len();
            let Some(instance) = instance else {
                if is_final {
                    break;
                }
                let mut output = requirement_failure_base(envelope, "forbidden_setup", case);
                output.insert("failed_step".to_owned(), json!(index));
                output.insert("step".to_owned(), requirement_step_json(step, &arguments));
                output.insert("step_results".to_owned(), json!([]));
                output.insert(
                    "loc".to_owned(),
                    json!({"line": step.line, "column": step.column}),
                );
                output.insert(
                    "hint".to_owned(),
                    json!("the setup steps of a forbidden case must be enabled and ok (the trace is broken)."),
                );
                output.insert("trace_type".to_owned(), json!("forbidden"));
                return Ok((Some(Value::Object(output)), has_contract));
            };
            let params = Value::Object(
                instance
                    .params
                    .iter()
                    .map(|(name, value)| (name.clone(), fsl_value_json(value)))
                    .collect(),
            );
            let result = monitor.step(&instance).map_err(|error| error.to_string())?;
            if result.violation.is_some() {
                if is_final {
                    break;
                }
                let mut output = requirement_failure_base(envelope, "forbidden_setup", case);
                output.insert("failed_step".to_owned(), json!(index));
                output.insert("step".to_owned(), requirement_step_json(step, &arguments));
                output.insert("step_results".to_owned(), json!([]));
                output.insert(
                    "loc".to_owned(),
                    json!({"line": step.line, "column": step.column}),
                );
                output.insert("trace_type".to_owned(), json!("forbidden"));
                return Ok((Some(Value::Object(output)), has_contract));
            }
            accepted_trace.push(json!({
                "action": display_name(&instance.action),
                "params": params,
            }));
            if is_final {
                let mut output = requirement_failure_base(envelope, "forbidden", case);
                output.insert("accepted_step".to_owned(), json!(index));
                output.insert("step".to_owned(), requirement_step_json(step, &arguments));
                output.insert("accepted_trace".to_owned(), Value::Array(accepted_trace));
                output.insert("state".to_owned(), state_json(&monitor.state));
                output.insert(
                    "loc".to_owned(),
                    json!({"line": case.line, "column": case.column}),
                );
                output.insert(
                    "hint".to_owned(),
                    json!("this operation should have been rejected but was accepted. A guard or invariant may be missing."),
                );
                output.insert("trace_type".to_owned(), json!("forbidden"));
                return Ok((Some(Value::Object(output)), has_contract));
            }
        }
    }
    Ok((None, has_contract))
}

/// Replay every symbolic witness through the solver-independent Monitor.
///
/// The first symbolic state is used when no caller-provided initial state exists,
/// because a legal symbolic initializer may leave components unconstrained.
///
/// # Errors
///
/// Returns a diagnostic when any counterexample, reachable witness, or deadlock
/// trace is not a concrete execution of `model`.
pub fn replay_bmc_witnesses(
    model: &KernelModel,
    result: &BmcResult,
    initial_state: Option<&fsl_runtime::State>,
) -> Result<(), String> {
    let replay = |trace: &[fsl_core::TraceStep]| {
        initial_state
            .or_else(|| trace.first().map(|entry| &entry.state))
            .map_or_else(
                || fsl_runtime::replay_trace(model.clone(), trace),
                |state| fsl_runtime::replay_trace_from_state(model.clone(), trace, state),
            )
    };
    if let Some(violation) = &result.violation {
        replay(&violation.trace).map_err(|error| error.to_string())?;
    }
    if let Some(violation) = &result.leadsto_violation {
        replay(&violation.trace).map_err(|error| error.to_string())?;
    }
    for witness in result.reachables.values().flatten() {
        replay(&witness.trace).map_err(|error| error.to_string())?;
    }
    if let Some(trace) = &result.deadlock_trace {
        replay(trace).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn statement_location(statements: &[fsl_core::KernelStatement]) -> Value {
    statements.first().map_or(Value::Null, |statement| {
        let span = match statement {
            fsl_core::KernelStatement::Assign { span, .. }
            | fsl_core::KernelStatement::If { span, .. }
            | fsl_core::KernelStatement::ForAll { span, .. } => span,
        };
        span.python_loc()
    })
}

/// Render the concrete Monitor's first partial-operation or type-bound failure.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn render_boundary_output(
    mut output: Map<String, Value>,
    model: &KernelModel,
    violation: &fsl_runtime::Violation,
    trace: &[fsl_core::TraceStep],
    options: &BmcOutputOptions<'_>,
) -> (Value, i32) {
    output.insert("result".to_owned(), json!("violated"));
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("violation_kind".to_owned(), json!(violation.kind));
    let action = trace.last().and_then(|entry| entry.action.as_ref());
    let definition = action.and_then(|action| {
        model
            .actions
            .iter()
            .find(|definition| definition.name == action.name)
    });
    let origin = match violation.kind.as_str() {
        "invariant" => model.property_origin("invariant", &violation.name),
        "trans" => model.property_origin("trans", &violation.name),
        "type_bound" => violation
            .name
            .strip_prefix("_bounds_")
            .and_then(|name| model.state_origin(name)),
        _ => action.and_then(|action| model.action_origin(&action.name)),
    };
    output.insert(
        "invariant".to_owned(),
        json!(
            origin
                .and_then(origin_display_name)
                .map_or_else(|| display_name(&violation.name), str::to_owned)
        ),
    );
    if let Some(origin) = origin {
        output.insert(
            "generated_name".to_owned(),
            json!(display_name(&violation.name)),
        );
        output.insert("origin".to_owned(), internal_origin_json(origin));
    }
    output.insert(
        "loc".to_owned(),
        origin
            .and_then(|origin| origin.primary.as_ref())
            .and_then(|site| site.span)
            .map_or_else(
                || {
                    if violation.kind == "partial_op" {
                        definition
                            .map_or(Value::Null, |action| statement_location(&action.statements))
                    } else {
                        Value::Null
                    }
                },
                fsl_syntax::Span::python_loc,
            ),
    );
    if violation.kind == "partial_op" {
        output.insert(
            "hint".to_owned(),
            json!("guard the action with requires q.size() > 0 (or bound the index)"),
        );
    }
    output.insert("violated_at_step".to_owned(), json!(violation.step));
    let violating = if violation.kind == "type_bound" {
        let detected = violation_bindings_json(
            model,
            &violation.kind,
            &violation.name,
            None,
            trace.last().map(|entry| &entry.state),
        );
        if detected.is_null() {
            json!([{}])
        } else {
            detected
        }
    } else {
        Value::Null
    };
    output.insert("violating_bindings".to_owned(), violating.clone());
    if violation.kind == "type_bound" {
        output.insert(
            "blame".to_owned(),
            violation_blame_json(model, &violation.kind, &violation.name, None, violating),
        );
    }
    output.insert(
        "last_action".to_owned(),
        action.map_or(Value::Null, |action| {
            let action_origin = model.action_origin(&action.name);
            let mut value = json!({
                "name": action_origin
                    .and_then(origin_display_name)
                    .map_or_else(|| display_name(&action.name), str::to_owned),
                "params": action.params.iter().map(|(name, value)| (
                    name.clone(), fsl_value_json(value)
                )).collect::<Map<_, _>>(),
                "loc": definition.map(|definition| definition.span.python_loc()),
            });
            if let Some(origin) = action_origin
                && let Value::Object(value) = &mut value
            {
                value.insert(
                    "generated_name".to_owned(),
                    json!(display_name(&action.name)),
                );
                value.insert("origin".to_owned(), internal_origin_json(origin));
            }
            value
        }),
    );
    let mut rendered_trace = trace_json(model, trace);
    if let Value::Array(entries) = &mut rendered_trace {
        for entry in entries.iter_mut().skip(1) {
            if let Value::Object(entry) = entry {
                entry.insert("blame".to_owned(), json!({"guards": [], "effects": []}));
            }
        }
    }
    output.insert("trace".to_owned(), rendered_trace);
    finish(&mut output, violation.step, options);
    if violation.kind == "partial_op" {
        output.insert(
            "faithfulness_class".to_owned(),
            json!("partial_op_unguarded"),
        );
        output.insert(
            "recommended_action".to_owned(),
            json!("add the missing guard / run bounded Monitor (replay)"),
        );
    }
    output.insert("trace_type".to_owned(), json!(violation.kind));
    (Value::Object(output), 1)
}

/// Render one complete bounded-verification envelope and its process status.
#[must_use]
pub fn render_bmc_output(
    envelope: Map<String, Value>,
    model: &KernelModel,
    result: &BmcResult,
    options: BmcOutputOptions<'_>,
) -> (Value, i32) {
    if let Some(violation) = &result.violation {
        return render_violation(envelope, model, violation, &options);
    }
    let unreached = result
        .reachables
        .iter()
        .filter(|(_, witness)| witness.is_none())
        .filter_map(|(name, _)| {
            model
                .reachables
                .iter()
                .find(|property| property.name == *name)
        })
        .collect::<Vec<_>>();
    if !unreached.is_empty() {
        return render_reachable_failure(envelope, model, result, &unreached, &options);
    }
    if options.deadlock == DeadlockMode::Error
        && let Some(step) = result.deadlock_step
    {
        return render_deadlock_failure(envelope, model, result, step, &options);
    }
    if let Some(violation) = &result.leadsto_violation {
        return render_leadsto_failure(envelope, model, violation, &options);
    }
    render_success(envelope, model, result, &options)
}

/// Render an explicit-state result through the same bounded-verification
/// projection used by native BMC and the browser Worker, then add the
/// explicit engine's closure and exploration metadata.
///
/// # Errors
///
/// Returns a diagnostic when any explicit witness fails Monitor replay.
pub fn render_explicit_output(
    envelope: Map<String, Value>,
    model: &KernelModel,
    result: &fsl_runtime::ExplicitResult,
    checked_bounds: Option<&BTreeSet<String>>,
    deadlock: DeadlockMode,
    elapsed_s: f64,
) -> Result<(Value, i32), String> {
    let compatible = explicit_as_bmc(result);
    replay_bmc_witnesses(model, &compatible, None)?;
    let statistics = VerificationStatistics::default();

    if let Some(violation) = &compatible.violation {
        let options = BmcOutputOptions {
            depth: result.depth,
            deadlock,
            checked_bounds,
            elapsed_s,
            statistics: &statistics,
        };
        let (mut output, status) = render_violation(envelope, model, violation, &options);
        add_explicit_metadata(&mut output, result);
        return Ok((output, status));
    }

    if deadlock == DeadlockMode::Error
        && let Some(step) = result.deadlock_step
    {
        let options = BmcOutputOptions {
            depth: result.depth,
            deadlock,
            checked_bounds,
            elapsed_s,
            statistics: &statistics,
        };
        let (mut output, status) =
            render_deadlock_failure(envelope, model, &compatible, step, &options);
        add_explicit_metadata(&mut output, result);
        return Ok((output, status));
    }

    if result.budget_exceeded {
        return Ok(render_explicit_budget(
            envelope,
            model,
            result,
            &compatible,
            checked_bounds,
            deadlock,
            elapsed_s,
            &statistics,
        ));
    }

    let unreached = compatible
        .reachables
        .iter()
        .filter(|(_, witness)| witness.is_none())
        .filter_map(|(name, _)| {
            model
                .reachables
                .iter()
                .find(|property| property.name == *name)
        })
        .collect::<Vec<_>>();
    if !unreached.is_empty() {
        let options = BmcOutputOptions {
            depth: result.depth_reached,
            deadlock,
            checked_bounds,
            elapsed_s,
            statistics: &statistics,
        };
        let (mut output, status) =
            render_reachable_failure(envelope, model, &compatible, &unreached, &options);
        if result.closure {
            mark_reachables_definitively_unreachable(&mut output);
        }
        add_explicit_metadata(&mut output, result);
        return Ok((output, status));
    }

    Ok(render_explicit_success(
        envelope,
        model,
        result,
        &compatible,
        checked_bounds,
        deadlock,
        elapsed_s,
        &statistics,
    ))
}

fn explicit_as_bmc(result: &fsl_runtime::ExplicitResult) -> BmcResult {
    BmcResult {
        spec: result.spec.clone(),
        depth: result.depth,
        violation: result.violation.as_ref().map(|violation| BmcViolation {
            kind: violation.violation.kind.clone(),
            name: violation.violation.name.clone(),
            step: violation.violation.step,
            last_action: violation
                .trace
                .last()
                .and_then(|entry| entry.action.as_ref())
                .map(|action| action.name.clone()),
            trace: violation.trace.clone(),
            leads_to: None,
        }),
        leadsto_violation: None,
        reachables: result
            .reachables
            .iter()
            .map(|(name, witness)| {
                (
                    name.clone(),
                    witness
                        .as_ref()
                        .map(|witness| fsl_verifier::ReachableWitness {
                            step: witness.step,
                            trace: witness.trace.clone(),
                        }),
                )
            })
            .collect(),
        deadlock_step: result.deadlock_step,
        deadlock_trace: result.deadlock_trace.clone(),
        action_coverage: result.action_coverage.clone(),
        frontier_progress: !result.closure && !result.budget_exceeded,
    }
}

#[allow(clippy::too_many_arguments)]
fn render_explicit_budget(
    mut output: Map<String, Value>,
    model: &KernelModel,
    result: &fsl_runtime::ExplicitResult,
    compatible: &BmcResult,
    checked_bounds: Option<&BTreeSet<String>>,
    deadlock: DeadlockMode,
    elapsed_s: f64,
    statistics: &VerificationStatistics,
) -> (Value, i32) {
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("unknown_budget"));
    let options = BmcOutputOptions {
        depth: result.depth_reached,
        deadlock,
        checked_bounds,
        elapsed_s,
        statistics,
    };
    add_common(&mut output, model, compatible, &options);
    output.insert("depth".to_owned(), json!(result.depth));
    output.insert("completeness".to_owned(), json!("unknown"));
    if deadlock == DeadlockMode::Ignore {
        output.insert("deadlock".to_owned(), json!({"found": false}));
    }
    output.insert(
        "hint".to_owned(),
        json!(format!(
            "explicit-state exploration reached its {}-state budget; increase --explicit-budget or use --engine bmc",
            result.states_explored
        )),
    );
    output.insert(
        "cost".to_owned(),
        serde_json::to_value(statistics.with_elapsed(elapsed_s))
            .expect("verification cost serializes"),
    );
    let mut value = Value::Object(output);
    add_explicit_metadata(&mut value, result);
    (value, 1)
}

#[allow(clippy::too_many_arguments)]
fn render_explicit_success(
    mut output: Map<String, Value>,
    model: &KernelModel,
    result: &fsl_runtime::ExplicitResult,
    compatible: &BmcResult,
    checked_bounds: Option<&BTreeSet<String>>,
    deadlock: DeadlockMode,
    elapsed_s: f64,
    statistics: &VerificationStatistics,
) -> (Value, i32) {
    if !result.closure {
        let options = BmcOutputOptions {
            depth: result.depth,
            deadlock,
            checked_bounds,
            elapsed_s,
            statistics,
        };
        let (mut output, status) = render_success(output, model, compatible, &options);
        add_explicit_metadata(&mut output, result);
        return (output, status);
    }

    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("proved"));
    let options = BmcOutputOptions {
        depth: result.depth_reached,
        deadlock,
        checked_bounds,
        elapsed_s,
        statistics,
    };
    add_common(&mut output, model, compatible, &options);
    output.insert("depth".to_owned(), json!(result.depth));
    output.insert("engine".to_owned(), json!("explicit"));
    output.insert("completeness".to_owned(), json!("unbounded"));
    if deadlock == DeadlockMode::Ignore {
        output.insert("deadlock".to_owned(), json!({"found": false}));
    }
    output.insert(
        "warnings".to_owned(),
        Value::Array(shared_warnings(model, compatible, &options)),
    );
    output.insert(
        "note".to_owned(),
        json!(
            "explicit-state exploration reached closure; invariants hold in every reachable state"
        ),
    );
    output.insert(
        "cost".to_owned(),
        serde_json::to_value(statistics.with_elapsed(elapsed_s))
            .expect("verification cost serializes"),
    );
    let mut value = Value::Object(output);
    add_explicit_metadata(&mut value, result);
    (value, 0)
}

fn add_explicit_metadata(output: &mut Value, result: &fsl_runtime::ExplicitResult) {
    let Some(output) = output.as_object_mut() else {
        return;
    };
    output.insert("engine".to_owned(), json!("explicit"));
    output.insert("closure".to_owned(), json!(result.closure));
    output.insert("states_explored".to_owned(), json!(result.states_explored));
    output.insert(
        "max_frontier_width".to_owned(),
        json!(result.max_frontier_width),
    );
    output.insert("depth_reached".to_owned(), json!(result.depth_reached));
}

fn mark_reachables_definitively_unreachable(output: &mut Value) {
    let Some(output) = output.as_object_mut() else {
        return;
    };
    if let Some(unreached) = output.get_mut("unreached").and_then(Value::as_array_mut) {
        for item in unreached {
            if let Value::Object(item) = item {
                item.insert("classification".to_owned(), json!("unreachable"));
                item.insert(
                    "hint".to_owned(),
                    json!(
                        "not witnessed before explicit state-space closure; the goal is unreachable"
                    ),
                );
            }
        }
    }
    output.insert(
        "hint".to_owned(),
        json!("explicit state-space closure proves that the requested reachable goal cannot be reached"),
    );
}

#[allow(clippy::too_many_lines)]
fn render_violation(
    mut output: Map<String, Value>,
    model: &KernelModel,
    violation: &BmcViolation,
    options: &BmcOutputOptions<'_>,
) -> (Value, i32) {
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("violated"));
    output.insert("violation_kind".to_owned(), json!(violation.kind));
    let (property_kind, property) = if violation.kind == "trans" {
        (
            "trans",
            model
                .transitions
                .iter()
                .find(|property| property.name == violation.name),
        )
    } else {
        (
            "invariant",
            model
                .invariants
                .iter()
                .find(|property| property.name == violation.name),
        )
    };
    let origin = model.property_origin(property_kind, &violation.name);
    let rendered_name = origin
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.declaration_path.last())
        .map_or_else(|| display_name(&violation.name), String::clone);
    if violation.kind == "trans" {
        output.insert("trans".to_owned(), json!(rendered_name));
    }
    output.insert("invariant".to_owned(), json!(rendered_name));
    if let Some(origin) = origin {
        output.insert(
            "generated_name".to_owned(),
            json!(display_name(&violation.name)),
        );
        output.insert("origin".to_owned(), internal_origin_json(origin));
    }
    if let Some(property) = property {
        insert_requirement_metadata(&mut output, &property.annotations, property.meta.as_ref());
    }
    output.insert(
        "loc".to_owned(),
        property.map_or(Value::Null, |property| property.span.python_loc()),
    );
    output.insert("violated_at_step".to_owned(), json!(violation.step));
    let violating = violation_bindings_json(
        model,
        &violation.kind,
        &violation.name,
        property.map(|property| &property.expr),
        violation.trace.last().map(|entry| &entry.state),
    );
    output.insert("violating_bindings".to_owned(), violating.clone());
    output.insert(
        "blame".to_owned(),
        violation_blame_json(
            model,
            &violation.kind,
            &violation.name,
            property.map(|property| &property.expr),
            violating,
        ),
    );
    output.insert(
        "last_action".to_owned(),
        violation
            .trace
            .last()
            .and_then(|entry| entry.action.as_ref())
            .map_or(Value::Null, |action| {
                let definition = model
                    .actions
                    .iter()
                    .find(|definition| definition.name == action.name);
                let origin = model.action_origin(&action.name);
                let mut rendered = json!({
                    "name": origin
                        .and_then(|origin| origin.primary.as_ref())
                        .and_then(|site| site.declaration_path.last())
                        .map_or_else(|| display_name(&action.name), String::clone),
                    "params": action.params.iter().map(|(name, value)| (
                        name.clone(), fsl_value_json(value)
                    )).collect::<Map<_, _>>(),
                    "loc": definition.map(|definition| definition.span.python_loc()),
                });
                if let Some(origin) = origin
                    && let Value::Object(rendered) = &mut rendered
                {
                    rendered.insert(
                        "generated_name".to_owned(),
                        json!(display_name(&action.name)),
                    );
                    rendered.insert("origin".to_owned(), internal_origin_json(origin));
                }
                rendered
            }),
    );
    let mut trace = trace_json(model, &violation.trace);
    if let Value::Array(entries) = &mut trace {
        for entry in entries.iter_mut().skip(1) {
            if let Value::Object(entry) = entry {
                entry.insert("blame".to_owned(), json!({"guards": [], "effects": []}));
            }
        }
    }
    output.insert("trace".to_owned(), trace);
    finish(&mut output, violation.step, options);
    output.insert(
        "trace_type".to_owned(),
        json!(if violation.name.starts_with("_deadline_") {
            "sla"
        } else {
            violation.kind.as_str()
        }),
    );
    (Value::Object(output), 1)
}

fn render_reachable_failure(
    mut output: Map<String, Value>,
    model: &KernelModel,
    result: &BmcResult,
    unreached: &[&fsl_core::PropertyDef],
    options: &BmcOutputOptions<'_>,
) -> (Value, i32) {
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("reachable_failed"));
    output.insert(
        "unreached".to_owned(),
        Value::Array(
            unreached
                .iter()
                .map(|property| {
                    let origin = model.property_origin("reachable", &property.name);
                    let mut item = json!({
                        "name": origin
                            .and_then(|origin| origin.primary.as_ref())
                            .and_then(|site| site.declaration_path.last())
                            .map_or_else(|| display_name(&property.name), String::clone),
                        "loc": property.span.python_loc(),
                        "classification": "insufficient_depth",
                        "hint": format!("not witnessed within depth {}; try a larger --depth", options.depth),
                        "faithfulness_class": "intent_unexercised",
                        "recommended_action": "add a single-shot reachable for the action / raise --depth",
                    });
                    if let Value::Object(item) = &mut item {
                        insert_requirement_metadata(
                            item,
                            &property.annotations,
                            property.meta.as_ref(),
                        );
                        if let Some(origin) = origin {
                            item.insert(
                                "generated_name".to_owned(),
                                json!(display_name(&property.name)),
                            );
                            item.insert("origin".to_owned(), internal_origin_json(origin));
                        }
                    }
                    item
                })
                .collect(),
        ),
    );
    add_common(&mut output, model, result, options);
    output.remove("reachables");
    output.remove("deadlock");
    output.insert(
        "hint".to_owned(),
        json!(format!(
            "within depth {} no trace satisfies the property; guards may be too strong (see action_coverage), or increase --depth",
            options.depth
        )),
    );
    output.insert("faithfulness_class".to_owned(), json!("intent_unexercised"));
    output.insert(
        "recommended_action".to_owned(),
        json!("add a single-shot reachable for the action / raise --depth"),
    );
    finish(&mut output, options.depth, options);
    output.insert("trace_type".to_owned(), json!("reachable"));
    (Value::Object(output), 1)
}

fn render_deadlock_failure(
    mut output: Map<String, Value>,
    model: &KernelModel,
    result: &BmcResult,
    step: usize,
    options: &BmcOutputOptions<'_>,
) -> (Value, i32) {
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("violated"));
    output.insert("violation_kind".to_owned(), json!("deadlock"));
    output.insert("invariant".to_owned(), json!("deadlock"));
    output.insert("violated_at_step".to_owned(), json!(step));
    if let Some(trace) = &result.deadlock_trace {
        output.insert("trace".to_owned(), trace_json(model, trace));
        output.insert(
            "last_action".to_owned(),
            trace.last().and_then(|entry| entry.action.as_ref()).map_or(
                Value::Null,
                |action| json!({"name": display_name(&action.name)}),
            ),
        );
    }
    finish(&mut output, step, options);
    output.insert("trace_type".to_owned(), json!("deadlock"));
    (Value::Object(output), 1)
}

fn render_leadsto_failure(
    mut output: Map<String, Value>,
    model: &KernelModel,
    violation: &BmcViolation,
    options: &BmcOutputOptions<'_>,
) -> (Value, i32) {
    let Some(details) = &violation.leads_to else {
        output.insert("result".to_owned(), json!("error"));
        output.insert("kind".to_owned(), json!("internal"));
        output.insert("message".to_owned(), json!("missing leadsTo diagnostics"));
        return (Value::Object(output), 3);
    };
    let property = model
        .leadstos
        .iter()
        .find(|property| property.name == violation.name);
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("violated"));
    output.insert("violation_kind".to_owned(), json!("leadsTo"));
    let name = origin_aware_property_name(&mut output, model, "leadsTo", &violation.name);
    output.insert("invariant".to_owned(), json!(name));
    output
        .entry("loc".to_owned())
        .or_insert_with(|| property.map_or(Value::Null, |property| property.span.python_loc()));
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
    output.insert("trace".to_owned(), trace_json(model, &violation.trace));
    output.insert("hint".to_owned(), json!(details.hint));
    finish(&mut output, options.depth, options);
    output.insert("trace_type".to_owned(), json!("leadsTo"));
    (Value::Object(output), 1)
}

fn render_success(
    mut output: Map<String, Value>,
    model: &KernelModel,
    result: &BmcResult,
    options: &BmcOutputOptions<'_>,
) -> (Value, i32) {
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("verified"));
    add_common(&mut output, model, result, options);
    if options.deadlock == DeadlockMode::Ignore {
        output.insert("deadlock".to_owned(), json!({"found": false}));
    }
    if !model.leadstos.is_empty() {
        output.insert(
            "leads_to".to_owned(),
            Value::Object(
                model
                    .leadstos
                    .iter()
                    .map(|property| {
                        let mut checked = json!({"checked_to_depth": options.depth});
                        if let Some(within) = property.within
                            && let Value::Object(entry) = &mut checked
                        {
                            entry.insert("within".to_owned(), json!(within));
                        }
                        (display_name(&property.name), checked)
                    })
                    .collect(),
            ),
        );
    }
    output.insert(
        "warnings".to_owned(),
        Value::Array(shared_warnings(model, result, options)),
    );
    output.insert(
        "note".to_owned(),
        json!(format!(
            "bounded verification: no violation within depth {}",
            options.depth
        )),
    );
    if result.frontier_progress {
        output.insert(
            "hint".to_owned(),
            json!(format!(
                "state space not saturated at depth {}; a violation could exist beyond depth {}; consider a larger --depth or the induction engine",
                options.depth, options.depth
            )),
        );
    }
    finish(&mut output, options.depth, options);
    (Value::Object(output), 0)
}

fn finish(output: &mut Map<String, Value>, checked: usize, options: &BmcOutputOptions<'_>) {
    output.insert("checked_to_depth".to_owned(), json!(checked));
    output.insert("completeness".to_owned(), json!("bounded"));
    output.insert(
        "cost".to_owned(),
        serde_json::to_value(options.statistics.with_elapsed(options.elapsed_s))
            .expect("verification cost serializes"),
    );
}

fn add_common(
    output: &mut Map<String, Value>,
    model: &KernelModel,
    result: &BmcResult,
    options: &BmcOutputOptions<'_>,
) {
    output.insert("depth".to_owned(), json!(options.depth));
    output.insert("checked_to_depth".to_owned(), json!(options.depth));
    output.insert("completeness".to_owned(), json!("bounded"));
    output.insert(
        "invariants_checked".to_owned(),
        Value::Array(
            invariant_names_selected(model, options.checked_bounds)
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );
    output.insert(
        "transitions_checked".to_owned(),
        Value::Array(
            model
                .transitions
                .iter()
                .map(|property| Value::String(display_name(&property.name)))
                .collect(),
        ),
    );
    output.insert(
        "reachables".to_owned(),
        Value::Object(
            result
                .reachables
                .iter()
                .filter_map(|(name, witness)| {
                    witness.as_ref().map(|witness| {
                        (
                            display_name(name),
                            json!({
                                "witnessed_at_step": witness.step,
                                "witness": trace_json(model, &witness.trace),
                            }),
                        )
                    })
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
                .map(|(name, covered)| {
                    (
                        display_name(name),
                        if *covered {
                            json!(true)
                        } else {
                            let action = model
                                .actions
                                .iter()
                                .find(|action| action.name == *name);
                            let mut diagnostic = json!({
                                "covered": false,
                                "blocking_requires": [],
                                "hint": coverage_hint(options.depth),
                                "faithfulness_class": "intent_unexercised",
                                "recommended_action": "add a single-shot reachable for the action / raise --depth",
                            });
                            if let Some(action) = action
                                && let Value::Object(entry) = &mut diagnostic
                            {
                                insert_requirement_metadata(
                                    entry,
                                    &action.annotations,
                                    action.meta.as_ref(),
                                );
                            }
                            diagnostic
                        },
                    )
                })
                .collect(),
        ),
    );
    output.insert(
        "deadlock".to_owned(),
        result.deadlock_step.map_or_else(
            || json!({"found": false}),
            |step| {
                json!({
                    "found": true,
                    "at_step": step,
                    "trace": result.deadlock_trace.as_ref().map(|trace| trace_json(model, trace)),
                })
            },
        ),
    );
}

fn shared_warnings(
    model: &KernelModel,
    result: &BmcResult,
    options: &BmcOutputOptions<'_>,
) -> Vec<Value> {
    fsl_runtime::verification_warnings(
        model,
        options.depth,
        options.deadlock == DeadlockMode::Warn,
        result.deadlock_step,
        result
            .deadlock_trace
            .as_ref()
            .and_then(|trace| trace.last())
            .map(|entry| &entry.state),
        &result.action_coverage,
    )
}

fn invariant_names_selected(
    model: &KernelModel,
    checked_bounds: Option<&BTreeSet<String>>,
) -> Vec<String> {
    let mut names = model
        .state
        .iter()
        .filter(|(_, ty)| has_bounds(model, ty))
        .map(|(name, _)| format!("_bounds_{name}"))
        .filter(|name| checked_bounds.is_none_or(|selected| selected.contains(name)))
        .map(|name| display_name(&name))
        .collect::<Vec<_>>();
    names.extend(
        model
            .invariants
            .iter()
            .map(|property| display_name(&property.name)),
    );
    names
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

fn coverage_hint(depth: usize) -> String {
    format!(
        "these requires clauses are unsatisfiable at every step up to depth {depth}; weaken one of them, add an action that establishes them, or increase --depth"
    )
}

fn bindings_json(bindings: &[std::collections::BTreeMap<String, FslValue>]) -> Value {
    Value::Array(
        bindings
            .iter()
            .map(|binding| {
                Value::Object(
                    binding
                        .iter()
                        .map(|(name, value)| (name.clone(), fsl_value_json(value)))
                        .collect(),
                )
            })
            .collect(),
    )
}

fn violation_bindings_json(
    model: &KernelModel,
    kind: &str,
    name: &str,
    expr: Option<&KernelExpr>,
    state: Option<&std::collections::BTreeMap<String, FslValue>>,
) -> Value {
    let Some(state) = state else {
        return Value::Null;
    };
    if kind == "type_bound" {
        let state_name = name.strip_prefix("_bounds_").unwrap_or(name);
        let Some((_, TypeRef::Map(_, value_ty))) = model
            .state
            .iter()
            .find(|(candidate, _)| candidate == state_name)
        else {
            return Value::Null;
        };
        let Some(FslValue::Map(entries)) = state.get(state_name) else {
            return Value::Null;
        };
        let bad = entries
            .iter()
            .filter(|(_, value)| {
                !fsl_runtime::value_conforms(value, value_ty, model).unwrap_or(false)
            })
            .map(|(key, _)| {
                let mut binding = std::collections::BTreeMap::new();
                binding.insert("key".to_owned(), key.clone());
                binding
            })
            .collect::<Vec<_>>();
        return if bad.is_empty() {
            Value::Null
        } else {
            bindings_json(&bad)
        };
    }
    let Some(expr) = expr else {
        return Value::Null;
    };
    fsl_runtime::violating_bindings(expr, state, model)
        .ok()
        .flatten()
        .map_or(Value::Null, |bindings| bindings_json(&bindings))
}

fn violation_blame_json(
    model: &KernelModel,
    kind: &str,
    name: &str,
    expr: Option<&KernelExpr>,
    violating_bindings: Value,
) -> Value {
    if kind == "type_bound" {
        let state_name = name.strip_prefix("_bounds_").unwrap_or(name);
        return json!({
            "conjuncts": [{
                "index": 0,
                "text": format!("{} stays within its declared type bounds", display_name(state_name)),
                "holds": false,
            }]
        });
    }
    let Some(expr) = expr else {
        return json!({"conjuncts": []});
    };
    let mut conjunct = json!({
        "index": 0,
        "text": crate::source_expr_text(model, expr),
        "holds": false,
    });
    if !violating_bindings.is_null()
        && let Value::Object(entry) = &mut conjunct
    {
        entry.insert("violating_bindings".to_owned(), violating_bindings);
    }
    json!({"conjuncts": [conjunct]})
}

fn origin_aware_property_name(
    output: &mut Map<String, Value>,
    model: &KernelModel,
    kind: &str,
    name: &str,
) -> String {
    let Some(origin) = model.property_origin(kind, name) else {
        return display_name(name);
    };
    output.insert("generated_name".to_owned(), json!(display_name(name)));
    output.insert("origin".to_owned(), internal_origin_json(origin));
    if let Some(span) = origin.primary.as_ref().and_then(|site| site.span) {
        output.insert("loc".to_owned(), span.python_loc());
    }
    origin_display_name(origin).map_or_else(|| display_name(name), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_model(source: &str) -> KernelModel {
        let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
            .expect("lower source");
        fsl_core::build_model(kernel).expect("build model")
    }

    fn test_envelope() -> Map<String, Value> {
        Map::from_iter([("fsl".to_owned(), json!("1.0"))])
    }

    #[test]
    fn explicit_renderer_rejects_a_corrupted_violation_before_rendering() {
        let model = checked_model(
            r"spec CorruptEvidence {
  state { done: Bool }
  init { done = false }
  action finish() { requires not done done = true }
  invariant NeverDone { not done }
}",
        );
        let explicit =
            fsl_runtime::verify_explicit(model.clone(), 2, 100).expect("run explicit verification");
        let mut explicit = explicit;
        let trace = &mut explicit
            .violation
            .as_mut()
            .expect("violation evidence")
            .trace;
        trace
            .last_mut()
            .expect("violating state")
            .state
            .insert("done".to_owned(), FslValue::Bool(false));

        let rendered = render_explicit_output(
            test_envelope(),
            &model,
            &explicit,
            None,
            DeadlockMode::Ignore,
            0.0,
        );
        assert!(rendered.is_err());
    }

    #[test]
    fn explicit_bounded_success_uses_byte_identical_shared_rendering() {
        let model = checked_model(
            r"spec SharedRendering {
  state { active: Bool }
  init { active = false }
  action toggle() { active = not active }
  invariant BooleanState { active or not active }
}",
        );
        let explicit =
            fsl_runtime::verify_explicit(model.clone(), 0, 100).expect("run explicit verification");
        assert!(!explicit.closure);
        let compatible = explicit_as_bmc(&explicit);
        let statistics = VerificationStatistics::default();
        let (bmc, bmc_status) = render_bmc_output(
            test_envelope(),
            &model,
            &compatible,
            BmcOutputOptions {
                depth: explicit.depth,
                deadlock: DeadlockMode::Ignore,
                checked_bounds: None,
                elapsed_s: 0.0,
                statistics: &statistics,
            },
        );
        let (mut explicit_output, explicit_status) = render_explicit_output(
            test_envelope(),
            &model,
            &explicit,
            None,
            DeadlockMode::Ignore,
            0.0,
        )
        .expect("explicit evidence replays");
        let explicit_envelope = explicit_output.as_object_mut().expect("explicit envelope");
        for key in [
            "engine",
            "closure",
            "states_explored",
            "max_frontier_width",
            "depth_reached",
        ] {
            explicit_envelope.remove(key);
        }

        assert_eq!(explicit_status, bmc_status);
        assert_eq!(
            serde_json::to_vec_pretty(&explicit_output).expect("serialize explicit output"),
            serde_json::to_vec_pretty(&bmc).expect("serialize BMC output")
        );
    }
}

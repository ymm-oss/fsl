// SPDX-License-Identifier: Apache-2.0

use super::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VerificationEngine {
    Bmc,
    Induction,
    Explicit,
}

impl VerificationEngine {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "bmc" => Ok(Self::Bmc),
            "induction" => Ok(Self::Induction),
            "explicit" => Ok(Self::Explicit),
            _ => Err("--engine must be bmc, induction, or explicit".to_owned()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DeadlockMode {
    Warn,
    Error,
    Ignore,
}

impl DeadlockMode {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "warn" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            "ignore" => Ok(Self::Ignore),
            _ => Err("--deadlock must be warn, error, or ignore".to_owned()),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ModelSelection<'a> {
    pub(super) path: &'a Path,
    pub(super) scope: Option<&'a ScopeBounds>,
    pub(super) property: Option<&'a str>,
    pub(super) excluded: &'a [String],
}

#[derive(Clone, Copy)]
pub(super) struct BmcRequest<'a> {
    pub(super) selection: ModelSelection<'a>,
    pub(super) depth: usize,
    pub(super) deadlock: DeadlockMode,
    pub(super) initial_state: Option<&'a std::collections::BTreeMap<String, FslValue>>,
}

#[derive(Clone, Copy)]
pub(super) struct InductionRequest<'a> {
    pub(super) selection: ModelSelection<'a>,
    pub(super) depth: usize,
    pub(super) deadlock: DeadlockMode,
    pub(super) k: usize,
    pub(super) auxiliary: &'a [(String, KernelExpr)],
}

#[derive(Clone, Copy)]
pub(super) struct ExplicitRequest<'a> {
    pub(super) selection: ModelSelection<'a>,
    pub(super) depth: usize,
    pub(super) deadlock: DeadlockMode,
    pub(super) budget: usize,
}

type CommandResult = (Value, i32);

fn origin_aware_property_name(
    output: &mut Map<String, Value>,
    model: &KernelModel,
    kind: &str,
    name: &str,
) -> String {
    let Some(origin) = model.property_origin(kind, name) else {
        return display(name);
    };
    output.insert("generated_name".to_owned(), json!(display(name)));
    output.insert(
        "origin".to_owned(),
        ::fslc_rust::internal_origin_json(origin),
    );
    if let Some(span) = origin.primary.as_ref().and_then(|site| site.span) {
        output.insert("loc".to_owned(), span.python_loc());
    }
    ::fslc_rust::origin_display_name(origin).map_or_else(|| display(name), str::to_owned)
}

fn origin_aware_action_json(
    model: &KernelModel,
    name: &str,
    params: &Map<String, Value>,
    fallback_loc: Value,
) -> Value {
    let Some(origin) = model.action_origin(name) else {
        return json!({"name": display(name), "params": params, "loc": fallback_loc});
    };
    let loc = origin
        .primary
        .as_ref()
        .and_then(|site| site.span)
        .map_or(fallback_loc, fsl_syntax::Span::python_loc);
    json!({
        "name": ::fslc_rust::origin_display_name(origin)
            .map_or_else(|| display(name), str::to_owned),
        "generated_name": display(name),
        "params": params,
        "loc": loc,
        "origin": ::fslc_rust::internal_origin_json(origin),
    })
}

struct PreparedBmc {
    model: KernelModel,
    checked_bounds: Option<std::collections::BTreeSet<String>>,
}

fn load_selected_model(selection: ModelSelection<'_>) -> Result<KernelModel, String> {
    selection
        .scope
        .map_or_else(
            || load_model(selection.path),
            |scope| load_model_scoped(selection.path, scope),
        )
        .and_then(|mut model| {
            select_properties(&mut model, selection.property, selection.excluded)?;
            Ok(model)
        })
}

pub(super) fn run_induction_filtered(request: InductionRequest<'_>) -> (Value, i32) {
    let InductionRequest {
        selection,
        depth,
        deadlock,
        k,
        auxiliary,
    } = request;
    let started = Instant::now();
    let (base_value, base_status) = run_bmc_filtered(BmcRequest {
        selection,
        depth,
        deadlock,
        initial_state: None,
    });
    let Value::Object(base) = &base_value else {
        return (
            error_output("internal", "BMC returned a non-object envelope"),
            3,
        );
    };
    if base.get("result").and_then(Value::as_str) != Some("verified") {
        return (base_value, base_status);
    }

    let model = match load_induction_model(selection, auxiliary) {
        Ok(model) => model,
        Err(output) => return output,
    };
    let mut solver = match fsl_solver_z3::Z3Solver::new() {
        Ok(solver) => solver,
        Err(error) => return (error_output("internal", &error.to_string()), 3),
    };
    let induction = match block_on_native(fsl_verifier::prove_induction(&model, &mut solver, k)) {
        Ok(result) => result,
        Err(error) => return (error_output("semantics", &error.to_string()), 2),
    };

    if let Some(cti) = &induction.cti {
        return render_induction_cti(&model, cti, depth, started);
    }

    let ranked = match block_on_native(fsl_verifier::prove_ranked_leadstos(&model, &mut solver)) {
        Ok(result) => result,
        Err(error) => return (error_output("semantics", &error.to_string()), 2),
    };
    if let Some(failure) = &ranked.failure {
        return render_rank_failure(&model, failure, depth, started);
    }
    render_induction_success(&model, base, &induction, &ranked, depth, started)
}

fn load_induction_model(
    selection: ModelSelection<'_>,
    auxiliary: &[(String, KernelExpr)],
) -> Result<KernelModel, CommandResult> {
    let mut model =
        load_selected_model(selection).map_err(|error| (semantic_error_output(&error), 2))?;
    model
        .invariants
        .extend(auxiliary.iter().map(|(name, expr)| fsl_core::PropertyDef {
            name: name.clone(),
            expr: expr.clone(),
            span: synthetic_span(),
            meta: None,
        }));
    Ok(model)
}

fn render_induction_cti(
    model: &KernelModel,
    cti: &fsl_verifier::InductionCti,
    depth: usize,
    started: Instant,
) -> (Value, i32) {
    let mut output = envelope();
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("unknown_cti"));
    let property_kind = if cti.kind == "trans" {
        "trans"
    } else {
        "invariant"
    };
    let name = origin_aware_property_name(&mut output, model, property_kind, &cti.name);
    if cti.kind == "trans" {
        output.insert("trans".to_owned(), json!(name));
    }
    output.insert("invariant".to_owned(), json!(name));
    output.insert("k".to_owned(), json!(cti.k));
    output.insert("checked_to_depth".to_owned(), json!(depth));
    output.insert("completeness".to_owned(), json!("bounded"));
    output.insert("trace_type".to_owned(), json!("induction_cti"));
    output.insert(
        "cti".to_owned(),
        json!({
            "states": ::fslc_rust::trace_json(model, &cti.trace),
            "violated_at": cti.k,
        }),
    );
    output.insert(
        "hint".to_owned(),
        json!("this state sequence satisfies all invariants but leads to a violation; the start state may be unreachable — add an auxiliary invariant that excludes it, then re-run"),
    );
    if cti.kind == "trans" {
        output.insert(
            "invariants_checked".to_owned(),
            Value::Array(
                invariant_names(model)
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
                    .map(|property| Value::String(display(&property.name)))
                    .collect(),
            ),
        );
    }
    output.insert(
        "cost".to_owned(),
        json!({"elapsed_s": started.elapsed().as_secs_f64()}),
    );
    (Value::Object(output), 1)
}

fn render_rank_failure(
    model: &KernelModel,
    failure: &fsl_verifier::RankFailure,
    depth: usize,
    started: Instant,
) -> (Value, i32) {
    let property = model
        .leadstos
        .iter()
        .find(|property| property.name == failure.name);
    let mut output = envelope();
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("unknown_cti"));
    output.insert("violation_kind".to_owned(), json!("leadsTo_rank"));
    let name = origin_aware_property_name(&mut output, model, "leadsTo", &failure.name);
    output.insert("invariant".to_owned(), json!(name));
    output
        .entry("loc".to_owned())
        .or_insert_with(|| property.map_or(Value::Null, |property| property.span.python_loc()));
    output.insert(
        "bindings".to_owned(),
        Value::Object(
            failure
                .bindings
                .iter()
                .map(|(name, value)| (name.clone(), ::fslc_rust::fsl_value_json(value)))
                .collect(),
        ),
    );
    output.insert(
        "measure".to_owned(),
        json!(ranking_measure_text(&failure.measure)),
    );
    output.insert("rank_failure".to_owned(), json!(failure.kind));
    if let Some(value) = failure.measure_value {
        output.insert("measure_value".to_owned(), json!(value));
    }
    if let Some(value) = failure.measure_before {
        output.insert("measure_before".to_owned(), json!(value));
    }
    if let Some(value) = failure.measure_after {
        output.insert("measure_after".to_owned(), json!(value));
    }
    if let Some(action_name) = &failure.action {
        let action = failure.trace.last().and_then(|entry| entry.action.as_ref());
        let definition = model
            .actions
            .iter()
            .find(|definition| definition.name == *action_name);
        let params = action
            .map(|action| {
                action
                    .params
                    .iter()
                    .map(|(name, value)| (name.clone(), ::fslc_rust::fsl_value_json(value)))
                    .collect::<Map<_, _>>()
            })
            .unwrap_or_default();
        output.insert(
            "last_action".to_owned(),
            origin_aware_action_json(
                model,
                action_name,
                &params,
                definition.map_or(Value::Null, |definition| definition.span.python_loc()),
            ),
        );
    }
    output.insert(
        "cti".to_owned(),
        json!({
            "states": ::fslc_rust::trace_json(model, &failure.trace),
            "violated_at": failure.trace.len().saturating_sub(1),
        }),
    );
    output.insert("hint".to_owned(), json!(failure.hint));
    output.insert("message".to_owned(), json!(failure.message));
    output.insert("checked_to_depth".to_owned(), json!(depth));
    output.insert("completeness".to_owned(), json!("bounded"));
    output.insert("trace_type".to_owned(), json!("induction_cti"));
    output.insert(
        "cost".to_owned(),
        json!({"elapsed_s": started.elapsed().as_secs_f64()}),
    );
    (Value::Object(output), 1)
}

fn render_induction_success(
    model: &KernelModel,
    base: &Map<String, Value>,
    induction: &fsl_verifier::InductionResult,
    ranked: &fsl_verifier::RankedLeadstoResult,
    depth: usize,
    started: Instant,
) -> (Value, i32) {
    let mut output = envelope();
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("proved"));
    output.insert("engine".to_owned(), json!("induction"));
    output.insert("completeness".to_owned(), json!("unbounded"));
    output.insert("checked_to_depth".to_owned(), json!(depth));
    output.insert(
        "k_used".to_owned(),
        Value::Object(
            induction
                .k_used
                .iter()
                .map(|(name, k)| (display(name), json!(k)))
                .collect(),
        ),
    );
    output.insert("base_depth".to_owned(), json!(depth));
    output.insert(
        "invariants_checked".to_owned(),
        Value::Array(
            invariant_names(model)
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
                .map(|property| Value::String(display(&property.name)))
                .collect(),
        ),
    );
    for key in ["action_coverage", "reachables"] {
        if let Some(value) = base.get(key) {
            output.insert(key.to_owned(), value.clone());
        }
    }
    let warnings = base
        .get("warnings")
        .and_then(Value::as_array)
        .map(|warnings| {
            warnings
                .iter()
                .filter(|warning| {
                    !warning
                        .get("message")
                        .and_then(Value::as_str)
                        .is_some_and(|message| message.contains("deadlock"))
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    output.insert("warnings".to_owned(), Value::Array(warnings));
    if let Some(leads_to) = base.get("leads_to") {
        let mut leads_to = leads_to.as_object().cloned().unwrap_or_default();
        for proof in &ranked.proofs {
            let entry = leads_to
                .entry(display(&proof.name))
                .or_insert_with(|| json!({"checked_to_depth": depth}));
            if let Value::Object(entry) = entry {
                entry.insert("proved".to_owned(), json!(true));
                entry.insert("completeness".to_owned(), json!("unbounded"));
                entry.insert("proof".to_owned(), json!("ranking"));
                entry.insert(
                    "decreases".to_owned(),
                    json!(ranking_measure_text(&proof.measure)),
                );
            }
        }
        output.insert("leads_to".to_owned(), Value::Object(leads_to));
        let note = if ranked.proofs.is_empty() {
            format!("invariants proved for all depths; leadsTo checked to depth {depth} only")
        } else if ranked.proofs.len() == model.leadstos.len() {
            "invariants and ranked leadsTo proved for all depths".to_owned()
        } else {
            format!(
                "invariants and ranked leadsTo proved for all depths; unranked leadsTo checked to depth {depth} only"
            )
        };
        output.insert("note".to_owned(), json!(note));
    }
    output.insert(
        "cost".to_owned(),
        json!({"elapsed_s": started.elapsed().as_secs_f64()}),
    );
    (Value::Object(output), 0)
}

fn ranking_measure_text(expr: &KernelExpr) -> String {
    let text = ::fslc_rust::expr_text(expr);
    if matches!(expr, KernelExpr::Binary { .. }) {
        format!("({text})")
    } else {
        text
    }
}

pub(super) fn run_explicit_filtered(request: ExplicitRequest<'_>) -> (Value, i32) {
    let started = Instant::now();
    let model = match load_selected_model(request.selection) {
        Ok(model) => model,
        Err(error) => return (semantic_error_output(&error), 2),
    };
    if model.actions.is_empty() {
        return (semantic_error_output("spec has no actions"), 2);
    }
    if model
        .actions
        .iter()
        .any(|action| duplicate_statement_write(&action.statements).is_some())
    {
        return (
            error_output(
                "semantics",
                "an action may not assign the same state location more than once",
            ),
            2,
        );
    }
    let checked_bounds = selected_implicit_bounds(
        &model,
        request.selection.property,
        request.selection.excluded,
    );
    let result = match fsl_runtime::verify_explicit_selected(
        model.clone(),
        request.depth,
        request.budget,
        checked_bounds.as_ref(),
    ) {
        Ok(result) => result,
        Err(error) => return (semantic_error_output(&error.to_string()), 2),
    };
    render_explicit_result(
        &model,
        &result,
        checked_bounds.as_ref(),
        request.deadlock,
        started,
    )
}

fn explicit_as_bmc(result: &fsl_runtime::ExplicitResult) -> fsl_verifier::BmcResult {
    fsl_verifier::BmcResult {
        spec: result.spec.clone(),
        depth: result.depth,
        violation: result
            .violation
            .as_ref()
            .map(|violation| fsl_verifier::BmcViolation {
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

fn render_explicit_result(
    model: &KernelModel,
    result: &fsl_runtime::ExplicitResult,
    checked_bounds: Option<&std::collections::BTreeSet<String>>,
    deadlock: DeadlockMode,
    started: Instant,
) -> CommandResult {
    let compatible = explicit_as_bmc(result);
    if let Some(violation) = &compatible.violation {
        let (mut output, status) = render_bmc_violation(model, violation, started);
        add_explicit_metadata(&mut output, result);
        return (output, status);
    }

    if deadlock == DeadlockMode::Error
        && let Some(step) = result.deadlock_step
    {
        let (mut output, status) = render_deadlock_failure(model, &compatible, step, started);
        add_explicit_metadata(&mut output, result);
        return (output, status);
    }

    if result.budget_exceeded {
        return render_explicit_budget(
            model,
            result,
            &compatible,
            checked_bounds,
            deadlock,
            started,
        );
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
        let (mut output, status) = render_reachable_failure(
            model,
            &compatible,
            &unreached,
            result.depth_reached,
            checked_bounds,
            started,
        );
        if result.closure {
            mark_reachables_definitively_unreachable(&mut output);
        }
        add_explicit_metadata(&mut output, result);
        return (output, status);
    }

    render_explicit_success(
        model,
        result,
        &compatible,
        checked_bounds,
        deadlock,
        started,
    )
}

fn render_explicit_budget(
    model: &KernelModel,
    result: &fsl_runtime::ExplicitResult,
    compatible: &fsl_verifier::BmcResult,
    checked_bounds: Option<&std::collections::BTreeSet<String>>,
    deadlock: DeadlockMode,
    started: Instant,
) -> CommandResult {
    let mut output = envelope();
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("unknown_budget"));
    add_common_verification(
        &mut output,
        model,
        compatible,
        result.depth_reached,
        checked_bounds,
    );
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
        json!({"elapsed_s": started.elapsed().as_secs_f64()}),
    );
    let mut value = Value::Object(output);
    add_explicit_metadata(&mut value, result);
    (value, 1)
}

fn render_explicit_success(
    model: &KernelModel,
    result: &fsl_runtime::ExplicitResult,
    compatible: &fsl_verifier::BmcResult,
    checked_bounds: Option<&std::collections::BTreeSet<String>>,
    deadlock: DeadlockMode,
    started: Instant,
) -> CommandResult {
    if !result.closure {
        let (mut output, status) = render_bmc_success(
            model,
            compatible,
            result.depth,
            checked_bounds,
            deadlock,
            started,
        );
        add_explicit_metadata(&mut output, result);
        return (output, status);
    }

    let mut output = envelope();
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("proved"));
    add_common_verification(
        &mut output,
        model,
        compatible,
        result.depth_reached,
        checked_bounds,
    );
    output.insert("depth".to_owned(), json!(result.depth));
    output.insert("engine".to_owned(), json!("explicit"));
    output.insert("completeness".to_owned(), json!("unbounded"));
    if deadlock == DeadlockMode::Ignore {
        output.insert("deadlock".to_owned(), json!({"found": false}));
    }
    output.insert(
        "warnings".to_owned(),
        Value::Array(verification_warnings(
            model,
            compatible,
            result.depth_reached,
            deadlock,
        )),
    );
    output.insert(
        "note".to_owned(),
        json!(
            "explicit-state exploration reached closure; invariants hold in every reachable state"
        ),
    );
    output.insert(
        "cost".to_owned(),
        json!({"elapsed_s": started.elapsed().as_secs_f64()}),
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

pub(super) fn run_bmc_filtered(request: BmcRequest<'_>) -> (Value, i32) {
    let started = Instant::now();
    let prepared = match prepare_bmc(&request, started) {
        Ok(prepared) => prepared,
        Err(output) => return output,
    };
    let result = match solve_bmc(&request, &prepared) {
        Ok(result) => result,
        Err(output) => return output,
    };
    render_bmc_result(&request, &prepared, &result, started)
}

fn prepare_bmc(request: &BmcRequest<'_>, started: Instant) -> Result<PreparedBmc, CommandResult> {
    let model = load_selected_model(request.selection)
        .map_err(|error| (semantic_error_output(&error), 2))?;
    let checked_bounds = selected_implicit_bounds(
        &model,
        request.selection.property,
        request.selection.excluded,
    );
    if model
        .actions
        .iter()
        .any(|action| duplicate_statement_write(&action.statements).is_some())
    {
        return Err((
            error_output(
                "semantics",
                "an action may not assign the same state location more than once",
            ),
            2,
        ));
    }
    if checked_bounds.is_none() && request.initial_state.is_none() {
        match fsl_runtime::find_boundary_violation(model.clone(), request.depth) {
            Ok(Some((violation, trace))) => {
                return Err(concrete_boundary_output(
                    &model, &violation, &trace, started,
                ));
            }
            Ok(None) => {}
            Err(error) => return Err((semantic_error_output(&error.to_string()), 2)),
        }
    }
    Ok(PreparedBmc {
        model,
        checked_bounds,
    })
}

fn solve_bmc(
    request: &BmcRequest<'_>,
    prepared: &PreparedBmc,
) -> Result<fsl_verifier::BmcResult, CommandResult> {
    let mut solver = match fsl_solver_z3::Z3Solver::new() {
        Ok(solver) => solver,
        Err(error) => return Err((error_output("internal", &error.to_string()), 3)),
    };
    let verification = if let Some(initial_state) = request.initial_state {
        block_on_native(fsl_verifier::verify_bounded_from_state(
            &prepared.model,
            &mut solver,
            request.depth,
            prepared.checked_bounds.as_ref(),
            initial_state,
        ))
    } else {
        block_on_native(fsl_verifier::verify_bounded_selected(
            &prepared.model,
            &mut solver,
            request.depth,
            prepared.checked_bounds.as_ref(),
        ))
    };
    let result = match verification {
        Ok(result) => result,
        Err(error) => return Err((semantic_error_output(&error.to_string()), 2)),
    };
    if let Err(error) = replay_all(&prepared.model, &result, request.initial_state) {
        return Err((error_output("internal", &error), 3));
    }
    Ok(result)
}

fn render_bmc_result(
    request: &BmcRequest<'_>,
    prepared: &PreparedBmc,
    result: &fsl_verifier::BmcResult,
    started: Instant,
) -> CommandResult {
    let model = &prepared.model;
    if let Some(violation) = &result.violation {
        return render_bmc_violation(model, violation, started);
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
        return render_reachable_failure(
            model,
            result,
            &unreached,
            request.depth,
            prepared.checked_bounds.as_ref(),
            started,
        );
    }

    if request.deadlock == DeadlockMode::Error {
        if let Some(step) = result.deadlock_step {
            return render_deadlock_failure(model, result, step, started);
        }
    }

    if let Some(violation) = &result.leadsto_violation {
        return render_leadsto_failure(model, violation, request.depth, started);
    }
    render_bmc_success(
        model,
        result,
        request.depth,
        prepared.checked_bounds.as_ref(),
        request.deadlock,
        started,
    )
}

#[allow(clippy::too_many_lines)]
fn render_bmc_violation(
    model: &KernelModel,
    violation: &fsl_verifier::BmcViolation,
    started: Instant,
) -> (Value, i32) {
    let mut output = envelope();
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
    let display_name = origin
        .and_then(|origin| origin.primary.as_ref())
        .and_then(|site| site.declaration_path.last())
        .map_or_else(|| display(&violation.name), String::clone);
    if violation.kind == "trans" {
        output.insert("trans".to_owned(), json!(display_name));
    }
    output.insert("invariant".to_owned(), json!(display_name));
    if let Some(origin) = origin {
        output.insert("generated_name".to_owned(), json!(display(&violation.name)));
        output.insert(
            "origin".to_owned(),
            ::fslc_rust::internal_origin_json(origin),
        );
    }
    if let Some(meta) = property.and_then(|property| property.meta.as_ref()) {
        output.insert("requirement".to_owned(), metadata(Some(meta)));
    }
    output.insert(
        "loc".to_owned(),
        property.map_or(Value::Null, |property| property.span.python_loc()),
    );
    output.insert("violated_at_step".to_owned(), json!(violation.step));
    let final_state = violation.trace.last().map(|entry| &entry.state);
    let violating = violation_bindings_json(
        model,
        violation.kind.as_str(),
        violation.name.as_str(),
        property.map(|property| &property.expr),
        final_state,
    );
    output.insert("violating_bindings".to_owned(), violating.clone());
    output.insert(
        "blame".to_owned(),
        violation_blame_json(
            violation.kind.as_str(),
            violation.name.as_str(),
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
                        .map_or_else(|| display(&action.name), String::clone),
                    "params": action.params.iter().map(|(name, value)| (
                        name.clone(), ::fslc_rust::fsl_value_json(value)
                    )).collect::<Map<_, _>>(),
                    "loc": definition.map(|definition| definition.span.python_loc()),
                });
                if let Some(origin) = origin
                    && let Value::Object(rendered) = &mut rendered
                {
                    rendered.insert("generated_name".to_owned(), json!(display(&action.name)));
                    rendered.insert(
                        "origin".to_owned(),
                        ::fslc_rust::internal_origin_json(origin),
                    );
                }
                rendered
            }),
    );
    let mut trace = ::fslc_rust::trace_json(model, &violation.trace);
    if let Value::Array(entries) = &mut trace {
        for entry in entries.iter_mut().skip(1) {
            if let Value::Object(entry) = entry {
                entry.insert("blame".to_owned(), json!({"guards": [], "effects": []}));
            }
        }
    }
    output.insert("trace".to_owned(), trace);
    finish(&mut output, violation.step, started);
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
    model: &KernelModel,
    result: &fsl_verifier::BmcResult,
    unreached: &[&fsl_core::PropertyDef],
    depth: usize,
    checked_bounds: Option<&std::collections::BTreeSet<String>>,
    started: Instant,
) -> (Value, i32) {
    let mut output = envelope();
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
                            .map_or_else(|| display(&property.name), String::clone),
                        "loc": property.span.python_loc(),
                        "classification": "insufficient_depth",
                        "hint": format!("not witnessed within depth {depth}; try a larger --depth"),
                        "faithfulness_class": "intent_unexercised",
                        "recommended_action": "add a single-shot reachable for the action / raise --depth",
                    });
                    if let Some(meta) = property.meta.as_ref()
                        && let Value::Object(item) = &mut item
                    {
                        item.insert("requirement".to_owned(), metadata(Some(meta)));
                    }
                    if let Some(origin) = origin
                        && let Value::Object(item) = &mut item
                    {
                        item.insert("generated_name".to_owned(), json!(display(&property.name)));
                        item.insert(
                            "origin".to_owned(),
                            ::fslc_rust::internal_origin_json(origin),
                        );
                    }
                    item
                })
                .collect(),
        ),
    );
    add_common_verification(&mut output, model, result, depth, checked_bounds);
    output.remove("reachables");
    output.remove("deadlock");
    output.insert(
        "hint".to_owned(),
        json!(format!(
            "within depth {depth} no trace satisfies the property; guards may be too strong (see action_coverage), or increase --depth"
        )),
    );
    output.insert("faithfulness_class".to_owned(), json!("intent_unexercised"));
    output.insert(
        "recommended_action".to_owned(),
        json!("add a single-shot reachable for the action / raise --depth"),
    );
    finish(&mut output, depth, started);
    output.insert("trace_type".to_owned(), json!("reachable"));
    (Value::Object(output), 1)
}

fn render_deadlock_failure(
    model: &KernelModel,
    result: &fsl_verifier::BmcResult,
    step: usize,
    started: Instant,
) -> (Value, i32) {
    let mut output = envelope();
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("violated"));
    output.insert("violation_kind".to_owned(), json!("deadlock"));
    output.insert("invariant".to_owned(), json!("deadlock"));
    output.insert("violated_at_step".to_owned(), json!(step));
    if let Some(trace) = &result.deadlock_trace {
        output.insert("trace".to_owned(), ::fslc_rust::trace_json(model, trace));
        output.insert(
            "last_action".to_owned(),
            trace
                .last()
                .and_then(|entry| entry.action.as_ref())
                .map_or(Value::Null, |action| json!({"name": display(&action.name)})),
        );
    }
    finish(&mut output, step, started);
    output.insert("trace_type".to_owned(), json!("deadlock"));
    (Value::Object(output), 1)
}

fn render_leadsto_failure(
    model: &KernelModel,
    violation: &fsl_verifier::BmcViolation,
    depth: usize,
    started: Instant,
) -> (Value, i32) {
    let Some(details) = &violation.leads_to else {
        return (error_output("internal", "missing leadsTo diagnostics"), 3);
    };
    let property = model
        .leadstos
        .iter()
        .find(|property| property.name == violation.name);
    let mut output = envelope();
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
                .map(|(name, value)| (name.clone(), ::fslc_rust::fsl_value_json(value)))
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
    output.insert(
        "trace".to_owned(),
        ::fslc_rust::trace_json(model, &violation.trace),
    );
    output.insert("hint".to_owned(), json!(details.hint));
    finish(&mut output, depth, started);
    output.insert("trace_type".to_owned(), json!("leadsTo"));
    (Value::Object(output), 1)
}

fn render_bmc_success(
    model: &KernelModel,
    result: &fsl_verifier::BmcResult,
    depth: usize,
    checked_bounds: Option<&std::collections::BTreeSet<String>>,
    deadlock: DeadlockMode,
    started: Instant,
) -> (Value, i32) {
    let mut output = envelope();
    output.insert("spec".to_owned(), json!(model.name));
    output.insert("result".to_owned(), json!("verified"));
    add_common_verification(&mut output, model, result, depth, checked_bounds);
    if deadlock == DeadlockMode::Ignore {
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
                        let mut checked = json!({"checked_to_depth": depth});
                        if let Some(within) = property.within
                            && let Value::Object(entry) = &mut checked
                        {
                            entry.insert("within".to_owned(), json!(within));
                        }
                        (display(&property.name), checked)
                    })
                    .collect(),
            ),
        );
    }
    let warnings = verification_warnings(model, result, depth, deadlock);
    output.insert("warnings".to_owned(), Value::Array(warnings));
    output.insert(
        "note".to_owned(),
        json!(format!(
            "bounded verification: no violation within depth {depth}"
        )),
    );
    if result.frontier_progress {
        output.insert(
            "hint".to_owned(),
            json!(format!(
                "state space not saturated at depth {depth}; a violation could exist beyond depth {depth}; consider a larger --depth or the induction engine"
            )),
        );
    }
    output.insert(
        "cost".to_owned(),
        json!({"elapsed_s": started.elapsed().as_secs_f64()}),
    );
    (Value::Object(output), 0)
}

fn verification_warnings(
    model: &KernelModel,
    result: &fsl_verifier::BmcResult,
    depth: usize,
    deadlock: DeadlockMode,
) -> Vec<Value> {
    let mut warnings = check_warnings(model);
    for property in &model.invariants {
        let KernelExpr::Binary { op, left, .. } = &property.expr else {
            continue;
        };
        if op != "=>" {
            continue;
        }
        if matches!(
            fsl_runtime::expression_reachable(model.clone(), left, depth),
            Ok(false)
        ) {
            let mut warning = json!({
                "kind": "vacuous_implication",
                "name": display(&property.name),
                "message": format!("invariant '{}' has an implication antecedent that is unreachable within depth {depth}", display(&property.name)),
                "hint": "the antecedent is not reachable within this depth; check whether an action that should establish it is missing, or whether the antecedent expression is wrong",
                "loc": property.span.python_loc(),
                "classification": "insufficient_depth",
                "blocking": [],
                "faithfulness_class": "intent_unexercised",
                "recommended_action": "add a single-shot reachable for the action / raise --depth",
            });
            if let Some(metadata) = &property.meta
                && let Value::Object(warning) = &mut warning
            {
                warning.insert(
                    "requirement".to_owned(),
                    json!({"id": metadata.id, "text": metadata.text}),
                );
            }
            warnings.push(warning);
        }
    }
    if deadlock == DeadlockMode::Warn
        && let Some(step) = result.deadlock_step
    {
        let state_summary = result
            .deadlock_trace
            .as_ref()
            .and_then(|trace| trace.last())
            .map(|entry| format_state_summary(model, &entry.state))
            .unwrap_or_default();
        warnings.push(json!({
            "message": format!("deadlock reachable at step {step} (state: {state_summary})"),
            "hint": "add an enabled action, declare intended stops in a terminal { } block, or use --deadlock=ignore if intentional",
        }));
    }
    for (name, covered) in &result.action_coverage {
        if !covered {
            warnings.push(json!({
                "message": format!("action '{}' is never enabled within depth {depth} — the spec may be vacuous (check its requires clauses)", display(name)),
                "hint": coverage_hint(depth),
            }));
        }
    }
    warnings
}

fn add_common_verification(
    output: &mut Map<String, Value>,
    model: &KernelModel,
    result: &fsl_verifier::BmcResult,
    depth: usize,
    checked_bounds: Option<&std::collections::BTreeSet<String>>,
) {
    output.insert("depth".to_owned(), json!(depth));
    output.insert("checked_to_depth".to_owned(), json!(depth));
    output.insert("completeness".to_owned(), json!("bounded"));
    output.insert(
        "invariants_checked".to_owned(),
        Value::Array(
            invariant_names_selected(model, checked_bounds)
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
                .map(|property| Value::String(display(&property.name)))
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
                            display(name),
                            json!({
                                "witnessed_at_step": witness.step,
                                "witness": ::fslc_rust::trace_json(model, &witness.trace),
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
                        display(name),
                        if *covered {
                            json!(true)
                        } else {
                            let requirement = model
                                .actions
                                .iter()
                                .find(|action| action.name == *name)
                                .and_then(|action| action.meta.as_ref())
                                .map(|meta| json!({"id": meta.id, "text": meta.text}));
                            let mut diagnostic = json!({
                                "covered": false,
                                "blocking_requires": [],
                                "hint": coverage_hint(depth),
                                "faithfulness_class": "intent_unexercised",
                                "recommended_action": "add a single-shot reachable for the action / raise --depth",
                            });
                            if let Some(requirement) = requirement
                                && let Value::Object(entry) = &mut diagnostic
                            {
                                entry.insert("requirement".to_owned(), requirement);
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
                    "trace": result.deadlock_trace.as_ref().map(|trace| ::fslc_rust::trace_json(model, trace)),
                })
            },
        ),
    );
}
fn synthetic_span() -> fsl_syntax::Span {
    let position = fsl_syntax::SourcePos {
        offset: 0,
        line: 1,
        column: 1,
    };
    fsl_syntax::Span {
        start: position,
        end: position,
    }
}

fn lemma_violated_steps(
    result: &Value,
    expression: &KernelExpr,
    model: &KernelModel,
) -> Vec<usize> {
    result
        .get("cti")
        .and_then(|cti| cti.get("states"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, entry)| {
            let state = entry.get("state")?.as_object()?;
            let state = load_snapshot_value_object(state, model).ok()?;
            let value = fsl_runtime::eval(
                expression,
                &state,
                &mut std::collections::BTreeMap::new(),
                model,
                None,
            )
            .ok()?;
            (value != FslValue::Bool(true)).then_some(index)
        })
        .collect()
}

fn adjudicate_lemma(
    model: &KernelModel,
    name: &str,
    expression: &KernelExpr,
    source: &str,
    depth: usize,
    k_ind: usize,
) -> Value {
    let mut candidate = model.clone();
    candidate.invariants.push(fsl_core::PropertyDef {
        name: name.to_owned(),
        expr: expression.clone(),
        span: synthetic_span(),
        meta: None,
    });
    let mut solver = match fsl_solver_z3::Z3Solver::new() {
        Ok(solver) => solver,
        Err(error) => {
            return json!({
                "expression":source,"name":name,"status":"rejected","used":false,
                "proof":{"result":"error","kind":"internal","message":error.to_string()},
            });
        }
    };
    let bounded =
        match block_on_native(fsl_verifier::verify_bounded(&candidate, &mut solver, depth)) {
            Ok(result) => result,
            Err(error) => {
                return json!({
                    "expression":source,"name":name,"status":"rejected","used":false,
                    "proof":{"result":"error","kind":"semantics","message":error.to_string()},
                });
            }
        };
    if let Some(violation) = bounded.violation {
        return json!({
            "expression":source,"name":name,"status":"rejected","used":false,
            "proof":{
                "result":"violated","violation_kind":violation.kind,
                "invariant":display(&violation.name),"violated_at_step":violation.step,
                "trace": ::fslc_rust::trace_json(&candidate, &violation.trace),
            },
        });
    }
    let mut solver = match fsl_solver_z3::Z3Solver::new() {
        Ok(solver) => solver,
        Err(error) => {
            return json!({
                "expression":source,"name":name,"status":"rejected","used":false,
                "proof":{"result":"error","kind":"internal","message":error.to_string()},
            });
        }
    };
    match block_on_native(fsl_verifier::prove_induction(
        &candidate,
        &mut solver,
        k_ind,
    )) {
        Ok(proof) if proof.cti.is_none() => json!({
            "expression":source,"name":name,"status":"proved","used":false,
            "proof":{
                "result":"proved","k":proof.k_used.get(name).copied().unwrap_or(k_ind),
                "checked_to_depth":depth,"completeness":"unbounded",
            },
        }),
        Ok(proof) => json!({
            "expression":source,"name":name,"status":"rejected","used":false,
            "proof":{
                "result":"unknown_cti","k":proof.cti.as_ref().map_or(k_ind,|cti|cti.k),
                "checked_to_depth":depth,"completeness":"bounded",
            },
        }),
        Err(error) => json!({
            "expression":source,"name":name,"status":"rejected","used":false,
            "proof":{"result":"error","kind":"semantics","message":error.to_string()},
        }),
    }
}

pub(super) fn run_induction_with_lemmas(path: &Path, options: &CliVerifyOptions) -> (Value, i32) {
    let deadlock = match DeadlockMode::parse(&options.deadlock) {
        Ok(mode) => mode,
        Err(error) => return (error_output("usage", &error), 2),
    };
    let selection = ModelSelection {
        path,
        scope: None,
        property: options.property.as_deref(),
        excluded: &options.exclude_properties,
    };
    let model = match load_lemma_model(path, options) {
        Ok(model) => model,
        Err(output) => return output,
    };
    let (original, _) = run_induction_filtered(InductionRequest {
        selection,
        depth: options.depth,
        deadlock,
        k: options.k_ind,
        auxiliary: &[],
    });
    let target = original
        .get("invariant")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut entries = Vec::new();
    let mut auxiliary = Vec::new();
    let mut auxiliary_sources = Vec::new();
    let mut exclusions = Vec::new();
    for (index, source) in options.lemmas.iter().enumerate() {
        let name = format!("AuxiliaryLemma{}", index + 1);
        let expression = match fsl_syntax::parse_expr(source) {
            Ok(expression) => expression,
            Err(error) => {
                entries.push(json!({
                    "expression":source,"name":name,"status":"rejected","used":false,
                    "proof":{"result":"error","kind":"parse","message":error.message,
                        "loc":error.span.python_loc()},
                }));
                continue;
            }
        };
        let mut entry = adjudicate_lemma(
            &model,
            &name,
            &expression,
            source,
            options.depth,
            options.k_ind,
        );
        if entry.get("status").and_then(Value::as_str) == Some("proved") {
            let violated_steps = lemma_violated_steps(&original, &expression, &model);
            if !violated_steps.is_empty() {
                if let Value::Object(entry) = &mut entry {
                    entry.insert("used".to_owned(), json!(true));
                }
                exclusions.push(json!({
                    "lemma":source,"target":target,"k":options.k_ind,
                    "violated_steps":violated_steps,
                    "cti":original.get("cti").cloned().unwrap_or(Value::Null),
                }));
                auxiliary.push((name.clone(), expression.clone()));
                auxiliary_sources.push(source.clone());
            }
        }
        entries.push(entry);
    }
    let (mut result, status) = run_induction_filtered(InductionRequest {
        selection,
        depth: options.depth,
        deadlock,
        k: options.k_ind,
        auxiliary: &auxiliary,
    });
    if let Value::Object(output) = &mut result {
        output.insert("lemmas".to_owned(), Value::Array(entries));
        output.insert("lemma_cti_exclusions".to_owned(), Value::Array(exclusions));
        if !auxiliary.is_empty() {
            output.insert(
                "auxiliary_invariant_recommendation".to_owned(),
                json!({
                    "message":"write the used proved lemmas into the specification as auxiliary invariants",
                    "declarations":auxiliary.iter().zip(auxiliary_sources.iter()).map(
                        |((name,_),source)| format!("invariant {name} {{ {source} }}")
                    ).collect::<Vec<_>>(),
                }),
            );
        }
    }
    (result, status)
}

fn load_lemma_model(path: &Path, options: &CliVerifyOptions) -> Result<KernelModel, CommandResult> {
    let mut model = load_model(path).map_err(|error| (semantic_error_output(&error), 2))?;
    select_properties(
        &mut model,
        options.property.as_deref(),
        &options.exclude_properties,
    )
    .map_err(|error| (semantic_error_output(&error), 2))?;
    if options.property.as_ref().is_some_and(|name| {
        model
            .transitions
            .iter()
            .any(|property| display(&property.name) == *name)
    }) {
        return Err((
            error_output(
                "usage",
                "--lemma can strengthen invariant induction, but cannot be used to prove a trans property",
            ),
            2,
        ));
    }
    Ok(model)
}

fn cache_enabled(options: &CliVerifyOptions) -> bool {
    options.use_cache
        && options.from_state.is_none()
        && std::env::var("FSLC_CACHE").map_or(true, |value| value.trim().to_lowercase() != "off")
}

fn cache_root() -> Option<PathBuf> {
    std::env::var_os("FSLC_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .map(|path| path.join("fslc"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join(".cache/fslc"))
        })
}

fn collect_fsl_sources(path: &Path, output: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_file() {
        if path.extension().and_then(std::ffi::OsStr::to_str) == Some("fsl") {
            output.push(path.to_path_buf());
        }
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() || file_type.is_file() {
            collect_fsl_sources(&entry.path(), output)?;
        }
    }
    Ok(())
}

fn verify_cache_keys(path: &Path, options: &CliVerifyOptions) -> Result<(String, String), String> {
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    let base = canonical.parent().unwrap_or_else(|| Path::new("."));
    let mut sources = Vec::new();
    collect_fsl_sources(base, &mut sources).map_err(|error| error.to_string())?;
    sources.sort();
    let mut digest = Sha256::new();
    digest.update(b"fslc-rust-verify-cache-v1\0");
    digest.update(env!("CARGO_PKG_VERSION").as_bytes());
    if options.engine == "explicit" {
        digest.update(b"\0backend=native-explicit\0");
    } else {
        digest.update(b"\0backend=native-z3\0solver=4.16.0\0");
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    digest.update(b"executable\0");
    digest.update(std::fs::read(executable).map_err(|error| error.to_string())?);
    digest.update(b"\0");
    for source in sources {
        digest.update(
            source
                .strip_prefix(base)
                .unwrap_or(&source)
                .as_os_str()
                .as_encoded_bytes(),
        );
        digest.update(b"\0");
        digest.update(std::fs::read(&source).map_err(|error| error.to_string())?);
        digest.update(b"\0");
    }
    if let Some(requirements) = options.requirements.as_deref() {
        digest.update(b"requirements\0");
        digest.update(std::fs::read(requirements).map_err(|error| error.to_string())?);
    }
    let base_options = json!({
        "path": canonical,
        "deadlock": options.deadlock,
        "engine": options.engine,
        "explicit_budget": options.explicit_budget,
        "k": options.k_ind,
        "vacuity": options.vacuity,
        "property": options.property,
        "exclude_properties": options.exclude_properties,
        "instances": options.scope.instances,
        "values": options.scope.values,
        "strict_tags": options.strict_tags,
        "lemmas": options.lemmas,
        "edition": options.edition,
    });
    digest.update(serde_json::to_vec(&base_options).map_err(|error| error.to_string())?);
    let xdepth = format!("{:x}", digest.clone().finalize());
    digest.update(b"\0depth=");
    digest.update(options.depth.to_string().as_bytes());
    Ok((format!("{:x}", digest.finalize()), xdepth))
}

fn verify_cache_path(key: &str) -> Option<PathBuf> {
    Some(
        cache_root()?
            .join("verify/v1")
            .join(&key[..2])
            .join(format!("{key}.json")),
    )
}

fn verify_cache_lookup(key: &str, xdepth: &str, depth: usize) -> Option<Value> {
    let path = verify_cache_path(key)?;
    if let Ok(bytes) = std::fs::read(path)
        && let Ok(entry) = serde_json::from_slice::<Value>(&bytes)
        && entry.get("schema").and_then(Value::as_str) == Some("fslc-rust-cache.v1")
        && entry.get("key").and_then(Value::as_str) == Some(key)
    {
        let mut output = entry.get("output")?.clone();
        output.as_object_mut()?.insert(
            "cache".to_owned(),
            json!({"hit": true, "key": key, "source": "exact"}),
        );
        return Some(output);
    }
    let pointer_path = cache_root()?
        .join("verify/v1/xdepth")
        .join(format!("{xdepth}.json"));
    let pointer: Value = serde_json::from_slice(&std::fs::read(pointer_path).ok()?).ok()?;
    let violation_step = usize::try_from(pointer.get("violated_at_step")?.as_u64()?).ok()?;
    if violation_step > depth {
        return None;
    }
    let target = pointer.get("entry_key")?.as_str()?;
    let entry: Value =
        serde_json::from_slice(&std::fs::read(verify_cache_path(target)?).ok()?).ok()?;
    let mut output = entry.get("output")?.clone();
    output.as_object_mut()?.insert(
        "cache".to_owned(),
        json!({"hit": true, "key": target, "source": "cross_depth"}),
    );
    Some(output)
}

fn verify_cache_store(key: &str, xdepth: &str, output: &Value) {
    if !matches!(
        output.get("result").and_then(Value::as_str),
        Some(
            "verified"
                | "proved"
                | "violated"
                | "reachable_failed"
                | "unknown_cti"
                | "unknown_budget"
        )
    ) {
        return;
    }
    let Some(path) = verify_cache_path(key) else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let temporary = parent.join(format!(".{}.{}.tmp", key, std::process::id()));
    let explicit = output.get("engine").and_then(Value::as_str) == Some("explicit");
    let entry = json!({
        "schema": "fslc-rust-cache.v1",
        "key": key,
        "backend": if explicit { "native-explicit" } else { "native-z3" },
        "solver_version": if explicit { Value::Null } else { json!("4.16.0") },
        "output": output,
    });
    if serde_json::to_vec(&entry)
        .ok()
        .and_then(|bytes| std::fs::write(&temporary, bytes).ok())
        .is_some()
    {
        let _ = std::fs::rename(&temporary, path);
    }
    if output.get("result").and_then(Value::as_str) == Some("violated")
        && let Some(step) = output.get("violated_at_step").and_then(Value::as_u64)
        && let Some(root) = cache_root()
    {
        let directory = root.join("verify/v1/xdepth");
        if std::fs::create_dir_all(&directory).is_ok() {
            let pointer = directory.join(format!("{xdepth}.json"));
            let temporary = directory.join(format!(".{xdepth}.{}.tmp", std::process::id()));
            if serde_json::to_vec(&json!({"entry_key":key,"violated_at_step":step}))
                .ok()
                .and_then(|bytes| std::fs::write(&temporary, bytes).ok())
                .is_some()
            {
                let _ = std::fs::rename(temporary, pointer);
            }
        }
    }
}

struct PreparedCliVerification {
    has_scope: bool,
    initial_state: Option<std::collections::BTreeMap<String, FslValue>>,
}

pub(super) fn run_verify_cli(path: &Path, options: &CliVerifyOptions) -> CommandResult {
    if !options.lemmas.is_empty() && options.engine != "induction" {
        return (
            error_output("usage", "--lemma requires --engine induction"),
            2,
        );
    }
    if options.from_state.is_some() && options.engine != "bmc" {
        return (
            error_output(
                "semantics",
                "--from-state is available only with the BMC engine; induction and explicit verification start from the spec init contract",
            ),
            2,
        );
    }
    if !options.lemmas.is_empty() {
        return run_induction_with_lemmas(path, options);
    }
    let prepared = match prepare_cli_verification(path, options) {
        Ok(prepared) => prepared,
        Err(output) => return output,
    };
    let cache_keys = cache_enabled(options)
        .then(|| verify_cache_keys(path, options).ok())
        .flatten();
    if let Some(output) = cached_verification(options, cache_keys.as_ref()) {
        return output;
    }
    let (output, status) = execute_cli_verification(path, options, &prepared);
    finalize_cli_verification(
        path,
        options,
        &prepared,
        cache_keys.as_ref(),
        output,
        status,
    )
}

fn prepare_cli_verification(
    path: &Path,
    options: &CliVerifyOptions,
) -> Result<PreparedCliVerification, CommandResult> {
    if let Ok(source) = std::fs::read_to_string(path)
        && let Err(error) = fsl_syntax::parse_surface_document(&source)
    {
        return Err((error_output("parse", &error.to_string()), 2));
    }
    let has_scope = !options.scope.instances.is_empty() || !options.scope.values.is_empty();
    if let Err(error) = validate_specialized_document(path) {
        return Err((semantic_error_output(&error), 2));
    }
    let snapshot_model = if has_scope {
        load_model_scoped(path, &options.scope)
    } else {
        load_model(path)
    };
    let initial_state = if let Some(snapshot_path) = options.from_state.as_deref() {
        let model = snapshot_model
            .as_ref()
            .map_err(|error| (semantic_error_output(error), 2))?;
        match load_state_snapshot(snapshot_path, model) {
            Ok(state) => Some(state),
            Err((kind, error)) => return Err((error_output(&kind, &error), 2)),
        }
    } else {
        None
    };
    if !has_scope && let Ok(model) = &snapshot_model {
        match validate_requirement_traces(path, model) {
            Ok((Some(failure), _)) => return Err((failure, 2)),
            Ok((None, _)) => {}
            Err(error) => return Err((semantic_error_output(&error), 2)),
        }
    }
    validate_cli_property_selection(path, options, has_scope)?;
    Ok(PreparedCliVerification {
        has_scope,
        initial_state,
    })
}

fn validate_cli_property_selection(
    path: &Path,
    options: &CliVerifyOptions,
    has_scope: bool,
) -> Result<(), CommandResult> {
    if options.property.is_none() && options.exclude_properties.is_empty() {
        return Ok(());
    }
    let mut model = if has_scope {
        load_model_scoped(path, &options.scope)
    } else {
        load_model(path)
    }
    .map_err(|error| (semantic_error_output(&error), 2))?;
    if options.engine == "induction"
        && let Some(name) = options.property.as_deref()
        && let Some(kind) = model
            .transitions
            .iter()
            .any(|item| display(&item.name) == name)
            .then_some("trans")
            .or_else(|| {
                model
                    .leadstos
                    .iter()
                    .any(|item| display(&item.name) == name)
                    .then_some("leadsTo")
            })
            .or_else(|| {
                model
                    .reachables
                    .iter()
                    .any(|item| display(&item.name) == name)
                    .then_some("reachable")
            })
    {
        return Err((
            error_output(
                "usage",
                &format!(
                    "--property {name} is a {kind}, which the induction engine cannot prove; check it with the default bmc engine"
                ),
            ),
            2,
        ));
    }
    select_properties(
        &mut model,
        options.property.as_deref(),
        &options.exclude_properties,
    )
    .map_err(|error| (error_output("usage", &error), 2))
}

fn cached_verification(
    options: &CliVerifyOptions,
    cache_keys: Option<&(String, String)>,
) -> Option<CommandResult> {
    let (key, xdepth) = cache_keys?;
    if std::env::var("FSLC_CACHE_VERIFY").as_deref() == Ok("1") {
        return None;
    }
    let output = verify_cache_lookup(key, xdepth, options.depth)?;
    let status = match output.get("result").and_then(Value::as_str) {
        Some("violated" | "reachable_failed" | "unknown_cti" | "unknown_budget") => 1,
        _ => 0,
    };
    Some((output, status))
}

fn execute_cli_verification(
    path: &Path,
    options: &CliVerifyOptions,
    prepared: &PreparedCliVerification,
) -> CommandResult {
    if prepared.has_scope
        || options.property.is_some()
        || !options.exclude_properties.is_empty()
        || prepared.initial_state.is_some()
    {
        let deadlock = match DeadlockMode::parse(&options.deadlock) {
            Ok(mode) => mode,
            Err(error) => return (error_output("usage", &error), 2),
        };
        let selection = ModelSelection {
            path,
            scope: prepared.has_scope.then_some(&options.scope),
            property: options.property.as_deref(),
            excluded: &options.exclude_properties,
        };
        return match VerificationEngine::parse(&options.engine) {
            Ok(VerificationEngine::Bmc) => run_bmc_filtered(BmcRequest {
                selection,
                depth: options.depth,
                deadlock,
                initial_state: prepared.initial_state.as_ref(),
            }),
            Ok(VerificationEngine::Induction) => run_induction_filtered(InductionRequest {
                selection,
                depth: options.depth,
                deadlock,
                k: options.k_ind,
                auxiliary: &[],
            }),
            Ok(VerificationEngine::Explicit) => run_explicit_filtered(ExplicitRequest {
                selection,
                depth: options.depth,
                deadlock,
                budget: options.explicit_budget,
            }),
            Err(error) => (error_output("usage", &error), 2),
        };
    }
    run_verify(
        path,
        options.depth,
        &options.deadlock,
        &options.engine,
        options.explicit_budget,
        options.k_ind,
    )
}

fn finalize_cli_verification(
    path: &Path,
    options: &CliVerifyOptions,
    prepared: &PreparedCliVerification,
    cache_keys: Option<&(String, String)>,
    mut output: Value,
    mut status: i32,
) -> CommandResult {
    if prepared.has_scope
        && output.get("result").and_then(Value::as_str) != Some("error")
        && let Some(envelope) = output.as_object_mut()
    {
        envelope.insert(
            "bounds_overrides".to_owned(),
            json!({
                "instances": options.scope.instances,
                "values": options.scope.values.iter().map(|(name, (lo, hi))| (
                    name.clone(), json!([lo, hi])
                )).collect::<Map<_, _>>(),
            }),
        );
    }
    add_snapshot_metadata(&mut output, options);
    if let Some(vacuity_status) = apply_vacuity_mode(&mut output, &options.vacuity) {
        status = vacuity_status;
    }
    if status == 0
        && options.strict_tags
        && let Err(output) =
            add_cli_strict_tag_warnings(path, options, prepared.has_scope, &mut output)
    {
        return output;
    }
    if let Some((key, xdepth)) = cache_keys {
        if std::env::var("FSLC_CACHE_VERIFY").as_deref() == Ok("1")
            && let Some(cached) = verify_cache_lookup(key, xdepth, options.depth)
            && cached.get("result") != output.get("result")
        {
            return (
                error_output(
                    "internal",
                    &format!(
                        "verify cache divergence: cached result={:?} fresh result={:?} for key={key}",
                        cached.get("result"),
                        output.get("result")
                    ),
                ),
                3,
            );
        }
        verify_cache_store(key, xdepth, &output);
    }
    (output, status)
}

fn add_snapshot_metadata(output: &mut Value, options: &CliVerifyOptions) {
    let Some(snapshot_path) = options.from_state.as_deref() else {
        return;
    };
    if output.get("result").and_then(Value::as_str) == Some("error") {
        return;
    }
    let Some(envelope) = output.as_object_mut() else {
        return;
    };
    envelope.insert(
        "initial_state".to_owned(),
        json!({
            "source": "snapshot",
            "path": snapshot_path,
            "complete": true,
            "replaces_spec_init": true,
        }),
    );
    envelope.insert(
        "faithfulness".to_owned(),
        json!({
            "scope": "bounded_from_snapshot",
            "spec_init": "not_used",
            "induction": "not_applicable",
        }),
    );
}

fn add_cli_strict_tag_warnings(
    path: &Path,
    options: &CliVerifyOptions,
    has_scope: bool,
    output: &mut Value,
) -> Result<(), CommandResult> {
    let model = if has_scope {
        load_model_scoped(path, &options.scope)
    } else {
        load_model(path)
    }
    .map_err(|error| (semantic_error_output(&error), 2))?;
    add_strict_tag_warnings(output, &model, path, true, options.requirements.as_deref())
        .map_err(|error| (error_output("io", &error), 2))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn repository_path(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repository root")
            .join(relative)
    }

    #[test]
    fn bmc_entrypoint_reports_a_verified_model() {
        let path = repository_path("examples/gallery/valid/tiny_turnstile.fsl");
        let excluded = Vec::new();
        let (output, status) = run_bmc_filtered(BmcRequest {
            selection: ModelSelection {
                path: &path,
                scope: None,
                property: None,
                excluded: &excluded,
            },
            depth: 4,
            deadlock: DeadlockMode::Ignore,
            initial_state: None,
        });
        assert_eq!(status, 0);
        assert_eq!(output["result"], "verified");
        assert_eq!(output["completeness"], "bounded");
    }

    #[test]
    fn induction_entrypoint_reports_a_counterexample_to_induction() {
        let path = repository_path("tests/fixtures/rust_port/induction_unknown_cti.fsl");
        let excluded = Vec::new();
        let (output, status) = run_induction_filtered(InductionRequest {
            selection: ModelSelection {
                path: &path,
                scope: None,
                property: None,
                excluded: &excluded,
            },
            depth: 4,
            deadlock: DeadlockMode::Ignore,
            k: 1,
            auxiliary: &[],
        });
        assert_eq!(status, 1);
        assert_eq!(output["result"], "unknown_cti");
        assert_eq!(output["invariant"], "Sync");
    }

    #[test]
    fn induction_counterexample_renderer_uses_domain_origin() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/domain_origin_violation.fsl");
        let model = load_model(&path).expect("domain model");
        let cti = fsl_verifier::InductionCti {
            kind: "invariant".to_owned(),
            name: "Order_mustBeApproved".to_owned(),
            k: 1,
            trace: Vec::new(),
        };
        let (output, status) = render_induction_cti(&model, &cti, 1, Instant::now());
        assert_eq!(status, 1);
        assert_eq!(output["invariant"], "mustBeApproved");
        assert_eq!(output["generated_name"], "Order_mustBeApproved");
        assert_eq!(output["origin"]["dialect"], "domain");
    }

    #[test]
    fn explicit_cache_keys_include_the_engine_and_state_budget() {
        let path = repository_path("examples/gallery/valid/tiny_turnstile.fsl");
        let bmc = CliVerifyOptions::default();
        let mut explicit = bmc.clone();
        explicit.engine = "explicit".to_owned();
        let mut smaller_budget = explicit.clone();
        smaller_budget.explicit_budget -= 1;

        let bmc_keys = verify_cache_keys(&path, &bmc).expect("BMC cache keys");
        let explicit_keys = verify_cache_keys(&path, &explicit).expect("explicit cache keys");
        let smaller_keys =
            verify_cache_keys(&path, &smaller_budget).expect("budget-specific cache keys");

        assert_ne!(bmc_keys, explicit_keys);
        assert_ne!(explicit_keys, smaller_keys);
    }
}

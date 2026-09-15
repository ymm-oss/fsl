// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::time::Instant;

use fsl_core::{FslValue, KernelExpr, KernelModel};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{
    CliVerifyOptions, ScopeBounds, SpecLoadError, add_strict_tag_warnings, apply_vacuity_mode,
    block_on_native, display, envelope, error_output, implements_error_output,
    implements_result_from_source_with_bounds, invariant_names,
    load_kernel_model_from_source_with_resolver, load_model, load_model_from_source,
    load_model_scoped, load_model_scoped_from_source, load_snapshot_value_object,
    load_state_snapshot, read_spec_source, select_properties, selected_implicit_bounds,
    semantic_error_output, spec_load_error_output, surface_parse_error_output,
    validate_requirement_traces_from_source, validate_specialized_document_from_source,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VerificationEngine {
    Bmc,
    Induction,
    Explicit,
    Auto,
}

impl VerificationEngine {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "bmc" => Ok(Self::Bmc),
            "induction" => Ok(Self::Induction),
            "explicit" => Ok(Self::Explicit),
            "auto" => Ok(Self::Auto),
            _ => Err("--engine must be bmc, induction, explicit, or auto".to_owned()),
        }
    }
}

pub(super) use fslc_rust::verification_output::DeadlockMode;

#[derive(Clone, Copy)]
pub(super) struct ModelSelection<'a> {
    pub(super) path: &'a Path,
    pub(super) model: Option<&'a KernelModel>,
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
    /// See `BmcOutputOptions::skip_vacuity_probe` (issue #729): only
    /// `execute_cli_verification`'s `--vacuity ignore` handling may set
    /// this `true`.
    pub(super) skip_vacuity_probe: bool,
}

#[derive(Clone, Copy)]
pub(super) struct InductionRequest<'a> {
    pub(super) selection: ModelSelection<'a>,
    pub(super) depth: usize,
    pub(super) deadlock: DeadlockMode,
    pub(super) k: usize,
    pub(super) auxiliary: &'a [(String, KernelExpr)],
    /// See `BmcRequest::skip_vacuity_probe`.
    pub(super) skip_vacuity_probe: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ExplicitRequest<'a> {
    pub(super) selection: ModelSelection<'a>,
    pub(super) depth: usize,
    pub(super) deadlock: DeadlockMode,
    pub(super) budget: usize,
    /// See `BmcRequest::skip_vacuity_probe`.
    pub(super) skip_vacuity_probe: bool,
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

struct SolvedBmc {
    result: fsl_verifier::BmcResult,
    statistics: fsl_solver::VerificationStatistics,
}

fn verification_cost(started: Instant, statistics: &fsl_solver::VerificationStatistics) -> Value {
    serde_json::to_value(statistics.with_elapsed(started.elapsed().as_secs_f64()))
        .expect("verification cost serializes")
}

fn load_selected_model(selection: ModelSelection<'_>) -> Result<KernelModel, SpecLoadError> {
    let mut model = load_unselected_model(selection)?;
    // A rejected `--property`/`--exclude` names a property that is absent from
    // the model, so there is no construct in the source to point at.
    select_properties(&mut model, selection.property, selection.excluded)
        .map_err(SpecLoadError::unlocated_semantic)?;
    Ok(model)
}

fn load_unselected_model(selection: ModelSelection<'_>) -> Result<KernelModel, SpecLoadError> {
    let model = match selection.model {
        Some(model) => model.clone(),
        None => selection.scope.map_or_else(
            || load_model(selection.path),
            |scope| load_model_scoped(selection.path, scope),
        )?,
    };
    Ok(model)
}

/// Restrict induction's obligations to a selected transition without deleting
/// the invariants that establish its base case and form its step hypothesis.
/// Other selector kinds keep `load_selected_model`'s existing model-restriction
/// semantics.
fn selected_transition_induction_model(
    selection: ModelSelection<'_>,
) -> Result<Option<KernelModel>, SpecLoadError> {
    let Some(selected) = selection.property else {
        return Ok(None);
    };
    let mut model = load_unselected_model(selection)?;
    if !model
        .transitions
        .iter()
        .any(|property| display(&property.name) == selected)
    {
        return Ok(None);
    }
    let invariant_hypotheses = model.invariants.clone();
    select_properties(&mut model, selection.property, selection.excluded)
        .map_err(SpecLoadError::unlocated_semantic)?;
    if model.transitions.is_empty() {
        // `--exclude-property` wins when it names the selected transition.
        return Ok(None);
    }
    model.invariants = invariant_hypotheses;
    Ok(Some(model))
}

#[allow(clippy::too_many_lines)]
pub(super) fn run_induction_filtered(request: InductionRequest<'_>) -> (Value, i32) {
    let InductionRequest {
        selection,
        depth,
        deadlock,
        k,
        auxiliary,
        skip_vacuity_probe,
    } = request;
    let selected_transition_model = match selected_transition_induction_model(selection) {
        Ok(model) => model,
        Err(error) => return (spec_load_error_output(&error), 2),
    };
    let selection = selected_transition_model
        .as_ref()
        .map_or(selection, |model| ModelSelection {
            path: selection.path,
            model: Some(model),
            scope: None,
            property: None,
            excluded: &[],
        });
    let started = Instant::now();
    let base_request = BmcRequest {
        selection,
        depth,
        deadlock,
        initial_state: None,
        skip_vacuity_probe,
    };
    let (base_prepared, base_solved) = match execute_bmc(&base_request, started) {
        Ok(execution) => execution,
        Err(output) => return output,
    };
    let (base_value, base_status) = fslc_rust::verification_output::render_bmc_output(
        envelope(),
        &base_prepared.model,
        &base_solved.result,
        fslc_rust::verification_output::BmcOutputOptions {
            depth: base_request.depth,
            deadlock: base_request.deadlock,
            checked_bounds: base_prepared.checked_bounds.as_ref(),
            elapsed_s: started.elapsed().as_secs_f64(),
            statistics: &base_solved.statistics,
            skip_vacuity_probe: base_request.skip_vacuity_probe,
        },
    );
    let Value::Object(base) = &base_value else {
        return (
            error_output("internal", "BMC returned a non-object envelope"),
            3,
        );
    };
    if base.get("result").and_then(Value::as_str) != Some("verified") {
        // A base-case `leadsTo` violation may be a raw BMC stall/lasso trace
        // that a ranking proof can diagnose more precisely (e.g. a missing
        // `fair` on the `helpful` action, or a permanently-blocked helpful
        // instance) -- mirrors the frozen Python reference's `prove()`,
        // which prefers `_prove_ranked_leadstos`' rank_failure over the raw
        // BMC counterexample whenever ranking has something more specific to
        // say.
        if base.get("result").and_then(Value::as_str) == Some("violated")
            && base.get("violation_kind").and_then(Value::as_str) == Some("leadsTo")
            && let Ok(model) = load_induction_model(selection, auxiliary)
            && let Ok(mut solver) = fsl_solver_z3::Z3Solver::new()
            && let Ok(ranked) =
                block_on_native(fsl_verifier::prove_ranked_leadstos(&model, &mut solver))
            && let Some(failure) = &ranked.failure
        {
            let mut statistics = base_solved.statistics.clone();
            statistics.merge(&fsl_solver::SmtSolver::statistics(&solver));
            return render_rank_failure(&model, failure, depth, started, &statistics);
        }
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
        let mut statistics = base_solved.statistics.clone();
        statistics.merge(&fsl_solver::SmtSolver::statistics(&solver));
        return render_induction_cti(&model, cti, depth, started, &statistics);
    }

    let ranked = match block_on_native(fsl_verifier::prove_ranked_leadstos(&model, &mut solver)) {
        Ok(result) => result,
        Err(error) => return (error_output("semantics", &error.to_string()), 2),
    };
    let mut statistics = base_solved.statistics.clone();
    statistics.merge(&fsl_solver::SmtSolver::statistics(&solver));
    if let Some(failure) = &ranked.failure {
        return render_rank_failure(&model, failure, depth, started, &statistics);
    }
    render_induction_success(
        &model,
        base,
        &induction,
        &ranked,
        depth,
        started,
        &statistics,
    )
}

fn load_induction_model(
    selection: ModelSelection<'_>,
    auxiliary: &[(String, KernelExpr)],
) -> Result<KernelModel, CommandResult> {
    let mut model =
        load_selected_model(selection).map_err(|error| (spec_load_error_output(&error), 2))?;
    model
        .invariants
        .extend(auxiliary.iter().map(|(name, expr)| fsl_core::PropertyDef {
            name: name.clone(),
            expr: expr.clone(),
            span: synthetic_span(),
            meta: None,
            annotations: fsl_core::Annotations::default(),
        }));
    Ok(model)
}

fn integer_state_type(model: &KernelModel, ty: &fsl_core::TypeRef) -> bool {
    match ty {
        fsl_core::TypeRef::Int | fsl_core::TypeRef::Range(_, _) => true,
        fsl_core::TypeRef::Named(name) => matches!(
            model.types.get(name),
            Some(fsl_core::TypeDef::Domain { .. })
        ),
        _ => false,
    }
}

fn monotone_direction(values: &[i64]) -> Result<Option<std::cmp::Ordering>, ()> {
    let mut direction = None;
    for pair in values.windows(2) {
        let current = pair[1].cmp(&pair[0]);
        if current == std::cmp::Ordering::Equal {
            continue;
        }
        if direction.is_some_and(|direction| direction != current) {
            return Err(());
        }
        direction = Some(current);
    }
    Ok(direction)
}

fn monotone_qualifies(direction: std::cmp::Ordering, start: i64, initial: i64) -> bool {
    match direction {
        std::cmp::Ordering::Greater => start < initial,
        std::cmp::Ordering::Less => start > initial,
        std::cmp::Ordering::Equal => false,
    }
}

fn monotone_suggestion(
    name: &str,
    direction: std::cmp::Ordering,
    start: i64,
    initial: i64,
    expression: &str,
) -> Option<(String, String)> {
    let (motion, side) = match direction {
        std::cmp::Ordering::Greater => ("increasing", "below"),
        std::cmp::Ordering::Less => ("decreasing", "above"),
        std::cmp::Ordering::Equal => return None,
    };
    monotone_qualifies(direction, start, initial).then(|| (
        expression.to_owned(),
        format!(
            "'{name}' only {motion} in this CTI but starts {side} its initial value {initial}; adding invariant {expression} may exclude this unreachable start state"
        ),
    ))
}

fn scalar_suggestion(
    initial_state: &std::collections::BTreeMap<String, FslValue>,
    trace: &[fsl_core::TraceStep],
    name: &str,
) -> Option<(String, String)> {
    let FslValue::Int(initial) = initial_state.get(name)? else {
        return None;
    };
    let values = trace
        .iter()
        .map(|step| match step.state.get(name) {
            Some(FslValue::Int(value)) => Some(*value),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let direction = monotone_direction(&values).ok()??;
    let public_name = display(name);
    let operator = if direction == std::cmp::Ordering::Greater {
        ">="
    } else {
        "<="
    };
    let expression = format!("{public_name} {operator} {initial}");
    monotone_suggestion(&public_name, direction, values[0], *initial, &expression)
}

fn map_suggestion(
    model: &KernelModel,
    initial_state: &std::collections::BTreeMap<String, FslValue>,
    trace: &[fsl_core::TraceStep],
    name: &str,
    ty: &fsl_core::TypeRef,
) -> Option<(String, String)> {
    let fsl_core::TypeRef::Map(key_ty, value_ty) = ty else {
        return None;
    };
    let fsl_core::TypeRef::Named(key_name) = key_ty.as_ref() else {
        return None;
    };
    if !integer_state_type(model, value_ty) {
        return None;
    }
    let FslValue::Map(initial_map) = initial_state.get(name)? else {
        return None;
    };
    let mut initial_values = initial_map.values();
    let FslValue::Int(initial) = initial_values.next()? else {
        return None;
    };
    if initial_values.any(|value| value != &FslValue::Int(*initial)) {
        return None;
    }
    let maps = trace
        .iter()
        .map(|step| match step.state.get(name) {
            Some(FslValue::Map(map)) => Some(map),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let keys = maps
        .iter()
        .flat_map(|map| map.keys().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut directions = std::collections::BTreeSet::new();
    let mut qualifying_start = None;
    for key in keys {
        let values = maps
            .iter()
            .map(|map| match map.get(&key) {
                Some(FslValue::Int(value)) => Some(*value),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        let direction = match monotone_direction(&values) {
            Ok(Some(direction)) => direction,
            Ok(None) => continue,
            Err(()) => return None,
        };
        directions.insert(direction);
        if monotone_qualifies(direction, values[0], *initial) {
            qualifying_start.get_or_insert(values[0]);
        }
    }
    if directions.len() != 1 {
        return None;
    }
    let start = qualifying_start?;
    let direction = directions.into_iter().next()?;
    let operator = if direction == std::cmp::Ordering::Greater {
        ">="
    } else {
        "<="
    };
    let public_name = display(name);
    let binder = if public_name == "k" { "key" } else { "k" };
    let expression = format!(
        "forall {binder}: {} {{ {public_name}[{binder}] {operator} {initial} }}",
        display(key_name)
    );
    monotone_suggestion(&public_name, direction, start, *initial, &expression)
}

fn suggested_invariants(
    model: &KernelModel,
    trace: &[fsl_core::TraceStep],
) -> Vec<(String, String)> {
    if trace.len() < 2 {
        return Vec::new();
    }
    let Ok(initial_state) = fsl_runtime::deterministic_initial_state(model) else {
        return Vec::new();
    };
    model
        .state
        .iter()
        .filter_map(|(name, ty)| {
            if integer_state_type(model, ty) {
                scalar_suggestion(&initial_state, trace, name)
            } else {
                map_suggestion(model, &initial_state, trace, name, ty)
            }
        })
        .collect()
}

fn render_induction_cti(
    model: &KernelModel,
    cti: &fsl_verifier::InductionCti,
    depth: usize,
    started: Instant,
    statistics: &fsl_solver::VerificationStatistics,
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
    let mut hint = "this state sequence satisfies all invariants but leads to a violation; the start state may be unreachable — add an auxiliary invariant that excludes it, then re-run".to_owned();
    if cti.kind == "invariant" {
        let suggestions = suggested_invariants(model, &cti.trace);
        if !suggestions.is_empty() {
            for (_, sentence) in &suggestions {
                hint.push(' ');
                hint.push_str(sentence);
            }
            output.insert(
                "suggested_invariants".to_owned(),
                Value::Array(
                    suggestions
                        .into_iter()
                        .map(|(expression, _)| Value::String(expression))
                        .collect(),
                ),
            );
        }
    }
    output.insert("hint".to_owned(), Value::String(hint));
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
    output.insert("cost".to_owned(), verification_cost(started, statistics));
    (Value::Object(output), 1)
}

fn render_rank_failure(
    model: &KernelModel,
    failure: &fsl_verifier::RankFailure,
    depth: usize,
    started: Instant,
    statistics: &fsl_solver::VerificationStatistics,
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
    insert_helpful_rank_failure_json(&mut output, model, failure);
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
    output.insert("cost".to_owned(), verification_cost(started, statistics));
    (Value::Object(output), 1)
}

fn render_induction_success(
    model: &KernelModel,
    base: &Map<String, Value>,
    induction: &fsl_verifier::InductionResult,
    ranked: &fsl_verifier::RankedLeadstoResult,
    depth: usize,
    started: Instant,
    statistics: &fsl_solver::VerificationStatistics,
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
        .map_or_else(Vec::new, |warnings| {
            fsl_runtime::induction_warnings(warnings)
        });
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
                if !proof.helpful.is_empty() {
                    entry.insert(
                        "helpful".to_owned(),
                        json!(
                            proof
                                .helpful
                                .iter()
                                .map(helpful_action_label)
                                .collect::<Vec<_>>()
                        ),
                    );
                }
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
    output.insert("cost".to_owned(), verification_cost(started, statistics));
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

fn helpful_action_label(helper: &fsl_core::HelpfulAction) -> String {
    let args = helper
        .args
        .iter()
        .map(::fslc_rust::expr_text)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}({args})", helper.action)
}

fn insert_helpful_rank_failure_json(
    output: &mut Map<String, Value>,
    model: &KernelModel,
    failure: &fsl_verifier::RankFailure,
) {
    if !failure.helpful.is_empty() {
        output.insert(
            "helpful".to_owned(),
            json!(
                failure
                    .helpful
                    .iter()
                    .map(helpful_action_label)
                    .collect::<Vec<_>>()
            ),
        );
    }
    if !failure.helpful_actions.is_empty() {
        output.insert(
            "helpful_actions".to_owned(),
            Value::Array(
                failure
                    .helpful_actions
                    .iter()
                    .map(|witness| {
                        let params = witness
                            .params
                            .iter()
                            .map(|(name, value)| (name.clone(), ::fslc_rust::fsl_value_json(value)))
                            .collect::<Map<_, _>>();
                        origin_aware_action_json(model, &witness.action, &params, Value::Null)
                    })
                    .collect(),
            ),
        );
    }
}

/// Prove the solver-dependent vacuity lanes (`docs/DESIGN-vacuity.md` §2 lanes
/// 3–6) for a run decided by the solver-free explicit-state engine.
///
/// The lanes describe the model, not the exploration, so `--engine explicit`
/// must surface the same vacuity kinds `--engine bmc` does; letting the engine
/// choice change which kinds `--vacuity error` can see would be exactly the
/// exit-code divergence `docs/DESIGN-rust-port.md` forbids. A solver or
/// semantics failure is surfaced, never swallowed.
type ExplicitSolverFindings = (
    Vec<fsl_verifier::VacuityFinding>,
    std::collections::BTreeMap<String, fsl_verifier::ReachableDiagnosis>,
);

fn explicit_solver_findings(
    model: &KernelModel,
    action_coverage: &std::collections::BTreeMap<String, bool>,
) -> Result<ExplicitSolverFindings, (Value, i32)> {
    let mut solver = fsl_solver_z3::Z3Solver::new()
        .map_err(|error| (error_output("internal", &error.to_string()), 3))?;
    block_on_native(async {
        let diagnostics = fsl_verifier::diagnose_reachables(model, &mut solver).await?;
        let vacuity =
            fsl_verifier::model_vacuity_findings(model, &mut solver, action_coverage).await?;
        Ok((vacuity, diagnostics))
    })
    .map_err(|error: fsl_verifier::VerifyError| (error_output("semantics", &error.to_string()), 2))
}

pub(super) fn run_explicit_filtered(request: ExplicitRequest<'_>) -> (Value, i32) {
    let started = Instant::now();
    let model = match load_selected_model(request.selection) {
        Ok(model) => model,
        Err(error) => return (spec_load_error_output(&error), 2),
    };
    if model.actions.is_empty() {
        return (semantic_error_output("spec has no actions"), 2);
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
        Err(error) => {
            return (
                fslc_rust::verification_output::render_runtime_error(envelope(), &error),
                2,
            );
        }
    };
    let (vacuity, reachable_diagnostics) =
        match explicit_solver_findings(&model, &result.action_coverage) {
            Ok(findings) => findings,
            Err(output) => return output,
        };
    finish_explicit_output(fslc_rust::verification_output::render_explicit_output(
        envelope(),
        &model,
        &result,
        checked_bounds.as_ref(),
        request.deadlock,
        started.elapsed().as_secs_f64(),
        &vacuity,
        &reachable_diagnostics,
        request.skip_vacuity_probe,
    ))
}

/// Composite engine: try explicit-state exploration first and fall back to
/// symbolic BMC when explicit cannot decide — either a fail-closed semantics
/// rejection (leadsTo, nondeterministic init, partial component init, …) or a
/// state-budget exhaustion (`unknown_budget`). Every real explicit verdict
/// (violated, deadlock, `reachable_failed`, verified, proved) is returned
/// unchanged and is never re-run under BMC.
///
/// The model is loaded once. The same static, pre-exploration gate that
/// `verify_explicit_selected` checks internally (`explicit_unsupported_reason`)
/// is consulted first so a known-unsupported model falls back without ever
/// starting BFS; an `Err` from the real run past that gate (which should not
/// happen, since the gate mirrors the engine's own check) is surfaced as a
/// genuine error rather than silently folded into the fallback narrative —
/// an unexpected explicit-engine defect must never be mistaken for an
/// ordinary, documented unsupported-feature case.
pub(super) fn run_auto_filtered(request: ExplicitRequest<'_>) -> (Value, i32) {
    let started = Instant::now();
    let model = match load_selected_model(request.selection) {
        Ok(model) => model,
        Err(error) => return (spec_load_error_output(&error), 2),
    };
    if model.actions.is_empty() {
        return (semantic_error_output("spec has no actions"), 2);
    }
    if let Some(reason) = fsl_runtime::explicit_unsupported_reason(&model) {
        return auto_fallback_to_bmc(request, &reason, "unsupported");
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
        Err(error) => {
            return (
                fslc_rust::verification_output::render_runtime_error(envelope(), &error),
                2,
            );
        }
    };
    let (vacuity, reachable_diagnostics) =
        match explicit_solver_findings(&model, &result.action_coverage) {
            Ok(findings) => findings,
            Err(output) => return output,
        };
    let (output, status) =
        finish_explicit_output(fslc_rust::verification_output::render_explicit_output(
            envelope(),
            &model,
            &result,
            checked_bounds.as_ref(),
            request.deadlock,
            started.elapsed().as_secs_f64(),
            &vacuity,
            &reachable_diagnostics,
            request.skip_vacuity_probe,
        ));
    if output.get("result").and_then(Value::as_str) == Some("unknown_budget") {
        return auto_fallback_to_bmc(request, &budget_fallback_reason(&output), "budget");
    }
    (output, status)
}

fn auto_fallback_to_bmc(request: ExplicitRequest<'_>, reason: &str, kind: &str) -> (Value, i32) {
    let (mut output, status) = run_bmc_filtered(BmcRequest {
        selection: request.selection,
        depth: request.depth,
        deadlock: request.deadlock,
        initial_state: None,
        skip_vacuity_probe: request.skip_vacuity_probe,
    });
    annotate_auto_fallback(&mut output, reason, kind);
    (output, status)
}

fn annotate_auto_fallback(output: &mut Value, reason: &str, kind: &str) {
    let Some(envelope) = output.as_object_mut() else {
        return;
    };
    if envelope.get("result").and_then(Value::as_str) == Some("error") {
        return;
    }
    envelope.insert("engine".to_owned(), json!("bmc"));
    envelope.insert(
        "engine_fallback".to_owned(),
        json!({"from": "explicit", "reason": reason, "kind": kind}),
    );
}

/// Builds the budget-exhaustion fallback reason from the explicit engine's
/// own `states_explored` count so the message names the actual state count
/// observed, not a generic placeholder.
fn budget_fallback_reason(explicit_output: &Value) -> String {
    match explicit_output
        .get("states_explored")
        .and_then(Value::as_u64)
    {
        Some(states_explored) => format!(
            "explicit-state exploration reached its {states_explored}-state budget; falling back to symbolic BMC"
        ),
        None => {
            "explicit-state exploration exceeded the state budget; falling back to symbolic BMC"
                .to_owned()
        }
    }
}

fn finish_explicit_output(rendered: Result<(Value, i32), String>) -> (Value, i32) {
    match rendered {
        Ok(output) => output,
        Err(error) => (error_output("internal", &error), 3),
    }
}

pub(super) fn run_bmc_filtered(request: BmcRequest<'_>) -> (Value, i32) {
    let started = Instant::now();
    let (prepared, solved) = match execute_bmc(&request, started) {
        Ok(execution) => execution,
        Err(output) => return output,
    };
    fslc_rust::verification_output::render_bmc_output(
        envelope(),
        &prepared.model,
        &solved.result,
        fslc_rust::verification_output::BmcOutputOptions {
            depth: request.depth,
            deadlock: request.deadlock,
            checked_bounds: prepared.checked_bounds.as_ref(),
            elapsed_s: started.elapsed().as_secs_f64(),
            statistics: &solved.statistics,
            skip_vacuity_probe: request.skip_vacuity_probe,
        },
    )
}

fn execute_bmc(
    request: &BmcRequest<'_>,
    started: Instant,
) -> Result<(PreparedBmc, SolvedBmc), CommandResult> {
    let prepared = prepare_bmc(request, started)?;
    let solved = solve_bmc(request, &prepared)?;
    Ok((prepared, solved))
}

fn prepare_bmc(request: &BmcRequest<'_>, started: Instant) -> Result<PreparedBmc, CommandResult> {
    let model = load_selected_model(request.selection)
        .map_err(|error| (spec_load_error_output(&error), 2))?;
    if request.initial_state.is_none() {
        fsl_runtime::check_init_write_ownership(&model).map_err(|error| {
            (
                fslc_rust::verification_output::render_runtime_error(envelope(), &error),
                2,
            )
        })?;
    }
    let checked_bounds = selected_implicit_bounds(
        &model,
        request.selection.property,
        request.selection.excluded,
    );
    // Some concrete boundary outcomes (notably an over-capacity Seq successor)
    // cannot be represented by the bounded symbolic value and must retain the
    // exact Monitor evidence. Action-context `partial_op` is deliberately not
    // consumed here: it belongs to `verify_bounded*` itself (#651).
    if checked_bounds.is_none()
        && request.initial_state.is_none()
        && fsl_runtime::deterministic_initial_state(&model).is_ok()
    {
        match fsl_runtime::find_boundary_violation(
            &model,
            request.depth,
            fsl_runtime::CONCRETE_PROBE_BUDGET,
        ) {
            Ok(probe) => {
                if let Some((violation, trace)) = probe.finding
                    && violation.kind != "partial_op"
                {
                    let statistics = fsl_solver::VerificationStatistics::default();
                    return Err(fslc_rust::verification_output::render_boundary_output(
                        envelope(),
                        &model,
                        &violation,
                        &trace,
                        &fslc_rust::verification_output::BmcOutputOptions {
                            depth: request.depth,
                            deadlock: request.deadlock,
                            checked_bounds: None,
                            elapsed_s: started.elapsed().as_secs_f64(),
                            statistics: &statistics,
                            // This branch renders `render_boundary_output`,
                            // which never emits `warnings`, so this value is
                            // inert here; threaded through for consistency.
                            skip_vacuity_probe: request.skip_vacuity_probe,
                        },
                    ));
                }
                // `exhausted && finding.is_none()` falls through here exactly
                // like a completed empty search: the pre-pass is an evidence
                // detour, not a verdict authority (issue #697).
            }
            Err(error) => return Err((semantic_error_output(&error.to_string()), 2)),
        }
    }
    Ok(PreparedBmc {
        model,
        checked_bounds,
    })
}

fn solve_bmc(request: &BmcRequest<'_>, prepared: &PreparedBmc) -> Result<SolvedBmc, CommandResult> {
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
    let mut result = match verification {
        Ok(result) => result,
        Err(error) => return Err((semantic_error_output(&error.to_string()), 2)),
    };
    if let Err(error) = ::fslc_rust::verification_output::replay_bmc_witnesses(
        &prepared.model,
        &result,
        request.initial_state,
    ) {
        return Err((error_output("internal", &error), 3));
    }
    // Static reachable diagnosis deliberately uses a separate solver session.
    // Even stack-isolated queries perturb backend query history and can change
    // underdetermined witness projections, which are byte-compared across the
    // native and browser backends.
    let mut statistics = fsl_solver::SmtSolver::statistics(&solver);
    let needs_reachable_diagnosis =
        result.violation.is_none() && result.reachables.values().any(Option::is_none);
    if needs_reachable_diagnosis {
        let mut diagnosis_solver = match fsl_solver_z3::Z3Solver::new() {
            Ok(solver) => solver,
            Err(error) => return Err((error_output("internal", &error.to_string()), 3)),
        };
        result.reachable_diagnostics = match block_on_native(fsl_verifier::diagnose_reachables(
            &prepared.model,
            &mut diagnosis_solver,
        )) {
            Ok(diagnostics) => diagnostics,
            Err(error) => return Err((semantic_error_output(&error.to_string()), 2)),
        };
        statistics.merge(&fsl_solver::SmtSolver::statistics(&diagnosis_solver));
    }
    Ok(SolvedBmc { result, statistics })
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
    candidate.invariants.clear();
    candidate.transitions.clear();
    candidate.leadstos.clear();
    candidate.reachables.clear();
    candidate.invariants.push(fsl_core::PropertyDef {
        name: name.to_owned(),
        expr: expression.clone(),
        span: synthetic_span(),
        meta: None,
        annotations: fsl_core::Annotations::default(),
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
        Ok(proof) => {
            let cti = proof.cti.as_ref().expect("non-proved induction has a CTI");
            json!({
                "expression":source,"name":name,"status":"rejected","used":false,
                "proof":{
                    "result":"unknown_cti","invariant":display(&cti.name),"k":cti.k,
                    "checked_to_depth":depth,"completeness":"bounded",
                    "trace_type":"induction_cti",
                    "cti":{
                        "states": ::fslc_rust::trace_json(&candidate, &cti.trace),
                        "violated_at":cti.k,
                    },
                },
            })
        }
        Err(error) => json!({
            "expression":source,"name":name,"status":"rejected","used":false,
            "proof":{"result":"error","kind":"semantics","message":error.to_string()},
        }),
    }
}

fn collision_free_lemma_name(
    index: usize,
    occupied_names: &mut std::collections::BTreeSet<String>,
) -> String {
    let mut name = format!("AuxiliaryLemma{}", index + 1);
    while occupied_names.contains(&name) {
        name.push_str("Candidate");
    }
    occupied_names.insert(name.clone());
    name
}

fn prepare_lemma_induction<'a>(
    path: &'a Path,
    options: &'a CliVerifyOptions,
    prepared: &'a PreparedCliVerification,
) -> Result<
    (
        ModelSelection<'a>,
        KernelModel,
        std::collections::BTreeSet<String>,
    ),
    CommandResult,
> {
    let model = prepared
        .model
        .as_ref()
        .map_err(|error| (spec_load_error_output(error), 2))?;
    let selection = ModelSelection {
        path,
        model: Some(model),
        scope: None,
        property: options.property.as_deref(),
        excluded: &options.exclude_properties,
    };
    let (selected, occupied_names) = load_lemma_model(selection)?;
    Ok((selection, selected, occupied_names))
}

fn run_induction_with_lemmas(
    path: &Path,
    options: &CliVerifyOptions,
    prepared: &PreparedCliVerification,
) -> (Value, i32) {
    let deadlock = match DeadlockMode::parse(&options.deadlock) {
        Ok(mode) => mode,
        Err(error) => return (error_output("usage", &error), 2),
    };
    let (selection, model, mut occupied_names) =
        match prepare_lemma_induction(path, options, prepared) {
            Ok(prepared) => prepared,
            Err(output) => return output,
        };
    let mut entries = Vec::new();
    let mut proved_candidates = Vec::new();
    let (mut auxiliary, mut auxiliary_sources) = (Vec::new(), Vec::new());
    let mut exclusions = Vec::new();
    for (index, source) in options.lemmas.iter().enumerate() {
        let name = collision_free_lemma_name(index, &mut occupied_names);
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
        let entry = adjudicate_lemma(
            &model,
            &name,
            &expression,
            source,
            options.depth,
            options.k_ind,
        );
        if entry.get("status").and_then(Value::as_str) == Some("proved") {
            proved_candidates.push((entries.len(), name.clone(), expression, source.clone()));
        }
        entries.push(entry);
    }
    // See `execute_cli_verification`'s matching comment: this is the
    // `--lemma` path's own derivation of the same CLI `--vacuity` option,
    // since `run_induction_with_lemmas` is called directly from
    // `run_verify_cli` rather than through `execute_cli_verification`.
    let skip_vacuity_probe = options.vacuity == "ignore";
    let (mut result, status) = loop {
        let (current, status) = run_induction_filtered(InductionRequest {
            selection,
            depth: options.depth,
            deadlock,
            k: options.k_ind,
            auxiliary: &auxiliary,
            skip_vacuity_probe,
        });
        if current.get("result").and_then(Value::as_str) != Some("unknown_cti")
            || current.get("violation_kind").and_then(Value::as_str) == Some("leadsTo_rank")
        {
            break (current, status);
        }
        let Some((candidate_index, violated_steps)) = proved_candidates
            .iter()
            .enumerate()
            .find_map(|(index, (_, _, expression, _))| {
                let steps = lemma_violated_steps(&current, expression, &model);
                (!steps.is_empty()).then_some((index, steps))
            })
        else {
            break (current, status);
        };
        let (entry_index, name, expression, source) = proved_candidates.remove(candidate_index);
        if let Value::Object(entry) = &mut entries[entry_index] {
            entry.insert("used".to_owned(), json!(true));
        }
        exclusions.push(json!({
            "lemma":source,
            "target":current.get("trans").or_else(|| current.get("invariant"))
                .cloned().unwrap_or(Value::Null),
            "k":current.get("k").cloned().unwrap_or_else(|| json!(options.k_ind)),
            "violated_steps":violated_steps,
            "cti":current.get("cti").cloned().unwrap_or(Value::Null),
        }));
        auxiliary.push((name, expression));
        auxiliary_sources.push(source);
    };
    let proved = result.get("result").and_then(Value::as_str) == Some("proved");
    if let Value::Object(output) = &mut result {
        output.insert("lemmas".to_owned(), Value::Array(entries));
        output.insert("lemma_cti_exclusions".to_owned(), Value::Array(exclusions));
        if proved && !auxiliary.is_empty() {
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

fn load_lemma_model(
    selection: ModelSelection<'_>,
) -> Result<(KernelModel, std::collections::BTreeSet<String>), CommandResult> {
    let unselected =
        load_unselected_model(selection).map_err(|error| (spec_load_error_output(&error), 2))?;
    let occupied_names = unselected
        .invariants
        .iter()
        .chain(&unselected.transitions)
        .chain(&unselected.reachables)
        .map(|property| property.name.clone())
        .chain(
            unselected
                .leadstos
                .iter()
                .map(|property| property.name.clone()),
        )
        .collect();
    let model = match selected_transition_induction_model(selection)
        .map_err(|error| (spec_load_error_output(&error), 2))?
    {
        Some(model) => model,
        None => {
            load_selected_model(selection).map_err(|error| (spec_load_error_output(&error), 2))?
        }
    };
    Ok((model, occupied_names))
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

fn verify_cache_keys(
    source_path: &Path,
    identity_path: &Path,
    options: &CliVerifyOptions,
    sources: &[(PathBuf, String)],
) -> Result<(String, String), String> {
    verify_cache_keys_for_engine(
        source_path,
        identity_path,
        options,
        &options.engine,
        sources,
    )
}

/// Cache keys are always computed for a concrete engine. The `auto` engine
/// never keys entries under the literal string "auto": lookups consult the
/// explicit and bmc keys, and stores use whichever engine actually decided,
/// so verdicts are shared with plain `--engine explicit`/`--engine bmc` runs.
fn verify_cache_keys_for_engine(
    source_path: &Path,
    identity_path: &Path,
    options: &CliVerifyOptions,
    engine: &str,
    sources: &[(PathBuf, String)],
) -> Result<(String, String), String> {
    verify_cache_keys_with_solver_version(
        source_path,
        identity_path,
        options,
        engine,
        fsl_solver_z3::version(),
        sources,
    )
}

fn verify_cache_keys_with_solver_version(
    source_path: &Path,
    identity_path: &Path,
    options: &CliVerifyOptions,
    engine: &str,
    solver_version: &str,
    sources: &[(PathBuf, String)],
) -> Result<(String, String), String> {
    verify_cache_keys_with_fingerprints(
        source_path,
        identity_path,
        options,
        engine,
        solver_version,
        env!("FSLC_IMPLEMENTATION_FINGERPRINT"),
        sources,
    )
}

/// `sources` is the resolved-absolute-path/content set actually read while
/// resolving this run's dependencies (issue #1023): the recorder wrapping
/// `FsResolver` during `prepare_cli_verification_from_source`'s model build
/// and, when the inline `implements` seam is active, during
/// `resolve_requirements_implements`. It replaces walking `source_path`'s
/// parent directory, which was a proxy for "what this spec depends on" that
/// was both too narrow (a dependency outside the parent directory was never
/// walked) and too broad (an unrelated sibling `.fsl` file was).
fn verify_cache_keys_with_fingerprints(
    // The dependency domain now comes entirely from `sources` (what was
    // actually read); this key no longer walks a directory rooted at the
    // checked spec's own path, so it is unused here. Kept as a parameter
    // because every caller still has it on hand and the signature reads as
    // "the key for verifying this source, given these deps" either way.
    _source_path: &Path,
    identity_path: &Path,
    options: &CliVerifyOptions,
    engine: &str,
    solver_version: &str,
    implementation_fingerprint: &str,
    sources: &[(PathBuf, String)],
) -> Result<(String, String), String> {
    let canonical_identity = identity_path
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut sorted_sources = sources.to_vec();
    sorted_sources.sort();
    let mut digest = Sha256::new();
    digest.update(b"fslc-rust-verify-cache-v3\0");
    digest.update(env!("CARGO_PKG_VERSION").as_bytes());
    digest.update(b"\0identity-source\0");
    digest.update(std::fs::read(identity_path).map_err(|error| error.to_string())?);
    if engine == "explicit" {
        digest.update(b"\0backend=native-explicit\0");
    } else {
        digest.update(b"\0backend=native-z3\0solver=");
        digest.update(solver_version.as_bytes());
        digest.update(b"\0");
    }
    digest.update(b"implementation\0");
    digest.update(implementation_fingerprint.as_bytes());
    digest.update(b"\0");
    for (path, content) in &sorted_sources {
        digest.update(path.as_os_str().as_encoded_bytes());
        digest.update(b"\0");
        digest.update(content.as_bytes());
        digest.update(b"\0");
    }
    if let Some(requirements) = options.requirements.as_deref() {
        digest.update(b"requirements\0");
        digest.update(std::fs::read(requirements).map_err(|error| error.to_string())?);
    }
    let base_options = json!({
        "path": canonical_identity,
        "deadlock": options.deadlock,
        "engine": engine,
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
    if !valid_cache_key(key) {
        return None;
    }
    Some(
        cache_root()?
            .join("verify/v3")
            .join(&key[..2])
            .join(format!("{key}.json")),
    )
}

fn valid_cache_key(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn verify_cache_lookup(key: &str, xdepth: &str, depth: usize) -> Option<Value> {
    if !valid_cache_key(xdepth) {
        return None;
    }
    let path = verify_cache_path(key)?;
    if let Ok(bytes) = std::fs::read(path)
        && let Ok(entry) = serde_json::from_slice::<Value>(&bytes)
        && let Some(mut output) = verified_cache_entry_output(&entry, key, xdepth)
    {
        output.as_object_mut()?.insert(
            "cache".to_owned(),
            json!({"hit": true, "key": key, "source": "exact"}),
        );
        return Some(output);
    }
    let pointer_path = cache_root()?
        .join("verify/v3/xdepth")
        .join(format!("{xdepth}.json"));
    let pointer: Value = serde_json::from_slice(&std::fs::read(pointer_path).ok()?).ok()?;
    if pointer.get("schema").and_then(Value::as_str) != Some("fslc-rust-cache-pointer.v2")
        || pointer.get("xdepth").and_then(Value::as_str) != Some(xdepth)
    {
        return None;
    }
    let violation_step = usize::try_from(pointer.get("violated_at_step")?.as_u64()?).ok()?;
    if violation_step > depth {
        return None;
    }
    let target = pointer.get("entry_key")?.as_str()?;
    let entry: Value =
        serde_json::from_slice(&std::fs::read(verify_cache_path(target)?).ok()?).ok()?;
    let mut output = verified_cross_depth_output(&entry, target, xdepth, violation_step)?;
    output.as_object_mut()?.insert(
        "cache".to_owned(),
        json!({"hit": true, "key": target, "source": "cross_depth"}),
    );
    Some(output)
}

fn verified_cache_entry_output(
    entry: &Value,
    expected_key: &str,
    expected_xdepth: &str,
) -> Option<Value> {
    (entry.get("schema").and_then(Value::as_str) == Some("fslc-rust-cache.v2")
        && entry.get("key").and_then(Value::as_str) == Some(expected_key)
        && entry.get("xdepth").and_then(Value::as_str) == Some(expected_xdepth))
    .then(|| entry.get("output").cloned())?
    .filter(|output| cached_output_status(output).is_some())
}

fn verified_cross_depth_output(
    entry: &Value,
    expected_key: &str,
    expected_xdepth: &str,
    violation_step: usize,
) -> Option<Value> {
    let output = verified_cache_entry_output(entry, expected_key, expected_xdepth)?;
    (output.get("result").and_then(Value::as_str) == Some("violated")
        && output.get("violated_at_step").and_then(Value::as_u64)
            == Some(u64::try_from(violation_step).ok()?))
    .then_some(output)
}

fn verify_cache_store(key: &str, xdepth: &str, output: &Value) {
    if !valid_cache_key(key) || !valid_cache_key(xdepth) {
        return;
    }
    // Cacheability, not success: `violated` and the three inconclusive
    // verdicts are failure-class and still worth storing. The predicate lives
    // beside `outcome_class` in `fslc_rust::outcome` so the vocabulary has one
    // home, and stays a distinct function so that a new result value forces an
    // explicit decision in both (issue #537 C2).
    if !fslc_rust::outcome::verify_cache_admits(output) {
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
        "schema": "fslc-rust-cache.v2",
        "key": key,
        "xdepth": xdepth,
        "backend": if explicit { "native-explicit" } else { "native-z3" },
        "solver_version": if explicit {
            Value::Null
        } else {
            json!(fsl_solver_z3::version())
        },
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
        let directory = root.join("verify/v3/xdepth");
        if std::fs::create_dir_all(&directory).is_ok() {
            let pointer = directory.join(format!("{xdepth}.json"));
            let temporary = directory.join(format!(".{xdepth}.{}.tmp", std::process::id()));
            if serde_json::to_vec(&json!({
                "schema": "fslc-rust-cache-pointer.v2",
                "xdepth": xdepth,
                "entry_key": key,
                "violated_at_step": step,
            }))
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
    is_agent_document: bool,
    model: Result<KernelModel, SpecLoadError>,
    initial_state: Option<std::collections::BTreeMap<String, FslValue>>,
    /// Compose-lowering warnings (e.g. `fair_not_inherited`), computed while
    /// lowering the surface document, before `build_model` drops the per-
    /// component information (like constituent `fair` markers) that produced
    /// them. `model`/`fsl_runtime::verification_warnings` cannot recover them.
    compose_warnings: Vec<Value>,
}

pub(super) fn run_verify_cli(
    path: &Path,
    cache_identity_path: &Path,
    options: &CliVerifyOptions,
) -> CommandResult {
    let source = match read_spec_source(path) {
        Ok(source) => source,
        Err(error) => return (spec_load_error_output(&error), 2),
    };
    run_verify_cli_from_source(path, cache_identity_path, &source, options)
}

/// Whether this run selects a subset of the model, and therefore does not
/// evaluate the inline `implements` seam.
///
/// This is the whole population of seam suppressors: `docs/LANGUAGE.md` names
/// these three options and nothing else, and both call sites read it from here
/// so the list cannot drift in one of them. #1008 is open against the semantics
/// of these three, so a change there has to land in one place.
fn seam_is_suppressed(options: &CliVerifyOptions, prepared: &PreparedCliVerification) -> bool {
    options.property.is_some()
        || !options.exclude_properties.is_empty()
        || prepared.initial_state.is_some()
}

/// A `FileResolver` decorator that records every dependency it actually
/// reads, as (path relative to `base`, content). This is the cache key's
/// dependency domain (issue #1023): passing this single instance into every
/// place `run_verify_cli_from_source` resolves a `from`/`use` dependency —
/// `prepare_cli_verification_from_source`'s model build and, when the
/// inline `implements` seam is active, `resolve_requirements_implements` —
/// makes the key's domain match what the verdict actually depends on by
/// construction, instead of a directory-walk proxy for it.
///
/// Recorded relative to `base` rather than as a resolved absolute path
/// (unlike the checked spec's own identity, which is hashed by content):
/// every `read` on one `RecordingResolver` shares the same fixed `base`
/// (nested `from`/`use` reuses the same resolver instance all the way
/// down), so the relative path is already unambiguous, and it matches what
/// the pre-#1023 directory walk hashed for an in-parent dependency
/// (`source.strip_prefix(base)`). A checked literate `.md` document is
/// verified through a per-process `.{stem}.literate-{pid}.fsl`
/// materialization (`literate_access.rs`) sitting beside it; that sibling
/// was excluded from the old walk for exactly this reason
/// (`is_literate_materialization`) and never reaches this resolver at all
/// (its own root source is read directly, by content, never through a
/// `from`/`use` clause) — but recording relative-to-base keeps that
/// property true by construction rather than by a second exclusion list,
/// instead of an absolute path that would embed a directory a symlinked
/// alias resolves through.
struct RecordingResolver {
    base: PathBuf,
    reads: std::cell::RefCell<Vec<(PathBuf, String)>>,
}

impl RecordingResolver {
    fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            base: base.into(),
            reads: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn take_sources(&self) -> Vec<(PathBuf, String)> {
        self.reads.borrow().clone()
    }
}

impl fsl_core::FileResolver for RecordingResolver {
    fn read(&self, path: &str) -> Result<String, fsl_core::CoreError> {
        let content = std::fs::read_to_string(self.base.join(path))
            .map_err(|error| fsl_core::CoreError::unlocated(error.to_string()))?;
        self.reads
            .borrow_mut()
            .push((PathBuf::from(path), content.clone()));
        Ok(content)
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn run_verify_cli_from_source(
    path: &Path,
    cache_identity_path: &Path,
    source: &str,
    options: &CliVerifyOptions,
) -> CommandResult {
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
    // Causal models are intentionally outside the kernel dialect dispatcher.
    // Check their syntax first so a malformed causal input retains its parser
    // diagnostic instead of being rewritten as an unknown kernel dialect.
    // Uses the already-captured `source` snapshot rather than re-reading
    // `path`, so this check can never observe a different root source than
    // the rest of this verification.
    if fsl_syntax::is_causal_source(source)
        && let Err(error) = fsl_syntax::parse_causal(source)
    {
        return super::causal::causal_parse_error_output(&error, false);
    }
    let recorder = RecordingResolver::new(path.parent().unwrap_or_else(|| Path::new(".")));
    let prepared = match prepare_cli_verification_from_source(path, source, options, &recorder) {
        Ok(prepared) => prepared,
        Err(output) => return output,
    };
    if !options.lemmas.is_empty() {
        let (mut output, mut status) = run_induction_with_lemmas(path, options, &prepared);
        // The `--lemma` path returns before `execute_cli_verification`, so the
        // inline `implements` seam has to be evaluated and folded here as well.
        // Without this, `verify --engine induction --lemma ...` is another way
        // to pass a broken seam with exit 0 (#1002), and the suppressor list in
        // `docs/LANGUAGE.md` would be missing an entry.
        if !seam_is_suppressed(options, &prepared)
            && let Ok(model) = &prepared.model
        {
            match implements_result_from_source_with_bounds(
                path,
                source,
                model,
                options.depth,
                &options.scope,
            ) {
                Ok(Some(implements)) => {
                    if let Some(code) =
                        fslc_rust::verification_output::attach_requirements_implements(
                            &mut output,
                            implements,
                        )
                    {
                        status = code;
                    }
                }
                Ok(None) => {}
                Err(error) => return (implements_error_output(&error), 2),
            }
        }
        return finalize_cli_verification(path, options, &prepared, None, output, status);
    }
    // The inline `implements` seam's dependency is read here, before the
    // cache key below, rather than inside `execute_cli_verification`
    // (issue #1023): the cache key must be able to see it, and the key is
    // computed before `execute_cli_verification` ever runs on a cache miss.
    // `selection_filtered` is computed exactly once here and threaded into
    // `execute_cli_verification` instead of being recomputed there, so
    // "is the seam active" stays a single fact rather than two copies of the
    // same check that could drift.
    let selection_filtered = seam_is_suppressed(options, &prepared);
    let implements_contract = if selection_filtered {
        None
    } else {
        match &prepared.model {
            Ok(model) => match fslc_rust::verification_output::resolve_requirements_implements(
                source,
                &recorder,
                model,
                &options.scope.instances,
                &options.scope.values,
            ) {
                Ok(contract) => contract,
                Err(error) => return (implements_error_output(&error), 2),
            },
            // `execute_cli_verification` reports this itself before it would
            // ever reach the implements seam.
            Err(_) => None,
        }
    };
    let sources = recorder.take_sources();
    // `auto` never keys a cache entry under the literal string "auto": a
    // lookup consults the explicit/bmc keys directly
    // (`cached_auto_verification`) and a store writes under whichever engine
    // actually decided (`store_auto_verification`), so verdicts are shared
    // with plain `--engine explicit`/`--engine bmc` runs of the same spec.
    let is_auto = options.engine == "auto";
    let cache_keys = (!is_auto && cache_enabled(options))
        .then(|| verify_cache_keys(path, cache_identity_path, options, &sources).ok())
        .flatten();
    if let Some(output) = cached_verification(options, cache_keys.as_ref()) {
        return output;
    }
    if is_auto
        && cache_enabled(options)
        && let Some(output) =
            cached_auto_verification(path, cache_identity_path, options, &prepared, &sources)
    {
        return output;
    }
    let (output, status) = execute_cli_verification(
        path,
        source,
        options,
        &prepared,
        selection_filtered,
        implements_contract,
    );
    let (output, status) = finalize_cli_verification(
        path,
        options,
        &prepared,
        cache_keys.as_ref(),
        output,
        status,
    );
    if is_auto && cache_enabled(options) {
        store_auto_verification(path, cache_identity_path, options, &output, &sources);
    }
    (output, status)
}

fn prepare_cli_verification_from_source(
    path: &Path,
    source: &str,
    options: &CliVerifyOptions,
    recorder: &RecordingResolver,
) -> Result<PreparedCliVerification, CommandResult> {
    let is_agent_document = match fsl_syntax::parse_surface_document(source) {
        Ok(fsl_syntax::SurfaceDocument::Agent(_)) => true,
        Ok(_) => false,
        Err(error) => return Err((surface_parse_error_output(&error), 2)),
    };
    let has_scope = !options.scope.instances.is_empty() || !options.scope.values.is_empty();
    if let Err(error) = validate_specialized_document_from_source(path, source) {
        return Err((semantic_error_output(&error), 2));
    }
    // `--instances`/`--values` scope overrides go through
    // `parse_kernel_source_with_bounds` (`load_model_scoped_from_source`),
    // which does not take a resolver at all: compose/`from` is not resolved
    // on this path, so there is nothing for `recorder` to observe here.
    let snapshot_model = if has_scope {
        load_model_scoped_from_source(path, source, &options.scope)
    } else {
        load_kernel_model_from_source_with_resolver(path, source, recorder).map(|(_, model)| model)
    };
    let initial_state = if let Some(snapshot_path) = options.from_state.as_deref() {
        let model = snapshot_model
            .as_ref()
            .map_err(|error| (spec_load_error_output(error), 2))?;
        match load_state_snapshot(snapshot_path, model) {
            Ok(state) => Some(state),
            Err((kind, error)) => return Err((error_output(&kind, &error), 2)),
        }
    } else {
        None
    };
    if !has_scope && let Ok(model) = &snapshot_model {
        match validate_requirement_traces_from_source(path, source, model) {
            Ok((Some(failure), _)) => return Err((failure, 2)),
            Ok((None, _)) => {}
            Err(error) => return Err((semantic_error_output(&error), 2)),
        }
    }
    // `--instances`/`--values` scope overrides go through
    // `parse_kernel_source_with_bounds` (`load_model_scoped`), a separate
    // lowering entrypoint that does not apply to compose documents, so
    // compose-lowering warnings are only collected in the unscoped case.
    let compose_warnings = if has_scope {
        Vec::new()
    } else {
        load_kernel_model_from_source_with_resolver(path, source, recorder)
            .map(|(kernel, _)| kernel.diagnostics().to_vec())
            .unwrap_or_default()
    };
    validate_cli_property_selection(
        path,
        source,
        options,
        has_scope,
        snapshot_model.as_ref().ok(),
    )?;
    Ok(PreparedCliVerification {
        has_scope,
        is_agent_document,
        model: snapshot_model,
        initial_state,
        compose_warnings,
    })
}

fn validate_cli_property_selection(
    path: &Path,
    source: &str,
    options: &CliVerifyOptions,
    has_scope: bool,
    prepared_model: Option<&KernelModel>,
) -> Result<(), CommandResult> {
    if options.property.is_none() && options.exclude_properties.is_empty() {
        return Ok(());
    }
    let mut model = match prepared_model {
        Some(model) => Ok(model.clone()),
        None if has_scope => load_model_scoped_from_source(path, source, &options.scope),
        None => load_model_from_source(path, source),
    }
    .map_err(|error| (spec_load_error_output(&error), 2))?;
    if options.engine == "induction"
        && let Some(name) = options.property.as_deref()
        && let Some(kind) = model
            .leadstos
            .iter()
            .any(|item| display(&item.name) == name)
            .then_some("leadsTo")
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
    let status = cached_output_status(&output)?;
    Some((output, status))
}

fn cached_output_status(output: &Value) -> Option<i32> {
    let output = output.as_object()?;
    if output.get("fsl").and_then(Value::as_str) != Some("1.0")
        || output.get("spec").and_then(Value::as_str)?.is_empty()
    {
        return None;
    }
    let completeness = output.get("completeness").and_then(Value::as_str);
    match output.get("result").and_then(Value::as_str) {
        Some("verified") if completeness == Some("bounded") => Some(0),
        Some("proved") if completeness == Some("unbounded") => Some(0),
        Some("violated") if valid_cached_violation(output, completeness) => Some(1),
        Some("reachable_failed")
            if matches!(completeness, Some("bounded" | "unbounded"))
                && output.get("unreached").and_then(Value::as_array).is_some() =>
        {
            Some(1)
        }
        Some("unknown_cti")
            if completeness == Some("bounded")
                && output.get("cti").and_then(Value::as_object).is_some() =>
        {
            Some(1)
        }
        Some("unknown_budget")
            if completeness == Some("unknown")
                && output
                    .get("states_explored")
                    .and_then(Value::as_u64)
                    .is_some() =>
        {
            Some(1)
        }
        _ => None,
    }
}

fn valid_cached_violation(output: &Map<String, Value>, completeness: Option<&str>) -> bool {
    if completeness != Some("bounded") || output.get("trace").and_then(Value::as_array).is_none() {
        return false;
    }
    match output.get("violation_kind").and_then(Value::as_str) {
        Some("leadsTo") => {
            output
                .get("pending_since")
                .and_then(Value::as_u64)
                .is_some()
                && output.get("bindings").and_then(Value::as_object).is_some()
                && output.get("trace_type").and_then(Value::as_str) == Some("leadsTo")
        }
        Some(_) => output
            .get("violated_at_step")
            .and_then(Value::as_u64)
            .is_some(),
        None => false,
    }
}

/// Gate check for `--engine auto`'s explicit-then-bmc fallback. Used both on
/// a fresh run's pre-check and to recompute a warm-cache hit's fallback
/// annotation, so the stamp always reflects what *this* invocation's own
/// gate decided rather than which earlier invocation happened to write the
/// bmc cache entry being reused (see `cached_auto_verification`).
fn explicit_gate_reason(
    path: &Path,
    options: &CliVerifyOptions,
    prepared: &PreparedCliVerification,
) -> Option<String> {
    let selection = ModelSelection {
        path,
        model: prepared.model.as_ref().ok(),
        scope: prepared.has_scope.then_some(&options.scope),
        property: options.property.as_deref(),
        excluded: &options.exclude_properties,
    };
    let model = load_selected_model(selection).ok()?;
    if model.actions.is_empty() {
        // A fresh run reports this as a genuine error (see `run_auto_filtered`),
        // never a fallback; returning `None` here lets the caller fall through
        // to a fresh execution instead of misreporting it as a bmc fallback.
        return None;
    }
    fsl_runtime::explicit_unsupported_reason(&model)
}

/// Cache lookup for `--engine auto`: consult both concrete engines' entries
/// before re-running anything. A cached explicit verdict wins (it may be a
/// closure proof), except `unknown_budget`, which auto never reports. The
/// bmc key is consulted only when *this* invocation's own gate logic
/// (`explicit_gate_reason`, or a cached explicit `unknown_budget` verdict)
/// determines a fresh run would fall back to it; the `engine_fallback` stamp
/// is always recomputed here, never read off the cache entry, so a warm hit
/// is indistinguishable from a fresh run no matter which earlier invocation
/// actually wrote that bmc entry. Failing all of that, the run proceeds
/// fresh — in particular, a bare bmc entry is never returned while explicit
/// is still viable and undecided, so warm-cache dispatch always matches
/// cold-cache dispatch.
fn cached_auto_verification(
    source_path: &Path,
    identity_path: &Path,
    options: &CliVerifyOptions,
    prepared: &PreparedCliVerification,
    sources: &[(PathBuf, String)],
) -> Option<CommandResult> {
    if std::env::var("FSLC_CACHE_VERIFY").as_deref() == Ok("1") {
        return None;
    }
    let explicit_cached =
        verify_cache_keys_for_engine(source_path, identity_path, options, "explicit", sources)
            .ok()
            .and_then(|(key, xdepth)| verify_cache_lookup(&key, &xdepth, options.depth));
    if let Some(output) = &explicit_cached
        && output.get("result").and_then(Value::as_str) != Some("unknown_budget")
    {
        let status = cached_output_status(output)?;
        return Some((output.clone(), status));
    }
    let (reason, kind) = if let Some(reason) = explicit_gate_reason(source_path, options, prepared)
    {
        (reason, "unsupported")
    } else if let Some(explicit) = &explicit_cached {
        (budget_fallback_reason(explicit), "budget")
    } else {
        return None;
    };
    let (key, xdepth) =
        verify_cache_keys_for_engine(source_path, identity_path, options, "bmc", sources).ok()?;
    let mut output = verify_cache_lookup(&key, &xdepth, options.depth)?;
    annotate_auto_fallback(&mut output, &reason, kind);
    let status = cached_output_status(&output)?;
    Some((output, status))
}

/// Cache store for `--engine auto`: entries are keyed by the engine that
/// actually decided. A post-fallback BMC verdict is stored as the plain BMC
/// envelope, carrying the same fields as what a plain `--engine bmc` run
/// would store — `engine`/`engine_fallback` are stripped before writing and
/// are never persisted on the entry; a later auto lookup recomputes the
/// fallback stamp itself (`cached_auto_verification`) instead of reading it
/// back.
fn store_auto_verification(
    source_path: &Path,
    identity_path: &Path,
    options: &CliVerifyOptions,
    output: &Value,
    sources: &[(PathBuf, String)],
) {
    match output.get("engine").and_then(Value::as_str) {
        Some("explicit") => {
            if let Ok((key, xdepth)) = verify_cache_keys_for_engine(
                source_path,
                identity_path,
                options,
                "explicit",
                sources,
            ) {
                verify_cache_store(&key, &xdepth, output);
            }
        }
        Some("bmc") => {
            let mut plain = output.clone();
            if let Some(envelope) = plain.as_object_mut() {
                envelope.remove("engine");
                envelope.remove("engine_fallback");
            }
            if let Ok((key, xdepth)) =
                verify_cache_keys_for_engine(source_path, identity_path, options, "bmc", sources)
            {
                verify_cache_store(&key, &xdepth, &plain);
            }
        }
        _ => {}
    }
}

fn execute_cli_verification(
    path: &Path,
    source: &str,
    options: &CliVerifyOptions,
    prepared: &PreparedCliVerification,
    selection_filtered: bool,
    implements_contract: Option<fsl_core::ImplementsContract>,
) -> CommandResult {
    if !(selection_filtered || prepared.has_scope) && prepared.is_agent_document {
        return (
            error_output(
                "parse",
                "agent documents cannot be verified as Kernel specs",
            ),
            2,
        );
    }
    let deadlock = match DeadlockMode::parse(&options.deadlock) {
        Ok(mode) => mode,
        Err(error) => return (error_output("usage", &error), 2),
    };
    let model = match &prepared.model {
        Ok(model) => model,
        Err(error) => return (spec_load_error_output(error), 2),
    };
    // The seam's dependency was already read in `run_verify_cli_from_source`
    // (issue #1023), before the cache key; only the (comparatively
    // expensive) refinement check runs here, after a cache miss.
    let implements = match fslc_rust::verification_output::check_requirements_implements(
        implements_contract,
        model,
        options.depth,
    ) {
        Ok(implements) => implements,
        Err(error) => return (implements_error_output(&error), 2),
    };
    let selection = ModelSelection {
        path,
        model: Some(model),
        scope: None,
        property: options.property.as_deref(),
        excluded: &options.exclude_properties,
    };
    // INVARIANT (issue #729): together with `run_induction_with_lemmas`'
    // matching derivation (the `--lemma` path bypasses this function), this
    // is the *only* place in the product that may set `skip_vacuity_probe`
    // to `true`. It is derived directly from the CLI's own `--vacuity`
    // option parsing, which both `verify` and `sweep` funnel through
    // `run_verify_cli` -- `sweep` builds its own `CliVerifyOptions` per
    // grid cell and calls `run_verify_cli` exactly like `verify` does.
    // Every other constructor of `BmcRequest`/`InductionRequest`/
    // `ExplicitRequest` in this codebase (the `ledger`/`html`/`mutate`
    // baseline in `rust/fslc/src/main.rs`'s `run_verify`, and tests) must
    // pass `false`.
    let skip_vacuity_probe = options.vacuity == "ignore";
    let (mut output, status) = match VerificationEngine::parse(&options.engine) {
        Ok(VerificationEngine::Bmc) => run_bmc_filtered(BmcRequest {
            selection,
            depth: options.depth,
            deadlock,
            initial_state: prepared.initial_state.as_ref(),
            skip_vacuity_probe,
        }),
        Ok(VerificationEngine::Induction) => run_induction_filtered(InductionRequest {
            selection,
            depth: options.depth,
            deadlock,
            k: options.k_ind,
            auxiliary: &[],
            skip_vacuity_probe,
        }),
        Ok(VerificationEngine::Explicit) => run_explicit_filtered(ExplicitRequest {
            selection,
            depth: options.depth,
            deadlock,
            budget: options.explicit_budget,
            skip_vacuity_probe,
        }),
        Ok(VerificationEngine::Auto) => run_auto_filtered(ExplicitRequest {
            selection,
            depth: options.depth,
            deadlock,
            budget: options.explicit_budget,
            skip_vacuity_probe,
        }),
        Err(error) => return (error_output("usage", &error), 2),
    };
    if !selection_filtered
        && let Some(code) = decorate_default_cli_verification(
            &mut output,
            source,
            model,
            implements,
            &prepared.compose_warnings,
        )
    {
        return (output, code);
    }
    (output, status)
}

fn decorate_default_cli_verification(
    output: &mut Value,
    source: &str,
    model: &KernelModel,
    implements: Option<Value>,
    compose_warnings: &[Value],
) -> Option<i32> {
    let implements_exit = implements.and_then(|implements| {
        fslc_rust::verification_output::attach_requirements_implements(output, implements)
    });
    if let Ok(ctx) = fsl_core::ModelWarningContext::from_source(model, source) {
        fsl_core::finalize_envelope_model_warnings(output, &ctx);
    }
    let Value::Object(envelope) = output else {
        return Some(3);
    };
    if !compose_warnings.is_empty()
        && envelope.get("result").and_then(Value::as_str) != Some("error")
        && let Some(Value::Array(warnings)) = envelope.get_mut("warnings")
    {
        // Compose-lowering warnings (e.g. `fair_not_inherited`) are computed
        // while lowering, before the checked KernelModel drops the per-
        // component information that produced them, so they cannot be
        // recovered from `model`/`fsl_runtime::verification_warnings` alone.
        warnings.splice(0..0, compose_warnings.iter().cloned());
    }
    implements_exit
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
        && let Err(output) = add_cli_strict_tag_warnings(
            path,
            options,
            prepared
                .model
                .as_ref()
                .expect("successful verification has a prepared model"),
            &mut output,
        )
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
    model: &KernelModel,
    output: &mut Value,
) -> Result<(), CommandResult> {
    add_strict_tag_warnings(output, model, path, true, options.requirements.as_deref())
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
                model: None,
                scope: None,
                property: None,
                excluded: &excluded,
            },
            depth: 4,
            deadlock: DeadlockMode::Ignore,
            initial_state: None,
            skip_vacuity_probe: false,
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
                model: None,
                scope: None,
                property: None,
                excluded: &excluded,
            },
            depth: 4,
            deadlock: DeadlockMode::Ignore,
            k: 1,
            auxiliary: &[],
            skip_vacuity_probe: false,
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
        let (output, status) = render_induction_cti(
            &model,
            &cti,
            1,
            Instant::now(),
            &fsl_solver::VerificationStatistics::default(),
        );
        assert_eq!(status, 1);
        assert_eq!(output["invariant"], "mustBeApproved");
        assert_eq!(output["generated_name"], "Order_mustBeApproved");
        assert_eq!(output["origin"]["dialect"], "domain");
    }

    #[test]
    fn corrupted_explicit_evidence_becomes_an_internal_exit_three() {
        let source = r"spec CorruptExplicitEvidence {
  state { done: Bool }
  init { done = false }
  action finish() { requires not done done = true }
  invariant NeverDone { not done }
}";
        let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
            .expect("lower explicit fixture");
        let model = fsl_core::build_model(kernel).expect("build explicit fixture");
        let mut result =
            fsl_runtime::verify_explicit(model.clone(), 2, 100).expect("run explicit verification");
        result
            .violation
            .as_mut()
            .expect("violation evidence")
            .trace
            .last_mut()
            .expect("violating state")
            .state
            .insert("done".to_owned(), FslValue::Bool(false));

        let rendered = fslc_rust::verification_output::render_explicit_output(
            envelope(),
            &model,
            &result,
            None,
            DeadlockMode::Ignore,
            0.0,
            &[],
            &std::collections::BTreeMap::new(),
            false,
        );
        let (output, status) = finish_explicit_output(rendered);
        assert_eq!(status, 3);
        assert_eq!(output["result"], "error");
        assert_eq!(output["kind"], "internal");
    }

    #[test]
    fn explicit_cache_keys_include_the_engine_and_state_budget() {
        let path = repository_path("examples/gallery/valid/tiny_turnstile.fsl");
        let bmc = CliVerifyOptions::default();
        let mut explicit = bmc.clone();
        explicit.engine = "explicit".to_owned();
        let mut smaller_budget = explicit.clone();
        smaller_budget.explicit_budget -= 1;

        let bmc_keys = verify_cache_keys(&path, &path, &bmc, &[]).expect("BMC cache keys");
        let explicit_keys =
            verify_cache_keys(&path, &path, &explicit, &[]).expect("explicit cache keys");
        let smaller_keys = verify_cache_keys(&path, &path, &smaller_budget, &[])
            .expect("budget-specific cache keys");

        assert_ne!(bmc_keys, explicit_keys);
        assert_ne!(explicit_keys, smaller_keys);
    }

    #[test]
    fn bmc_cache_keys_include_the_loaded_solver_version() {
        let path = repository_path("examples/gallery/valid/tiny_turnstile.fsl");
        let options = CliVerifyOptions::default();

        let current = verify_cache_keys_with_solver_version(
            &path,
            &path,
            &options,
            "bmc",
            "Z3 4.16.0.0",
            &[],
        )
        .expect("current solver cache keys");
        let updated = verify_cache_keys_with_solver_version(
            &path,
            &path,
            &options,
            "bmc",
            "Z3 4.17.0.0",
            &[],
        )
        .expect("updated solver cache keys");

        assert_ne!(current, updated);
    }

    #[test]
    fn cache_entries_and_cross_depth_pointers_fail_closed_on_mismatch() {
        const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        const XDEPTH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let valid = json!({
            "schema": "fslc-rust-cache.v2",
            "key": KEY,
            "xdepth": XDEPTH,
            "output": {
                "fsl": "1.0",
                "spec": "CacheTest",
                "result": "violated",
                "violation_kind": "invariant",
                "violated_at_step": 2,
                "trace": [],
                "completeness": "bounded",
            },
        });
        assert_eq!(
            verified_cross_depth_output(&valid, KEY, XDEPTH, 2),
            valid.get("output").cloned()
        );

        for mismatched in [
            json!({"schema":"other","key":KEY,"xdepth":XDEPTH,"output":valid["output"]}),
            json!({"schema":"fslc-rust-cache.v2","key":"other","xdepth":XDEPTH,"output":valid["output"]}),
            json!({"schema":"fslc-rust-cache.v2","key":KEY,"xdepth":"other","output":valid["output"]}),
            json!({"schema":"fslc-rust-cache.v2","key":KEY,"xdepth":XDEPTH,"output":{"fsl":"1.0","spec":"CacheTest","result":"verified","completeness":"bounded"}}),
            json!({"schema":"fslc-rust-cache.v2","key":KEY,"xdepth":XDEPTH,"output":{"fsl":"1.0","spec":"CacheTest","result":"violated","violation_kind":"invariant","violated_at_step":3,"trace":[],"completeness":"bounded"}}),
        ] {
            assert!(
                verified_cross_depth_output(&mismatched, KEY, XDEPTH, 2).is_none(),
                "accepted mismatched cache entry: {mismatched}"
            );
        }

        for malformed in [
            "",
            "a",
            "é",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            assert!(verify_cache_path(malformed).is_none());
        }
        assert!(
            verified_cache_entry_output(
                &json!({"schema":"fslc-rust-cache.v2","key":KEY,"xdepth":XDEPTH,"output":{}}),
                KEY,
                XDEPTH,
            )
            .is_none()
        );
        assert!(verified_cache_entry_output(
            &json!({"schema":"fslc-rust-cache.v2","key":KEY,"xdepth":XDEPTH,"output":{"result":"bogus"}}),
            KEY,
            XDEPTH,
        )
        .is_none());
    }

    /// Parses `source` (rooted at `path`) through `resolver`, returning the
    /// dependency `sources` the recorder observed. A minimal stand-in for
    /// what `prepare_cli_verification_from_source` does in production, used
    /// here so a unit test can drive `verify_cache_keys` without the full
    /// CLI path.
    fn recorded_sources(
        path: &Path,
        source: &str,
        resolver: &RecordingResolver,
    ) -> Vec<(PathBuf, String)> {
        load_kernel_model_from_source_with_resolver(path, source, resolver)
            .expect("lower compose source");
        resolver.take_sources()
    }

    const SYMLINK_ALIAS_COMPOSE_SOURCE: &str = r#"compose Example {
  use DependencyA as dep from "dependency.fsl"
  invariant DepNonNeg { dep.n >= 0 }
}
"#;

    const SYMLINK_ALIAS_DEPENDENCY_SOURCE_A: &str = "spec DependencyA {
  state { n: 0..3 }
  init { n = 0 }
  action bump() { n = 0 }
}
";
    const SYMLINK_ALIAS_DEPENDENCY_SOURCE_B: &str = "spec DependencyA {
  state { n: 0..3 }
  init { n = 0 }
  action bump() { n = 1 }
}
";

    /// #1023's cache key is built from what the resolver actually reads, not
    /// a directory walk, so this test drives the same resolution production
    /// uses (`load_kernel_model_from_source_with_resolver`) instead of
    /// asserting on an unreferenced sibling file (the pre-#1023 version of
    /// this test hashed `dependency.fsl` only because the directory walk
    /// swept it up, whether or not `materialized` actually depended on it —
    /// exactly the "too broad" side of the domain mismatch #1023 fixes).
    /// `dependency.fsl` is now a real `use ... from` dependency of the
    /// composed `materialized` document, so editing it must still change the
    /// key — preserving this test's original concern (a dependency beside a
    /// symlinked alias is tracked) as a requirement on the *resolved* set,
    /// not a side effect of walking `alias`'s directory.
    #[cfg(unix)]
    #[test]
    fn literate_cache_keys_track_dependencies_beside_a_symlink_alias() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "fslc-literate-symlink-cache-{}",
            std::process::id()
        ));
        let real = root.join("real");
        let alias = root.join("alias");
        std::fs::create_dir_all(&real).expect("create real directory");
        std::fs::create_dir_all(&alias).expect("create alias directory");
        let real_document = real.join("spec.md");
        std::fs::write(
            &real_document,
            format!("```fsl\n{SYMLINK_ALIAS_COMPOSE_SOURCE}```\n"),
        )
        .expect("write real document");
        let alias_document = alias.join("spec.md");
        symlink(&real_document, &alias_document).expect("create document alias");
        let materialized = alias.join(".spec.literate-1.fsl");
        std::fs::write(&materialized, SYMLINK_ALIAS_COMPOSE_SOURCE).expect("write materialization");
        std::fs::write(
            alias.join("dependency.fsl"),
            SYMLINK_ALIAS_DEPENDENCY_SOURCE_A,
        )
        .expect("write dependency");

        let options = CliVerifyOptions::default();
        let resolver = RecordingResolver::new(&alias);
        let sources = recorded_sources(&materialized, SYMLINK_ALIAS_COMPOSE_SOURCE, &resolver);
        let before = verify_cache_keys(&materialized, &alias_document, &options, &sources)
            .expect("cache key before dependency edit");
        std::fs::write(
            alias.join("dependency.fsl"),
            SYMLINK_ALIAS_DEPENDENCY_SOURCE_B,
        )
        .expect("edit dependency");
        let resolver = RecordingResolver::new(&alias);
        let sources = recorded_sources(&materialized, SYMLINK_ALIAS_COMPOSE_SOURCE, &resolver);
        let after = verify_cache_keys(&materialized, &alias_document, &options, &sources)
            .expect("cache key after dependency edit");
        assert_ne!(before, after);

        // Calibration: resolving from the symlink's *real* target directory
        // instead of the alias directory the checked document actually
        // lives beside must fail to find `dependency.fsl` at all (it was
        // only ever written under `alias/`), proving this test's resolution
        // is exercising the alias side and not vacuously passing.
        let wrong_base_resolver = RecordingResolver::new(&real);
        let wrong_base_result = load_kernel_model_from_source_with_resolver(
            &materialized,
            SYMLINK_ALIAS_COMPOSE_SOURCE,
            &wrong_base_resolver,
        );
        let wrong_base_error = wrong_base_result.expect_err(
            "resolving from the real (non-alias) directory should fail to find \
             dependency.fsl, which only exists under alias/",
        );
        assert!(
            format!("{wrong_base_error:?}").contains("dependency"),
            "expected the failure to name dependency.fsl, not an unrelated error: {wrong_base_error:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn cache_keys_include_the_compiled_implementation_fingerprint() {
        let path = repository_path("examples/gallery/valid/tiny_turnstile.fsl");
        let options = CliVerifyOptions::default();

        let current = verify_cache_keys_with_fingerprints(
            &path,
            &path,
            &options,
            "bmc",
            "Z3 4.16.0.0",
            "implementation-a",
            &[],
        )
        .expect("current implementation cache keys");
        let updated = verify_cache_keys_with_fingerprints(
            &path,
            &path,
            &options,
            "bmc",
            "Z3 4.16.0.0",
            "implementation-b",
            &[],
        )
        .expect("updated implementation cache keys");

        assert_ne!(current, updated);
    }

    /// Preservation control (issue #1023, not a detector): the per-process
    /// literate materialization (`literate_access.rs`, `.{stem}.literate-
    /// {pid}.fsl`) sitting beside the checked `.md` document is never read
    /// through `RecordingResolver` (it is the checked document's own root
    /// source, read directly by content, never a `from`/`use` target), so
    /// its run-local PID-bearing name cannot enter the key's dependency set
    /// by construction. This already passes before #1023 (the pre-existing
    /// `is_literate_materialization` exclusion, now removed as dead code,
    /// covered the same case for the directory walk), and must keep passing
    /// after: a literate document verified twice must hit the cache the
    /// second time.
    #[test]
    fn literate_materialization_never_enters_the_cache_key_dependency_set() {
        const SOURCE: &str = "spec Example { state { x: Int } init { x = 0 } }\n";
        let root = std::env::temp_dir().join(format!(
            "fslc-literate-materialization-cache-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create fixture directory");
        let document = root.join("doc.md");
        std::fs::write(&document, format!("```fsl\n{SOURCE}```\n")).expect("write document");
        let materialized = root.join(".doc.literate-1.fsl");
        std::fs::write(&materialized, SOURCE).expect("write materialization");

        let options = CliVerifyOptions::default();
        let resolver = RecordingResolver::new(&root);
        let sources = recorded_sources(&materialized, SOURCE, &resolver);
        assert!(
            sources.is_empty(),
            "a root document with no from/use dependency must record none: {sources:?}"
        );
        let first = verify_cache_keys(&materialized, &document, &options, &sources)
            .expect("first cache key");
        let second = verify_cache_keys(&materialized, &document, &options, &sources)
            .expect("second cache key");
        assert_eq!(
            first, second,
            "re-verifying the same literate document must reach the same key \
             (a run-local PID in the materialization's name must not leak in): {first:?} vs {second:?}"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}

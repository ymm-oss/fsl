// SPDX-License-Identifier: Apache-2.0

//! `fslc refine`, and the refinement checks `check` and `verify` fold into
//! their envelopes: governance preservations and requirements `implements`.

use std::path::Path;

use fsl_core::{FslValue, KernelModel, TraceStep, TypeRef};
use fslc_rust::outcome::Verdict;
use serde_json::{Map, Value, json};

use crate::cli_options::ScopeBounds;
use crate::cli_output::{
    display_binding, envelope, error_output, located_error_output, spec_load_error_output,
    trace_last_action,
};
use crate::process::block_on_native;
use crate::spec_loading::load_model;

/// Derive governance output from one caller-owned root-source snapshot.
pub(crate) fn governance_result_from_source(
    path: &Path,
    source: &str,
    depth: usize,
) -> Result<Option<Value>, fslc_rust::verification_output::GovernanceOutputError> {
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    fslc_rust::verification_output::governance_output(
        source,
        &resolver,
        crate::process::parse_options(),
        |preservation| {
            let (result, status) = run_refine(
                &base.join(&preservation.after_path),
                &base.join(&preservation.before_path),
                &base.join(&preservation.refinement_path),
                depth,
            );
            preservation_refine_result(&result, status, preservation.span)
        },
    )
}

/// A preservation's `result` from its `run_refine` envelope and exit, or the
/// governance diagnostic for a refinement that did not reach a verdict:
/// status 2 is a document error (located `type`, exit 2), status 3 an
/// internal one (located `internal`, exit 3, issue #1303).
pub(crate) fn preservation_refine_result(
    result: &Value,
    status: i32,
    span: fsl_syntax::Span,
) -> Result<Value, fslc_rust::verification_output::GovernanceOutputError> {
    if status > 1 {
        let message = result
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("governance preservation refinement failed");
        let (line, column) = (span.start.line, span.start.column);
        return Err(if status == 2 {
            fslc_rust::verification_output::GovernanceOutputError::new(message, line, column)
        } else {
            fslc_rust::verification_output::GovernanceOutputError::internal(message, line, column)
        });
    }
    Ok(result
        .get("result")
        .cloned()
        .unwrap_or_else(|| json!("error")))
}

/// Evaluate a governance document's preservations and fold them into a
/// `check`/`verify` envelope (issue #1303).
///
/// Returns the exit code the fold implies, `None` to leave the caller's own
/// exit alone (no governance document, every preservation `refines`, or an
/// envelope that already fails), or the governance diagnostic envelope with
/// its exit (2 for any contract error `check` reports, 3 for an internal
/// refinement failure). An `error` envelope is
/// returned untouched without running any preservation: the run already
/// failed for a reason of its own. Preservations always run at depth 8, the
/// depth `docs/manual/LANGUAGE.md` §13 publishes, whatever `--depth` the
/// command was given for the document's own kernel.
pub(crate) fn fold_governance_from_source(
    path: &Path,
    source: &str,
    output: &mut Value,
) -> Result<Option<i32>, (Value, i32)> {
    if output.get("result").and_then(Value::as_str) == Some("error") {
        return Ok(None);
    }
    match governance_result_from_source(path, source, 8) {
        Ok(Some(governance)) => Ok(fslc_rust::verification_output::attach_governance(
            output, governance,
        )),
        Ok(None) => Ok(None),
        Err(error) => Err((
            fslc_rust::verification_output::render_governance_error(envelope(), &error),
            error.exit_status(),
        )),
    }
}

pub(crate) fn implements_result_from_source_with_bounds(
    path: &Path,
    source: &str,
    model: &KernelModel,
    depth: usize,
    scope: &ScopeBounds,
) -> Result<Option<Value>, fslc_rust::verification_output::RequirementsImplementsError> {
    let resolver = fsl_core::FsResolver::new(path.parent().unwrap_or_else(|| Path::new(".")));
    fslc_rust::verification_output::requirements_implements_output_with_bounds(
        source,
        &resolver,
        model,
        depth,
        &scope.instances,
        &scope.values,
        crate::process::parse_options(),
    )
}

pub(crate) fn implements_error_output(
    error: &fslc_rust::verification_output::RequirementsImplementsError,
) -> Value {
    fslc_rust::verification_output::render_requirements_implements_error(envelope(), error)
}

/// `{"kind", "name"}` of the abstract step's own violation behind an
/// `abs_step_violation` (issue #1302), with the name spelled the way the
/// abstraction's own `verify` would report it (scoped to the corresponded
/// abstract action, so a compose component keeps its public name).
pub(crate) fn abs_violation_json(
    abstraction: &KernelModel,
    action_map: &std::collections::BTreeMap<String, String>,
    failure: &fsl_runtime::RefinementFailure,
    violation: &fsl_runtime::Violation,
) -> Value {
    let abs_action = failure
        .impl_action
        .as_ref()
        .and_then(|action| action_map.get(&action.name))
        .map(String::as_str);
    json!({
        "kind": violation.kind,
        "name": abstraction.action_scoped_display_name(&violation.name, abs_action),
    })
}

pub(crate) fn mismatch_paths(
    model: &KernelModel,
    expected: Option<&std::collections::BTreeMap<String, FslValue>>,
    actual: Option<&std::collections::BTreeMap<String, FslValue>>,
) -> Vec<String> {
    fn collect(
        model: &KernelModel,
        ty: &TypeRef,
        expected: Option<&FslValue>,
        actual: Option<&FslValue>,
        path: &str,
        paths: &mut Vec<String>,
    ) {
        if expected == actual {
            return;
        }
        if let (
            TypeRef::Map(_, value_ty),
            Some(FslValue::Map(expected)),
            Some(FslValue::Map(actual)),
        ) = (ty, expected, actual)
        {
            let mut keys = expected.keys().chain(actual.keys()).collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            for key in keys {
                let key_text = display_binding(key);
                let expected_value = expected.get(key);
                let actual_value = actual.get(key);
                if expected_value == actual_value {
                    continue;
                }
                if let (
                    TypeRef::Named(type_name),
                    Some(FslValue::Struct {
                        fields: expected_fields,
                        ..
                    }),
                    Some(FslValue::Struct {
                        fields: actual_fields,
                        ..
                    }),
                ) = (value_ty.as_ref(), expected_value, actual_value)
                    && let Some(fields) = model.struct_fields(type_name)
                {
                    for (field, _) in fields {
                        if expected_fields.get(field) != actual_fields.get(field) {
                            paths.push(format!("{path}[{key_text}].{field}"));
                        }
                    }
                    continue;
                }
                paths.push(format!("{path}[{key_text}]"));
            }
            return;
        }
        paths.push(path.to_owned());
    }

    let mut paths = Vec::new();
    for (name, ty) in &model.state {
        collect(
            model,
            ty,
            expected.and_then(|state| state.get(name)),
            actual.and_then(|state| state.get(name)),
            &model.names().public_state(name).into_string(),
            &mut paths,
        );
    }
    paths
}

/// Render `RefinementCheck::impl_violation`: the implementation itself
/// violates a type bound, invariant, `trans`, or `ensures` within `depth`,
/// discovered before any refinement correspondence was even attempted. Not
/// `refines` and not `refinement_failed` (#466) — this is a property of the
/// refinement *input*, so it is reported with the evidence shape `fslc verify`
/// uses for a violation plus an explanatory `note`, matching the frozen
/// Python reference's `refine()` (`src/fslc/refine.py`, accepted as [1.2.9]
/// in CHANGELOG.md). The `result` is `impl_violated`, the one word inline
/// `implements` and governance preservations use for the same outcome
/// (#1315); the Python reference still spells it `violated`.
pub(crate) fn impl_self_violation_output(
    implementation: &KernelModel,
    violation: &fsl_runtime::Violation,
    trace: &[TraceStep],
    depth: usize,
) -> Map<String, Value> {
    let mut output = envelope();
    output.insert("impl".to_owned(), json!(implementation.name));
    output.insert("result".to_owned(), json!(Verdict::ImplViolated.result()));
    output.insert("violation_kind".to_owned(), json!(violation.kind));
    let name = implementation.action_scoped_display_name(&violation.name, trace_last_action(trace));
    if violation.kind == "trans" {
        output.insert("trans".to_owned(), json!(name));
    }
    output.insert("invariant".to_owned(), json!(name));
    output.insert("violated_at_step".to_owned(), json!(violation.step));
    output.insert("checked_to_depth".to_owned(), json!(depth));
    output.insert(
        "impl_trace".to_owned(),
        fslc_rust::trace_json(implementation, trace),
    );
    output.insert(
        "note".to_owned(),
        json!(
            "this result is a property of the impl spec itself (a refinement input), not a refinement failure; verify the impl spec independently before checking refinement"
        ),
    );
    output.insert("trace_type".to_owned(), json!("refinement"));
    output
}

/// `fslc refine`'s verdict when the correspondence walk hit
/// `fsl_runtime::IMPLEMENTS_SEARCH_BUDGET` before deciding within the depth:
/// the same `unknown_budget` / `states_explored` vocabulary (and exit 1) the
/// inline `implements` seam reports for the same cutoff. The progress stage
/// is not run: a safety correspondence that was never decided cannot be the
/// premise of a progress check.
pub(crate) fn refine_budget_output(
    checked: &fsl_runtime::RefinementCheck,
    states_explored: usize,
) -> Map<String, Value> {
    let mut output = envelope();
    output.insert("impl".to_owned(), json!(checked.implementation()));
    output.insert("abs".to_owned(), json!(checked.abstraction()));
    output.insert("result".to_owned(), json!("unknown_budget"));
    output.insert("states_explored".to_owned(), json!(states_explored));
    output.insert(
        "hint".to_owned(),
        json!("refinement correspondence search reached its fixed state budget before deciding within the depth; lower --depth or narrow the verify {} domains (instances/values) of both specs"),
    );
    output
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run_refine(
    implementation_path: &Path,
    abstraction_path: &Path,
    mapping_path: &Path,
    depth: usize,
) -> (Value, i32) {
    let implementation = match load_model(implementation_path) {
        Ok(model) => model,
        Err(error) => return (spec_load_error_output(&error), 2),
    };
    let abstraction = match load_model(abstraction_path) {
        Ok(model) => model,
        Err(error) => return (spec_load_error_output(&error), 2),
    };
    let source = match std::fs::read_to_string(mapping_path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let mapping = match fsl_core::parse_refinement_with_options(
        &source,
        &implementation,
        &abstraction,
        crate::process::parse_options(),
    ) {
        Ok(mapping) => mapping,
        Err(error) => {
            return (
                error.span.map_or_else(
                    || error_output("type", &error.message),
                    |span| located_error_output("type", &error.message, span),
                ),
                2,
            );
        }
    };
    let checked =
        match fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, depth) {
            Ok(checked) => checked,
            Err(error) => return (error_output("type", &error.to_string()), 2),
        };
    let failure = match checked.verdict() {
        fsl_runtime::RefinementVerdict::ImplViolated { violation, trace } => {
            return (
                Value::Object(impl_self_violation_output(
                    &implementation,
                    violation,
                    trace,
                    checked.depth(),
                )),
                Verdict::ImplViolated.exit_code(),
            );
        }
        fsl_runtime::RefinementVerdict::BudgetExhausted { states_explored } => {
            return (
                Value::Object(refine_budget_output(&checked, states_explored)),
                1,
            );
        }
        fsl_runtime::RefinementVerdict::Failed(failure) => Some(failure),
        fsl_runtime::RefinementVerdict::Refines => None,
    };
    let progress = if failure.is_none() && !mapping.progress.is_empty() {
        let mut solver = match fsl_solver_z3::Z3Solver::new() {
            Ok(solver) => solver,
            Err(error) => return (error_output("internal", &error.to_string()), 3),
        };
        match block_on_native(fsl_verifier::check_refinement_progress(
            &implementation,
            &abstraction,
            &mapping,
            &mut solver,
            depth,
        )) {
            Ok(progress) => Some(progress),
            Err(error) => return (error_output("semantics", &error.to_string()), 2),
        }
    } else {
        None
    };
    let mut output = envelope();
    output.insert("impl".to_owned(), json!(checked.implementation()));
    output.insert("abs".to_owned(), json!(checked.abstraction()));
    if let Some(failure) = failure {
        output.insert("result".to_owned(), json!("refinement_failed"));
        output.insert("kind".to_owned(), json!(failure.kind));
        if let Some(at) = &failure.at {
            output.insert("at".to_owned(), json!(at));
        }
        output.insert("violated_at_step".to_owned(), json!(failure.step));
        if let Some(action) = &failure.impl_action {
            let definition = implementation
                .actions
                .iter()
                .find(|definition| definition.name == action.name);
            output.insert(
                "impl_action".to_owned(),
                json!({
                    "name": implementation.action_display_name(&action.name),
                    "params": action.params.iter().map(|(name, value)| (
                        name.clone(), fslc_rust::fsl_value_json(value)
                    )).collect::<Map<_, _>>(),
                    "loc": definition.map(|definition| definition.span.python_loc()),
                }),
            );
        }
        if let Some(state) = &failure.alpha_before {
            output.insert("abs_before".to_owned(), fslc_rust::state_json(state));
        }
        if let Some(state) = &failure.alpha_after_expected {
            output.insert(
                "abs_after_expected".to_owned(),
                fslc_rust::state_json(state),
            );
        }
        if let Some(state) = &failure.alpha_after_actual {
            output.insert("abs_after_actual".to_owned(), fslc_rust::state_json(state));
        }
        // `abs_step_violation` (issue #1302): the abstract step's own
        // violation and the successor it rejected. The latter is not an
        // expected state, so it is a separate key, never `abs_after_expected`.
        if let Some(violation) = &failure.abs_violation {
            output.insert(
                "abs_violation".to_owned(),
                abs_violation_json(&abstraction, checked.action_map(), failure, violation),
            );
        }
        if let Some(state) = &failure.alpha_after_attempted {
            output.insert(
                "abs_after_attempted".to_owned(),
                fslc_rust::state_json(state),
            );
        }
        output.insert(
            "mismatch".to_owned(),
            Value::Array(
                mismatch_paths(
                    &abstraction,
                    failure.alpha_after_expected.as_ref(),
                    failure.alpha_after_actual.as_ref(),
                )
                .into_iter()
                .map(Value::String)
                .collect(),
            ),
        );
        output.insert(
            "impl_trace".to_owned(),
            fslc_rust::trace_json(&implementation, &failure.impl_trace),
        );
        output.insert(
            "hint".to_owned(),
            json!("the impl step does not correspond to the mapped abs action; fix the map expressions, the action correspondence, or guard the impl action"),
        );
        output.insert("trace_type".to_owned(), json!("refinement"));
        return (Value::Object(output), 1);
    }
    if let Some(violation) = progress
        .as_ref()
        .and_then(|progress| progress.violation.as_ref())
    {
        let Some(details) = &violation.leads_to else {
            return (
                error_output("internal", "missing refinement progress diagnostics"),
                3,
            );
        };
        if let Err(error) = fsl_runtime::replay_trace(implementation.clone(), &violation.trace) {
            return (error_output("internal", &error.to_string()), 3);
        }
        let declaration = mapping
            .progress
            .iter()
            .find(|declaration| declaration.leads_to == violation.name);
        let property = abstraction
            .leadstos
            .iter()
            .find(|property| property.name == violation.name);
        output.insert("result".to_owned(), json!("refinement_failed"));
        output.insert("kind".to_owned(), json!("progress_lost"));
        output.insert(
            "progress_failure".to_owned(),
            json!(if details.stutter {
                "deadlock_or_stall_blocks_progress"
            } else {
                "lasso_blocks_progress"
            }),
        );
        output.insert("violation_kind".to_owned(), json!("leadsTo"));
        output.insert(
            "invariant".to_owned(),
            json!(
                abstraction
                    .names()
                    .public_violation(&violation.name, None)
                    .into_string()
            ),
        );
        output.insert(
            "bindings".to_owned(),
            Value::Object(
                details
                    .bindings
                    .iter()
                    .map(|(name, value)| (name.clone(), fslc_rust::fsl_value_json(value)))
                    .collect(),
            ),
        );
        output.insert("pending_since".to_owned(), json!(details.pending_since));
        output.insert("stutter".to_owned(), json!(details.stutter));
        output.insert(
            "impl_trace".to_owned(),
            fslc_rust::trace_json(&implementation, &violation.trace),
        );
        output.insert(
            "progress".to_owned(),
            json!({
                "leadsTo": abstraction.names().public_violation(&violation.name, None).into_string(),
                "actions": declaration
                    .map(|item| item.actions.iter().map(|action| action.0.clone()).collect::<Vec<_>>())
                    .unwrap_or_default(),
            }),
        );
        output.insert(
            "hint".to_owned(),
            json!("the impl refines the abstract safety contract, but admits an execution where the pulled-back abstract leadsTo remains pending. Fairness must come from lower-layer `fair action` declarations for the implementation actions named by preserve progress; action mappings do not create fairness or prove implementation conformance by themselves"),
        );
        if let Some(property) = property {
            output.insert("loc".to_owned(), property.span.python_loc());
        }
        if let Some(loop_start) = details.loop_start {
            output.insert("loop_start".to_owned(), json!(loop_start));
        }
        output.insert(
            "faithfulness_class".to_owned(),
            json!("liveness_not_refined"),
        );
        output.insert(
            "recommended_action".to_owned(),
            json!(
                "re-prove liveness at each layer or add preserve progress to the refinement mapping"
            ),
        );
        output.insert("trace_type".to_owned(), json!("refinement"));
        return (Value::Object(output), 1);
    }
    output.insert("result".to_owned(), json!("refines"));
    output.insert("checked_to_depth".to_owned(), json!(checked.depth()));
    output.insert(
        "action_map".to_owned(),
        Value::Object(
            fslc_rust::verification_output::sorted_by_published(checked.action_map(), |name| {
                implementation.names().public_action(name)
            })
            .into_iter()
            .map(|(name, _, target)| {
                (
                    name.into_string(),
                    json!(abstraction.action_display_name(target)),
                )
            })
            .collect(),
        ),
    );
    if checked.abs_has_ensures() {
        output.insert(
            "note".to_owned(),
            json!("abs ensures are checked only on the abstract steps the impl's transitions correspond to within checked_to_depth; verify/prove the abstract spec separately"),
        );
    }
    if let Some(progress) = progress
        && !progress.checked.is_empty()
    {
        output.insert(
            "progress".to_owned(),
            Value::Object(
                fslc_rust::verification_output::sorted_by_published(&progress.checked, |name| {
                    implementation.names().public_property(name)
                })
                .into_iter()
                .map(|(name, _, actions)| {
                    (
                        name.into_string(),
                        json!({
                            "checked_to_depth": depth,
                            "actions": actions
                                .iter()
                                .map(|action| implementation.action_display_name(action))
                                .collect::<Vec<_>>(),
                        }),
                    )
                })
                .collect(),
            ),
        );
    }
    (Value::Object(output), 0)
}

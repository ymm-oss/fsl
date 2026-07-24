// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use fsl_core::{FileResolver, FslValue, KernelExpr, KernelModel, TypeRef};
use serde_json::{Map, Value, json};

use super::{
    block_on_native, display, envelope, error_output, json_mismatches, load_snapshot_value_object,
    mapping_json_expr, parse_params, read_jsonl_records, required_option_value,
};

#[allow(clippy::too_many_lines)]
pub(super) fn causal_command(
    mut args: impl Iterator<Item = String>,
) -> Result<(Value, i32), String> {
    let subcommand = args.next().ok_or_else(|| {
        "usage: fslc causal <check|analyze|verify-expectations|observe-expectations|diff|ledger> ..."
            .to_owned()
    })?;
    match subcommand.as_str() {
        "check" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "fslc causal check requires a file".to_owned())?,
            );
            if let Some(option) = args.next() {
                return Err(format!("unknown causal check option '{option}'"));
            }
            Ok(run_causal_check(&path))
        }
        "analyze" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "fslc causal analyze requires a file".to_owned())?,
            );
            let mut projection = None;
            let mut profile = None;
            let mut format = "json".to_owned();
            let mut evidence = Vec::new();
            let mut lifecycle = Vec::new();
            let mut as_of = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--projection" => {
                        projection = Some(required_option_value(&mut args, "--projection")?);
                    }
                    "--profile" => {
                        profile = Some(required_option_value(&mut args, "--profile")?);
                    }
                    "--format" => format = required_option_value(&mut args, "--format")?,
                    "--evidence" => evidence.push(PathBuf::from(required_option_value(
                        &mut args,
                        "--evidence",
                    )?)),
                    "--lifecycle" => lifecycle.push(PathBuf::from(required_option_value(
                        &mut args,
                        "--lifecycle",
                    )?)),
                    "--as-of" => as_of = Some(required_option_value(&mut args, "--as-of")?),
                    _ => return Err(format!("unknown causal analyze option '{option}'")),
                }
            }
            Ok(run_causal_analyze(
                &path,
                projection.as_deref(),
                profile.as_deref(),
                &format,
                &evidence,
                &lifecycle,
                as_of.as_deref(),
            ))
        }
        "verify-expectations" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "fslc causal verify-expectations requires a file".to_owned())?,
            );
            let mut depth = 8_usize;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--depth" => {
                        depth = required_option_value(&mut args, "--depth")?
                            .parse()
                            .map_err(|_| "--depth requires a positive integer".to_owned())?;
                    }
                    _ => {
                        return Err(format!(
                            "unknown causal verify-expectations option '{option}'"
                        ));
                    }
                }
            }
            Ok(run_causal_verify_expectations(&path, depth))
        }
        "observe-expectations" => {
            let path =
                PathBuf::from(args.next().ok_or_else(|| {
                    "fslc causal observe-expectations requires a file".to_owned()
                })?);
            let mut from_log = None;
            let mut mapping = None;
            let mut scope_path = None;
            let mut period_start = None;
            let mut period_end = None;
            let mut out = None;
            let mut lifecycle_out = None;
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--from-log" => {
                        from_log = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--from-log",
                        )?));
                    }
                    "--mapping" => {
                        mapping = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--mapping",
                        )?));
                    }
                    "--scope" => {
                        scope_path =
                            Some(PathBuf::from(required_option_value(&mut args, "--scope")?));
                    }
                    "--period-start" => {
                        period_start = Some(required_option_value(&mut args, "--period-start")?);
                    }
                    "--period-end" => {
                        period_end = Some(required_option_value(&mut args, "--period-end")?);
                    }
                    "--out" => {
                        out = Some(PathBuf::from(required_option_value(&mut args, "--out")?));
                    }
                    "--lifecycle-out" => {
                        lifecycle_out = Some(PathBuf::from(required_option_value(
                            &mut args,
                            "--lifecycle-out",
                        )?));
                    }
                    _ => {
                        return Err(format!(
                            "unknown causal observe-expectations option '{option}'"
                        ));
                    }
                }
            }
            let Some(from_log) = from_log else {
                return Ok((error_output("usage", "--from-log is required"), 2));
            };
            let Some(mapping) = mapping else {
                return Ok((error_output("usage", "--mapping is required"), 2));
            };
            let Some(scope_path) = scope_path else {
                return Ok((
                    error_output(
                        "usage",
                        "--scope is required (observation scope is never inferred from log content)",
                    ),
                    2,
                ));
            };
            let Some(period_start) = period_start else {
                return Ok((
                    error_output(
                        "usage",
                        "--period-start is required (observation period is never inferred from log content)",
                    ),
                    2,
                ));
            };
            let Some(period_end) = period_end else {
                return Ok((
                    error_output(
                        "usage",
                        "--period-end is required (observation period is never inferred from log content)",
                    ),
                    2,
                ));
            };
            Ok(run_causal_observe_expectations(
                &path,
                &from_log,
                &mapping,
                &scope_path,
                &period_start,
                &period_end,
                out.as_deref(),
                lifecycle_out.as_deref(),
            ))
        }
        "diff" => {
            let before = PathBuf::from(
                args.next()
                    .ok_or_else(|| "fslc causal diff requires two files".to_owned())?,
            );
            let after = PathBuf::from(
                args.next()
                    .ok_or_else(|| "fslc causal diff requires two files".to_owned())?,
            );
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--format" => {
                        if required_option_value(&mut args, "--format")? != "json" {
                            return Err("causal diff supports only --format json".to_owned());
                        }
                    }
                    _ => return Err(format!("unknown causal diff option '{option}'")),
                }
            }
            Ok(run_causal_diff(&before, &after))
        }
        "ledger" => {
            let path = PathBuf::from(
                args.next()
                    .ok_or_else(|| "fslc causal ledger requires a file".to_owned())?,
            );
            let mut plans = Vec::new();
            let mut evidence = Vec::new();
            let mut lifecycle = Vec::new();
            let mut as_of = None;
            let mut format = "json".to_owned();
            while let Some(option) = args.next() {
                match option.as_str() {
                    "--plans" => {
                        plans.push(PathBuf::from(required_option_value(&mut args, "--plans")?));
                    }
                    "--evidence" => evidence.push(PathBuf::from(required_option_value(
                        &mut args,
                        "--evidence",
                    )?)),
                    "--lifecycle" => lifecycle.push(PathBuf::from(required_option_value(
                        &mut args,
                        "--lifecycle",
                    )?)),
                    "--as-of" => as_of = Some(required_option_value(&mut args, "--as-of")?),
                    "--format" => format = required_option_value(&mut args, "--format")?,
                    _ => return Err(format!("unknown causal ledger option '{option}'")),
                }
            }
            if format != "json" {
                return Err("causal ledger supports only --format json".to_owned());
            }
            Ok(run_causal_ledger(
                &path,
                &plans,
                &evidence,
                &lifecycle,
                as_of.as_deref(),
            ))
        }
        other => Err(format!(
            "unknown causal subcommand '{other}' (expected check | analyze | verify-expectations | observe-expectations | diff | ledger; there is deliberately no 'causal verify')"
        )),
    }
}

fn causal_error_output(error: &fsl_tools::CausalError) -> (Value, i32) {
    let kind = if error.kind == "parse" {
        "parse"
    } else {
        "semantics"
    };
    let mut output = error_output(kind, &error.message);
    if let Some(object) = output.as_object_mut() {
        object.insert("diagnostic".to_owned(), json!(error.kind));
        object.insert(
            "loc".to_owned(),
            json!({"line": error.line, "column": error.column}),
        );
    }
    (output, 2)
}

fn load_causal_model(
    path: &Path,
) -> Result<(fsl_tools::CausalModel, Vec<fsl_tools::CausalWarning>), (Value, i32)> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        (
            error_output("io", &format!("cannot read {}: {error}", path.display())),
            2,
        )
    })?;
    let base = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let resolver = fsl_core::FsResolver::new(base);
    fsl_tools::build_causal_model(&source, &resolver).map_err(|error| causal_error_output(&error))
}

fn merge_causal_envelope(value: Value) -> (Value, i32) {
    let Value::Object(body) = value else {
        return (error_output("internal", "invalid causal result"), 3);
    };
    let mut output = envelope();
    output.extend(body);
    (Value::Object(output), 0)
}

pub(super) fn run_causal_check(path: &Path) -> (Value, i32) {
    match load_causal_model(path) {
        Ok((model, warnings)) => {
            merge_causal_envelope(fsl_tools::causal_check_json(&model, &warnings))
        }
        Err(error) => error,
    }
}

type LoadedEvidence = (
    std::collections::BTreeMap<String, fsl_tools::EvidenceArtifact>,
    fsl_tools::SupportOverlay,
);

fn load_causal_evidence(
    model: &fsl_tools::CausalModel,
    evidence_paths: &[PathBuf],
    lifecycle_paths: &[PathBuf],
    as_of: Option<&str>,
) -> Result<LoadedEvidence, (Value, i32)> {
    let read_json = |path: &PathBuf| -> Result<Value, (Value, i32)> {
        let source = std::fs::read_to_string(path).map_err(|error| {
            (
                error_output("io", &format!("cannot read {}: {error}", path.display())),
                2,
            )
        })?;
        serde_json::from_str(&source).map_err(|error| {
            (
                error_output(
                    "parse",
                    &format!("invalid JSON in {}: {error}", path.display()),
                ),
                2,
            )
        })
    };
    let evidence_error = |error: &fsl_tools::EvidenceError| {
        let mut output = error_output("semantics", &error.message);
        if let Some(object) = output.as_object_mut() {
            object.insert("diagnostic".to_owned(), json!(error.kind));
        }
        (output, 2)
    };
    let mut artifacts = std::collections::BTreeMap::new();
    for path in evidence_paths {
        let value = read_json(path)?;
        let artifact = fsl_tools::parse_artifact(&value).map_err(|error| evidence_error(&error))?;
        if artifacts
            .insert(artifact.evidence_id.clone(), artifact)
            .is_some()
        {
            return Err((
                error_output(
                    "semantics",
                    "duplicate evidence_id across --evidence inputs",
                ),
                2,
            ));
        }
    }
    for path in lifecycle_paths {
        let value = read_json(path)?;
        let (evidence_id, status) = fsl_tools::validate_lifecycle_chain(&value, &artifacts)
            .map_err(|error| evidence_error(&error))?;
        if let Some(artifact) = artifacts.get_mut(&evidence_id) {
            artifact.lifecycle_status = status;
        }
    }
    let overlay = fsl_tools::aggregate_support(model, &artifacts, as_of);
    Ok((artifacts, overlay))
}

#[allow(clippy::too_many_lines)]
fn run_causal_analyze(
    path: &Path,
    projection: Option<&str>,
    profile: Option<&str>,
    format: &str,
    evidence_paths: &[PathBuf],
    lifecycle_paths: &[PathBuf],
    as_of: Option<&str>,
) -> (Value, i32) {
    if !matches!(format, "json" | "dot" | "mermaid") {
        return (
            error_output(
                "usage",
                &format!("unsupported causal analyze format: {format}"),
            ),
            2,
        );
    }
    let (model, _) = match load_causal_model(path) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };
    let evidence = if evidence_paths.is_empty() {
        None
    } else {
        match load_causal_evidence(&model, evidence_paths, lifecycle_paths, as_of) {
            Ok(loaded) => Some(loaded),
            Err(error) => return error,
        }
    };
    let analysis = match (projection, profile) {
        (Some(projection), None) => match projection {
            "causal_graph" => fsl_tools::causal_graph_projection(&model),
            "causal_timeline" => fsl_tools::causal_timeline_projection(&model),
            "causal_traceability_graph" => fsl_tools::causal_traceability_projection(&model),
            "causal_evidence_graph" => {
                let Some((artifacts, overlay)) = &evidence else {
                    return (
                        error_output(
                            "usage",
                            "--projection causal_evidence_graph requires at least one --evidence artifact",
                        ),
                        2,
                    );
                };
                fsl_tools::causal_evidence_graph(&model, artifacts, overlay)
            }
            other => {
                return (
                    error_output("usage", &format!("unknown causal projection '{other}'")),
                    2,
                );
            }
        },
        (None, Some("causal-review")) => {
            if format != "json" {
                return (
                    error_output(
                        "usage",
                        "--profile causal-review supports only --format json",
                    ),
                    2,
                );
            }
            let mut review = fsl_tools::causal_review_json(&model);
            if let Some((_, overlay)) = &evidence
                && let Some(object) = review.as_object_mut()
            {
                if let Some(Value::Array(findings)) = object.get_mut("findings") {
                    findings.extend(overlay.findings.iter().cloned());
                }
                if let Some(Value::Array(entries)) = object.get_mut("not_evaluable") {
                    entries.extend(overlay.not_evaluable.iter().cloned());
                }
                object.insert("causal_support".to_owned(), json!(overlay.support));
            }
            review
        }
        (None, Some(other)) => {
            return (
                error_output("usage", &format!("unknown causal profile '{other}'")),
                2,
            );
        }
        (Some(_), Some(_)) | (None, None) => {
            return (
                error_output(
                    "usage",
                    "fslc causal analyze requires exactly one of --projection or --profile",
                ),
                2,
            );
        }
    };
    if format == "json" {
        return merge_causal_envelope(analysis);
    }
    let content = if format == "mermaid" {
        fsl_tools::causal_mermaid(&analysis)
    } else {
        fsl_tools::causal_dot(&analysis)
    };
    let mut output = envelope();
    output.insert("result".to_owned(), json!("causal_analyzed"));
    output.insert("formal_result".to_owned(), json!("not_run"));
    output.insert(
        "projection".to_owned(),
        analysis.get("projection").cloned().unwrap_or(Value::Null),
    );
    output.insert("format".to_owned(), json!(format));
    output.insert("content".to_owned(), json!(content));
    (Value::Object(output), 0)
}

#[allow(clippy::too_many_lines)]
fn run_causal_verify_expectations(path: &Path, depth: usize) -> (Value, i32) {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            return (
                error_output("io", &format!("cannot read {}: {error}", path.display())),
                2,
            );
        }
    };
    let surface = match fsl_syntax::parse_causal(&source) {
        Ok(surface) => surface,
        Err(error) => {
            let mut output = error_output("parse", &error.to_string());
            if let Some(object) = output.as_object_mut() {
                object.insert("loc".to_owned(), error.span.python_loc());
            }
            return (output, 2);
        }
    };
    let (model, _) = match load_causal_model(path) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };
    let base = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let resolver = fsl_core::FsResolver::new(base);
    let compiled = match fsl_tools::compile_expectations(&surface, &model, &resolver) {
        Ok(compiled) => compiled,
        Err(error) => return causal_error_output(&error),
    };
    let mut expectations = Vec::new();
    for expectation in &compiled {
        let mut solver = match fsl_solver_z3::Z3Solver::new() {
            Ok(solver) => solver,
            Err(error) => return (error_output("internal", &error.to_string()), 3),
        };
        let result = match block_on_native(fsl_verifier::verify_bounded(
            &expectation.model,
            &mut solver,
            depth,
        )) {
            Ok(result) => result,
            Err(error) => {
                return (
                    error_output(
                        "semantics",
                        &format!("expectation '{}': {error}", expectation.id),
                    ),
                    2,
                );
            }
        };
        let violated_here = result
            .leadsto_violation
            .as_ref()
            .is_some_and(|violation| violation.name == expectation.property);
        let base_violation = result.violation.is_some()
            || result
                .leadsto_violation
                .as_ref()
                .is_some_and(|violation| violation.name != expectation.property);
        if base_violation {
            return (
                error_output(
                    "semantics",
                    &format!(
                        "expectation '{}': the target spec itself is not clean at depth {depth}; fix the spec before checking expectations",
                        expectation.id
                    ),
                ),
                2,
            );
        }
        expectations.push(json!({
            "id": format!("expectation:{}", expectation.id),
            "verdict": if violated_here { "violated" } else { "pass" },
            "assurance": "bounded",
            "checked_to_depth": depth,
            "within_ticks": expectation.within_ticks,
            "clock": expectation.clock,
            "trigger_kind": expectation.trigger_kind,
            "derived_from_claim": expectation
                .derived_from_claim
                .as_ref()
                .map(|claim| format!("claim:{claim}")),
            "do_not_assume": [
                "The causal claim is proved",
                "No unmodeled common cause exists",
                "Expectation violation refutes the causal claim"
            ],
        }));
    }
    let claims: Vec<Value> = model
        .claims
        .values()
        .map(|claim| {
            json!({
                "id": format!("claim:{}", claim.id),
                "formal_assurance": fsl_tools::FORMAL_ASSURANCE_NOT_RUN,
                "causal_support": fsl_tools::CAUSAL_SUPPORT_UNTESTED,
            })
        })
        .collect();
    let mut output = envelope();
    output.insert("result".to_owned(), json!("causal_expectations_checked"));
    output.insert("schema_version".to_owned(), json!("causal-expectations.v0"));
    output.insert(
        "formal_result".to_owned(),
        json!(fsl_tools::FORMAL_ASSURANCE_NOT_RUN),
    );
    output.insert("model".to_owned(), json!(model.name));
    output.insert("claims".to_owned(), json!(claims));
    output.insert("expectations".to_owned(), json!(expectations));
    output.insert(
        "do_not_assume".to_owned(),
        json!([
            "The causal claims are true",
            "A passing expectation proves the causal claim",
            "Expectation violation refutes the causal claim"
        ]),
    );
    (Value::Object(output), 0)
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn run_causal_observe_expectations(
    path: &Path,
    log_path: &Path,
    mapping_path: &Path,
    scope_path: &Path,
    period_start: &str,
    period_end: &str,
    out_path: Option<&Path>,
    lifecycle_out_path: Option<&Path>,
) -> (Value, i32) {
    // ── Parse causal source and compile expectations ──────────────────
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            return (
                error_output("io", &format!("cannot read {}: {error}", path.display())),
                2,
            );
        }
    };
    let surface = match fsl_syntax::parse_causal(&source) {
        Ok(surface) => surface,
        Err(error) => {
            let mut output = error_output("parse", &error.to_string());
            if let Some(object) = output.as_object_mut() {
                object.insert("loc".to_owned(), error.span.python_loc());
            }
            return (output, 2);
        }
    };
    let (model, _) = match load_causal_model(path) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };
    let base = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let resolver = fsl_core::FsResolver::new(base);
    let compiled = match fsl_tools::compile_expectations(&surface, &model, &resolver) {
        Ok(compiled) => compiled,
        Err(error) => return causal_error_output(&error),
    };
    if compiled.is_empty() {
        return (
            error_output("semantics", "causal model has no expectations to observe"),
            2,
        );
    }

    // ── Parse mapping and log ────────────────────────────────────────
    let mapping_source = match std::fs::read_to_string(mapping_path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let mapping = match fsl_syntax::parse_surface_document(&mapping_source) {
        Ok(fsl_syntax::SurfaceDocument::Refinement(mapping)) => mapping,
        Ok(_) => return (error_output("type", "expected refinement mapping file"), 2),
        Err(error) => return (error_output("parse", &error.to_string()), 2),
    };
    if let Some(span) = crate::untyped_replay_enum_mapping_span(&mapping) {
        return (
            crate::located_error_output(
                "type",
                "enum conversion or abstraction requires a typed impl model and is not supported by causal --from-log mappings",
                span,
            ),
            2,
        );
    }
    let records = match read_jsonl_records(log_path) {
        Ok(records) => records,
        Err(error) => return (error_output("io", &error), 2),
    };

    // ── Parse scope ──────────────────────────────────────────────────
    let scope_raw = match std::fs::read_to_string(scope_path) {
        Ok(source) => source,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let scope: Value = match serde_json::from_str(&scope_raw) {
        Ok(scope) => scope,
        Err(error) => {
            return (
                error_output("io", &format!("invalid scope JSON: {error}")),
                2,
            );
        }
    };

    // ── Provenance digests ───────────────────────────────────────────
    let model_digest = fsl_tools::canonical_json(&json!(source));
    let model_digest = sha256_digest(&model_digest);
    let log_raw = match std::fs::read_to_string(log_path) {
        Ok(raw) => raw,
        Err(error) => return (error_output("io", &error.to_string()), 2),
    };
    let log_digest = sha256_digest(&log_raw);
    let mapping_digest = sha256_digest(&mapping_source);

    // ── Build mapping lookup tables ──────────────────────────────────
    let maps_auto = mapping
        .items
        .iter()
        .any(|item| matches!(item, fsl_syntax::RefinementItem::MapsAuto(_)));
    let state_maps: std::collections::BTreeMap<&str, (Option<&fsl_syntax::Binder>, &KernelExpr)> =
        mapping
            .items
            .iter()
            .filter_map(|item| match item {
                fsl_syntax::RefinementItem::Map {
                    name, binder, expr, ..
                } => Some((name.as_str(), (binder.as_ref(), expr.as_ref()))),
                _ => None,
            })
            .collect();
    let action_maps: std::collections::BTreeMap<
        &str,
        (&[fsl_syntax::RefinementParam], &fsl_syntax::ActionTarget),
    > = mapping
        .items
        .iter()
        .filter_map(|item| match item {
            fsl_syntax::RefinementItem::Action {
                name,
                params,
                target,
                ..
            } => Some((name.as_str(), (params.as_slice(), target))),
            _ => None,
        })
        .collect();

    // ── Load the original kernel spec for conformance ────────────────
    // Each compiled expectation has its own augmented model, but we need
    // the original spec model to run conformance and mapping evaluation.
    let first_import = surface.uses.first().map(|import| &import.path);
    let spec_model = if let Some(import_path) = first_import {
        let spec_source = match resolver.read(import_path) {
            Ok(source) => source,
            Err(error) => {
                return (
                    error_output(
                        "io",
                        &format!("cannot read '{}': {}", import_path, error.message),
                    ),
                    2,
                );
            }
        };
        let kernel = match fsl_core::parse_kernel_source(&spec_source, &resolver) {
            Ok(kernel) => kernel,
            Err(error) => return (error_output("semantics", &error.to_string()), 2),
        };
        match fsl_core::build_model(kernel) {
            Ok(model) => model,
            Err(error) => return (error_output("semantics", &error.to_string()), 2),
        }
    } else {
        return (
            error_output("semantics", "causal model has no uses imports"),
            2,
        );
    };

    // ── Replay: conformance + bounded liveness per expectation ───────
    let mut monitor = match fsl_runtime::Monitor::new(spec_model.clone()) {
        Ok(monitor) => monitor,
        Err(error) => {
            return (
                error_output("internal", &format!("monitor init: {error}")),
                3,
            );
        }
    };

    // Build a BoundedLivenessMonitor per compiled expectation.
    let mut liveness_monitors: Vec<fsl_runtime::BoundedLivenessMonitor> = Vec::new();
    for expectation in &compiled {
        match fsl_runtime::BoundedLivenessMonitor::new(expectation.model.clone()) {
            Ok(liveness) => liveness_monitors.push(liveness),
            Err(error) => {
                return (
                    error_output(
                        "internal",
                        &format!("liveness monitor for '{}': {error}", expectation.id),
                    ),
                    3,
                );
            }
        }
    }

    // Observe initial state (step 0).
    let init_state = monitor.state.clone();
    for (index, (expectation, liveness)) in compiled
        .iter()
        .zip(liveness_monitors.iter_mut())
        .enumerate()
    {
        let extended = extend_with_ghost(&init_state, expectation, "", &spec_model);
        if let Err(error) = liveness.observe(&extended, 0) {
            return (
                error_output(
                    "internal",
                    &format!(
                        "liveness observe init for '{}': {error}",
                        compiled[index].id
                    ),
                ),
                3,
            );
        }
    }

    // Track per-expectation verdicts and violation step.
    let mut verdicts: Vec<Option<usize>> = vec![None; compiled.len()];
    let mut events_observed = 0_usize;
    let events_unmapped = 0_usize;

    for (record_index, (_line_number, record)) in records.iter().enumerate() {
        let step = record_index + 1;
        let mapped = (|| -> Result<(String, String, Map<String, Value>, Value), String> {
            let record = record
                .as_object()
                .ok_or_else(|| "log record must be an object".to_owned())?;
            let source_action = record
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| "record.action must be a string".to_owned())?;
            let params = record
                .get("params")
                .and_then(Value::as_object)
                .ok_or_else(|| "record.params must be an object".to_owned())?;
            let raw = record
                .get("state")
                .and_then(Value::as_object)
                .ok_or_else(|| "record.state must be an object".to_owned())?;
            let mapping_result = action_maps.get(source_action).map_or_else(
                || {
                    if maps_auto {
                        let action = spec_model
                            .actions
                            .iter()
                            .find(|action| display(&action.name) == source_action)
                            .ok_or_else(|| {
                                format!("no action mapping for log action '{source_action}'")
                            })?;
                        Ok((
                            None,
                            fsl_syntax::ActionTarget::Action(
                                action.name.clone(),
                                action
                                    .params
                                    .iter()
                                    .map(|param| KernelExpr::Var(param.name().to_owned()))
                                    .collect(),
                            ),
                        ))
                    } else {
                        Err(format!(
                            "no action mapping for log action '{source_action}'"
                        ))
                    }
                },
                |(source_params, target)| Ok((Some(*source_params), (*target).clone())),
            )?;
            let (source_params, target) = mapping_result;
            if let Some(source_params) = source_params {
                let expected = source_params
                    .iter()
                    .map(|param| param.name.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let observed = params
                    .keys()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>();
                if expected != observed {
                    return Err(format!(
                        "parameter mismatch for log action '{source_action}'"
                    ));
                }
            }
            let (target_action, expressions) = match target {
                fsl_syntax::ActionTarget::Stutter => ("stutter".to_owned(), Vec::new()),
                fsl_syntax::ActionTarget::Action(name, expressions) => (name, expressions),
            };
            let mut mapped_params = Map::new();
            if target_action != "stutter" {
                let action = spec_model
                    .actions
                    .iter()
                    .find(|action| action.name == target_action)
                    .ok_or_else(|| format!("unknown mapped action '{target_action}'"))?;
                if action.params.len() != expressions.len() {
                    return Err(format!(
                        "parameter mismatch for mapped action '{target_action}'"
                    ));
                }
                for (param, expression) in action.params.iter().zip(&expressions) {
                    mapped_params.insert(
                        param.name().to_owned(),
                        mapping_json_expr(expression, raw, params, &spec_model)?,
                    );
                }
            }
            let mut observed = Map::new();
            for (name, ty) in &spec_model.state {
                let display_name = display(name);
                let value = if let Some((binder, expression)) = state_maps.get::<str>(name) {
                    if binder.is_some() {
                        let TypeRef::Map(key_ty, _) = ty else {
                            return Err(format!(
                                "indexed map on non-Map variable '{display_name}'"
                            ));
                        };
                        let binder_name = match binder.expect("checked") {
                            fsl_syntax::Binder::Typed { name, .. }
                            | fsl_syntax::Binder::Range { name, .. }
                            | fsl_syntax::Binder::Collection { name, .. } => name,
                        };
                        let mut values = Map::new();
                        for key in spec_model
                            .map_key_values(key_ty)
                            .map_err(|error| error.to_string())?
                        {
                            let key_json = fslc_rust::fsl_value_json(&key);
                            let key_name = key_json
                                .as_str()
                                .map_or_else(|| key_json.to_string(), str::to_owned);
                            let mut bindings = Map::new();
                            bindings.insert(binder_name.clone(), key_json);
                            values.insert(
                                key_name,
                                mapping_json_expr(expression, raw, &bindings, &spec_model)?,
                            );
                        }
                        Value::Object(values)
                    } else {
                        mapping_json_expr(expression, raw, &Map::new(), &spec_model)?
                    }
                } else if maps_auto {
                    raw.get(&display_name)
                        .or_else(|| raw.get(name))
                        .cloned()
                        .ok_or_else(|| format!("mapped state is missing '{display_name}'"))?
                } else {
                    return Err(format!(
                        "no map for abstract state variable '{display_name}'"
                    ));
                };
                observed.insert(display_name, value);
            }
            Ok((
                source_action.to_owned(),
                target_action,
                mapped_params,
                Value::Object(observed),
            ))
        })();
        let (source_action, target_action, mapped_params, observed) = match mapped {
            Ok(mapped) => mapped,
            Err(error) => {
                return (
                    json!({
                        "fsl": "1.0",
                        "result": "error",
                        "kind": "observation_replay_failed",
                        "message": format!("log record {record_index}: {error}"),
                        "failed_at_record": record_index,
                        "do_not_assume": DO_NOT_ASSUME_CAUSAL_OBSERVATION,
                    }),
                    2,
                );
            }
        };

        // Step the conformance monitor.
        if target_action != "stutter" {
            let action = spec_model
                .actions
                .iter()
                .find(|action| action.name == target_action)
                .expect("validated mapped action");
            let parsed = match parse_params(&spec_model, action, &mapped_params) {
                Ok(parsed) => parsed,
                Err(error) => {
                    return (
                        json!({
                            "fsl": "1.0",
                            "result": "error",
                            "kind": "observation_replay_failed",
                            "message": format!("log record {record_index}: param mapping: {error}"),
                            "failed_at_record": record_index,
                            "do_not_assume": DO_NOT_ASSUME_CAUSAL_OBSERVATION,
                        }),
                        2,
                    );
                }
            };
            let enabled = match monitor.enabled() {
                Ok(enabled) => enabled,
                Err(error) => return (error_output("internal", &error.to_string()), 3),
            };
            let Some(instance) = enabled
                .iter()
                .find(|instance| instance.action == target_action && instance.params == parsed)
            else {
                return (
                    json!({
                        "fsl": "1.0",
                        "result": "error",
                        "kind": "observation_replay_nonconformant",
                        "message": format!(
                            "log record {record_index}: action '{source_action}' (mapped to '{}') is not enabled; evidence cannot be generated from a nonconformant log",
                            display(&target_action)
                        ),
                        "failed_at_record": record_index,
                        "do_not_assume": DO_NOT_ASSUME_CAUSAL_OBSERVATION,
                    }),
                    2,
                );
            };
            if let Err(error) = monitor.step(instance) {
                return (error_output("internal", &error.to_string()), 3);
            }
        }

        // Convert observed state to FslValue for liveness monitoring.
        let observed_fsl = match load_snapshot_value_object(
            observed.as_object().expect("mapped state is an object"),
            &spec_model,
        ) {
            Ok(state) => state,
            Err(error) => {
                return (
                    json!({
                        "fsl": "1.0",
                        "result": "error",
                        "kind": "observation_replay_failed",
                        "message": format!("log record {record_index}: state conversion: {error}"),
                        "failed_at_record": record_index,
                        "do_not_assume": DO_NOT_ASSUME_CAUSAL_OBSERVATION,
                    }),
                    2,
                );
            }
        };

        // Compare observed state against the monitor's computed state.
        let expected = fslc_rust::state_json(&monitor.state);
        let parsed_observed = fslc_rust::state_json(&observed_fsl);
        let mismatches = json_mismatches(&expected, &parsed_observed, "");
        if !mismatches.is_empty() {
            return (
                json!({
                    "fsl": "1.0",
                    "result": "error",
                    "kind": "observation_replay_nonconformant",
                    "message": format!(
                        "log record {record_index}: state mismatch between observed and spec-computed state; evidence cannot be generated from a nonconformant log"
                    ),
                    "failed_at_record": record_index,
                    "expected_state": expected,
                    "observed_state": parsed_observed,
                    "mismatches": mismatches,
                    "do_not_assume": DO_NOT_ASSUME_CAUSAL_OBSERVATION,
                }),
                2,
            );
        }

        events_observed += 1;

        // Feed extended state (with ghost) to each expectation's liveness monitor.
        for (index, (expectation, liveness)) in compiled
            .iter()
            .zip(liveness_monitors.iter_mut())
            .enumerate()
        {
            if verdicts[index].is_some() {
                continue;
            }
            let extended =
                extend_with_ghost(&observed_fsl, expectation, &target_action, &spec_model);
            match liveness.observe(&extended, step) {
                Ok(Some(_violation)) => {
                    verdicts[index] = Some(step);
                }
                Ok(None) => {}
                Err(error) => {
                    return (
                        error_output(
                            "internal",
                            &format!(
                                "liveness observe for '{}' at step {step}: {error}",
                                expectation.id
                            ),
                        ),
                        3,
                    );
                }
            }
        }
    }

    // ── Build per-expectation results and evidence artifacts ─────────
    let mut expectation_results = Vec::new();
    let mut artifacts = Vec::new();
    let mut lifecycle_records = Vec::new();

    for (index, expectation) in compiled.iter().enumerate() {
        let verdict = if verdicts[index].is_some() {
            "violated"
        } else {
            "pass"
        };

        let expectation_id = format!("expectation:{}", expectation.id);
        let expectation_digest = sha256_digest(&fsl_tools::canonical_json(&json!({
            "id": expectation.id,
            "property": expectation.property,
            "within_ticks": expectation.within_ticks,
            "trigger_kind": expectation.trigger_kind,
        })));

        expectation_results.push(json!({
            "id": expectation_id,
            "verdict": verdict,
            "assurance": "replay-observed",
            "within_ticks": expectation.within_ticks,
            "clock": expectation.clock,
            "trigger_kind": expectation.trigger_kind,
            "derived_from_claim": expectation.derived_from_claim.as_ref().map(|id| format!("claim:{id}")),
            "event_counts": {
                "observed": events_observed,
                "unmapped": events_unmapped,
                "missing_required": 0
            },
            "do_not_assume": DO_NOT_ASSUME_CAUSAL_OBSERVATION,
        }));

        // Generate evidence artifact only for expectations linked to claims.
        if let Some(claim_id) = &expectation.derived_from_claim {
            let claim = model.claims.get(claim_id);
            let claim_version = claim.map_or(1, |claim| claim.version);

            let evidence_id = format!("OBS_{}_{}", expectation.id, model.name);

            let mut artifact = json!({
                "schema_version": fsl_tools::EVIDENCE_SCHEMA_VERSION,
                "evidence_id": evidence_id,
                "claims": [{
                    "id": format!("claim:{claim_id}"),
                    "version": claim_version
                }],
                "design": "observational",
                "source_study_id": null,
                "derived_from": [],
                "support": "inconclusive",
                "scope": scope,
                "period": {
                    "start": period_start,
                    "end": period_end,
                    "valid_until": period_end
                },
                "observation": {
                    "kind": "expectation_replay",
                    "expectation_id": expectation_id,
                    "expectation_digest": expectation_digest,
                    "verdict": verdict,
                    "assurance": "replay-observed",
                    "event_counts": {
                        "observed": events_observed,
                        "unmapped": events_unmapped,
                        "missing_required": 0
                    },
                    "digests": {
                        "model": model_digest,
                        "log": log_digest,
                        "mapping": mapping_digest,
                        "study_protocol": null
                    }
                },
                "formal_result": fsl_tools::FORMAL_ASSURANCE_NOT_RUN,
                "artifact_digest": ""
            });
            let digest = fsl_tools::artifact_digest(&artifact);
            artifact
                .as_object_mut()
                .expect("artifact is object")
                .insert("artifact_digest".to_owned(), json!(digest));

            // Build lifecycle record.
            let mut lifecycle_record = json!({
                "sequence": 1,
                "status": "active",
                "superseded_by": null,
                "recorded_at": format!("{}T00:00:00Z", period_end),
                "previous_record_digest": null,
                "record_digest": ""
            });
            let record_digest =
                fsl_tools::lifecycle_record_digest(&evidence_id, &digest, &lifecycle_record);
            lifecycle_record
                .as_object_mut()
                .expect("record is object")
                .insert("record_digest".to_owned(), json!(record_digest));

            let lifecycle_chain = json!({
                "schema_version": fsl_tools::LIFECYCLE_SCHEMA_VERSION,
                "evidence_id": evidence_id,
                "artifact_digest": digest,
                "records": [lifecycle_record]
            });

            artifacts.push(artifact);
            lifecycle_records.push(lifecycle_chain);
        }
    }

    // ── Write output files ───────────────────────────────────────────
    // One file per artifact so each is independently consumable by
    // `fslc causal analyze --evidence`. Single artifact uses --out as-is;
    // multiple artifacts suffix the expectation id.
    let mut evidence_paths = Vec::new();
    let mut lifecycle_paths = Vec::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        let evidence_id = artifact
            .get("evidence_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if let Some(base) = out_path {
            let file_path = if artifacts.len() == 1 {
                base.to_path_buf()
            } else {
                let stem = base
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("evidence");
                let extension = base
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("json");
                base.with_file_name(format!("{stem}.{evidence_id}.{extension}"))
            };
            let out_json = serde_json::to_string_pretty(artifact).expect("valid JSON");
            if let Err(error) = std::fs::write(&file_path, format!("{out_json}\n")) {
                return (
                    error_output(
                        "io",
                        &format!("cannot write {}: {error}", file_path.display()),
                    ),
                    2,
                );
            }
            evidence_paths.push(file_path.display().to_string());
        }
        if let Some(base) = lifecycle_out_path
            && let Some(lifecycle) = lifecycle_records.get(index)
        {
            let file_path = if lifecycle_records.len() == 1 {
                base.to_path_buf()
            } else {
                let stem = base
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("lifecycle");
                let extension = base
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("json");
                base.with_file_name(format!("{stem}.{evidence_id}.{extension}"))
            };
            let lifecycle_json = serde_json::to_string_pretty(lifecycle).expect("valid JSON");
            if let Err(error) = std::fs::write(&file_path, format!("{lifecycle_json}\n")) {
                return (
                    error_output(
                        "io",
                        &format!("cannot write {}: {error}", file_path.display()),
                    ),
                    2,
                );
            }
            lifecycle_paths.push(file_path.display().to_string());
        }
    }

    // ── Build CLI envelope ───────────────────────────────────────────
    let claims: Vec<Value> = model
        .claims
        .values()
        .map(|claim| {
            json!({
                "id": format!("claim:{}", claim.id),
                "formal_assurance": fsl_tools::FORMAL_ASSURANCE_NOT_RUN,
                "causal_support": fsl_tools::CAUSAL_SUPPORT_UNTESTED,
            })
        })
        .collect();
    let mut output = envelope();
    output.insert("result".to_owned(), json!("causal_expectations_observed"));
    output.insert("schema_version".to_owned(), json!("causal-observation.v0"));
    output.insert(
        "formal_result".to_owned(),
        json!(fsl_tools::FORMAL_ASSURANCE_NOT_RUN),
    );
    output.insert("model".to_owned(), json!(model.name));
    output.insert("claims".to_owned(), json!(claims));
    output.insert("expectations".to_owned(), json!(expectation_results));
    output.insert("events_observed".to_owned(), json!(events_observed));
    output.insert("artifacts_generated".to_owned(), json!(artifacts.len()));
    if !artifacts.is_empty() {
        output.insert(
            "artifact_digests".to_owned(),
            json!(
                artifacts
                    .iter()
                    .filter_map(|artifact| artifact.get("artifact_digest").cloned())
                    .collect::<Vec<_>>()
            ),
        );
    }
    output.insert(
        "do_not_assume".to_owned(),
        json!(DO_NOT_ASSUME_CAUSAL_OBSERVATION),
    );
    (Value::Object(output), 0)
}

const DO_NOT_ASSUME_CAUSAL_OBSERVATION: [&str; 5] = [
    "The causal claim is proved",
    "Temporal co-occurrence establishes causality",
    "No unmodeled common cause exists",
    "Expectation violation refutes the causal claim",
    "Unobserved behavior did not occur",
];

fn extend_with_ghost(
    base_state: &std::collections::BTreeMap<String, FslValue>,
    expectation: &fsl_tools::CompiledExpectation,
    target_action: &str,
    _spec_model: &KernelModel,
) -> std::collections::BTreeMap<String, FslValue> {
    let mut extended = base_state.clone();
    if expectation.trigger_kind == "action" {
        let ghost_name = format!("_expectation_fired_{}", expectation.id);
        let fires = expectation
            .trigger_action
            .as_ref()
            .is_some_and(|trigger| trigger == target_action);
        extended.insert(ghost_name, FslValue::Bool(fires));
    }
    extended
}

fn sha256_digest(text: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn run_causal_ledger(
    path: &Path,
    plan_paths: &[PathBuf],
    evidence_paths: &[PathBuf],
    lifecycle_paths: &[PathBuf],
    as_of: Option<&str>,
) -> (Value, i32) {
    let (model, _) = match load_causal_model(path) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };

    // Load plan artifacts.
    let mut plans = std::collections::BTreeMap::new();
    for plan_path in plan_paths {
        let source = match std::fs::read_to_string(plan_path) {
            Ok(source) => source,
            Err(error) => return (error_output("io", &error.to_string()), 2),
        };
        let value: Value = match serde_json::from_str(&source) {
            Ok(value) => value,
            Err(error) => {
                return (
                    error_output(
                        "io",
                        &format!("invalid JSON in {}: {error}", plan_path.display()),
                    ),
                    2,
                );
            }
        };
        let plan = match fsl_tools::parse_plan(&value) {
            Ok(plan) => plan,
            Err(error) => return (error_output(error.kind, &error.message), 2),
        };
        plans.insert(plan.plan_id.clone(), plan);
    }

    // Load evidence + lifecycle (reuse existing pattern).
    let (artifacts, overlay) =
        match load_causal_evidence(&model, evidence_paths, lifecycle_paths, as_of) {
            Ok(loaded) => loaded,
            Err(error) => return error,
        };

    // Apply lifecycle status to plans from the same lifecycle files.
    for lifecycle_path in lifecycle_paths {
        let Ok(source) = std::fs::read_to_string(lifecycle_path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&source) else {
            continue;
        };
        if let Some(plan_id) = value.get("evidence_id").and_then(Value::as_str)
            && let Some(plan) = plans.get_mut(plan_id)
        {
            // Cross-check lifecycle chain's artifact_digest against the plan's.
            if let Some(chain_digest) = value.get("artifact_digest").and_then(Value::as_str)
                && chain_digest != plan.declared_digest
            {
                return (
                    error_output(
                        "causal_plan_digest_mismatch",
                        &format!(
                            "lifecycle chain for plan '{plan_id}' declares artifact_digest {chain_digest} but plan has {}",
                            plan.declared_digest
                        ),
                    ),
                    2,
                );
            }
            let empty_artifacts = std::collections::BTreeMap::new();
            let Ok((_, status)) = fsl_tools::validate_lifecycle_chain(&value, &empty_artifacts)
            else {
                continue;
            };
            plan.lifecycle_status = status;
        }
    }

    // Build ledger projection.
    let ledger_body = fsl_tools::build_ledger(&model, &plans, &artifacts, &overlay, as_of);
    let Value::Object(body) = ledger_body else {
        return (error_output("internal", "invalid ledger result"), 3);
    };
    let mut output = envelope();
    output.extend(body);
    (Value::Object(output), 0)
}

fn run_causal_diff(before: &Path, after: &Path) -> (Value, i32) {
    let (before_model, _) = match load_causal_model(before) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };
    let (after_model, _) = match load_causal_model(after) {
        Ok(loaded) => loaded,
        Err(error) => return error,
    };
    merge_causal_envelope(fsl_tools::causal_diff_json(&before_model, &after_model))
}

// SPDX-License-Identifier: Apache-2.0

//! Reading a spec into a checked model, the state snapshots that seed a run,
//! and the checked-document validations that run on the same source.

use std::path::Path;

use fsl_core::{FslValue, KernelModel, KernelSpec, TypeDef, TypeRef};
use fslc_rust::migration::SourceEdition;
use fslc_rust::outcome::outcome_class;
use fslc_rust::spec_load::{SemanticDiagnostic, SpecLoadError, kernel_load_error};
use fslc_rust::verification_output::{CheckedTraceContract, DiagnosticClass, TraceGate};
use serde_json::{Map, Value};

use crate::cli_options::ScopeBounds;
use crate::cli_output::{envelope, spec_load_error_output};

/// Admit each direct input slot once and pass captured bytes and diagnostics
/// inward. The callback owns envelope finalization, including nested results.
pub(crate) fn run_admitted_roots(
    roots: Vec<(&Path, Result<String, SpecLoadError>)>,
    requested: Option<fsl_syntax::Edition>,
    stderr_on_success: bool,
    run: impl FnOnce(&[String], &[SourceEdition]) -> (Value, i32),
) -> (Value, i32) {
    let mut sources = Vec::new();
    let mut editions = Vec::new();
    for (path, source) in roots {
        let source = match source {
            Ok(source) => source,
            Err(error) => return (spec_load_error_output(&error), 2),
        };
        let edition = match fslc_rust::migration::admit_source(
            &source,
            &path.to_string_lossy(),
            requested,
            crate::process::parse_options(),
            &envelope,
            true,
        ) {
            Ok(edition) => edition,
            Err(error) => return error,
        };
        sources.push(source);
        editions.push(edition);
    }
    let result = run(&sources, &editions);
    if stderr_on_success && raw_delivery_allowed(&result) {
        for edition in editions {
            for warning in edition.warnings() {
                eprintln!("warning: {warning}");
            }
        }
    }
    result
}

pub(crate) fn finish_admitted(
    result: (Value, i32),
    editions: &[SourceEdition],
    stderr_on_success: bool,
) -> (Value, i32) {
    if stderr_on_success && raw_delivery_allowed(&result) {
        result
    } else {
        let result = editions[0].finish(result);
        editions[1..].iter().fold(result, |result, edition| {
            edition.warnings_only().finish(result)
        })
    }
}

pub(crate) fn run_admitted_root(
    path: &Path,
    source: Result<String, SpecLoadError>,
    requested: Option<fsl_syntax::Edition>,
    stderr_on_success: bool,
    run: impl FnOnce(&str) -> (Value, i32),
) -> (Value, i32) {
    run_admitted_roots(
        vec![(path, source)],
        requested,
        stderr_on_success,
        |sources, editions| finish_admitted(run(&sources[0]), editions, stderr_on_success),
    )
}

/// Whether a producer's output may be delivered as raw bytes instead of its
/// JSON envelope (issue #537 C2, Verdict Conservation Law).
///
/// Raw delivery *replaces* the envelope that would otherwise carry a failure's
/// `kind`/`message`/`loc`, so every raw-output path must first confirm the
/// producer succeeded. This is an **additional** conjunct and never a
/// replacement for the structural field guard a site already has: `db import`
/// prints raw source only for `result:"imported"` and keeps the JSON envelope
/// for `imported_with_warnings`, which is also success-class.
///
/// Three of the nine raw-output sites previously had no guard at all
/// (`--readable`, `--ts`, `domain expand`). Measured rather than assumed: no
/// failure path can currently reach any of those extractions, because the
/// extracted field is inserted at exactly one place in each producer, after
/// every error return -- `"readable"` in `run_explain`, `"entities"` in
/// `fsl_tools::analyze_typestate`, `"kernel_source"` in `run_domain_expand`.
/// The guard therefore records an invariant that was until now unstated. It is
/// not idle: `fsl-tools/src/domain.rs` builds a `result:"violated"` envelope
/// that also carries `kernel_source`, so the invariant is one routing change
/// away from breaking.
pub(crate) fn raw_delivery_allowed(result: &(Value, i32)) -> bool {
    (result.1 == 0 && result.0.get("$schema").is_some_and(Value::is_string))
        || (outcome_class(&result.0).is_success()
            && match result.0["result"].as_str() {
                Some("explained") => result.0["readable"].is_string(),
                Some("typestate") => result.0["entities"].is_array(),
                Some("generated" | "analyzed") => result.0["content"].is_string(),
                _ => true,
            })
}

pub(crate) fn validate_specialized_document_from_source(
    path: &Path,
    source: &str,
) -> Result<(), SpecLoadError> {
    fslc_rust::check_stages::validate_specialized_document(
        source,
        &path.display().to_string(),
        crate::process::parse_options(),
    )
}

pub(crate) fn validate_requirement_trace_source(
    source: &str,
    model: &KernelModel,
) -> Result<TraceGate, String> {
    fslc_rust::verification_output::validate_requirement_trace_source(
        &envelope(),
        source,
        model,
        crate::process::parse_options(),
    )
}

/// [`fslc_rust::verification_output::require_trace_gate`] under the process
/// parse options: the checked contract, or the envelope to report (exit 2).
pub(crate) fn require_trace_gate(
    source: &str,
    model: &KernelModel,
) -> Result<Option<CheckedTraceContract>, Value> {
    fslc_rust::verification_output::require_trace_gate(
        &envelope(),
        source,
        model,
        crate::process::parse_options(),
    )
}

/// A native Z3 solver for one question of the requirements checks
/// ([`fslc_rust::requirements_solver`]).
pub(crate) fn z3_solver() -> Result<fsl_solver_z3::Z3Solver, String> {
    fsl_solver_z3::Z3Solver::new().map_err(|error| error.to_string())
}

/// The questions of [`reject_refuted_requirements`] already asked in this
/// process: a source, the model it was asked of, and the answer.
type AskedRequirements = Vec<(
    String,
    KernelModel,
    Option<fslc_rust::requirements_solver::Rejection>,
)>;

/// Reject `source` when the solver refutes a requirements check the
/// finite-domain enumeration deferred (decisions/048 J3), with the diagnostic
/// `check` reports. Every native command that loads a spec passes through
/// here (or through `check`'s shared stages), so a spec `check` rejects is
/// rejected by all of them.
///
/// A command may load one spec more than once (`verify` lowers it again for
/// the compose warnings); the answer for a source and model already asked is
/// reused, so the solver runs once per question. Without the `fsl5` gate
/// nothing is asked or kept.
pub(crate) fn reject_refuted_requirements(
    source: &str,
    model: &KernelModel,
) -> Result<(), SpecLoadError> {
    static ASKED: std::sync::Mutex<AskedRequirements> = std::sync::Mutex::new(Vec::new());
    let options = crate::process::parse_options();
    if !options
        .unstable()
        .contains(fsl_syntax::UnstableFeature::Fsl5)
    {
        return Ok(());
    }
    let mut asked = ASKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let rejection =
        if let Some((_, _, rejection)) = asked
            .iter()
            .find(|(asked_source, asked_model, _)| asked_source == source && asked_model == model)
        {
            rejection.clone()
        } else {
            let rejection = crate::process::block_on_native(
                fslc_rust::requirements_solver::refute(source, model, options, &mut z3_solver),
            );
            asked.push((source.to_owned(), model.clone(), rejection.clone()));
            rejection
        };
    rejection.map_or(Ok(()), |rejection| Err(rejection.load_error()))
}

/// The declared model of `source` without [`reject_refuted_requirements`],
/// for a command that verifies a scoped model (`--instances`/`--values`) and
/// reads the declared one only beside it: the scoped load asks the solver of
/// the model the command verifies, and asking it of this one too would run
/// it a second time.
pub(crate) fn load_declared_model_beside_scope(
    path: &Path,
    source: &str,
) -> Result<KernelModel, SpecLoadError> {
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    fslc_rust::check_stages::load_kernel_model(
        source,
        &path.to_string_lossy(),
        &fsl_core::FsResolver::new(base),
        crate::process::parse_options(),
    )
    .map(|(_, model)| model)
}

pub(crate) fn load_model(path: &Path) -> Result<KernelModel, SpecLoadError> {
    load_kernel_model(path).map(|(_, _, model)| model)
}

pub(crate) fn load_model_from_source(
    path: &Path,
    source: &str,
) -> Result<KernelModel, SpecLoadError> {
    load_kernel_model_from_source(path, source).map(|(_, model)| model)
}

pub(crate) fn load_kernel_model(
    path: &Path,
) -> Result<(String, KernelSpec, KernelModel), SpecLoadError> {
    let source = read_spec_source(path)?;
    let (kernel, model) = load_kernel_model_from_source(path, &source)?;
    Ok((source, kernel, model))
}

/// Build a checked Kernel/model from a caller-owned root-source snapshot.
///
/// `load_kernel_model` remains the path-reading compatibility entry point;
/// callers that already captured their source — whether to avoid a second
/// root-path read or because they also need a specialized surface projection
/// — use this variant. The resolver can still read imported dependencies.
pub(crate) fn load_kernel_model_from_source(
    path: &Path,
    source: &str,
) -> Result<(KernelSpec, KernelModel), SpecLoadError> {
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    load_kernel_model_from_source_with_resolver(path, source, &resolver)
}

/// `load_kernel_model_from_source` with a caller-owned resolver, so a
/// cache-key computation can observe the dependency reads (issue #1023) by
/// passing a recording resolver instead of a plain `FsResolver`.
pub(crate) fn load_kernel_model_from_source_with_resolver(
    path: &Path,
    source: &str,
    resolver: &dyn fsl_core::FileResolver,
) -> Result<(KernelSpec, KernelModel), SpecLoadError> {
    let (kernel, model) = fslc_rust::check_stages::load_kernel_model(
        source,
        &path.to_string_lossy(),
        resolver,
        crate::process::parse_options(),
    )?;
    reject_refuted_requirements(source, &model)?;
    Ok((kernel, model))
}

/// Read a spec file, classifying a read failure as `io` rather than letting the
/// message-string classifier fall through to `semantics` (issue 497: a missing
/// single analyze input reported `semantics` while the same missing file in a
/// batch reported `io`).
pub(crate) fn read_spec_source(path: &Path) -> Result<String, SpecLoadError> {
    std::fs::read_to_string(path)
        .map_err(|error| SpecLoadError::Io(format!("{}: {error}", path.display())))
}

#[allow(clippy::too_many_lines)]
pub(crate) fn snapshot_value(
    model: &KernelModel,
    ty: &TypeRef,
    value: &Value,
    path: &str,
) -> Result<FslValue, String> {
    let type_error = || format!("{path} has a value incompatible with {ty:?}");
    match ty {
        TypeRef::Bool => value.as_bool().map(FslValue::Bool).ok_or_else(type_error),
        TypeRef::Int => value.as_i64().map(FslValue::Int).ok_or_else(type_error),
        TypeRef::Range(lo, hi) => {
            let number = value.as_i64().ok_or_else(type_error)?;
            if number < *lo || number > *hi {
                return Err(format!(
                    "{path} value {number} is out of range [{lo}..{hi}]"
                ));
            }
            Ok(FslValue::Int(number))
        }
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => {
                snapshot_value(model, &TypeRef::Range(*lo, *hi), value, path)
            }
            Some(TypeDef::Enum { members, .. }) => {
                let member = value.as_str().ok_or_else(type_error)?;
                if !members.iter().any(|candidate| candidate == member) {
                    return Err(format!(
                        "{path} enum member '{member}' is not one of {}",
                        members.join(", ")
                    ));
                }
                Ok(FslValue::Enum {
                    type_name: name.clone(),
                    member: member.to_owned(),
                })
            }
            Some(TypeDef::Struct { fields }) => {
                let object = value.as_object().ok_or_else(type_error)?;
                for key in object.keys() {
                    if !fields.iter().any(|(field, _)| field == key) {
                        return Err(format!("unknown struct field '{path}.{key}'"));
                    }
                }
                let parsed = fields
                    .iter()
                    .map(|(field, field_ty)| {
                        let field_value = object
                            .get(field)
                            .ok_or_else(|| format!("missing struct field '{path}.{field}'"))?;
                        Ok((
                            field.clone(),
                            snapshot_value(
                                model,
                                field_ty,
                                field_value,
                                &format!("{path}.{field}"),
                            )?,
                        ))
                    })
                    .collect::<Result<_, String>>()?;
                Ok(FslValue::Struct {
                    type_name: name.clone(),
                    fields: parsed,
                })
            }
            None => Err(format!("unknown type '{name}'")),
        },
        TypeRef::Option(inner) => {
            if value.is_null() {
                Ok(FslValue::None)
            } else if matches!(inner.as_ref(), TypeRef::Option(_)) {
                let object = value.as_object().ok_or_else(|| {
                    format!(
                        "{path} nested Option must be null or {{\"kind\":\"some\",\"value\":...}}"
                    )
                })?;
                if object.len() != 2
                    || object.get("kind") != Some(&Value::String("some".to_owned()))
                    || !object.contains_key("value")
                {
                    return Err(format!(
                        "{path} nested Option must be null or {{\"kind\":\"some\",\"value\":...}}"
                    ));
                }
                Ok(FslValue::Some(Box::new(snapshot_value(
                    model,
                    inner,
                    object.get("value").expect("checked value key"),
                    path,
                )?)))
            } else {
                Ok(FslValue::Some(Box::new(snapshot_value(
                    model, inner, value, path,
                )?)))
            }
        }
        TypeRef::Map(key_ty, value_ty) => {
            let object = value.as_object().ok_or_else(type_error)?;
            let keys = model
                .map_key_values(key_ty)
                .map_err(|error| error.to_string())?;
            let mut entries = std::collections::BTreeMap::new();
            for key in keys {
                let rendered = match &key {
                    FslValue::Int(value) => value.to_string(),
                    FslValue::Bool(value) => value.to_string(),
                    FslValue::Enum { member, .. } => member.clone(),
                    _ => return Err(format!("{path} has a non-scalar map key")),
                };
                let entry = object
                    .get(&rendered)
                    .ok_or_else(|| format!("missing map key '{path}[{rendered}]'"))?;
                entries.insert(
                    key,
                    snapshot_value(model, value_ty, entry, &format!("{path}[{rendered}]"))?,
                );
            }
            if object.len() != entries.len() {
                return Err(format!("{path} contains an unknown map key"));
            }
            Ok(FslValue::Map(entries))
        }
        TypeRef::Set(element_ty) => {
            let items = value.as_array().ok_or_else(type_error)?;
            Ok(FslValue::Set(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        snapshot_value(model, element_ty, item, &format!("{path}[{index}]"))
                    })
                    .collect::<Result<_, _>>()?,
            ))
        }
        TypeRef::Seq(element_ty, capacity) => {
            let items = value.as_array().ok_or_else(type_error)?;
            if items.len() > *capacity {
                return Err(format!(
                    "{path} sequence length {} exceeds capacity {capacity}",
                    items.len()
                ));
            }
            Ok(FslValue::Seq(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        snapshot_value(model, element_ty, item, &format!("{path}[{index}]"))
                    })
                    .collect::<Result<_, _>>()?,
            ))
        }
        TypeRef::Relation(source_ty, target_ty) => {
            let items = value.as_array().ok_or_else(type_error)?;
            Ok(FslValue::Relation(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let pair = item
                            .as_array()
                            .filter(|pair| pair.len() == 2)
                            .ok_or_else(|| format!("{path}[{index}] must be a pair"))?;
                        Ok((
                            snapshot_value(
                                model,
                                source_ty,
                                &pair[0],
                                &format!("{path}[{index}][0]"),
                            )?,
                            snapshot_value(
                                model,
                                target_ty,
                                &pair[1],
                                &format!("{path}[{index}][1]"),
                            )?,
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            ))
        }
    }
}

pub(crate) fn load_state_snapshot(
    path: &Path,
    model: &KernelModel,
) -> Result<std::collections::BTreeMap<String, FslValue>, (String, String)> {
    let source = std::fs::read_to_string(path).map_err(|error| {
        (
            "io".to_owned(),
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("file not found: {}", path.display())
            } else {
                error.to_string()
            },
        )
    })?;
    let value: Value = serde_json::from_str(&source).map_err(|error| {
        (
            "io".to_owned(),
            format!(
                "invalid state JSON at line {}, column {}: {}",
                error.line(),
                error.column(),
                error
            ),
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        (
            "type".to_owned(),
            "state snapshot must be a JSON object".to_owned(),
        )
    })?;
    load_snapshot_value_object(object, model).map_err(|error| ("type".to_owned(), error))
}

pub(crate) fn load_snapshot_value_object(
    object: &Map<String, Value>,
    model: &KernelModel,
) -> Result<std::collections::BTreeMap<String, FslValue>, String> {
    for key in object.keys() {
        if !model
            .state
            .iter()
            .any(|(name, _)| model.names().public_state(name).into_string() == *key)
        {
            return Err(format!("unknown state variable '{key}'"));
        }
    }
    model
        .state
        .iter()
        .map(|(name, ty)| {
            let display_name = model.names().public_state(name).into_string();
            let value = object
                .get(&display_name)
                .ok_or_else(|| format!("missing state variable '{display_name}'"))?;
            Ok((
                name.clone(),
                snapshot_value(model, ty, value, &format!("state.{display_name}"))?,
            ))
        })
        .collect()
}

pub(crate) fn load_model_scoped(
    path: &Path,
    scope: &ScopeBounds,
) -> Result<KernelModel, SpecLoadError> {
    let source = read_spec_source(path)?;
    load_model_scoped_from_source(path, &source, scope)
}

pub(crate) fn load_model_scoped_from_source(
    _path: &Path,
    source: &str,
    scope: &ScopeBounds,
) -> Result<KernelModel, SpecLoadError> {
    let kernel = match fsl_core::parse_kernel_source_with_bounds_with_options(
        source,
        &scope.instances,
        &scope.values,
        crate::process::parse_options(),
    ) {
        Ok(kernel) => kernel,
        // A rejected `--instances`/`--values` bound is a CLI argument
        // defect, not a construct in the spec, so it owns no location.
        Err(error) if error.kind == fsl_core::CoreErrorKind::ScopeOverrideWithoutBounds => {
            return Err(SpecLoadError::Semantic(Box::new(SemanticDiagnostic {
                class: DiagnosticClass::Core(error.kind),
                diagnostic_code: error.diagnostic_code,
                ..SemanticDiagnostic::unlocated(error.message)
            })));
        }
        Err(error) => {
            return Err(kernel_load_error(
                source,
                &error,
                crate::process::parse_options(),
            ));
        }
    };
    let model = fsl_core::build_model(kernel).map_err(|error| {
        SpecLoadError::Semantic(Box::new(SemanticDiagnostic::from_model_error(&error)))
    })?;
    // Asked of the scoped model: the overrides narrow the domains the
    // lowering enumerated, and so the ones the solver is asked over.
    reject_refuted_requirements(source, &model)?;
    Ok(model)
}

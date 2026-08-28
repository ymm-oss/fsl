// SPDX-License-Identifier: Apache-2.0

//! WASM entry point used exclusively inside the browser verification Worker.

use std::collections::BTreeMap;

use fsl_core::{CoreError, FileResolver, KernelModel, model_warnings};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}

/// Native unit tests exercise pre-solver error paths without a browser clock.
#[cfg(not(target_arch = "wasm32"))]
const fn performance_now() -> f64 {
    0.0
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
            name_resolution: false,
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

fn implements_error(
    solver_version: &str,
    failure: &fslc_rust::verification_output::RequirementsImplementsError,
) -> Value {
    fslc_rust::verification_output::render_requirements_implements_error(
        envelope(solver_version),
        failure,
    )
}

/// Render a solver or verifier failure, which names no construct in the source:
/// it carries no `loc` (issue 555) and resolves no name, so it keeps the
/// message-based classification (issue 565).
fn verifier_error(solver_version: &str, failure: &impl std::fmt::Display) -> Value {
    fslc_rust::verification_output::render_semantic_error(
        envelope(solver_version),
        &failure.to_string(),
        None,
        false,
    )
}

fn build(request: &Request, solver_version: &str) -> Result<(KernelModel, Vec<Value>), Value> {
    let resolver = MemoryResolver {
        files: request.files.clone(),
    };
    // Classified by the same `kernel_load_error` the native CLI runs and
    // rendered by the same dispatch. This stage used to be hard-coded to
    // `kind:"parse"` here while native reported `semantics`/`type` for the very
    // same input (issue #556); a second classifier on this side is what
    // produced that divergence, so there must not be one.
    let kernel =
        fsl_core::parse_kernel_source_with_file(&request.source, &resolver, &request.source_file)
            .map_err(|failure| {
            fslc_rust::spec_load::render_spec_load_error(
                envelope(solver_version),
                &fslc_rust::spec_load::kernel_load_error(&request.source, &failure),
            )
        })?;
    // Compose-lowering warnings (e.g. `fair_not_inherited`) are computed while
    // lowering, before `build_model` drops the per-component information that
    // produced them, so they must be captured here rather than derived from
    // the checked KernelModel below.
    let diagnostics = kernel.diagnostics().to_vec();
    let model = fsl_core::build_model(kernel).map_err(|failure| {
        // The same span and classification the native CLI reports, so the
        // Worker envelope does not diverge from `fslc` (issues 555, 565).
        let loc = fslc_rust::verification_output::model_error_loc(&failure);
        fslc_rust::verification_output::render_semantic_error(
            envelope(solver_version),
            &failure.to_string(),
            loc,
            failure.name_resolution,
        )
    })?;
    Ok((model, diagnostics))
}

async fn check(request: &Request, solver_version: &str) -> Value {
    if let Some((output, _)) = fslc_rust::frontend_output::ai_project_check_output(
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
    let (model, compose_warnings) = match build(request, solver_version) {
        Ok(built) => built,
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
    let warnings = compose_warnings
        .into_iter()
        .chain(model_warnings(&model))
        .collect::<Vec<_>>();
    output.insert("warnings".to_owned(), Value::Array(warnings));
    let mut output = add_frontend_metadata(
        request,
        solver_version,
        &model,
        has_trace_contract,
        8,
        Value::Object(output),
    );
    match governance_output(request).await {
        Ok(Some(governance)) => {
            output
                .as_object_mut()
                .expect("check envelope")
                .insert("governance".to_owned(), governance);
        }
        Ok(None) => {}
        Err(failure) => {
            return fslc_rust::verification_output::render_governance_error(
                envelope(solver_version),
                &failure,
            );
        }
    }
    output
}

async fn governance_output(
    request: &Request,
) -> Result<Option<Value>, fslc_rust::verification_output::GovernanceOutputError> {
    let resolver = MemoryResolver {
        files: request.files.clone(),
    };
    let resolver_ref = &resolver;
    fslc_rust::verification_output::governance_output_async(
        &request.source,
        resolver_ref,
        |preservation| {
            let preservation = preservation.clone();
            let resolver = resolver_ref;
            async move {
                let implementation_source = resolver
                    .read(&preservation.after_path)
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let abstraction_source = resolver
                    .read(&preservation.before_path)
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let mapping_source = resolver
                    .read(&preservation.refinement_path)
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let implementation = fsl_core::build_model(
                    fsl_core::parse_kernel_source_with_file(
                        &implementation_source,
                        resolver,
                        &preservation.after_path,
                    )
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?,
                )
                .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let abstraction = fsl_core::build_model(
                    fsl_core::parse_kernel_source_with_file(
                        &abstraction_source,
                        resolver,
                        &preservation.before_path,
                    )
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?,
                )
                .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                let mapping =
                    fsl_core::parse_refinement(&mapping_source, &implementation, &abstraction)
                        .map_err(|failure| governance_error(failure.message, preservation.span))?;
                let checked =
                    fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, 8)
                        .map_err(|failure| {
                            governance_error(failure.to_string(), preservation.span)
                        })?;
                if checked.failure.is_some() {
                    return Ok(json!("refinement_failed"));
                }
                if !mapping.progress.is_empty() {
                    let mut solver = fsl_solver_z3js::Z3JsSolver::new();
                    let progress = fsl_verifier::check_refinement_progress(
                        &implementation,
                        &abstraction,
                        &mapping,
                        &mut solver,
                        8,
                    )
                    .await
                    .map_err(|failure| governance_error(failure.to_string(), preservation.span))?;
                    if progress.violation.is_some() {
                        return Ok(json!("refinement_failed"));
                    }
                }
                Ok(json!(if checked.failure.is_some() {
                    "refinement_failed"
                } else {
                    "refines"
                }))
            }
        },
    )
    .await
}

fn governance_error(
    message: impl Into<String>,
    span: fsl_syntax::Span,
) -> fslc_rust::verification_output::GovernanceOutputError {
    fslc_rust::verification_output::GovernanceOutputError::new(
        message,
        span.start.line,
        span.start.column,
    )
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
        Err(failure) => return implements_error(solver_version, &failure),
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

#[allow(clippy::too_many_lines)]
async fn verify(request: &Request, solver_version: &str) -> Value {
    let started = performance_now();
    if let Err(failure) = fsl_syntax::parse_surface_document(&request.source) {
        // Same envelope `check` renders, and the one the native CLI now
        // renders for every spec-reading command: `kind:"parse"` with
        // `diagnostic_code` and `loc` (#484).
        return fslc_rust::frontend_output::render_surface_parse_error(
            envelope(solver_version),
            &failure,
        );
    }
    let (model, compose_warnings) = match build(request, solver_version) {
        Ok(built) => built,
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
    // Preserve exact concrete evidence for boundary outcomes the bounded
    // symbolic value cannot represent. `partial_op` is intentionally left to
    // the public symbolic verifier boundary itself (#651).
    if fsl_runtime::deterministic_initial_state(&model).is_ok() {
        match fsl_runtime::find_boundary_violation(
            &model,
            request.options.depth,
            fsl_runtime::CONCRETE_PROBE_BUDGET,
        ) {
            Ok(probe) => {
                if let Some((violation, trace)) = probe.finding
                    && violation.kind != "partial_op"
                {
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
                            // The Worker request surface has no `--vacuity`
                            // option at all (issue #729): always compute.
                            skip_vacuity_probe: false,
                        },
                    )
                    .0;
                }
                // `exhausted && finding.is_none()` falls through here exactly
                // like a completed empty search (issue #697; same contract
                // as the native pre-pass in `rust/fslc/src/verification.rs`).
            }
            Err(failure) => {
                return verifier_error(solver_version, &failure);
            }
        }
    }
    let mut solver = fsl_solver_z3js::Z3JsSolver::new();
    let mut result =
        match fsl_verifier::verify_bounded(&model, &mut solver, request.options.depth).await {
            Ok(result) => result,
            Err(failure) => {
                fsl_solver_z3js::reset();
                return verifier_error(solver_version, &failure);
            }
        };
    if let Err(failure) =
        fslc_rust::verification_output::replay_bmc_witnesses(&model, &result, None)
    {
        fsl_solver_z3js::reset();
        return error(solver_version, "internal", failure);
    }
    let mut statistics = fsl_solver::SmtSolver::statistics(&solver);
    // The browser bridge owns one global Z3 solver, so a fresh Rust wrapper is
    // not a fresh solver session. Reset only after witness replay has consumed
    // the BMC model; diagnosis then starts from an assertion-free backend
    // without perturbing the witness-producing query sequence.
    fsl_solver_z3js::reset();
    if let Err(failure) = add_reachable_diagnostics(&model, &mut result, &mut statistics).await {
        return verifier_error(solver_version, &failure);
    }
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
            // The Worker request surface has no `--vacuity` option at all
            // (issue #729): always compute.
            skip_vacuity_probe: false,
        },
    );
    finalize_verify_output(
        request,
        solver_version,
        &model,
        has_trace_contract,
        output,
        compose_warnings,
    )
}

/// Apply the common post-verification metadata after a BMC result is rendered.
///
/// This stays separate from solver execution so the native-host unit tests can
/// exercise the `verify` caller's `implements` error return without a browser
/// Z3 bridge.  The Worker and the native CLI both delegate the final
/// `implements` rendering to [`fslc_rust::verification_output`].
fn finalize_verify_output(
    request: &Request,
    solver_version: &str,
    model: &KernelModel,
    has_trace_contract: bool,
    mut output: Value,
    compose_warnings: Vec<Value>,
) -> Value {
    prepend_compose_warnings(&mut output, compose_warnings);
    add_frontend_metadata(
        request,
        solver_version,
        model,
        has_trace_contract,
        request.options.depth,
        output,
    )
}

fn prepend_compose_warnings(output: &mut Value, compose_warnings: Vec<Value>) {
    if !compose_warnings.is_empty()
        && let Some(object) = output.as_object_mut()
        && object.get("result").and_then(Value::as_str) != Some("error")
        && let Some(Value::Array(warnings)) = object.get_mut("warnings")
    {
        // See the matching comment in native `run_verify` (rust/fslc/src/main.rs):
        // compose-lowering warnings must be captured before `build_model` drops
        // per-component fairness information, so they cannot come from `model`.
        warnings.splice(0..0, compose_warnings);
    }
}

async fn add_reachable_diagnostics(
    model: &fsl_core::KernelModel,
    result: &mut fsl_verifier::BmcResult,
    statistics: &mut fsl_solver::VerificationStatistics,
) -> Result<(), fsl_verifier::VerifyError> {
    if result.violation.is_some() || result.reachables.values().all(Option::is_some) {
        return Ok(());
    }
    let mut diagnosis_solver = fsl_solver_z3js::Z3JsSolver::new();
    result.reachable_diagnostics =
        fsl_verifier::diagnose_reachables(model, &mut diagnosis_solver).await?;
    statistics.merge(&fsl_solver::SmtSolver::statistics(&diagnosis_solver));
    Ok(())
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
        "check" => check(&request, &solver_version).await,
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
    use std::collections::BTreeSet;
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    use super::*;
    use fsl_core::{FslValue, TraceAction, TraceStep, state_summary, trace_json};
    use fsl_verifier::{BmcResult, BmcViolation, LeadsToViolation};

    const TEST_SOLVER_VERSION: &str = "Z3 4.16.0.0";

    /// Every Worker `check`/`verify` error-return route has exactly one row.
    ///
    /// This is a test-local inventory, not a reflection of `check`/`verify`.
    /// Adding an implementation return without adding an `ErrorRoute` variant
    /// remains a population risk: the registry cannot prove that route is
    /// compared.  The exhaustive inventory guard below prevents omissions only
    /// after a route has been deliberately added to this enum.
    ///
    /// `Compared` rows name a full-envelope `assert_eq!(worker, native)` cell.
    /// `NotComparable` rows are deliberately retained with the concrete
    /// boundary that prevents a native/Worker pair; they are not a tolerated
    /// envelope difference.  The native CLI's `run_verify*` composition remains
    /// binary-private, and the native-host unit test has no initialized browser
    /// Z3 bridge, so a solver-dependent Worker return cannot be driven alongside
    /// the native composite route without widening that public API.
    #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
    enum ErrorRoute {
        CheckAiProject,
        CheckSurfaceParse,
        CheckBuild,
        CheckRequirementTrace,
        CheckGovernance,
        CheckImplements,
        VerifySurfaceParse,
        VerifyBuild,
        VerifyRequirementTrace,
        VerifyDeadlockOption,
        VerifyBoundary,
        VerifyVerifier,
        VerifyReplay,
        VerifyReachableDiagnostics,
        VerifyImplements,
    }

    impl ErrorRoute {
        const COUNT: usize = 15;

        const ALL: [Self; Self::COUNT] = [
            Self::CheckAiProject,
            Self::CheckSurfaceParse,
            Self::CheckBuild,
            Self::CheckRequirementTrace,
            Self::CheckGovernance,
            Self::CheckImplements,
            Self::VerifySurfaceParse,
            Self::VerifyBuild,
            Self::VerifyRequirementTrace,
            Self::VerifyDeadlockOption,
            Self::VerifyBoundary,
            Self::VerifyVerifier,
            Self::VerifyReplay,
            Self::VerifyReachableDiagnostics,
            Self::VerifyImplements,
        ];

        /// Adding a variant to `ErrorRoute` breaks this match, which is the only
        /// compile-time forcing point.  Having added an arm here, also raise
        /// `COUNT`, append the variant to `ALL`, and add its
        /// `ERROR_ROUTE_REGISTRY` row: each of those three is enforced, but by a
        /// different mechanism, and the diagnostics do not name each other.
        ///
        /// Measured on 2026-08-28, one isolated mutation each, reverted to
        /// SHA-256 `7b769a4e…` between them:
        ///
        /// - variant only -> `E0004 non-exhaustive patterns` (`exit=101`)
        /// - `COUNT` raised, `ALL` left alone -> `E0308 expected an array with a
        ///   size of 16, found one with a size of 15` (`exit=101`)
        /// - `ALL` appended, registry row omitted ->
        ///   `error_route_registry_is_total_and_exclusions_are_specific` fails
        ///   with `left: {0..=14}` vs `right: {0..=15}` (`exit=101`)
        /// - variant plus this arm only, `COUNT` left at its old value ->
        ///   `cargo test` **passes** (`exit=0`); only
        ///   `clippy -D warnings` rejects it, as `variant is never constructed`
        ///   (`exit=101`).  That last catcher is incidental rather than designed,
        ///   and its message does not mention `COUNT`, `ALL`, or the registry --
        ///   which is why this comment exists.
        const fn discriminant(self) -> usize {
            match self {
                Self::CheckAiProject => 0,
                Self::CheckSurfaceParse => 1,
                Self::CheckBuild => 2,
                Self::CheckRequirementTrace => 3,
                Self::CheckGovernance => 4,
                Self::CheckImplements => 5,
                Self::VerifySurfaceParse => 6,
                Self::VerifyBuild => 7,
                Self::VerifyRequirementTrace => 8,
                Self::VerifyDeadlockOption => 9,
                Self::VerifyBoundary => 10,
                Self::VerifyVerifier => 11,
                Self::VerifyReplay => 12,
                Self::VerifyReachableDiagnostics => 13,
                Self::VerifyImplements => 14,
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum RouteCoverage {
        Compared { cell: &'static str },
        NotComparable(NonComparableReason),
    }

    /// An explicit native/Worker boundary for a route that cannot form a
    /// full-envelope comparison pair in this native-host test.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum NonComparableReason {
        DeadlockWorkerRequestOption,
        VerifierRequiresBrowserSolverAndPrivateNativeComposite,
        ReplayRequiresBrowserSolverAndPrivateNativeComposite,
        ReachableDiagnosticsRequiresBrowserSolverAndPrivateNativeComposite,
    }

    impl NonComparableReason {
        const fn detail(self) -> &'static str {
            match self {
                Self::DeadlockWorkerRequestOption => {
                    "`options.deadlock` is a Worker request field; native CLI argument parsing is a distinct public input path, so no native request pair exists."
                }
                Self::VerifierRequiresBrowserSolverAndPrivateNativeComposite => {
                    "a native/Worker composite verify pair requires the binary-private native `run_verify*` API and an initialized browser Z3 bridge; neither is available to this native-host test."
                }
                Self::ReplayRequiresBrowserSolverAndPrivateNativeComposite => {
                    "replay is reached only after a browser-solver BMC result; the equivalent native composite verifier is binary-private and cannot be invoked here without public API expansion."
                }
                Self::ReachableDiagnosticsRequiresBrowserSolverAndPrivateNativeComposite => {
                    "reachable diagnosis is reached only after browser-solver BMC; a native composite pair is unavailable without exposing `run_verify*`."
                }
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct RouteRegistration {
        route: ErrorRoute,
        coverage: RouteCoverage,
    }

    const ERROR_ROUTE_REGISTRY: &[RouteRegistration] = &[
        RouteRegistration {
            route: ErrorRoute::CheckAiProject,
            coverage: RouteCoverage::Compared {
                cell: "check_error_envelopes_match_native_across_parse_guard_and_name",
            },
        },
        RouteRegistration {
            route: ErrorRoute::CheckSurfaceParse,
            coverage: RouteCoverage::Compared {
                cell: "check_error_envelopes_match_native_across_parse_guard_and_name",
            },
        },
        RouteRegistration {
            route: ErrorRoute::CheckBuild,
            coverage: RouteCoverage::Compared {
                cell: "build_rejects_duplicate_action_writes",
            },
        },
        RouteRegistration {
            route: ErrorRoute::CheckRequirementTrace,
            coverage: RouteCoverage::Compared {
                cell: "check_requirement_trace_error_envelope_matches_native",
            },
        },
        RouteRegistration {
            route: ErrorRoute::CheckGovernance,
            coverage: RouteCoverage::Compared {
                cell: "check_rejects_an_incomplete_governance_contract",
            },
        },
        RouteRegistration {
            route: ErrorRoute::CheckImplements,
            coverage: RouteCoverage::Compared {
                cell: "check_keeps_inline_enum_conversion_error_location",
            },
        },
        RouteRegistration {
            route: ErrorRoute::VerifySurfaceParse,
            coverage: RouteCoverage::Compared {
                cell: "verify_surface_parse_error_envelope_matches_native",
            },
        },
        RouteRegistration {
            route: ErrorRoute::VerifyBuild,
            coverage: RouteCoverage::Compared {
                cell: "verify_build_error_envelope_matches_native",
            },
        },
        RouteRegistration {
            route: ErrorRoute::VerifyRequirementTrace,
            coverage: RouteCoverage::Compared {
                cell: "verify_requirement_trace_error_envelope_matches_native",
            },
        },
        RouteRegistration {
            route: ErrorRoute::VerifyDeadlockOption,
            coverage: RouteCoverage::NotComparable(
                NonComparableReason::DeadlockWorkerRequestOption,
            ),
        },
        RouteRegistration {
            route: ErrorRoute::VerifyBoundary,
            coverage: RouteCoverage::Compared {
                cell: "verify_boundary_error_envelope_matches_native",
            },
        },
        RouteRegistration {
            route: ErrorRoute::VerifyVerifier,
            coverage: RouteCoverage::NotComparable(
                NonComparableReason::VerifierRequiresBrowserSolverAndPrivateNativeComposite,
            ),
        },
        RouteRegistration {
            route: ErrorRoute::VerifyReplay,
            coverage: RouteCoverage::NotComparable(
                NonComparableReason::ReplayRequiresBrowserSolverAndPrivateNativeComposite,
            ),
        },
        RouteRegistration {
            route: ErrorRoute::VerifyReachableDiagnostics,
            coverage: RouteCoverage::NotComparable(
                NonComparableReason::ReachableDiagnosticsRequiresBrowserSolverAndPrivateNativeComposite,
            ),
        },
        RouteRegistration {
            route: ErrorRoute::VerifyImplements,
            coverage: RouteCoverage::Compared {
                cell: "verify_implements_error_envelope_matches_native",
            },
        },
    ];

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
        let mut future = Box::pin(future);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            std::thread::yield_now();
        }
    }

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
                skip_vacuity_probe: false,
            },
        )
        .0
    }

    /// Render the native check path's error payload with the Worker-owned
    /// delivery metadata. The metadata is deliberately shared here so the
    /// comparison has no excluded output fields; it is not a native identity
    /// assertion (`versions.verifier` necessarily names `fsl-wasm`).
    fn native_check_error(request: &Request, solver_version: &str) -> Value {
        if let Some((output, status)) = fslc_rust::frontend_output::ai_project_check_output(
            &request.source,
            &request.source_file,
            envelope(solver_version),
        ) {
            assert_eq!(status, 2, "test fixture must be a failing AI project");
            return output;
        }
        if let Err(failure) =
            fsl_syntax::parse_document(fsl_syntax::SourceFile::new(&request.source))
        {
            return fslc_rust::frontend_output::render_surface_parse_error(
                envelope(solver_version),
                &failure,
            );
        }
        let resolver = MemoryResolver {
            files: request.files.clone(),
        };
        let diagnostic = fslc_rust::source_diagnostic::diagnostics(
            &request.source,
            &request.source_file,
            &resolver,
        )
        .into_iter()
        .find(|diagnostic| diagnostic.kind != "migration")
        .expect("test fixture must fail native source diagnostics");
        fslc_rust::verification_output::render_semantic_error(
            envelope(solver_version),
            &diagnostic.message,
            diagnostic.located.then(|| diagnostic.span.python_loc()),
            diagnostic.kind == "name",
        )
    }

    fn assert_worker_check_error_matches_native(request: &Request, fixture: &str) {
        let worker = block_on(check(request, TEST_SOLVER_VERSION));
        let native = native_check_error(request, TEST_SOLVER_VERSION);
        assert_eq!(
            worker, native,
            "Worker/native error envelope diverged for {fixture}"
        );
    }

    fn native_requirement_trace_error(request: &Request, solver_version: &str) -> Value {
        let resolver = MemoryResolver {
            files: request.files.clone(),
        };
        let kernel = fsl_core::parse_kernel_source_with_file(
            &request.source,
            &resolver,
            &request.source_file,
        )
        .expect("fixture must lower before its requirement trace failure");
        let model = fsl_core::build_model(kernel).expect("fixture must build before trace failure");
        fslc_rust::verification_output::validate_requirement_trace_source(
            &envelope(solver_version),
            &request.source,
            &model,
        )
        .expect("fixture trace validation must run")
        .0
        .expect("fixture must fail native requirement trace validation")
    }

    fn assert_worker_requirement_trace_error_matches_native(
        request: &Request,
        fixture: &str,
        command: &str,
    ) {
        let worker = match command {
            "check" => block_on(check(request, TEST_SOLVER_VERSION)),
            "verify" => block_on(verify(request, TEST_SOLVER_VERSION)),
            _ => panic!("unsupported Worker command {command}"),
        };
        let native = native_requirement_trace_error(request, TEST_SOLVER_VERSION);
        assert_eq!(
            worker, native,
            "Worker/native {command} requirement-trace envelope diverged for {fixture}"
        );
    }

    fn native_verify_surface_parse_error(request: &Request, solver_version: &str) -> Value {
        let failure = fsl_syntax::parse_surface_document(&request.source)
            .expect_err("fixture must fail native verify surface parsing");
        fslc_rust::frontend_output::render_surface_parse_error(envelope(solver_version), &failure)
    }

    fn assert_worker_verify_surface_parse_error_matches_native(request: &Request, fixture: &str) {
        let worker = block_on(verify(request, TEST_SOLVER_VERSION));
        let native = native_verify_surface_parse_error(request, TEST_SOLVER_VERSION);
        assert_eq!(
            worker, native,
            "Worker/native verify surface-parse envelope diverged for {fixture}"
        );
    }

    fn native_verify_boundary_error(request: &Request, solver_version: &str) -> Value {
        let resolver = MemoryResolver {
            files: request.files.clone(),
        };
        let kernel = fsl_core::parse_kernel_source_with_file(
            &request.source,
            &resolver,
            &request.source_file,
        )
        .expect("fixture must lower before its boundary violation");
        let model =
            fsl_core::build_model(kernel).expect("fixture must build before boundary violation");
        let (violation, trace) = fsl_runtime::find_boundary_violation(
            &model,
            request.options.depth,
            fsl_runtime::CONCRETE_PROBE_BUDGET,
        )
        .expect("boundary probe must run")
        .finding
        .expect("fixture must find a boundary violation");
        assert_ne!(
            violation.kind, "partial_op",
            "fixture must use the boundary return"
        );
        fslc_rust::verification_output::render_boundary_output(
            envelope(solver_version),
            &model,
            &violation,
            &trace,
            &fslc_rust::verification_output::BmcOutputOptions {
                depth: request.options.depth,
                deadlock: fslc_rust::verification_output::DeadlockMode::parse(
                    &request.options.deadlock,
                )
                .expect("fixture has a valid deadlock mode"),
                checked_bounds: None,
                elapsed_s: 0.0,
                statistics: &fsl_solver::VerificationStatistics::default(),
                skip_vacuity_probe: false,
            },
        )
        .0
    }

    fn assert_worker_verify_boundary_error_matches_native(request: &Request, fixture: &str) {
        let worker = block_on(verify(request, TEST_SOLVER_VERSION));
        let native = native_verify_boundary_error(request, TEST_SOLVER_VERSION);
        assert_eq!(
            worker, native,
            "Worker/native verify boundary envelope diverged for {fixture}"
        );
    }

    fn native_governance_error(request: &Request, solver_version: &str) -> Value {
        let resolver = MemoryResolver {
            files: request.files.clone(),
        };
        let failure = fslc_rust::verification_output::governance_output(
            &request.source,
            &resolver,
            |preservation| {
                let error = resolver
                    .read(&preservation.after_path)
                    .expect_err("fixture must leave governance dependency absent");
                Err(governance_error(error.to_string(), preservation.span))
            },
        )
        .expect_err("fixture must fail native governance output");
        fslc_rust::verification_output::render_governance_error(envelope(solver_version), &failure)
    }

    fn assert_worker_governance_error_matches_native(request: &Request, fixture: &str) {
        let worker = block_on(check(request, TEST_SOLVER_VERSION));
        let native = native_governance_error(request, TEST_SOLVER_VERSION);
        assert_eq!(
            worker, native,
            "Worker/native governance error diverged for {fixture}"
        );
    }

    fn native_implements_error(request: &Request, solver_version: &str) -> Value {
        let resolver = MemoryResolver {
            files: request.files.clone(),
        };
        let kernel = fsl_core::parse_kernel_source_with_file(
            &request.source,
            &resolver,
            &request.source_file,
        )
        .expect("fixture must lower before its implements failure");
        let model = fsl_core::build_model(kernel).expect("fixture must build before implements");
        let failure = fslc_rust::verification_output::requirements_implements_output(
            &request.source,
            &resolver,
            &model,
            8,
        )
        .expect_err("fixture must fail native implements output");
        fslc_rust::verification_output::render_requirements_implements_error(
            envelope(solver_version),
            &failure,
        )
    }

    fn assert_worker_implements_error_matches_native(request: &Request, fixture: &str) {
        let worker = block_on(check(request, TEST_SOLVER_VERSION));
        let native = native_implements_error(request, TEST_SOLVER_VERSION);
        assert_eq!(
            worker, native,
            "Worker/native implements error diverged for {fixture}"
        );
    }

    fn assert_verify_finalization_implements_error_matches_native(
        request: &Request,
        fixture: &str,
    ) {
        let resolver = MemoryResolver {
            files: request.files.clone(),
        };
        let kernel = fsl_core::parse_kernel_source_with_file(
            &request.source,
            &resolver,
            &request.source_file,
        )
        .expect("fixture must lower before its implements failure");
        let model = fsl_core::build_model(kernel).expect("fixture must build before implements");
        // The error branch intentionally replaces the rendered BMC payload
        // with the native implements envelope, so this value is inert while
        // still exercising `verify`'s extracted finalization caller.
        let worker = finalize_verify_output(
            request,
            TEST_SOLVER_VERSION,
            &model,
            false,
            json!({"result": "verified"}),
            Vec::new(),
        );
        let native = native_implements_error(request, TEST_SOLVER_VERSION);
        assert_eq!(
            worker, native,
            "Worker/native verify implements envelope diverged for {fixture}"
        );
    }

    #[test]
    fn error_route_registry_is_total_and_exclusions_are_specific() {
        let expected_discriminants = (0..ErrorRoute::COUNT).collect::<BTreeSet<_>>();
        let all_discriminants = ErrorRoute::ALL
            .into_iter()
            .map(ErrorRoute::discriminant)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            all_discriminants, expected_discriminants,
            "ErrorRoute::ALL must contain every discriminant exactly once"
        );
        let registered_discriminants = ERROR_ROUTE_REGISTRY
            .iter()
            .map(|entry| entry.route.discriminant())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            registered_discriminants, expected_discriminants,
            "every Worker error route needs one registry row"
        );
        assert_eq!(
            registered_discriminants.len(),
            ERROR_ROUTE_REGISTRY.len(),
            "Worker error-route registry contains duplicates"
        );
        for entry in ERROR_ROUTE_REGISTRY {
            match entry.coverage {
                RouteCoverage::Compared { cell } => assert!(
                    !cell.trim().is_empty(),
                    "{:?} is compared without a detector cell",
                    entry.route
                ),
                RouteCoverage::NotComparable(reason) => {
                    let expected_reason = match entry.route {
                        ErrorRoute::VerifyDeadlockOption => {
                            NonComparableReason::DeadlockWorkerRequestOption
                        }
                        ErrorRoute::VerifyVerifier => {
                            NonComparableReason::VerifierRequiresBrowserSolverAndPrivateNativeComposite
                        }
                        ErrorRoute::VerifyReplay => {
                            NonComparableReason::ReplayRequiresBrowserSolverAndPrivateNativeComposite
                        }
                        ErrorRoute::VerifyReachableDiagnostics => {
                            NonComparableReason::ReachableDiagnosticsRequiresBrowserSolverAndPrivateNativeComposite
                        }
                        route => panic!("{route:?} is non-comparable without a route-specific reason"),
                    };
                    assert_eq!(
                        reason,
                        expected_reason,
                        "{:?} has the wrong comparison boundary: {}",
                        entry.route,
                        reason.detail()
                    );
                    assert!(!reason.detail().trim().is_empty());
                }
            }
        }
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

        let worker = build(&request, TEST_SOLVER_VERSION)
            .expect_err("duplicate write must fail in Worker build");
        let native = native_check_error(&request, TEST_SOLVER_VERSION);
        assert_eq!(
            worker, native,
            "Worker/native duplicate-write envelope diverged"
        );
    }

    #[test]
    fn check_error_envelopes_match_native_across_parse_guard_and_name() {
        for (fixture, source, source_file) in [
            (
                "AI project parse",
                include_str!("../../fslc/tests/fixtures/error_envelope_broken_ai_project.fsl"),
                "broken_ai_project.fsl",
            ),
            (
                "surface parse",
                include_str!("../../../examples/gallery/errors/parse_missing_expression.fsl"),
                "parse_missing_expression.fsl",
            ),
            (
                "domain guard",
                include_str!("../../fslc/tests/fixtures/domain_await_routing_rejected.fsl"),
                "await_routing_rejected.fsl",
            ),
            (
                "domain name",
                include_str!(
                    "../../fslc/tests/fixtures/domain_characterization/invalid_unknown_name.fsl"
                ),
                "invalid_unknown_name.fsl",
            ),
        ] {
            let request = Request {
                cmd: "check".to_owned(),
                source: source.to_owned(),
                source_file: source_file.to_owned(),
                files: BTreeMap::new(),
                options: Options::default(),
            };
            assert_worker_check_error_matches_native(&request, fixture);
        }
    }

    #[test]
    fn verify_surface_parse_error_envelope_matches_native() {
        let request = Request {
            cmd: "verify".to_owned(),
            source: include_str!("../../../examples/gallery/errors/parse_missing_expression.fsl")
                .to_owned(),
            source_file: "parse_missing_expression.fsl".to_owned(),
            files: BTreeMap::new(),
            options: Options::default(),
        };
        assert_worker_verify_surface_parse_error_matches_native(&request, "surface parse");
    }

    #[test]
    fn check_requirement_trace_error_envelope_matches_native() {
        let request = Request {
            cmd: "check".to_owned(),
            source: include_str!(
                "../../fslc/tests/fixtures/requirements_acceptance_walk_violation.fsl"
            )
            .to_owned(),
            source_file: "requirements_acceptance_walk_violation.fsl".to_owned(),
            files: BTreeMap::new(),
            options: Options::default(),
        };
        assert_worker_requirement_trace_error_matches_native(
            &request,
            "requirements acceptance walk",
            "check",
        );
    }

    #[test]
    fn verify_build_error_envelope_matches_native() {
        let request = Request {
            cmd: "verify".to_owned(),
            source: "spec Duplicate { state { x: Bool } init { x = false } action write_twice() { x = true x = false } }".to_owned(),
            source_file: "duplicate.fsl".to_owned(),
            files: BTreeMap::new(),
            options: Options::default(),
        };
        let worker = block_on(verify(&request, TEST_SOLVER_VERSION));
        let native = native_check_error(&request, TEST_SOLVER_VERSION);
        assert_eq!(
            worker, native,
            "Worker/native verify build envelope diverged"
        );
    }

    #[test]
    fn verify_requirement_trace_error_envelope_matches_native() {
        let request = Request {
            cmd: "verify".to_owned(),
            source: include_str!(
                "../../fslc/tests/fixtures/requirements_acceptance_walk_violation.fsl"
            )
            .to_owned(),
            source_file: "requirements_acceptance_walk_violation.fsl".to_owned(),
            files: BTreeMap::new(),
            options: Options::default(),
        };
        assert_worker_requirement_trace_error_matches_native(
            &request,
            "requirements acceptance walk",
            "verify",
        );
    }

    #[test]
    fn verify_boundary_error_envelope_matches_native() {
        let request = Request {
            cmd: "verify".to_owned(),
            source: include_str!(
                "../../../examples/gallery/errors/violated_type_bound_missing_guard.fsl"
            )
            .to_owned(),
            source_file: "violated_type_bound_missing_guard.fsl".to_owned(),
            files: BTreeMap::new(),
            options: Options {
                depth: 2,
                deadlock: "warn".to_owned(),
            },
        };
        assert_worker_verify_boundary_error_matches_native(&request, "type bound");
    }

    #[test]
    fn verify_implements_error_envelope_matches_native() {
        let request = Request {
            cmd: "verify".to_owned(),
            source: r#"requirements Impl {
  implements Abs from "abs.fsl" {
    enum conversion stage ImplStage -> AbsStage { A -> A }
    map status = convert(stage, stage)
    action step() -> step()
  }
  enum ImplStage { A, B }
  state { stage: ImplStage }
  init { stage = A }
  action step() { stage = B }
}
"#
            .to_owned(),
            source_file: "impl.fsl".to_owned(),
            files: BTreeMap::from([(
                "abs.fsl".to_owned(),
                "spec Abs { enum AbsStage { A, B } state { status: AbsStage } init { status = A } action step() { status = B } }".to_owned(),
            )]),
            options: Options::default(),
        };
        assert_verify_finalization_implements_error_matches_native(
            &request,
            "inline enum conversion",
        );
    }

    #[test]
    fn check_rejects_an_incomplete_governance_contract() {
        let request = Request {
            cmd: "check".to_owned(),
            source: include_str!("../../../examples/gallery/errors/governance_missing_before.fsl")
                .to_owned(),
            source_file: "governance_missing_before.fsl".to_owned(),
            files: BTreeMap::new(),
            options: Options::default(),
        };

        assert_worker_governance_error_matches_native(&request, "missing governance before");
    }

    #[test]
    fn check_rejects_a_missing_governance_dependency_at_its_reference() {
        let request = Request {
            cmd: "check".to_owned(),
            source: include_str!("../../fslc/tests/fixtures/governance_missing_dependency.fsl")
                .to_owned(),
            source_file: "governance_missing_dependency.fsl".to_owned(),
            files: BTreeMap::new(),
            options: Options::default(),
        };

        assert_worker_governance_error_matches_native(&request, "missing governance dependency");
    }

    #[test]
    fn check_keeps_inline_enum_conversion_error_location() {
        let request = Request {
            cmd: "check".to_owned(),
            source: r#"requirements Impl {
  implements Abs from "abs.fsl" {
    enum conversion stage ImplStage -> AbsStage { A -> A }
    map status = convert(stage, stage)
    action step() -> step()
  }
  enum ImplStage { A, B }
  state { stage: ImplStage }
  init { stage = A }
  action step() { stage = B }
}
"#
            .to_owned(),
            source_file: "impl.fsl".to_owned(),
            files: BTreeMap::from([(
                "abs.fsl".to_owned(),
                "spec Abs { enum AbsStage { A, B } state { status: AbsStage } init { status = A } action step() { status = B } }".to_owned(),
            )]),
            options: Options::default(),
        };

        assert_worker_implements_error_matches_native(&request, "inline enum conversion");
    }

    #[test]
    fn check_preserves_enum_abstraction_verdicts_and_locations() {
        let source = r#"requirements Impl {
  implements Abs from "abs.fsl" {
    enum abstraction stage ImplStage -> AbsStage { A -> X B -> X C -> Y }
    map status = abstract(stage, stage)
    action hold() -> hold()
    action advance() -> advance()
  }
  enum ImplStage { C, B, A }
  state { stage: ImplStage }
  init { stage = A }
  action hold() { requires stage == A stage = B }
  action advance() { requires stage == B stage = C }
}
"#;
        let request = |source: String| {
            Request {
            cmd: "check".to_owned(),
            source,
            source_file: "impl.fsl".to_owned(),
            files: BTreeMap::from([(
                "abs.fsl".to_owned(),
                "spec Abs { enum AbsStage { Y, X, Unused } state { status: AbsStage } init { status = X } action hold() { requires status == X status = X } action advance() { requires status == X status = Y } }".to_owned(),
            )]),
            options: Options::default(),
        }
        };

        let success = block_on(check(&request(source.to_owned()), TEST_SOLVER_VERSION));
        assert_eq!(success["result"], "ok", "{success}");
        assert_eq!(success["implements"]["result"], "refines", "{success}");

        let wrong = block_on(check(
            &request(source.replace("C -> Y", "C -> X")),
            TEST_SOLVER_VERSION,
        ));
        assert_eq!(wrong["result"], "ok", "{wrong}");
        assert_eq!(
            wrong["implements"]["result"], "refinement_failed",
            "{wrong}"
        );

        let incomplete = request(source.replace(" C -> Y", ""));
        assert_worker_implements_error_matches_native(&incomplete, "incomplete enum abstraction");
    }

    #[test]
    fn verified_result_contains_shared_warnings() {
        let model = model_from(
            "spec Warnings { state { x: Bool } init { x = false } \
             @requirement(\"REQ-BLOCKED\", \"blocked action\") \
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
            reachable_diagnostics: BTreeMap::new(),
            deadlock_step: Some(0),
            deadlock_trace: Some(vec![initial]),
            action_coverage: BTreeMap::from([("blocked".to_owned(), false)]),
            frontier_progress: false,
            vacuity: Vec::new(),
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
        assert_eq!(warnings[2]["kind"], json!("never_enabled_action"));
        assert_eq!(warnings[2]["name"], json!("blocked"));
        assert!(warnings[2]["loc"].is_object());
        assert_eq!(
            warnings[2]["requirement"],
            json!({"id": "REQ-BLOCKED", "text": "blocked action"})
        );
        assert_eq!(
            warnings[2]["requirements"].as_array().map(Vec::len),
            Some(1)
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
            reachable_diagnostics: BTreeMap::new(),
            deadlock_step: Some(1),
            deadlock_trace: Some(Vec::new()),
            action_coverage: BTreeMap::new(),
            frontier_progress: false,
            vacuity: Vec::new(),
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

    #[test]
    fn nested_option_trace_matches_native_encoding() {
        let model = model_from(
            "spec NestedOption {
               type Bit = 0..1
               state { x: Option<Option<Bit>> }
               init { x = none }
               action wrap() { x = some(none) }
               action fill() { x = some(some(1)) }
             }",
        );
        let trace = vec![
            TraceStep {
                step: 0,
                state: BTreeMap::from([("x".to_owned(), FslValue::None)]),
                action: None,
                changes: BTreeMap::new(),
            },
            TraceStep {
                step: 1,
                state: BTreeMap::from([("x".to_owned(), FslValue::Some(Box::new(FslValue::None)))]),
                action: Some(TraceAction {
                    name: "wrap".to_owned(),
                    params: BTreeMap::new(),
                }),
                changes: BTreeMap::new(),
            },
            TraceStep {
                step: 2,
                state: BTreeMap::from([(
                    "x".to_owned(),
                    FslValue::Some(Box::new(FslValue::Some(Box::new(FslValue::Int(1))))),
                )]),
                action: Some(TraceAction {
                    name: "fill".to_owned(),
                    params: BTreeMap::new(),
                }),
                changes: BTreeMap::new(),
            },
        ];

        let worker = trace_json(&model, &trace);
        let native = fslc_rust::trace_json(&model, &trace);
        assert_eq!(worker, native);
        assert_eq!(worker[1]["state"]["x"], json!({"kind":"some","value":null}));
        assert_eq!(worker[2]["state"]["x"], json!({"kind":"some","value":1}));
        assert_eq!(
            worker[1]["changes"],
            json!({"x":{"from":null,"to":{"kind":"some","value":null}}})
        );
        assert_eq!(
            worker[2]["changes"],
            json!({"x":{"from":{"kind":"some","value":null},"to":{"kind":"some","value":1}}})
        );
        assert_eq!(state_summary(&model, &trace[1].state), "x=some(none)");
        assert_eq!(state_summary(&model, &trace[2].state), "x=some(some(1))");

        let struct_model = model_from(
            "spec StructFields {
               struct Packet { kind: Int, value: Int }
               state { packet: Packet }
               init { packet = Packet { kind: 0, value: 0 } }
             }",
        );
        let struct_trace = vec![
            TraceStep {
                step: 0,
                state: BTreeMap::from([(
                    "packet".to_owned(),
                    FslValue::Struct {
                        type_name: "Packet".to_owned(),
                        fields: BTreeMap::from([
                            ("kind".to_owned(), FslValue::Int(0)),
                            ("value".to_owned(), FslValue::Int(0)),
                        ]),
                    },
                )]),
                action: None,
                changes: BTreeMap::new(),
            },
            TraceStep {
                step: 1,
                state: BTreeMap::from([(
                    "packet".to_owned(),
                    FslValue::Struct {
                        type_name: "Packet".to_owned(),
                        fields: BTreeMap::from([
                            ("kind".to_owned(), FslValue::Int(1)),
                            ("value".to_owned(), FslValue::Int(1)),
                        ]),
                    },
                )]),
                action: Some(TraceAction {
                    name: "write".to_owned(),
                    params: BTreeMap::new(),
                }),
                changes: BTreeMap::new(),
            },
        ];
        let rendered = trace_json(&struct_model, &struct_trace);
        assert_eq!(rendered[1]["state"]["packet"], json!({"kind":1,"value":1}));
        assert!(rendered[1]["changes"].get("packet[kind]").is_some());
        assert!(rendered[1]["changes"].get("packet[value]").is_some());
        assert!(rendered[1]["changes"].get("packet").is_none());
    }
}

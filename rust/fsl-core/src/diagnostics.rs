// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use fsl_syntax::{Annotations, MetaTag};
use serde_json::{Map, Value, json};

use crate::dialect::{requirements_has_implements, requirements_trace_contract};
use crate::{CoreError, KernelModel, TypeRef, display_name};

/// Typed `kind` for the model-level "no user invariants" warning.
pub const NO_USER_INVARIANTS_KIND: &str = "no_user_invariants";

const NO_USER_INVARIANTS_MESSAGE: &str =
    "spec declares no user invariants (only implicit type bounds are checked)";

/// Source-aware inputs for finalizing model-level warnings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelWarningContext<'a> {
    pub model: &'a KernelModel,
    pub has_forbidden: bool,
    pub has_implements: bool,
}

impl<'a> ModelWarningContext<'a> {
    /// Build the context needed to apply #961's safety-bearing suppression rule.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError`] when the source cannot be parsed as requirements metadata.
    pub fn from_source(model: &'a KernelModel, source: &str) -> Result<Self, CoreError> {
        let has_forbidden = requirements_trace_contract(source)?
            .is_some_and(|contract| !contract.forbidden.is_empty());
        let has_implements = requirements_has_implements(source)?;
        Ok(Self {
            model,
            has_forbidden,
            has_implements,
        })
    }
}

/// Whether safety-bearing declarations suppress the no-user-invariants warning.
#[must_use]
pub fn suppresses_no_user_invariants_warning(ctx: &ModelWarningContext<'_>) -> bool {
    !ctx.model.invariants.is_empty()
        || !ctx.model.transitions.is_empty()
        || ctx.has_forbidden
        || ctx.has_implements
}

/// Whether `warning` is the model-level no-user-invariants diagnostic.
#[must_use]
pub fn is_no_user_invariants_warning(warning: &Value) -> bool {
    warning.get("kind").and_then(Value::as_str) == Some(NO_USER_INVARIANTS_KIND)
        || warning.get("message").and_then(Value::as_str) == Some(NO_USER_INVARIANTS_MESSAGE)
}

/// Apply the shared #961 suppression rule to model-level warnings.
#[must_use]
pub fn finalize_model_warnings(
    mut warnings: Vec<Value>,
    ctx: &ModelWarningContext<'_>,
) -> Vec<Value> {
    if suppresses_no_user_invariants_warning(ctx) {
        warnings.retain(|warning| !is_no_user_invariants_warning(warning));
    }
    warnings
}

/// Finalize warnings already attached to a CLI/Worker envelope.
pub fn finalize_envelope_model_warnings(output: &mut Value, ctx: &ModelWarningContext<'_>) {
    let Value::Object(envelope) = output else {
        return;
    };
    if envelope.get("result").and_then(Value::as_str) == Some("error") {
        return;
    }
    let Some(Value::Array(warnings)) = envelope.get_mut("warnings") else {
        return;
    };
    let finalized = finalize_model_warnings(std::mem::take(warnings), ctx);
    *warnings = finalized;
}

/// Return implementation versions for native and Worker check/verify envelopes.
#[must_use]
pub fn version_metadata(
    verifier: &str,
    verifier_version: &str,
    solver_backend: &str,
    solver_version: &str,
) -> Value {
    json!({
        "verifier": {"name": verifier, "version": verifier_version},
        "core": {"name": "fsl-core", "version": env!("CARGO_PKG_VERSION")},
        "solver": {
            "name": "z3",
            "backend": solver_backend,
            "version": solver_version,
        },
    })
}

#[must_use]
pub fn model_warnings(model: &KernelModel) -> Vec<Value> {
    let mut warnings = model
        .state
        .iter()
        .filter(|(_, ty)| {
            matches!(ty, TypeRef::Map(key, _) if matches!(key.as_ref(), TypeRef::Int))
        })
        .map(|(name, _)| {
            json!({
                "message": format!("Map<Int, ...> on '{}' is deprecated; use a bounded domain type as key", display_name(name)),
                "hint": "declare `type K = 0..<max>` and use `Map<K, ...>`",
            })
        })
        .collect::<Vec<_>>();
    if model.invariants.is_empty() && model.transitions.is_empty() {
        warnings.push(json!({
            "kind": NO_USER_INVARIANTS_KIND,
            "message": NO_USER_INVARIANTS_MESSAGE,
        }));
    }
    warnings
}

/// Return the deterministic requirement projection for checked annotations.
///
/// # Panics
///
/// Panics only when passed annotations that bypassed checked-model validation.
#[must_use]
pub fn requirement_metadata(annotations: &Annotations, legacy: Option<&MetaTag>) -> Vec<Value> {
    let mut requirements = annotations
        .requirements()
        .expect("checked model annotations are valid")
        .into_iter()
        .map(|requirement| json!({"id":requirement.id,"text":requirement.text}))
        .collect::<Vec<_>>();
    if requirements.is_empty()
        && let Some(meta) = legacy.filter(|meta| !meta.id.eq_ignore_ascii_case("undecided"))
    {
        requirements.push(json!({"id":meta.id,"text":meta.text}));
    }
    requirements
}

/// Add singular and plural requirement projections to a diagnostic object.
///
/// # Panics
///
/// Panics only when passed annotations that bypassed checked-model validation.
pub fn insert_requirement_metadata(
    output: &mut Map<String, Value>,
    annotations: &Annotations,
    legacy: Option<&MetaTag>,
) {
    let requirements = requirement_metadata(annotations, legacy);
    if let Some(first) = requirements.first() {
        output.insert("requirement".to_owned(), first.clone());
        output.insert("requirements".to_owned(), Value::Array(requirements));
    }
}

/// The complete, closed set of warning `kind` values `--vacuity` selects
/// over (`warn`/`error`/`ignore`). Extends the frozen Python compatibility
/// reference's set with native product lanes. Kept as an explicit enumeration rather than
/// a `"vacuous_"` name-prefix check: `always_true_requires`,
/// `tautology_over_frozen`, `urgency_freeze`, and `vacuous_deadline` are
/// vacuity findings but do
/// not share that prefix, so a prefix check silently exempts them from both
/// `--vacuity ignore` (they would stay in `warnings`) and `--vacuity error`
/// (a hollow spec carrying only one of these kinds would never fail closed).
///
/// `never_enabled_action` is bounded action-coverage evidence: it means that
/// no instance of an action was enabled through the checked depth. It belongs
/// in this set so callers can explicitly require clean bounded coverage with
/// `--vacuity error`, while the default remains a warning.
///
/// `vacuity_probe_truncated` (issue #729) is not itself a vacuity finding --
/// it means the concrete `vacuous_implication`/`vacuous_leadsto`
/// reachability probe was cut off by its state budget before it could
/// establish either verdict. It belongs in this set for the same reason
/// the four kinds above do: `--vacuity error`'s contract is "vacuity
/// evidence is clean," and a spec whose probe never completed has not
/// earned that claim -- gating it as an informational, non-selected kind
/// would let `--vacuity error` pass a spec without ever having established
/// non-vacuity, which is a strictly weaker contract than today's.
pub const VACUITY_KINDS: [&str; 8] = [
    "never_enabled_action",
    "vacuous_implication",
    "vacuous_leadsto",
    "tautology_over_frozen",
    "urgency_freeze",
    "vacuous_deadline",
    "always_true_requires",
    "vacuity_probe_truncated",
];

/// Whether `kind` is one of the closed [`VACUITY_KINDS`] `--vacuity` selects over.
#[must_use]
pub fn is_vacuity_kind(kind: &str) -> bool {
    VACUITY_KINDS.contains(&kind)
}

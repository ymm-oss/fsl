// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use fsl_syntax::{DomainEffect, DomainSaga, DomainSpec, SyntaxExpr};
use serde_json::{Value, json};

use crate::domain_naming::snake;

/// The standing `fsl-domain` assumptions for a domain document. Exposed for
/// `fslc domain replay`'s wiring, which needs the same finite-domain-model /
/// generated-scaffold / saga-observed-history set that `check`/`analyze`
/// already compute here, rather than a duplicated copy.
#[must_use]
pub fn assumptions(domain: &DomainSpec) -> Vec<Value> {
    let mut values = vec![
        json!({"id":"DOMAIN-ASSUME-FINITE-DOMAIN-MODEL","text":"domain IDs and undeclared scalar input types are modeled as finite 0..1 ranges unless declared explicitly"}),
        json!({"id":"DOMAIN-ASSUME-GENERATED-SCAFFOLD","text":"generated Functional DDD code is an implementation scaffold; runtime conformance still requires an adapter/replay evidence boundary"}),
    ];
    if !domain.sagas.is_empty() {
        values.push(json!({"id":"DOMAIN-ASSUME-SAGA-OBSERVED-HISTORY","text":"saga awaits and compensation 'after' clauses are lowered with per-step event observations; durable process history requires runtime replay evidence"}));
    }
    values
}
/// The event that satisfies an effect's request, per `DESIGN-domain.md`'s
/// `handles`-with-`request_event`-fallback contract.
fn request_event(effect: &DomainEffect) -> Option<&str> {
    effect
        .handles
        .as_deref()
        .or(effect.request_event.as_deref())
}

/// Sagas that own `effect`: a step or a compensation `emits` the effect's
/// request event (`docs/DESIGN-domain.md`'s canonical saga example emits a
/// request event from a `compensation` block, e.g.
/// `InventoryReleaseRequested`, so a compensation-only emitter must count as
/// owning too). `DESIGN-effect.md` allows a `reliable` effect's outbox
/// boundary to live on the effect *or its owning saga*; an unrelated saga's
/// outbox must not silence the finding.
fn owning_sagas<'a>(domain: &'a DomainSpec, effect: &DomainEffect) -> Vec<&'a DomainSaga> {
    let Some(request_event) = request_event(effect) else {
        return Vec::new();
    };
    let emits_request_event = |events: &[String]| events.iter().any(|event| event == request_event);
    domain
        .sagas
        .iter()
        .filter(|saga| {
            saga.steps
                .iter()
                .any(|step| emits_request_event(&step.emits))
                || saga
                    .compensations
                    .iter()
                    .any(|compensation| emits_request_event(&compensation.emits))
        })
        .collect()
}

fn effect_findings(
    domain: &DomainSpec,
    effect: &DomainEffect,
    assumptions: &[Value],
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut add = |kind: &str, severity: &str, rule: &str, witness: Value| {
        out.push(json!({"schema_version":"fsl-domain-finding.v0","fsl":"fsl-domain-effect.v0","result":"violated","kind":kind,"severity":severity,"domain":domain.name,"failed_rule":rule,"guarantee_kind":"structural","evidence":{"kind":"static_check","formal_proof":false},"witness":witness,"repair_candidates":[],"assumptions":assumptions,"effect":effect.name}));
    };
    if effect.irreversible && effect.idempotency_key.is_none() {
        add(
            "irreversible_effect_without_idempotency_key",
            "error",
            "idempotency_for_irreversible_effect",
            json!({"effect":effect.name,"irreversible":true}),
        );
    }
    if effect.async_effect && effect.timeout_event.is_none() && effect.retry.max_attempts.is_none()
    {
        add(
            "pending_effect_without_timeout_or_fallback",
            "warning",
            "timeout_or_fallback_for_pending_effect",
            json!({"effect":effect.name}),
        );
    }
    if effect.irreversible && effect.compensation_events.is_empty() {
        add(
            "missing_compensation_for_irreversible_effect",
            "warning",
            "irreversible_effect_has_compensation_or_acceptance",
            json!({"effect":effect.name,"irreversible":true}),
        );
    }
    if effect.reliable && effect.outbox.is_none() {
        let owning = owning_sagas(domain, effect);
        let covered = !owning.is_empty() && owning.iter().all(|saga| !saga.outboxes.is_empty());
        if !covered {
            let mut witness = json!({"effect":effect.name});
            if !owning.is_empty() {
                let mut uncovered_sagas = owning
                    .iter()
                    .filter(|saga| saga.outboxes.is_empty())
                    .map(|saga| saga.name.clone())
                    .collect::<Vec<_>>();
                uncovered_sagas.sort_unstable();
                witness["uncovered_sagas"] = json!(uncovered_sagas);
            }
            add(
                "reliable_effect_without_outbox_boundary",
                "warning",
                "reliable_effect_has_outbox",
                witness,
            );
        }
    }
    out
}
fn actions(domain: &DomainSpec) -> Vec<String> {
    let mut out = Vec::new();
    for aggregate in &domain.aggregates {
        for decide in &aggregate.decides {
            out.push(format!(
                "{}_{}",
                snake(&aggregate.name),
                snake(&decide.command)
            ));
        }
    }
    for effect in &domain.effects {
        for outcome in effect.outcome_events() {
            out.push(format!(
                "{}_complete_{}",
                snake(&effect.name),
                snake(outcome)
            ));
        }
        if effect.retry.max_attempts.is_some() {
            out.push(format!("{}_retry", snake(&effect.name)));
        }
    }
    for saga in &domain.sagas {
        let mut observed = BTreeSet::new();
        for step in &saga.steps {
            observed.extend(step.awaits.iter());
        }
        for compensation in &saga.compensations {
            observed.insert(&compensation.trigger_event);
            observed.insert(&compensation.after_event);
        }
        for event in observed {
            if fsl_core::domain_effect_owns_event(domain, event) {
                continue;
            }
            out.push(format!(
                "saga_{}_observe_{}",
                snake(&saga.name),
                snake(event)
            ));
        }
        for step in &saga.steps {
            out.push(format!("saga_{}_{}", snake(&saga.name), snake(&step.name)));
            if step.timeout_event.is_some() {
                out.push(format!(
                    "saga_{}_{}_timeout",
                    snake(&saga.name),
                    snake(&step.name)
                ));
            }
        }
        for item in &saga.compensations {
            out.push(format!(
                "saga_{}_compensate_{}_after_{}",
                snake(&saga.name),
                snake(&item.trigger_event),
                snake(&item.after_event)
            ));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Build a specialized domain check envelope around the shared kernel result.
///
/// # Errors
///
/// Returns an error when textual Kernel rendering rejects the domain AST.
pub fn check_domain(domain: &DomainSpec, kernel: &Value) -> Result<Value, fsl_core::CoreError> {
    let kernel_source = domain_kernel_source(domain)?;
    let assumptions = assumptions(domain);
    let findings = domain
        .effects
        .iter()
        .flat_map(|effect| effect_findings(domain, effect, &assumptions))
        .collect::<Vec<_>>();
    let hard = findings
        .iter()
        .any(|finding| finding["severity"] == "error");
    if hard {
        Ok(
            json!({"result":"violated","dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"formal_result":"not_run","findings":findings,"assumptions":assumptions,"kernel_source":kernel_source}),
        )
    } else {
        // The nested kernel is the ground truth for the aggregate-invariant
        // verdict: only "verified"/"proved" may report success. Every other
        // kernel result (violated, reachable_failed, unknown_cti,
        // unknown_budget, ...) must fold through to the top-level verdict,
        // matching the frozen Python reference's `domain_check.py`.
        let kernel_result = kernel.get("result").cloned().unwrap_or(Value::Null);
        let result = if matches!(kernel_result.as_str(), Some("verified" | "proved")) {
            "verified_under_assumptions"
        } else {
            "violated"
        };
        Ok(
            json!({"result":result,"dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"spec":domain.name,"formal_result":kernel_result,"kernel":kernel,"findings":findings,"assumptions":assumptions,"generated_actions":actions(domain)}),
        )
    }
}

/// Emit the stable structural domain analysis projection.
///
/// # Errors
///
/// Returns an error when the domain document contains a construct that
/// parses but has no executable lowering on either consumer path
/// (#710/#711/#712): this walks the same guard `check_domain` reaches
/// through `domain_kernel_source`, so every consumer of this raw-`DomainSpec`
/// projection -- present or future -- rejects the same specs `check` does,
/// instead of the guard living only in one caller's call site (#726).
pub fn analyze_domain(domain: &DomainSpec) -> Result<Value, fsl_core::CoreError> {
    // The rendered kernel source itself is not part of this projection's
    // output (issue #723 owns giving these constructs represented
    // semantics); this call exists solely for its fail-closed guard.
    domain_kernel_source(domain)?;
    let assumptions = assumptions(domain);
    let findings = domain
        .effects
        .iter()
        .flat_map(|effect| effect_findings(domain, effect, &assumptions))
        .collect::<Vec<_>>();
    Ok(
        json!({"result":"analyzed","dialect":"fsl-domain-effect.v0","finding_schema_version":"fsl-domain-finding.v0","domain":domain.name,"profile":domain.implementation_profile,"aggregates":domain.aggregates.iter().map(|a|json!({"name":a.name,"id_type":a.id_type,"state":a.state.iter().map(|f|json!({"name":f.name.text,"type":f.type_name.render_source()})).collect::<Vec<_>>(),"commands":a.commands.iter().map(|x|&x.name).collect::<Vec<_>>(),"events":a.events.iter().map(|x|&x.name).collect::<Vec<_>>(),"errors":a.errors.iter().map(|x|&x.name).collect::<Vec<_>>(),"invariants":a.invariants.iter().map(|x|&x.name.text).collect::<Vec<_>>() })).collect::<Vec<_>>(),"effects":domain.effects.iter().map(|e|json!({"name":e.name,"async":e.async_effect,"reliable":e.reliable,"irreversible":e.irreversible,"handles":e.handles.as_ref().or(e.request_event.as_ref()),"outcomes":e.outcome_events(),"correlation_id":e.correlation_id.as_ref().map(SyntaxExpr::render_source),"idempotency_key":e.idempotency_key.as_ref().map(SyntaxExpr::render_source),"retry_max_attempts":e.retry.max_attempts,"timeout_event":e.timeout_event,"outbox":e.outbox,"inbox":e.inbox})).collect::<Vec<_>>(),"sagas":domain.sagas.iter().map(|s|json!({"name":s.name,"starts_on":s.starts_on,"steps":s.steps.iter().map(|x|json!({"name":x.name,"async":x.async_step,"requires":x.requires.iter().map(SyntaxExpr::render_source).collect::<Vec<_>>(),"emits":x.emits,"awaits_mode":x.awaits_mode,"awaits":x.awaits,"timeout_event":x.timeout_event})).collect::<Vec<_>>(),"compensations":s.compensations.iter().map(|x|json!({"trigger_event":x.trigger_event,"after_event":x.after_event,"emits":x.emits})).collect::<Vec<_>>(),"outboxes":s.outboxes,"inboxes":s.inboxes,"invariants":s.invariants.iter().map(|x|&x.name.text).collect::<Vec<_>>() })).collect::<Vec<_>>(),"findings":findings,"assumptions":assumptions}),
    )
}

/// Render a compact executable kernel catalog used by expand and review tools.
///
/// # Errors
///
/// Returns an error when the domain AST has conflicting explicit outcome roles.
pub fn domain_kernel_source(domain: &DomainSpec) -> Result<String, fsl_core::CoreError> {
    fsl_core::domain_kernel_source(domain)
}

/// Generate the native implementation scaffold for a supported target.
///
/// # Errors
///
/// Returns an error for an unsupported target instead of silently treating it
/// as TypeScript.
/// Project only the source-level grouping and spelling that the closed Public
/// Kernel contract cannot represent. Emitters consume the resulting JSON, not
/// this private frontend model.
#[must_use]
pub fn domain_scaffold_metadata(domain: &DomainSpec) -> Value {
    let field = |value: &fsl_syntax::DomainField| json!({"name":value.name.text,"type_name":value.type_name.render_source()});
    json!({
        "$schema":crate::domain_codegen::METADATA_SCHEMA_ID,
        "schema_version":crate::domain_codegen::METADATA_SCHEMA_VERSION,
        "name":domain.name,
        "types":domain.types.iter().map(|value|json!({
            "name":value.name,
            "kind":value.kind,
            "members":value.members,
            "fields":value.fields.iter().map(&field).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "aggregates":domain.aggregates.iter().map(|aggregate|json!({
            "name":aggregate.name,
            "id_type":aggregate.id_type,
            "state":aggregate.state.iter().map(&field).collect::<Vec<_>>(),
            "commands":aggregate.commands.iter().map(|value|json!({
                "name":value.name,
                "inputs":value.inputs.iter().map(&field).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "events":aggregate.events.iter().map(|value|json!({
                "name":value.name,
                "fields":value.fields.iter().map(&field).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "errors":aggregate.errors.iter().map(|value|&value.name).collect::<Vec<_>>(),
            "decides":aggregate.decides.iter().map(|value|json!({
                "command":value.command,
                "requires":value.requires.iter().map(SyntaxExpr::render_source).collect::<Vec<_>>(),
                "rejects":value.rejects.iter().map(|reject|json!({
                    "error":reject.error,
                    "condition":reject.condition.render_source()
                })).collect::<Vec<_>>(),
                "emits":value.emits
            })).collect::<Vec<_>>(),
            "evolves":aggregate.evolves.iter().map(|value|json!({
                "event":value.event,
                "assignments":value.assignments.iter().map(|assignment|json!({
                    "target":assignment.target.render_source(),
                    "value":assignment.value.render_source()
                })).collect::<Vec<_>>()
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "effects":domain.effects.iter().map(|value|json!({
            "name":value.name,
            "handles":value.handles,
            "request_event":value.request_event,
            "outcomes":value.outcome_events(),
            "retry_max_attempts":value.retry.max_attempts
        })).collect::<Vec<_>>(),
        "sagas":domain.sagas.iter().map(|value|json!({
            "name":value.name,
            "starts_on":value.starts_on,
            "steps":value.steps.iter().map(|step|json!({
                "name":step.name,
                "emits":step.emits,
                "timeout_event":step.timeout_event
            })).collect::<Vec<_>>(),
            "compensations":value.compensations.iter().map(|item|json!({
                "trigger_event":item.trigger_event,
                "after_event":item.after_event,
                "emits":item.emits
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    })
}

/// Generate target files from Public Kernel v1 plus the versioned compatibility
/// metadata needed for source-level DDD names that lowering does not retain.
///
/// # Errors
///
/// Returns an error when either input contract is incompatible or inconsistent,
/// or when the target is unsupported.
pub fn domain_scaffold(kernel: &Value, metadata: &Value, target: &str) -> Result<Value, String> {
    let files = crate::domain_codegen::generate(kernel, metadata, target)?;
    let domain = kernel
        .pointer("/spec/name")
        .and_then(Value::as_str)
        .ok_or_else(|| "public Kernel root.spec.name must be a string".to_owned())?;
    Ok(json!({
        "result":"generated",
        "dialect":"fsl-domain-effect.v0",
        "domain":domain,
        "target":target,
        "files":files.into_iter().map(|(path, content)| json!({"path":path,"content":content})).collect::<Vec<_>>()
    }))
}

/// Reuse the TypeScript generator for the adapter snippets embedded by
/// `domain testgen`; this prevents a second adapter/effect implementation.
///
/// # Errors
///
/// Returns an error when the Public Kernel or metadata contract is invalid or
/// inconsistent.
pub fn domain_adapter_files(
    kernel: &Value,
    metadata: &Value,
) -> Result<Vec<(String, String)>, String> {
    let mut files = crate::domain_codegen::generate(kernel, metadata, "typescript")?
        .into_iter()
        .filter(|(path, _)| path.ends_with("/adapter.ts") || path == "effects.ts")
        .collect::<Vec<_>>();
    files.sort_by_key(|(path, _)| (path == "effects.ts", path.clone()));
    Ok(files)
}

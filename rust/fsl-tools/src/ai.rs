// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use fsl_syntax::AiComponent;
use serde_json::{Value, json};

fn assumptions(observed: bool) -> Vec<Value> {
    let mut values = vec![
        json!({"id":"AI-ASSUME-CAPABILITY-DECLARATIONS","text":"tool and authority declarations are complete for the checked AI component boundary"}),
        json!({"id":"AI-ASSUME-RUNTIME-GUARD","text":"hard contracts are enforced by the runtime guard before external tool side effects occur"}),
        json!({"id":"AI-ASSUME-NO-PROBABILITY-IN-KERNEL","text":"hard-contract checks add no probability, percentile, or evaluator semantics to the kernel"}),
    ];
    if observed {
        values.push(json!({"id":"AI-ASSUME-OBSERVABILITY-COVERAGE","text":"runtime replay is evidence only; absence from logs is not a proof that a tool or capability is unused"}));
    }
    values
}

#[allow(clippy::too_many_arguments)]
fn finding(
    component: &AiComponent,
    tool: Option<&str>,
    violation: &str,
    rule: &str,
    kind: &str,
    guarantee: &str,
    witness: &Value,
    assumptions: &[Value],
) -> Value {
    json!({
        "schema_version":"fsl-ai-finding.v0","fsl":"fsl-ai-hard.v0","result":"violated",
        "kind":kind,"severity":"error","component":component.name,"contract":"hard","tool":tool,
        "failed_rule":rule,"violation":violation,"guarantee_kind":guarantee,
        "evidence":{"kind":"runtime_replay","formal_proof":false},"witness":witness,
        "minimal_conflict_set":{"component":component.name,"tool":tool},"repair_candidates":[],
        "assumptions":assumptions,"redaction":{"policy":"tool names, schema names, and redacted event metadata only; prompts and tool args are not emitted by default"}
    })
}

#[allow(clippy::too_many_arguments)]
fn hard_finding(
    component: &AiComponent,
    tool: Option<&str>,
    violation: &str,
    rule: &str,
    kind: &str,
    guarantee: &str,
    evidence_kind: &str,
    witness: &Value,
    minimal_conflict_set: &Value,
    repair_candidates: &Value,
    assumptions: &[Value],
) -> Value {
    json!({
        "schema_version":"fsl-ai-finding.v0","fsl":"fsl-ai-hard.v0","result":"violated",
        "kind":kind,"severity":"error","component":component.name,"contract":"hard","tool":tool,
        "failed_rule":rule,"violation":violation,"guarantee_kind":guarantee,
        "evidence":{"kind":evidence_kind,"formal_proof":matches!(evidence_kind,"bmc"|"induction")},
        "witness":witness,"minimal_conflict_set":minimal_conflict_set,"repair_candidates":repair_candidates,
        "assumptions":assumptions,"redaction":{"policy":"tool names, schema names, and redacted event metadata only; prompts and tool args are not emitted by default"}
    })
}

fn authority_repairs(tool_name: &str) -> Value {
    json!([
        {"kind":"authority_change","weakens_spec":false,"description":format!("remove {tool_name} from executable authority or from forbidden, after policy review")},
        {"kind":"runtime_guard","weakens_spec":false,"description":format!("block {tool_name} before side effects when forbidden authority applies")},
    ])
}

fn approval_repairs(tool_name: &str) -> Value {
    json!([
        {"kind":"authority_change","weakens_spec":false,"description":format!("add {tool_name} to requires_human_approval")},
        {"kind":"workflow_change","weakens_spec":false,"description":format!("insert a human approval token before executing {tool_name}")},
        {"kind":"tool_change","weakens_spec":true,"description":format!("mark {tool_name} reversible only if the external side effect is actually reversible")},
    ])
}

fn schema_repairs(tool_name: &str) -> Value {
    json!([
        {"kind":"schema_declaration","weakens_spec":false,"description":format!("declare the input schema expected for {tool_name}")},
    ])
}

fn repairs_for_kernel_violation(violation: &str, tool_name: &str) -> Value {
    match violation {
        "human_approval_required_before_irreversible_tool" => json!([
            {"kind":"workflow_change","weakens_spec":false,"description":format!("insert a human_approval event before executing {tool_name}")},
            {"kind":"runtime_guard","weakens_spec":false,"description":format!("block {tool_name} until a valid approval token exists")},
        ]),
        "forbidden_tool_call" => json!([
            {"kind":"runtime_guard","weakens_spec":false,"description":format!("block {tool_name} before external side effects")},
            {"kind":"authority_change","weakens_spec":true,"description":format!("remove {tool_name} from forbidden only if policy explicitly permits it")},
        ]),
        _ => json!([]),
    }
}

/// Structural (pre-kernel) hard-contract findings: `tool_authority` (a
/// forbidden tool also declared executable), `human_approval_required` (an
/// irreversible tool with no explicit `requires_human_approval`, mirroring
/// `docs/DESIGN-ai-hard.md`'s rule text rather than the narrower "and it is
/// in `may_execute`" precondition), and `tool_schema_declared` (an
/// executable tool with no declared schema).
fn static_ai_findings(component: &AiComponent, assumptions: &[Value]) -> Vec<Value> {
    let sets = fsl_core::ai_tool_sets(component);
    let mut findings = Vec::new();
    for &tool_name in sets.forbidden.intersection(&sets.executable) {
        let authority_kind = if component
            .authority
            .may_execute
            .iter()
            .any(|rule| rule.name == tool_name)
        {
            "may_execute"
        } else {
            "requires_human_approval"
        };
        findings.push(hard_finding(
            component,
            Some(tool_name),
            "forbidden_tool_declared_executable",
            "tool_authority",
            "ai_hard_contract_violation",
            "syntactic_hard",
            "static_check",
            &json!({"tool":tool_name,"authority":["forbidden", authority_kind]}),
            &json!({"component":component.name,"tool":tool_name}),
            &authority_repairs(tool_name),
            assumptions,
        ));
    }
    // Explicit-only: distinct from `sets.approval_required`, which already
    // folds irreversible tools in -- this must catch the tool that has not
    // (yet) been explicitly listed.
    let explicit_approval = component
        .authority
        .requires_human_approval
        .iter()
        .map(|rule| rule.name.as_str())
        .collect::<BTreeSet<_>>();
    for tool in &component.tools {
        if tool.irreversible
            && !explicit_approval.contains(tool.name.as_str())
            && !sets.forbidden.contains(tool.name.as_str())
        {
            findings.push(hard_finding(
                component,
                Some(&tool.name),
                "irreversible_tool_without_human_approval_guard",
                "human_approval_required",
                "ai_hard_contract_violation",
                "syntactic_hard",
                "static_check",
                &json!({"tool":tool.name,"irreversible":true,"requires_human_approval":false}),
                &json!({"component":component.name,"tool":tool.name}),
                &approval_repairs(&tool.name),
                assumptions,
            ));
        }
    }
    for tool in &component.tools {
        if sets.executable.contains(tool.name.as_str()) && tool.schema.is_none() {
            findings.push(hard_finding(
                component,
                Some(&tool.name),
                "executable_tool_without_schema",
                "tool_schema_declared",
                "ai_hard_contract_violation",
                "syntactic_hard",
                "static_check",
                &json!({"tool":tool.name,"schema":Value::Null}),
                &json!({"component":component.name,"tool":tool.name}),
                &schema_repairs(&tool.name),
                assumptions,
            ));
        }
    }
    findings
}

fn kernel_projection(kernel: &Value) -> Value {
    let Value::Object(kernel) = kernel else {
        return kernel.clone();
    };
    Value::Object(
        [
            "result",
            "spec",
            "depth",
            "checked_to_depth",
            "completeness",
            "invariant",
            "violation_kind",
        ]
        .into_iter()
        .filter_map(|key| {
            kernel
                .get(key)
                .cloned()
                .map(|value| (key.to_owned(), value))
        })
        .collect(),
    )
}

/// Translate a violated generated invariant (`forbidden_tool_blocked` /
/// `human_approval_required`) back into an `ai_hard_contract_violation`
/// finding. Both invariants are structural tautologies given the generated
/// guards (no `execute_*` action exists for a forbidden tool; an
/// approval-required tool's `execute_*` action always `requires
/// human_approved[tool]`), so this is depth-independent defense-in-depth
/// mirroring the frozen reference's `_translate_kernel_result`, not the
/// primary detection path.
fn translate_kernel_violation(
    component: &AiComponent,
    kernel: &Value,
    assumptions: &[Value],
) -> Vec<Value> {
    if kernel.get("result").and_then(Value::as_str) != Some("violated")
        || kernel.get("violation_kind").and_then(Value::as_str) != Some("invariant")
    {
        return Vec::new();
    }
    let Some(inv_name) = kernel.get("invariant").and_then(Value::as_str) else {
        return Vec::new();
    };
    let sets = fsl_core::ai_tool_sets(component);
    let translated = sets
        .forbidden
        .iter()
        .find(|&&tool_name| fsl_core::ai_forbidden_invariant_name(tool_name) == inv_name)
        .map(|&tool_name| {
            (
                "ai_hard_contract_violation",
                "forbidden_tool_call",
                "forbidden_tool_blocked",
                tool_name,
            )
        })
        .or_else(|| {
            sets.approval_required
                .difference(&sets.forbidden)
                .find(|&&tool_name| fsl_core::ai_approval_invariant_name(tool_name) == inv_name)
                .map(|&tool_name| {
                    (
                        "ai_hard_contract_violation",
                        "human_approval_required_before_irreversible_tool",
                        "human_approval_required",
                        tool_name,
                    )
                })
        });
    let Some((kind, violation, rule, tool_name)) = translated else {
        return Vec::new();
    };
    let evidence_kind = if kernel.get("completeness").and_then(Value::as_str) == Some("induction") {
        "induction"
    } else {
        "bmc"
    };
    vec![hard_finding(
        component,
        Some(tool_name),
        violation,
        rule,
        kind,
        "syntactic_hard",
        evidence_kind,
        &json!({"invariant":inv_name,"trace":kernel.get("trace").cloned().unwrap_or_else(|| json!([]))}),
        &json!({"component":component.name,"tool":tool_name,"invariant":inv_name}),
        &repairs_for_kernel_violation(violation, tool_name),
        assumptions,
    )]
}

/// Check the structural hard-contract portion of an AI component.
///
/// `kernel` is the *unprojected* result of verifying the generated kernel
/// spec (`fslc verify`'s full JSON envelope for the lowered `ai_component`),
/// so witness construction can use fields like `trace` that the published
/// `kernel` projection below deliberately omits.
#[must_use]
pub fn check_ai(component: &AiComponent, kernel: &Value) -> Value {
    let assumptions = assumptions(false);
    let static_findings = static_ai_findings(component, &assumptions);
    let guarantee_boundary = json!({"proved":"kernel safety facts over the finite hard-contract expansion","evaluator_supported":"external evaluator evidence and never reported as formal proof","statistically_supported":"external statistical evidence and never reported as formal proof","runtime_replay":"observed evidence, not proof"});
    if !static_findings.is_empty() {
        return json!({
            "result":"violated","dialect":"fsl-ai-hard.v0","finding_schema_version":"fsl-ai-finding.v0",
            "ai_component":component.name,"guarantee_boundary":guarantee_boundary,
            "assumptions":assumptions,"findings":static_findings,"formal_result":"not_run","kernel":Value::Null,
        });
    }
    let translated = translate_kernel_violation(component, kernel, &assumptions);
    let kernel_result = kernel
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or("error")
        .to_owned();
    let (result, findings, formal_result) = if !translated.is_empty() {
        ("violated".to_owned(), translated, kernel_result)
    } else if matches!(kernel_result.as_str(), "verified" | "proved") {
        (
            "verified_under_assumptions".to_owned(),
            Vec::new(),
            kernel_result,
        )
    } else {
        (kernel_result.clone(), Vec::new(), kernel_result)
    };
    json!({
        "result":result,"dialect":"fsl-ai-hard.v0","finding_schema_version":"fsl-ai-finding.v0",
        "ai_component":component.name,"guarantee_boundary":guarantee_boundary,
        "assumptions":assumptions,"findings":findings,"formal_result":formal_result,
        "kernel":kernel_projection(kernel),
    })
}

/// Replay already-parsed runtime events against an AI hard contract.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn replay_ai(component: &AiComponent, events: &[Value]) -> Value {
    let assumptions = assumptions(true);
    let mut findings = Vec::new();
    let mut approvals = BTreeSet::new();
    let tools = component
        .tools
        .iter()
        .map(|tool| (tool.name.as_str(), tool))
        .collect::<std::collections::BTreeMap<_, _>>();
    let sets = fsl_core::ai_tool_sets(component);
    for (index, event) in events.iter().enumerate() {
        let event_type = event
            .get("event")
            .or_else(|| event.get("type"))
            .and_then(Value::as_str);
        if event_type == Some("human_approval") {
            if let Some(tool) = event.get("tool").and_then(Value::as_str) {
                approvals.insert(tool.to_owned());
            }
            continue;
        }
        let calls = event
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| {
                if event_type == Some("tool_call") {
                    vec![event.clone()]
                } else {
                    Vec::new()
                }
            });
        for call in calls {
            let tool_name = call
                .get("tool")
                .or_else(|| call.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mode = call
                .get("mode")
                .or_else(|| call.get("phase"))
                .and_then(Value::as_str)
                .unwrap_or("execute");
            let Some(tool) = tools.get(tool_name) else {
                findings.push(finding(component,Some(tool_name),"undeclared_tool_observed","runtime_observation","observed_contract_violation","runtime_observed",&json!({"event_index":index,"reason":"observed tool call is not declared by the AI component"}),&assumptions));
                continue;
            };
            if mode == "suggest"
                && !sets.suggestible.contains(tool_name)
                && !sets.executable.contains(tool_name)
            {
                findings.push(finding(component,Some(tool_name),"suggestion_without_authority","tool_authority","ai_hard_contract_violation","syntactic_hard",&json!({"event_index":index,"reason":"tool suggestion is outside may_suggest/may_execute authority"}),&assumptions));
            }
            if mode == "execute" && !sets.executable.contains(tool_name) {
                findings.push(finding(component,Some(tool_name),"execution_without_authority","tool_authority","ai_hard_contract_violation","syntactic_hard",&json!({"event_index":index,"reason":"tool execution is outside may_execute/requires_human_approval authority"}),&assumptions));
            }
            if mode == "execute"
                && component
                    .authority
                    .forbidden
                    .iter()
                    .any(|rule| rule.name == tool_name)
            {
                findings.push(finding(component,Some(tool_name),"forbidden_tool_call","forbidden_tool_blocked","ai_hard_contract_violation","syntactic_hard",&json!({"event_index":index,"reason":"forbidden tool was observed in execute mode","event":"tool_call","component":component.name,"tool":tool_name,"mode":mode,"tool_schema":call.get("tool_schema"),"schema_valid":call.get("schema_valid"),"arg_keys":call.get("args").and_then(Value::as_object).map(|args|args.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()}),&assumptions));
            }
            if mode == "execute"
                && component
                    .authority
                    .requires_human_approval
                    .iter()
                    .any(|rule| rule.name == tool_name)
                && !approvals.remove(tool_name)
            {
                findings.push(finding(component,Some(tool_name),"human_approval_required_before_irreversible_tool","human_approval_required","ai_hard_contract_violation","syntactic_hard",&json!({"event_index":index,"reason":"tool execution was observed before human approval"}),&assumptions));
            }
            if call.get("schema_valid") == Some(&Value::Bool(false)) {
                findings.push(finding(
                    component,
                    Some(tool_name),
                    "tool_schema_invalid",
                    "tool_schema_declared",
                    "ai_hard_contract_violation",
                    "syntactic_hard",
                    &json!({"event_index":index}),
                    &assumptions,
                ));
            }
            if let (Some(expected), Some(observed)) = (
                &tool.schema,
                call.get("tool_schema").and_then(Value::as_str),
            ) && expected != observed
            {
                findings.push(finding(
                    component,
                    Some(tool_name),
                    "tool_schema_mismatch",
                    "runtime_observation",
                    "observed_contract_violation",
                    "runtime_observed",
                    &json!({"event_index":index,"expected":expected,"observed":observed}),
                    &assumptions,
                ));
            }
            if !tool.preconditions.is_empty() {
                match call.get("preconditions").and_then(Value::as_object) {
                    None => {
                        // A declared precondition with no evidence object at
                        // all previously passed silently -- only an explicit
                        // `false` value was ever caught.
                        findings.push(finding(
                            component,
                            Some(tool_name),
                            "business_precondition_mismatch",
                            "tool_precondition_declared",
                            "ai_hard_contract_violation",
                            "syntactic_hard",
                            &json!({"event_index":index,"missing_preconditions":tool.preconditions}),
                            &assumptions,
                        ));
                    }
                    Some(observed) => {
                        for name in &tool.preconditions {
                            if observed.get(name) != Some(&Value::Bool(true)) {
                                findings.push(finding(
                                    component,
                                    Some(tool_name),
                                    "business_precondition_mismatch",
                                    "tool_precondition_declared",
                                    "ai_hard_contract_violation",
                                    "syntactic_hard",
                                    &json!({"event_index":index,"precondition":name,"observed":observed.get(name)}),
                                    &assumptions,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    json!({"result":if findings.is_empty(){"replay_conformant"}else{"replay_nonconformant"},"dialect":"fsl-ai-hard.v0","finding_schema_version":"fsl-ai-finding.v0","event_schema_version":"fsl-ai-event.v0","ai_component":component.name,"events_checked":events.len(),"formal_result":"not_run","evidence":{"kind":"runtime_replay","formal_proof":false},"assumptions":assumptions,"findings":findings,"note":"runtime replay is separate from formal proof; statistical and evaluator-backed contracts are external evidence"})
}

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

/// Check the structural hard-contract portion of an AI component.
#[must_use]
pub fn check_ai(component: &AiComponent, kernel: Value) -> Value {
    let assumptions = assumptions(false);
    let mut findings = Vec::new();
    let approvals = component
        .authority
        .requires_human_approval
        .iter()
        .collect::<BTreeSet<_>>();
    for tool in &component.tools {
        if tool.irreversible
            && component.authority.may_execute.contains(&tool.name)
            && !approvals.contains(&tool.name)
        {
            findings.push(finding(
                component,
                Some(&tool.name),
                "irreversible_tool_without_human_approval_guard",
                "human_approval_required",
                "ai_hard_contract_violation",
                "syntactic_hard",
                &json!({"tool":tool.name,"irreversible":true}),
                &assumptions,
            ));
        }
    }
    let violated = !findings.is_empty();
    json!({
        "result":if violated{"violated"}else{"verified_under_assumptions"},"dialect":"fsl-ai-hard.v0",
        "finding_schema_version":"fsl-ai-finding.v0","ai_component":component.name,
        "guarantee_boundary":{"proved":"kernel safety facts over the finite hard-contract expansion","evaluator_supported":"external evaluator evidence and never reported as formal proof","statistically_supported":"external statistical evidence and never reported as formal proof","runtime_replay":"observed evidence, not proof"},
        "assumptions":assumptions,"findings":findings,"formal_result":if violated{"not_run"}else{"verified"},
        "kernel":if violated{Value::Null}else{kernel}
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
            if mode == "execute"
                && component
                    .authority
                    .forbidden
                    .iter()
                    .any(|item| item == tool_name)
            {
                findings.push(finding(component,Some(tool_name),"forbidden_tool_call","forbidden_tool_blocked","ai_hard_contract_violation","syntactic_hard",&json!({"event_index":index,"reason":"forbidden tool was observed in execute mode","event":"tool_call","component":component.name,"tool":tool_name,"mode":mode,"tool_schema":call.get("tool_schema"),"schema_valid":call.get("schema_valid"),"arg_keys":call.get("args").and_then(Value::as_object).map(|args|args.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()}),&assumptions));
            }
            if mode == "execute"
                && component
                    .authority
                    .requires_human_approval
                    .iter()
                    .any(|item| item == tool_name)
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
            if call
                .get("preconditions")
                .and_then(Value::as_object)
                .is_some_and(|values| values.values().any(|value| value == &Value::Bool(false)))
            {
                findings.push(finding(
                    component,
                    Some(tool_name),
                    "business_precondition_mismatch",
                    "tool_precondition_declared",
                    "ai_hard_contract_violation",
                    "syntactic_hard",
                    &json!({"event_index":index}),
                    &assumptions,
                ));
            }
        }
    }
    json!({"result":if findings.is_empty(){"replay_conformant"}else{"replay_nonconformant"},"dialect":"fsl-ai-hard.v0","finding_schema_version":"fsl-ai-finding.v0","event_schema_version":"fsl-ai-event.v0","ai_component":component.name,"events_checked":events.len(),"formal_result":"not_run","evidence":{"kind":"runtime_replay","formal_proof":false},"assumptions":assumptions,"findings":findings,"note":"runtime replay is separate from formal proof; statistical and evaluator-backed contracts are external evidence"})
}

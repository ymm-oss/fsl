// SPDX-License-Identifier: Apache-2.0

//! Structural analysis for the recursive fsl-ai `agent` dialect (issue #468).
//!
//! Mirrors the frozen reference's `src/fslc/ai_agent.py` behaviorally: lexical
//! nesting defines namespace/scope only (`AI-ASSUME-NESTING-NOT-DELEGATION`),
//! a child agent receives no implicit authority/context
//! (`AI-ASSUME-NO-IMPLICIT-INHERITANCE`) and must stay inside its immediate
//! parent's declared boundary via explicit `grant`, and runtime collaboration
//! is declared separately via `orchestration` edges. A `grant` that exceeds
//! the parent boundary is a check-time `AgentError` (semantics error,
//! `docs/LANGUAGE.md` §13.6), distinct from the six `agent_structural_violation`
//! finding kinds below, which are reported findings, not parse/validation
//! failures.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fsl_syntax::{AiAuthority, AiFailurePolicy, AiLoc, AiTool, SurfaceAgent};
use serde_json::{Value, json};

const AI_AGENT_DIALECT_VERSION: &str = "fsl-ai-agent.v0";
const AI_FINDING_SCHEMA_VERSION: &str = "fsl-ai-finding.v0";

#[derive(Clone, Debug)]
pub struct AgentError {
    pub message: String,
    pub loc: Option<AiLoc>,
    pub hint: Option<String>,
}

impl fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AgentError {}

fn agent_err<T>(message: impl Into<String>, loc: Option<AiLoc>) -> Result<T, AgentError> {
    Err(AgentError {
        message: message.into(),
        loc,
        hint: None,
    })
}

fn agent_err_hint<T>(
    message: impl Into<String>,
    loc: Option<AiLoc>,
    hint: impl Into<String>,
) -> Result<T, AgentError> {
    Err(AgentError {
        message: message.into(),
        loc,
        hint: Some(hint.into()),
    })
}

struct AgentInfo<'a> {
    agent: &'a SurfaceAgent,
    path: Vec<String>,
    parent: Option<Vec<String>>,
    available_authority: BTreeSet<String>,
    available_context: BTreeSet<String>,
}

fn path_str(path: &[String]) -> String {
    path.join(".")
}

fn all_tool_names(agent: &SurfaceAgent) -> BTreeSet<String> {
    let mut names: BTreeSet<String> = agent.tool_names.iter().cloned().collect();
    names.extend(agent.tools.iter().map(|tool| tool.name.clone()));
    names
}

fn authority_names(authority: &AiAuthority) -> BTreeSet<String> {
    authority
        .may_suggest
        .iter()
        .chain(&authority.may_execute)
        .chain(&authority.requires_human_approval)
        .map(|rule| rule.name.clone())
        .collect()
}

fn declared_authority_boundary(agent: &SurfaceAgent) -> BTreeSet<String> {
    let mut set = all_tool_names(agent);
    set.extend(authority_names(&agent.authority));
    set
}

fn high_authority_tools(agent: &SurfaceAgent) -> Vec<String> {
    let approval: BTreeSet<&str> = agent
        .authority
        .requires_human_approval
        .iter()
        .map(|rule| rule.name.as_str())
        .collect();
    let mut names: BTreeSet<String> = agent
        .tools
        .iter()
        .filter(|tool| tool.irreversible || approval.contains(tool.name.as_str()))
        .map(|tool| tool.name.clone())
        .collect();
    names.extend(
        agent
            .authority
            .requires_human_approval
            .iter()
            .map(|rule| rule.name.clone()),
    );
    names.into_iter().collect()
}

fn granted(agent: &SurfaceAgent, kind: &str) -> BTreeSet<String> {
    agent
        .grants
        .iter()
        .filter(|grant| grant.kind == kind)
        .flat_map(|grant| grant.names.iter().cloned())
        .collect()
}

fn first_grant_loc(agent: &SurfaceAgent, kind: &str) -> AiLoc {
    agent
        .grants
        .iter()
        .find(|grant| grant.kind == kind)
        .map_or(agent.loc, |grant| grant.loc)
}

fn tool_map(agent: &SurfaceAgent) -> BTreeMap<String, AiTool> {
    let mut map: BTreeMap<String, AiTool> = agent
        .tool_names
        .iter()
        .map(|name| {
            (
                name.clone(),
                AiTool {
                    name: name.clone(),
                    schema: None,
                    irreversible: false,
                    preconditions: Vec::new(),
                    effect: None,
                    annotations: fsl_syntax::Annotations::default(),
                    loc: None,
                },
            )
        })
        .collect();
    for tool in &agent.tools {
        map.insert(tool.name.clone(), tool.clone());
    }
    map
}

fn dedupe(values: &[String], label: &str, path: &str, loc: AiLoc) -> Result<(), AgentError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value.as_str()) {
            return agent_err(
                format!("agent '{path}' declares duplicate {label} '{value}'"),
                Some(loc),
            );
        }
    }
    Ok(())
}

fn validate_local_duplicates(node: &SurfaceAgent, path: &[String]) -> Result<(), AgentError> {
    let component = path_str(path);
    dedupe(
        &node
            .children
            .iter()
            .map(|child| child.name.clone())
            .collect::<Vec<_>>(),
        "child agent",
        &component,
        node.loc,
    )?;
    dedupe(
        &node
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>(),
        "tool",
        &component,
        node.loc,
    )?;
    dedupe(&node.tool_names, "tool", &component, node.loc)?;
    let tool_set: BTreeSet<&str> = node.tools.iter().map(|tool| tool.name.as_str()).collect();
    let name_set: BTreeSet<&str> = node.tool_names.iter().map(String::as_str).collect();
    let overlap = tool_set
        .intersection(&name_set)
        .copied()
        .collect::<Vec<_>>();
    if !overlap.is_empty() {
        return agent_err(
            format!(
                "agent '{component}' declares duplicate tool {}",
                overlap.join(", ")
            ),
            Some(node.loc),
        );
    }
    dedupe(
        &node
            .outputs
            .iter()
            .map(|output| output.name.clone())
            .collect::<Vec<_>>(),
        "output",
        &component,
        node.loc,
    )?;
    Ok(())
}

fn validate_output_visibility(
    node: &SurfaceAgent,
    parent_info: Option<&AgentInfo<'_>>,
    path: &[String],
) -> Result<(), AgentError> {
    let parent_children: BTreeSet<String> = parent_info.map_or_else(BTreeSet::new, |parent| {
        parent
            .agent
            .children
            .iter()
            .map(|child| child.name.clone())
            .collect()
    });
    let own_children: BTreeSet<String> = node
        .children
        .iter()
        .map(|child| child.name.clone())
        .collect();
    let mut allowed: BTreeSet<String> = std::iter::once("self".to_owned())
        .chain(own_children)
        .collect();
    if parent_info.is_some() {
        allowed.insert("parent".to_owned());
        for name in &parent_children {
            if name != &node.name {
                allowed.insert(name.clone());
            }
        }
    }
    for output in &node.outputs {
        let unknown = output
            .visibility
            .iter()
            .filter(|target| !allowed.contains(target.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !unknown.is_empty() {
            return agent_err_hint(
                format!(
                    "agent '{}' output '{}' has unknown visibility target: {}",
                    path_str(path),
                    output.name,
                    unknown.join(", ")
                ),
                Some(output.loc),
                "visibility targets must be parent, self, child agents, or sibling agents in the parent scope",
            );
        }
    }
    Ok(())
}

fn require_child(
    name: &str,
    child_names: &BTreeSet<&str>,
    node: &SurfaceAgent,
    loc: AiLoc,
    label: &str,
) -> Result<(), AgentError> {
    if !child_names.contains(name) {
        return agent_err_hint(
            format!("{label} '{name}' is not a child agent of '{}'", node.name),
            Some(loc),
            "orchestration and failure_policy edges are separate from lexical nesting and reference immediate children",
        );
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn walk<'a>(
    node: &'a SurfaceAgent,
    parent_info: Option<&AgentInfo<'a>>,
) -> Result<Vec<AgentInfo<'a>>, AgentError> {
    let path = match parent_info {
        None => vec![node.name.clone()],
        Some(parent) => {
            let mut path = parent.path.clone();
            path.push(node.name.clone());
            path
        }
    };

    let (available_authority, available_context) = if let Some(parent) = parent_info {
        let grant_authority = granted(node, "authority");
        let grant_context = granted(node, "context");
        let extra_authority = grant_authority
            .difference(&parent.available_authority)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !extra_authority.is_empty() {
            return agent_err_hint(
                format!(
                    "agent '{}' grant authority exceeds parent boundary: {}",
                    path_str(&path),
                    extra_authority.into_iter().collect::<Vec<_>>().join(", ")
                ),
                Some(first_grant_loc(node, "authority")),
                "grant only tools/capabilities declared in the immediate parent boundary",
            );
        }
        let extra_context = grant_context
            .difference(&parent.available_context)
            .cloned()
            .collect::<BTreeSet<_>>();
        if !extra_context.is_empty() {
            return agent_err_hint(
                format!(
                    "agent '{}' grant context exceeds parent boundary: {}",
                    path_str(&path),
                    extra_context.into_iter().collect::<Vec<_>>().join(", ")
                ),
                Some(first_grant_loc(node, "context")),
                "grant only context symbols declared in the immediate parent boundary",
            );
        }
        (grant_authority, grant_context)
    } else {
        if let Some(grant) = node.grants.first() {
            return agent_err_hint(
                format!(
                    "top-level agent '{}' cannot declare grant {}",
                    node.name, grant.kind
                ),
                Some(grant.loc),
                "declare root authority/context directly, and grant only inside nested agents",
            );
        }
        (
            declared_authority_boundary(node),
            node.context.iter().cloned().collect(),
        )
    };

    validate_local_duplicates(node, &path)?;
    let child_names: BTreeSet<&str> = node
        .children
        .iter()
        .map(|child| child.name.as_str())
        .collect();
    for edge in &node.orchestration {
        require_child(
            &edge.source,
            &child_names,
            node,
            edge.loc,
            "orchestration source",
        )?;
        require_child(
            &edge.target,
            &child_names,
            node,
            edge.loc,
            "orchestration target",
        )?;
    }
    for gate in &node.review_gates {
        require_child(gate, &child_names, node, node.loc, "review_gate")?;
    }
    for policy in &node.failure_policy {
        require_child(
            &policy.agent,
            &child_names,
            node,
            policy.loc,
            "failure_policy source",
        )?;
    }

    let info = AgentInfo {
        agent: node,
        path: path.clone(),
        parent: parent_info.map(|parent| parent.path.clone()),
        available_authority,
        available_context,
    };
    if !node.outputs.is_empty() {
        validate_output_visibility(node, parent_info, &path)?;
    }

    let mut all = vec![info];
    for child in &node.children {
        let child_infos = walk(child, Some(&all[0]))?;
        all.extend(child_infos);
    }
    Ok(all)
}

fn agent_assumptions() -> Vec<Value> {
    vec![
        json!({"id":"AI-ASSUME-AGENT-DECLARATIONS","text":"agent authority, context, tool, visibility, and orchestration declarations are complete"}),
        json!({"id":"AI-ASSUME-NESTING-NOT-DELEGATION","text":"lexical nesting defines namespace and scope only; runtime collaboration comes from orchestration edges"}),
        json!({"id":"AI-ASSUME-NO-IMPLICIT-INHERITANCE","text":"nested agents do not inherit parent authority or context without explicit grant declarations"}),
    ]
}

#[allow(clippy::too_many_arguments)]
fn finding(
    component: &str,
    tool: Option<&str>,
    failed_rule: &str,
    violation: &str,
    severity: &str,
    witness: &Value,
    minimal_conflict_set: &Value,
    repair_candidates: &Value,
    assumptions: &[Value],
) -> Value {
    json!({
        "schema_version": AI_FINDING_SCHEMA_VERSION, "fsl": AI_AGENT_DIALECT_VERSION, "result": "violated",
        "kind": "agent_structural_violation", "severity": severity, "component": component, "contract": "agent_structure",
        "tool": tool, "failed_rule": failed_rule, "violation": violation, "guarantee_kind": "agent_structural",
        "evidence": {"kind": "static_agent_graph", "formal_proof": false},
        "witness": witness, "minimal_conflict_set": minimal_conflict_set, "repair_candidates": repair_candidates,
        "assumptions": assumptions, "redaction": {"policy": "agent, tool, context, and graph labels only; prompts and tool args are not emitted"},
    })
}

fn reachability(
    edges: &[fsl_syntax::AiDelegationEdge],
    nodes: impl Iterator<Item = String>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut graph: BTreeMap<String, BTreeSet<String>> =
        nodes.map(|node| (node, BTreeSet::new())).collect();
    for edge in edges {
        graph
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.target.clone());
        graph.entry(edge.target.clone()).or_default();
    }
    let mut closure = graph;
    loop {
        let snapshot = closure.clone();
        let mut changed = false;
        for (node, targets) in &mut closure {
            let mut expanded = targets.clone();
            for target in targets.iter() {
                if let Some(more) = snapshot.get(target) {
                    expanded.extend(more.iter().cloned());
                }
            }
            if &expanded != targets {
                *targets = expanded;
                changed = true;
            }
            let _ = node;
        }
        if !changed {
            break;
        }
    }
    closure
}

/// Compute the six documented `agent_structural_violation` finding kinds
/// (`docs/DESIGN-ai-hard.md` "Rules enforced as stable semantics") over an
/// already-validated agent tree.
#[allow(clippy::too_many_lines)]
fn agent_findings(
    infos: &BTreeMap<Vec<String>, AgentInfo<'_>>,
    assumptions: &[Value],
) -> Vec<Value> {
    let mut findings = Vec::new();
    let by_path: BTreeMap<String, &AgentInfo<'_>> = infos
        .values()
        .map(|info| (path_str(&info.path), info))
        .collect();

    for info in infos.values() {
        let node = info.agent;
        let component = path_str(&info.path);
        if info.parent.is_some() {
            let used_authority = declared_authority_boundary(node);
            let exceeded = used_authority
                .difference(&info.available_authority)
                .cloned()
                .collect::<BTreeSet<_>>();
            if !exceeded.is_empty() {
                findings.push(finding(
                    &component,
                    exceeded.iter().next().map(String::as_str),
                    "authority_grant_subset",
                    "child_authority_exceeds_parent_authority",
                    "error",
                    &json!({"agent": component, "used_authority": used_authority, "granted_authority": info.available_authority, "exceeded": exceeded}),
                    &json!({"agent": component, "exceeded_authority": exceeded}),
                    &json!([{"kind":"grant_change","weakens_spec":false,"description":"grant only authority inside the parent boundary or remove the child use"}]),
                    assumptions,
                ));
            }
            let mut used_context = node.context.clone();
            used_context.sort();
            let context_set: BTreeSet<String> = node.context.iter().cloned().collect();
            let context_exceeded = context_set
                .difference(&info.available_context)
                .cloned()
                .collect::<BTreeSet<_>>();
            if !context_exceeded.is_empty() {
                findings.push(finding(
                    &component,
                    None,
                    "context_grant_subset",
                    "child_context_exceeds_parent_context",
                    "error",
                    &json!({"agent": component, "used_context": used_context, "granted_context": info.available_context, "exceeded": context_exceeded}),
                    &json!({"agent": component, "exceeded_context": context_exceeded}),
                    &json!([{"kind":"grant_change","weakens_spec":false,"description":"grant only context inside the parent boundary or remove the child read"}]),
                    assumptions,
                ));
            }
        }

        for tool in &node.tools {
            let approval_declared = node
                .authority
                .requires_human_approval
                .iter()
                .any(|rule| rule.name == tool.name);
            if tool.irreversible && !approval_declared {
                findings.push(finding(
                    &component,
                    Some(&tool.name),
                    "human_approval_path",
                    "irreversible_operation_without_human_approval_path",
                    "error",
                    &json!({"agent": component, "tool": tool.name, "irreversible": true, "requires_human_approval": false}),
                    &json!({"agent": component, "tool": tool.name}),
                    &json!([{"kind":"authority_change","weakens_spec":false,"description":format!("add {} to requires_human_approval or route it to a human review state", tool.name)}]),
                    assumptions,
                ));
            }
        }
    }

    for info in infos.values() {
        let parent = info.agent;
        if parent.children.is_empty() {
            continue;
        }
        let child_paths: BTreeMap<String, Vec<String>> = parent
            .children
            .iter()
            .map(|child| {
                let mut path = info.path.clone();
                path.push(child.name.clone());
                (child.name.clone(), path)
            })
            .collect();
        let reachability = reachability(&parent.orchestration, child_paths.keys().cloned());

        for child in &parent.children {
            let source_path = &child_paths[&child.name];
            for output in &child.outputs {
                for target in &output.visibility {
                    if target == "parent" || target == "self" || !child_paths.contains_key(target) {
                        continue;
                    }
                    let reachable_from_child =
                        reachability.get(&child.name).cloned().unwrap_or_default();
                    if !reachable_from_child.contains(target) {
                        findings.push(finding(
                            &path_str(source_path),
                            None,
                            "visibility_requires_delegation",
                            "visibility_leak_across_sibling_agents",
                            "error",
                            &json!({"output": output.name, "source_agent": path_str(source_path), "target_agent": path_str(&child_paths[target]), "delegation_path_exists": false}),
                            &json!({"source_agent": path_str(source_path), "target_agent": path_str(&child_paths[target]), "output": output.name}),
                            &json!([{"kind":"orchestration_change","weakens_spec":false,"description":"add an orchestration path for the declared sibling visibility or remove the visibility target"}]),
                            assumptions,
                        ));
                    }
                }
            }
        }

        for (source_name, reachable) in &reachability {
            let Some(source_info) = by_path.get(&path_str(&child_paths[source_name])) else {
                continue;
            };
            if source_info.agent.trust.as_deref() != Some("low") {
                continue;
            }
            for target_name in reachable {
                let target_path = &child_paths[target_name];
                let target_info = &infos[target_path];
                let high_tools = high_authority_tools(target_info.agent);
                if !high_tools.is_empty() {
                    findings.push(finding(
                        &path_str(&child_paths[source_name]),
                        Some(high_tools[0].as_str()),
                        "tool_reachability_graph",
                        "low_trust_agent_path_to_high_authority_tool",
                        "error",
                        &json!({"source_agent": path_str(&child_paths[source_name]), "source_trust": "low", "target_agent": path_str(target_path), "high_authority_tools": high_tools}),
                        &json!({"source_agent": path_str(&child_paths[source_name]), "target_agent": path_str(target_path)}),
                        &json!([{"kind":"orchestration_change","weakens_spec":false,"description":"route low-trust output through a review gate before it can influence high-authority tools"}]),
                        assumptions,
                    ));
                }
            }
        }

        if !parent.review_gates.is_empty() {
            let gates: BTreeSet<String> = parent.review_gates.iter().cloned().collect();
            for (source_name, reachable) in &reachability {
                if gates.contains(source_name) {
                    continue;
                }
                for target_name in reachable {
                    if gates.contains(target_name) {
                        continue;
                    }
                    let target_path = &child_paths[target_name];
                    let target_info = &infos[target_path];
                    let high_tools = high_authority_tools(target_info.agent);
                    if high_tools.is_empty() {
                        continue;
                    }
                    let has_review_path = gates.iter().any(|gate| {
                        reachable.contains(gate)
                            && reachability
                                .get(gate)
                                .is_some_and(|reach| reach.contains(target_name))
                    });
                    if !has_review_path {
                        findings.push(finding(
                            &path_str(&info.path),
                            Some(high_tools[0].as_str()),
                            "policy_review_gate",
                            "policy_review_bypass_in_orchestration",
                            "error",
                            &json!({"parent_agent": path_str(&info.path), "source_agent": path_str(&child_paths[source_name]), "target_agent": path_str(target_path), "review_gates": gates}),
                            &json!({"parent_agent": path_str(&info.path), "source_agent": path_str(&child_paths[source_name]), "target_agent": path_str(target_path)}),
                            &json!([{"kind":"orchestration_change","weakens_spec":false,"description":"insert the declared review gate on paths to high-authority agents"}]),
                            assumptions,
                        ));
                    }
                }
            }
        }
    }

    findings
}

fn tool_ir(tool: &AiTool) -> Value {
    json!({"name": tool.name, "schema": tool.schema, "irreversible": tool.irreversible, "preconditions": tool.preconditions, "effect": tool.effect})
}

fn authority_ir(authority: &AiAuthority) -> Value {
    json!({
        "may_suggest": authority.may_suggest.iter().map(|rule| rule.name.clone()).collect::<Vec<_>>(),
        "may_execute": authority.may_execute.iter().map(|rule| rule.name.clone()).collect::<Vec<_>>(),
        "requires_human_approval": authority.requires_human_approval.iter().map(|rule| rule.name.clone()).collect::<Vec<_>>(),
        "forbidden": authority.forbidden.iter().map(|rule| rule.name.clone()).collect::<Vec<_>>(),
    })
}

fn failure_ir(policy: &AiFailurePolicy, path: &[String]) -> Value {
    let mut source_path = path.to_vec();
    source_path.push(policy.agent.clone());
    json!({"source": path_str(&source_path), "condition": policy.condition, "action": policy.action, "target": policy.target, "retry_limit": policy.retry_limit})
}

fn agent_ir(agent: &SurfaceAgent, prefix: &[String]) -> Value {
    let mut path = prefix.to_vec();
    path.push(agent.name.clone());
    json!({
        "path": path_str(&path),
        "name": agent.name,
        "model": agent.model,
        "prompt": agent.prompt,
        "trust": agent.trust,
        "context": agent.context,
        "tools": agent.tools.iter().map(tool_ir).collect::<Vec<_>>(),
        "tool_names": agent.tool_names,
        "authority": authority_ir(&agent.authority),
        "grants": agent.grants.iter().map(|grant| json!({"kind": grant.kind, "names": grant.names})).collect::<Vec<_>>(),
        "outputs": agent.outputs.iter().map(|output| json!({"name": output.name, "visibility": output.visibility})).collect::<Vec<_>>(),
        "review_gates": agent.review_gates,
        "orchestration": agent.orchestration.iter().map(|edge| {
            let mut source_path = path.clone();
            source_path.push(edge.source.clone());
            let mut target_path = path.clone();
            target_path.push(edge.target.clone());
            json!({"source": edge.source, "target": edge.target, "source_path": path_str(&source_path), "target_path": path_str(&target_path)})
        }).collect::<Vec<_>>(),
        "failure_policy": agent.failure_policy.iter().map(|policy| failure_ir(policy, &path)).collect::<Vec<_>>(),
        "contracts": agent.contracts.iter().map(|contract| json!({"hard_rules": contract.hard_rules})).collect::<Vec<_>>(),
        "children": agent.children.iter().map(|child| agent_ir(child, &path)).collect::<Vec<_>>(),
    })
}

fn visibility_target_path(
    path: &[String],
    parent: Option<&[String]>,
    target: &str,
) -> Option<String> {
    if target == "self" {
        return Some(path_str(path));
    }
    if target == "parent" {
        return parent.map(path_str);
    }
    if let Some(parent) = parent {
        let mut extended = parent.to_vec();
        extended.push(target.to_owned());
        Some(path_str(&extended))
    } else {
        let mut extended = path.to_vec();
        extended.push(target.to_owned());
        Some(path_str(&extended))
    }
}

fn graph_summary(infos: &BTreeMap<Vec<String>, AgentInfo<'_>>) -> Value {
    let mut scope_tree = Vec::new();
    let mut authority_graph = Vec::new();
    let mut information_flow_graph = Vec::new();
    let mut delegation_graph = Vec::new();
    let mut tool_reachability_graph = Vec::new();
    let mut failure_policies = Vec::new();

    for info in infos.values() {
        let node = info.agent;
        let path = &info.path;
        let child_paths: BTreeMap<String, Vec<String>> = node
            .children
            .iter()
            .map(|child| {
                let mut p = path.clone();
                p.push(child.name.clone());
                (child.name.clone(), p)
            })
            .collect();
        scope_tree.push(json!({
            "path": path_str(path),
            "parent": info.parent.as_ref().map(|parent| path_str(parent)),
            "children": node.children.iter().map(|child| path_str(&child_paths[&child.name])).collect::<Vec<_>>(),
        }));
        if let Some(parent) = &info.parent {
            authority_graph.push(json!({
                "agent": path_str(path),
                "parent": path_str(parent),
                "granted_authority": info.available_authority,
                "granted_context": info.available_context,
            }));
        }
        for edge in &node.orchestration {
            let mut source_path = path.clone();
            source_path.push(edge.source.clone());
            let mut target_path = path.clone();
            target_path.push(edge.target.clone());
            delegation_graph.push(json!({
                "parent": path_str(path),
                "source": path_str(&source_path),
                "target": path_str(&target_path),
            }));
        }
        for output in &node.outputs {
            for target in &output.visibility {
                information_flow_graph.push(json!({
                    "source": format!("{}.output.{}", path_str(path), output.name),
                    "target": visibility_target_path(path, info.parent.as_deref(), target),
                }));
            }
        }
        for (tool_name, tool) in tool_map(node) {
            tool_reachability_graph.push(json!({
                "agent": path_str(path),
                "tool": tool_name,
                "irreversible": tool.irreversible,
                "requires_human_approval": node.authority.requires_human_approval.iter().any(|rule| rule.name == tool_name),
                "may_execute": node.authority.may_execute.iter().any(|rule| rule.name == tool_name),
                "may_suggest": node.authority.may_suggest.iter().any(|rule| rule.name == tool_name),
            }));
        }
        for policy in &node.failure_policy {
            failure_policies.push(failure_ir(policy, path));
        }
    }

    json!({
        "scope_tree": scope_tree,
        "delegation_graph": delegation_graph,
        "authority_graph": authority_graph,
        "information_flow_graph": information_flow_graph,
        "tool_reachability_graph": tool_reachability_graph,
        "failure_policy": failure_policies,
    })
}

/// Analyze a recursive `agent` document: validate the lexical tree (grant
/// boundaries, unknown orchestration/`review_gate`/`failure_policy` child
/// references, output visibility targets -- all check-time errors) and
/// compute the six structural finding kinds.
///
/// # Errors
///
/// Returns [`AgentError`] for a tree-validation failure (see above); this is
/// a `kind:"semantics"` check-time error at the CLI layer, not a finding.
pub fn analyze_ai_agent(agent: &SurfaceAgent) -> Result<Value, AgentError> {
    let infos_vec = walk(agent, None)?;
    let infos: BTreeMap<Vec<String>, AgentInfo<'_>> = infos_vec
        .into_iter()
        .map(|info| (info.path.clone(), info))
        .collect();
    let assumptions = agent_assumptions();
    let findings = agent_findings(&infos, &assumptions);
    let result = if findings.is_empty() {
        "agent_analyzed"
    } else {
        "violated"
    };
    Ok(json!({
        "result": result,
        "dialect": AI_AGENT_DIALECT_VERSION,
        "finding_schema_version": AI_FINDING_SCHEMA_VERSION,
        "ai_agent": agent.name,
        "formal_result": "not_run",
        "evidence": {"kind": "static_agent_graph", "formal_proof": false},
        "guarantee_boundary": {
            "proved": "not claimed for recursive agent composition",
            "agent_structural": "lexical scope, grant subset, delegation, visibility, failure policy, and tool-reachability structure",
            "runtime_replay": "not run for agent composition",
            "evaluator_supported": "outside this structural analysis",
            "statistically_supported": "outside this structural analysis",
        },
        "assumptions": assumptions,
        "agent_ir": agent_ir(agent, &[]),
        "graph_summary": graph_summary(&infos),
        "findings": findings,
        "note": "recursive agent analysis is structural only; it does not prove LLM semantic correctness or statistical/evaluator-backed quality claims",
    }))
}

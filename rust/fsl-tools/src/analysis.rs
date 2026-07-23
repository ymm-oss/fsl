// SPDX-License-Identifier: Apache-2.0

//! Deterministic Typed Semantic Graph projection for checked kernel models.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{
    Annotations, KernelBinder as Binder, KernelExpr as Expr, KernelLValue as LValue, KernelModel,
    KernelStatement as Statement, TypeDef, TypeRef,
};
use fsl_syntax::MetaTag;
use serde_json::{Map, Value, json};

use crate::analysis_graph;

/// Build one structured, review-only analysis finding.
#[must_use]
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub fn review_finding(
    finding_type: &str,
    confidence: f64,
    involved_nodes: Value,
    witness: Value,
    why: &str,
    repairs: Value,
    caveats: Value,
    loc: Option<Value>,
) -> Value {
    let mut finding = json!({
        "finding_id":"",
        "analysis":"structure",
        "finding_type":finding_type,
        "severity":"review_required",
        "confidence":confidence,
        "formal_status":"not_a_violation",
        "involved_nodes":involved_nodes,
        "witness":witness,
        "why_it_matters":why,
        "candidate_repairs":repairs,
        "do_not_assume":caveats,
    });
    if let (Some(loc), Value::Object(object)) = (loc, &mut finding) {
        object.insert("loc".to_owned(), loc);
    }
    finding
}

fn tsg_nodes(tsg: &Value) -> std::collections::BTreeMap<String, Value> {
    tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|node| Some((node.get("id")?.as_str()?.to_owned(), node.clone())))
        .collect()
}

/// Derive deterministic structural review findings from a TSG projection.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn structural_review_findings(tsg: &Value) -> Vec<Value> {
    let nodes = tsg_nodes(tsg);
    let edges = tsg["edges"].as_array().cloned().unwrap_or_default();
    let property_kinds = ["invariant", "trans", "leadsTo", "reachable"]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let scenario_kinds = ["acceptance", "forbidden"]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let mut findings = Vec::new();

    for requirement in nodes.values().filter(|node| node["kind"] == "requirement") {
        let id = requirement["id"].as_str().unwrap_or_default();
        let useful = edges.iter().any(|edge| {
            edge["kind"] == "covers"
                && edge["from"] == id
                && nodes
                    .get(edge["to"].as_str().unwrap_or_default())
                    .is_some_and(|node| {
                        let kind = node["kind"].as_str().unwrap_or_default();
                        property_kinds.contains(kind)
                            || scenario_kinds.contains(kind)
                            || matches!(kind, "action" | "kpi" | "control")
                    })
        });
        if !useful {
            findings.push(review_finding(
                "disconnected_requirement",
                0.8,
                json!([id]),
                json!({"kind":"isolated_node","node":id}),
                "The requirement is declared but is not connected to an action, property, acceptance scenario, forbidden scenario, governance control, or refinement mapping in the structural graph.",
                json!([{"kind":"add_traceability_anchor","template":"Attach the requirement id to a relevant action/property or add an acceptance/forbidden scenario."}]),
                json!(["The requirement is invalid.","The implementation is missing behavior."]),
                None,
            ));
        }
    }

    let action_states = edges
        .iter()
        .filter(|edge| matches!(edge["kind"].as_str(), Some("reads" | "writes")))
        .filter(|edge| {
            nodes
                .get(edge["from"].as_str().unwrap_or_default())
                .is_some_and(|node| node["kind"] == "action")
                && nodes
                    .get(edge["to"].as_str().unwrap_or_default())
                    .is_some_and(|node| node["kind"] == "state")
        })
        .filter_map(|edge| edge["to"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    let scenario_ids = nodes
        .values()
        .filter(|node| scenario_kinds.contains(node["kind"].as_str().unwrap_or_default()))
        .filter_map(|node| node["id"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    let scenario_actions = edges.iter().any(|edge| {
        scenario_ids.contains(edge["from"].as_str().unwrap_or_default())
            && nodes
                .get(edge["to"].as_str().unwrap_or_default())
                .is_some_and(|node| node["kind"] == "action")
    });
    let scenario_states = edges
        .iter()
        .filter(|edge| scenario_ids.contains(edge["from"].as_str().unwrap_or_default()))
        .filter(|edge| {
            nodes
                .get(edge["to"].as_str().unwrap_or_default())
                .is_some_and(|node| node["kind"] == "state")
        })
        .filter_map(|edge| edge["to"].as_str().map(str::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    let related = action_states
        .union(&scenario_states)
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    for property in nodes
        .values()
        .filter(|node| property_kinds.contains(node["kind"].as_str().unwrap_or_default()))
    {
        if property.get("meta").is_some() {
            continue;
        }
        let id = property["id"].as_str().unwrap_or_default();
        let reads = edges
            .iter()
            .filter(|edge| {
                edge["from"] == id && matches!(edge["kind"].as_str(), Some("reads" | "checks"))
            })
            .filter(|edge| {
                nodes
                    .get(edge["to"].as_str().unwrap_or_default())
                    .is_some_and(|node| node["kind"] == "state")
            })
            .filter_map(|edge| edge["to"].as_str().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>();
        if (!reads.is_empty() && !reads.is_disjoint(&related))
            || (scenario_actions && property["kind"] == "reachable")
        {
            continue;
        }
        findings.push(review_finding(
            "unanchored_property",
            0.7,
            json!([id]),
            json!({"kind":"unanchored_node","node":id,"reads":reads}),
            "The user property is not connected to requirement metadata, scenarios, governance metadata, or an action-state anchor in the structural graph.",
            json!([{"kind":"add_traceability_anchor","template":"Attach a requirement tag or add a scenario/action-state anchor that explains why this property exists."}]),
            json!(["The property is wrong.","The property should be deleted."]),
            None,
        ));
    }

    let mut written = std::collections::BTreeSet::new();
    let mut read = std::collections::BTreeSet::new();
    for edge in &edges {
        if !nodes
            .get(edge["to"].as_str().unwrap_or_default())
            .is_some_and(|node| node["kind"] == "state")
        {
            continue;
        }
        match edge["kind"].as_str() {
            Some("writes") => {
                written.insert(edge["to"].as_str().unwrap_or_default().to_owned());
            }
            Some("reads" | "checks") => {
                read.insert(edge["to"].as_str().unwrap_or_default().to_owned());
            }
            _ => {}
        }
    }
    for state in nodes.values().filter(|node| node["kind"] == "state") {
        let id = state["id"].as_str().unwrap_or_default();
        if written.contains(id) {
            continue;
        }
        let readers = edges
            .iter()
            .filter(|edge| {
                edge["to"] == id && matches!(edge["kind"].as_str(), Some("reads" | "checks"))
            })
            .filter_map(|edge| edge["from"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        findings.push(review_finding(
            "unwritten_state",
            if read.contains(id) { 0.76 } else { 0.68 },
            json!([id]),
            json!({"kind":"state_has_no_action_writes","node":id,"read_by":readers}),
            "The state variable is initialized but no action writes it in the structural graph.",
            json!([{"kind":"review_state_role","template":"Make the value a const/model parameter if it is intentionally fixed, or add the missing action/effect that changes it."}]),
            json!(["The state variable is useless.","A verifier property is violated.","The variable is safe to delete without checking generated dialect state."]),
            state.get("loc").cloned(),
        ));
    }

    let relevance_seeds = [
        "invariant",
        "trans",
        "leadsTo",
        "reachable",
        "acceptance",
        "forbidden",
        "guard",
        "ensures",
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    let mut relevant = std::collections::BTreeSet::new();
    let mut effect_targets = std::collections::BTreeMap::new();
    let mut effect_reads =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for node in nodes.values().filter(|node| node["kind"] == "effect") {
        if let (Some(id), Some(target)) = (node["id"].as_str(), node["target"].as_str()) {
            effect_targets.insert(id.to_owned(), format!("state:{target}"));
        }
    }
    for edge in &edges {
        let Some(target) = edge["to"].as_str() else {
            continue;
        };
        if !nodes
            .get(target)
            .is_some_and(|node| node["kind"] == "state")
        {
            continue;
        }
        let Some(source) = edge["from"].as_str() else {
            continue;
        };
        let source_kind = nodes
            .get(source)
            .and_then(|node| node["kind"].as_str())
            .unwrap_or_default();
        if matches!(edge["kind"].as_str(), Some("reads" | "checks"))
            && relevance_seeds.contains(source_kind)
        {
            relevant.insert(target.to_owned());
        }
        if edge["kind"] == "reads" && source_kind == "effect" {
            effect_reads
                .entry(source.to_owned())
                .or_default()
                .insert(target.to_owned());
        }
    }
    loop {
        let mut changed = false;
        for (effect, target) in &effect_targets {
            if !relevant.contains(target) {
                continue;
            }
            for read in effect_reads.get(effect).into_iter().flatten() {
                changed |= relevant.insert(read.clone());
            }
        }
        if !changed {
            break;
        }
    }
    let mut writers =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for edge in &edges {
        if edge["kind"] != "writes" {
            continue;
        }
        let (Some(source), Some(target)) = (edge["from"].as_str(), edge["to"].as_str()) else {
            continue;
        };
        if !nodes
            .get(target)
            .is_some_and(|node| node["kind"] == "state")
        {
            continue;
        }
        let writer = nodes.get(source).and_then(|node| {
            if node["kind"] == "action" {
                Some(source)
            } else {
                node["action"].as_str()
            }
        });
        if let Some(writer) = writer {
            writers
                .entry(target.to_owned())
                .or_default()
                .insert(writer.to_owned());
        }
    }
    let relevance_seed_kinds = relevance_seeds.into_iter().collect::<Vec<_>>();
    for state in nodes.values().filter(|node| node["kind"] == "state") {
        let id = state["id"].as_str().unwrap_or_default();
        let state_writers = writers.get(id).cloned().unwrap_or_default();
        if state_writers.is_empty()
            || relevant.contains(id)
            || state_writers.iter().any(|writer| {
                nodes
                    .get(writer)
                    .is_some_and(|node| node.get("meta").is_some())
            })
        {
            continue;
        }
        findings.push(review_finding(
            "unread_state",
            0.64,
            json!([id]),
            json!({
                "kind":"state_influences_no_check",
                "node":id,
                "writers":state_writers,
                "relevance_seed_kinds":relevance_seed_kinds,
                "message":"No transitive relevance chain reaches a guard, property, ensures clause, or scenario.",
            }),
            "The state variable is written, but its value does not transitively influence a guard, property, ensures clause, or acceptance/forbidden scenario in the structural graph.",
            json!([
                {"kind":"add_property_or_guard","template":"Add the missing invariant/trans/leadsTo/reachable, scenario expectation, ensures clause, or guard that consumes this state if it is part of the contract."},
                {"kind":"review_state_role","template":"If this is intentional audit/history/ghost state, tag or document the writing action so reviewers know why the state is externally consumed."}
            ]),
            json!([
                "The state variable is safe to delete.",
                "The value is semantically irrelevant to external tooling, runtime logs, audit requirements, or generated dialect behavior.",
                "A verifier property is violated."
            ]),
            state.get("loc").cloned(),
        ));
    }
    findings
}

fn display(name: &str) -> String {
    name.replacen("__", ".", 1)
}

#[allow(clippy::needless_pass_by_value)]
fn node(id: String, kind: &str, name: Option<String>, loc: Option<Value>) -> Map<String, Value> {
    let mut result = Map::new();
    result.insert("id".to_owned(), json!(id));
    result.insert("kind".to_owned(), json!(kind));
    if let Some(name) = name {
        result.insert("label".to_owned(), json!(display(&name)));
        result.insert("name".to_owned(), json!(name));
    }
    if let Some(loc) = loc {
        result.insert("loc".to_owned(), loc);
    }
    result
}

fn edge(from: &str, kind: &str, to: &str) -> Value {
    json!({
        "id":format!("edge:{from}:{kind}:{to}"),
        "kind":kind,
        "from":from,
        "to":to,
    })
}

fn requirement_metadata(annotations: &Annotations, legacy: Option<&MetaTag>) -> Vec<Value> {
    let requirements = annotations
        .requirements()
        .expect("checked model annotations are valid")
        .into_iter()
        .map(|requirement| json!({"id":requirement.id,"text":requirement.text}))
        .collect::<Vec<_>>();
    if requirements.is_empty() {
        legacy
            .filter(|meta| !meta.id.eq_ignore_ascii_case("undecided"))
            .map_or_else(Vec::new, |meta| {
                vec![json!({"id":meta.id,"text":meta.text})]
            })
    } else {
        requirements
    }
}

fn add_requirement_metadata(
    node: &mut Map<String, Value>,
    annotations: &Annotations,
    legacy: Option<&MetaTag>,
) {
    let requirements = requirement_metadata(annotations, legacy);
    if let Some(first) = requirements.first() {
        node.insert("meta".to_owned(), first.clone());
        node.insert("requirements".to_owned(), Value::Array(requirements));
    }
}

fn public_type(model: &KernelModel, ty: &TypeRef) -> Value {
    match ty {
        TypeRef::Int => json!(["int"]),
        TypeRef::Bool => json!(["bool"]),
        TypeRef::Range(lo, hi) => json!(["domain", lo, hi]),
        TypeRef::Named(name) => match model.types.get(name) {
            Some(TypeDef::Domain { lo, hi, .. }) => json!(["domain", lo, hi]),
            Some(TypeDef::Enum { .. }) => json!(["enum", display(name)]),
            Some(TypeDef::Struct { .. }) => json!(["struct", display(name)]),
            None => json!(["named", display(name)]),
        },
        TypeRef::Map(key, value) => {
            json!(["map", public_type(model, key), public_type(model, value)])
        }
        TypeRef::Relation(left, right) => {
            json!([
                "relation",
                public_type(model, left),
                public_type(model, right)
            ])
        }
        TypeRef::Set(value) => json!(["set", public_type(model, value)]),
        TypeRef::Seq(value, cap) => json!(["seq", public_type(model, value), cap]),
        TypeRef::Option(value) => json!(["option", public_type(model, value)]),
    }
}

fn binder_reads(
    binder: &Binder,
    state: &BTreeSet<String>,
    bound: &BTreeSet<String>,
) -> BTreeSet<String> {
    match binder {
        Binder::Typed { where_expr, .. } => where_expr
            .as_deref()
            .map_or_else(BTreeSet::new, |expr| expr_reads_bound(expr, state, bound)),
        Binder::Range {
            lo, hi, where_expr, ..
        } => {
            let mut reads = expr_reads_bound(lo, state, bound);
            reads.extend(expr_reads_bound(hi, state, bound));
            if let Some(expr) = where_expr {
                reads.extend(expr_reads_bound(expr, state, bound));
            }
            reads
        }
        Binder::Collection {
            collection,
            where_expr,
            ..
        } => {
            let mut reads = expr_reads_bound(collection, state, bound);
            if let Some(expr) = where_expr {
                reads.extend(expr_reads_bound(expr, state, bound));
            }
            reads
        }
    }
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}

#[allow(clippy::too_many_lines)]
fn expr_reads_bound(
    expr: &Expr,
    state: &BTreeSet<String>,
    bound: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut reads = BTreeSet::new();
    match expr {
        Expr::Var(name) => {
            if state.contains(name) && !bound.contains(name) {
                reads.insert(name.clone());
            }
        }
        Expr::Some(value)
        | Expr::Neg(value)
        | Expr::Not(value)
        | Expr::Field(value, _)
        | Expr::Stage { entity: value, .. } => {
            reads.extend(expr_reads_bound(value, state, bound));
        }
        Expr::Index(base, index)
        | Expr::BinaryNamed {
            left: base,
            right: index,
            ..
        } => {
            reads.extend(expr_reads_bound(base, state, bound));
            reads.extend(expr_reads_bound(index, state, bound));
        }
        Expr::Method { receiver, args, .. } => {
            reads.extend(expr_reads_bound(receiver, state, bound));
            for arg in args {
                reads.extend(expr_reads_bound(arg, state, bound));
            }
        }
        Expr::Binary { left, right, .. } => {
            reads.extend(expr_reads_bound(left, state, bound));
            reads.extend(expr_reads_bound(right, state, bound));
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            reads.extend(expr_reads_bound(condition, state, bound));
            reads.extend(expr_reads_bound(then_expr, state, bound));
            reads.extend(expr_reads_bound(else_expr, state, bound));
        }
        Expr::Is { expr, .. } | Expr::UnaryNamed { expr, .. } => {
            reads.extend(expr_reads_bound(expr, state, bound));
        }
        Expr::Set(values) | Expr::Seq(values) => {
            for value in values {
                reads.extend(expr_reads_bound(value, state, bound));
            }
        }
        Expr::Struct { fields, .. } => {
            for (_, value) in fields {
                reads.extend(expr_reads_bound(value, state, bound));
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                reads.extend(expr_reads_bound(arg, state, bound));
            }
        }
        Expr::Quantified { binder, body, .. } => {
            let mut next = bound.clone();
            next.insert(binder_name(binder).to_owned());
            reads.extend(binder_reads(binder, state, &next));
            reads.extend(expr_reads_bound(body, state, &next));
        }
        Expr::Aggregate { binder, value, .. } => {
            let mut next = bound.clone();
            next.insert(binder_name(binder).to_owned());
            reads.extend(binder_reads(binder, state, &next));
            if let Some(value) = value {
                reads.extend(expr_reads_bound(value, state, &next));
            }
        }
        Expr::TernaryNamed {
            first,
            second,
            third,
            ..
        } => {
            reads.extend(expr_reads_bound(first, state, bound));
            reads.extend(expr_reads_bound(second, state, bound));
            reads.extend(expr_reads_bound(third, state, bound));
        }
        Expr::Num(_) | Expr::Bool(_) | Expr::None => {}
    }
    reads
}

fn expr_reads(expr: &Expr, state: &BTreeSet<String>) -> BTreeSet<String> {
    expr_reads_bound(expr, state, &BTreeSet::new())
}

fn lvalue_root(value: &LValue) -> &str {
    match value {
        LValue::Var(name) | LValue::Index(name, _) => name,
        LValue::Field(base, _) => lvalue_root(base),
    }
}

fn counter_delta(name: &str, expr: &Expr, model: &KernelModel) -> Option<i64> {
    fn scalar(expr: &Expr, model: &KernelModel) -> Option<i64> {
        match expr {
            Expr::Num(value) => Some(*value),
            Expr::Var(name) => match model.consts.get(name) {
                Some(fsl_core::FslValue::Int(value)) => Some(*value),
                _ => None,
            },
            Expr::Neg(value) => scalar(value, model).map(|value| -value),
            _ => None,
        }
    }
    let Expr::Binary { op, left, right } = expr else {
        return None;
    };
    match op.as_str() {
        "+" if matches!(left.as_ref(), Expr::Var(value) if value == name) => scalar(right, model),
        "+" if matches!(right.as_ref(), Expr::Var(value) if value == name) => scalar(left, model),
        "-" if matches!(left.as_ref(), Expr::Var(value) if value == name) => {
            scalar(right, model).map(|value| -value)
        }
        _ => None,
    }
}

fn scan_counter_statements(
    statements: &[Statement],
    counters: &BTreeSet<String>,
    model: &KernelModel,
    nested: bool,
    deltas: &mut BTreeMap<String, i64>,
    excluded: &mut BTreeSet<String>,
) {
    for statement in statements {
        match statement {
            Statement::Assign { target, value, .. } => {
                let root = lvalue_root(target);
                if !counters.contains(root) {
                    continue;
                }
                if nested || !matches!(target, LValue::Var(name) if name == root) {
                    excluded.insert(root.to_owned());
                } else if let Some(delta) = counter_delta(root, value, model) {
                    *deltas.entry(root.to_owned()).or_default() += delta;
                } else {
                    excluded.insert(root.to_owned());
                }
            }
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                scan_counter_statements(then_statements, counters, model, true, deltas, excluded);
                scan_counter_statements(else_statements, counters, model, true, deltas, excluded);
            }
            Statement::ForAll { statements, .. } => {
                scan_counter_statements(statements, counters, model, true, deltas, excluded);
            }
        }
    }
}

fn integer_gcd(mut left: i64, mut right: i64) -> i64 {
    left = left.abs();
    right = right.abs();
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left.max(1)
}

fn weighted_sum_text(weights: &BTreeMap<String, i64>) -> String {
    let mut parts = Vec::new();
    for (name, weight) in weights {
        if *weight == 0 {
            continue;
        }
        let term = if weight.abs() == 1 {
            name.clone()
        } else {
            format!("{}*{name}", weight.abs())
        };
        if parts.is_empty() {
            parts.push(if *weight > 0 {
                term
            } else {
                format!("-{term}")
            });
        } else {
            parts.push(if *weight > 0 {
                format!("+ {term}")
            } else {
                format!("- {term}")
            });
        }
    }
    parts.join(" ")
}

/// Derive deterministic, review-only weighted-sum conservation candidates.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn conservation_review_findings(model: &KernelModel) -> Vec<Value> {
    let counters = model
        .state
        .iter()
        .filter(|(_, ty)| matches!(ty, TypeRef::Int))
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    if counters.len() < 2 {
        return Vec::new();
    }
    let mut excluded = BTreeSet::new();
    let mut actions = model.actions.iter().collect::<Vec<_>>();
    actions.sort_by_key(|action| &action.name);
    let mut rows = Vec::new();
    for action in actions {
        let mut deltas = BTreeMap::new();
        scan_counter_statements(
            &action.statements,
            &counters,
            model,
            false,
            &mut deltas,
            &mut excluded,
        );
        rows.push((format!("action:{}", action.name), deltas));
    }
    let eligible = counters
        .iter()
        .filter(|counter| {
            !excluded.contains(*counter)
                && rows
                    .iter()
                    .any(|(_, row)| row.get(*counter).copied().unwrap_or_default() != 0)
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut findings = Vec::new();
    for left in 0..eligible.len() {
        for right in left + 1..eligible.len() {
            let first = rows.iter().find_map(|(_, row)| {
                let a = row.get(&eligible[left]).copied().unwrap_or_default();
                let b = row.get(&eligible[right]).copied().unwrap_or_default();
                (a != 0 || b != 0).then_some((a, b))
            });
            let Some((a, b)) = first else {
                continue;
            };
            let divisor = integer_gcd(a, b);
            let mut left_weight = b / divisor;
            let mut right_weight = -a / divisor;
            if left_weight < 0 || (left_weight == 0 && right_weight < 0) {
                left_weight = -left_weight;
                right_weight = -right_weight;
            }
            if left_weight == 0
                || right_weight == 0
                || rows.iter().any(|(_, row)| {
                    left_weight * row.get(&eligible[left]).copied().unwrap_or_default()
                        + right_weight * row.get(&eligible[right]).copied().unwrap_or_default()
                        != 0
                })
            {
                continue;
            }
            let weights = BTreeMap::from([
                (eligible[left].clone(), left_weight),
                (eligible[right].clone(), right_weight),
            ]);
            let action_effects = rows
                .iter()
                .filter_map(|(action, row)| {
                    let deltas = weights
                        .keys()
                        .filter_map(|name| {
                            let delta = row.get(name).copied().unwrap_or_default();
                            (delta != 0).then_some((name.clone(), delta))
                        })
                        .collect::<BTreeMap<_, _>>();
                    (!deltas.is_empty()).then_some(json!({
                        "action":action,
                        "deltas":deltas,
                        "weighted_sum_delta":deltas.iter().map(|(name,delta)|weights[name]*delta).sum::<i64>(),
                    }))
                })
                .collect::<Vec<_>>();
            if action_effects.len() < 2 {
                continue;
            }
            let expression = weighted_sum_text(&weights);
            let involved = weights
                .keys()
                .map(|name| format!("state:{name}"))
                .chain(
                    action_effects
                        .iter()
                        .filter_map(|item| item["action"].as_str().map(str::to_owned)),
                )
                .collect::<BTreeSet<_>>();
            findings.push(review_finding(
                "conservation_candidate",
                0.6,
                json!(involved),
                json!({
                    "kind":"weighted_sum_conservation_candidate",
                    "expression":expression,
                    "weights":weights,
                    "action_net_effects":action_effects,
                    "excluded_counters":excluded,
                }),
                "Counter-like effects structurally preserve this weighted sum, which may indicate an implicit invariant worth declaring and proving.",
                json!([{"kind":"add_invariant_then_verify","template":format!("Declare `invariant Conservation {{ {expression} == <initial value> }}` and run `fslc verify` plus `--engine induction` to prove it.")}]),
                json!(["The weighted sum is actually invariant.","The absence of a candidate means no conservation law exists.","This finding is a proof; it is only structural evidence and must be checked by verify."]),
                None,
            ));
        }
    }
    findings.truncate(8);
    findings
}

fn lvalue_reads(value: &LValue, state: &BTreeSet<String>) -> BTreeSet<String> {
    match value {
        LValue::Index(_, index) => expr_reads(index, state),
        LValue::Field(base, _) => lvalue_reads(base, state),
        LValue::Var(_) => BTreeSet::new(),
    }
}

fn statement_reads(statements: &[Statement], state: &BTreeSet<String>) -> BTreeSet<String> {
    let mut reads = BTreeSet::new();
    for statement in statements {
        match statement {
            Statement::Assign { target, value, .. } => {
                reads.extend(lvalue_reads(target, state));
                reads.extend(expr_reads(value, state));
            }
            Statement::If {
                condition,
                then_statements,
                else_statements,
                ..
            } => {
                reads.extend(expr_reads(condition, state));
                reads.extend(statement_reads(then_statements, state));
                reads.extend(statement_reads(else_statements, state));
            }
            Statement::ForAll {
                binder, statements, ..
            } => {
                reads.extend(binder_reads(
                    binder,
                    state,
                    &BTreeSet::from([binder_name(binder).to_owned()]),
                ));
                reads.extend(statement_reads(statements, state));
            }
        }
    }
    reads
}

fn assignments<'a>(statements: &'a [Statement], output: &mut Vec<(&'a LValue, &'a Expr, Value)>) {
    for statement in statements {
        match statement {
            Statement::Assign {
                target,
                value,
                span,
            } => output.push((target, value, span.python_loc())),
            Statement::If {
                then_statements,
                else_statements,
                ..
            } => {
                assignments(then_statements, output);
                assignments(else_statements, output);
            }
            Statement::ForAll { statements, .. } => assignments(statements, output),
        }
    }
}

fn physical_names(model: &KernelModel, logical: &str, ty: &TypeRef) -> Vec<String> {
    fn struct_fields(model: &KernelModel, logical: &str, name: &str) -> Vec<String> {
        match model.types.get(name) {
            Some(TypeDef::Struct { fields }) => fields
                .iter()
                .flat_map(|(field, ty)| match ty {
                    TypeRef::Option(_) => vec![
                        format!("{logical}__{field}__present"),
                        format!("{logical}__{field}__value"),
                    ],
                    _ => vec![format!("{logical}__{field}")],
                })
                .collect(),
            _ => vec![logical.to_owned()],
        }
    }
    match ty {
        TypeRef::Option(_) => vec![format!("{logical}__present"), format!("{logical}__value")],
        TypeRef::Seq(_, _) => vec![format!("{logical}__data"), format!("{logical}__len")],
        TypeRef::Named(name) if matches!(model.types.get(name), Some(TypeDef::Struct { .. })) => {
            struct_fields(model, logical, name)
        }
        TypeRef::Map(_, value) => match value.as_ref() {
            TypeRef::Option(_) => vec![format!("{logical}__present"), format!("{logical}__value")],
            TypeRef::Named(name)
                if matches!(model.types.get(name), Some(TypeDef::Struct { .. })) =>
            {
                struct_fields(model, logical, name)
            }
            _ => vec![logical.to_owned()],
        },
        _ => vec![logical.to_owned()],
    }
}

struct Builder<'a> {
    model: &'a KernelModel,
    state: BTreeSet<String>,
    nodes: BTreeMap<String, Value>,
    edges: BTreeMap<String, Value>,
}

impl<'a> Builder<'a> {
    fn new(model: &'a KernelModel) -> Self {
        Self {
            model,
            state: model.state.iter().map(|(name, _)| name.clone()).collect(),
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
        }
    }
    fn add_node(&mut self, value: Map<String, Value>, declared: bool) {
        let id = value["id"].as_str().unwrap_or_default().to_owned();
        self.nodes.insert(id.clone(), Value::Object(value));
        if declared {
            self.add_edge(edge(&format!("spec:{}", self.model.name), "declares", &id));
        }
    }
    fn add_edge(&mut self, value: Value) {
        if let Some(id) = value["id"].as_str() {
            self.edges.insert(id.to_owned(), value);
        }
    }
    fn add_reads(&mut self, source: &str, reads: &BTreeSet<String>) {
        for name in reads {
            self.add_edge(edge(source, "reads", &format!("state:{name}")));
        }
    }
    fn add_checks(&mut self, source: &str, reads: &BTreeSet<String>) {
        for name in reads {
            self.add_edge(edge(source, "checks", &format!("state:{name}")));
        }
    }
    fn add_writes(&mut self, source: &str, root: &str) {
        if self.state.contains(root) {
            self.add_edge(edge(source, "writes", &format!("state:{root}")));
        }
    }

    #[allow(clippy::too_many_lines)]
    fn build(mut self) -> Value {
        self.add_node(
            node(
                format!("spec:{}", self.model.name),
                "spec",
                Some(self.model.name.clone()),
                None,
            ),
            false,
        );
        for (name, ty) in &self.model.state {
            let mut value = node(format!("state:{name}"), "state", Some(name.clone()), None);
            value.insert("type".to_owned(), public_type(self.model, ty));
            self.add_node(value, true);
            for physical in physical_names(self.model, name, ty) {
                let mut value = node(
                    format!("phys_state:{physical}"),
                    "phys_state",
                    Some(physical.clone()),
                    None,
                );
                value.insert("label".to_owned(), json!(physical));
                value.insert("logical".to_owned(), json!(name));
                self.add_node(value, true);
                self.add_edge(edge(
                    &format!("state:{name}"),
                    "expands_to",
                    &format!("phys_state:{physical}"),
                ));
            }
        }
        for action in &self.model.actions {
            let name = &action.name;
            let action_id = format!("action:{name}");
            let mut value = node(
                action_id.clone(),
                "action",
                Some(name.clone()),
                Some(action.span.python_loc()),
            );
            value.insert("fair".to_owned(), json!(action.fair));
            value.insert("sync".to_owned(), json!(false));
            add_requirement_metadata(&mut value, &action.annotations, action.meta.as_ref());
            self.add_node(value, true);
            let mut action_reads = BTreeSet::new();
            for (index, requirement) in action.requires.iter().enumerate() {
                let id = format!("guard:{name}:{index}");
                let mut value = node(
                    id.clone(),
                    "guard",
                    Some(format!("{name}:{index}")),
                    action
                        .require_spans
                        .get(index)
                        .map(|span| span.python_loc()),
                );
                value.insert(
                    "label".to_owned(),
                    json!(format!("{} requires {index}", display(name))),
                );
                value.insert("expr".to_owned(), requirement.python_ast());
                value.insert("action".to_owned(), json!(action_id));
                self.add_node(value, false);
                self.add_edge(edge(&action_id, "has_guard", &id));
                let reads = expr_reads(requirement, &self.state);
                action_reads.extend(reads.clone());
                self.add_reads(&id, &reads);
            }
            let mut effects = Vec::new();
            assignments(&action.statements, &mut effects);
            for (index, (target, expr, loc)) in effects.into_iter().enumerate() {
                let id = format!("effect:{name}:{index}");
                let root = lvalue_root(target);
                let mut value = node(
                    id.clone(),
                    "effect",
                    Some(format!("{name}:{index}")),
                    Some(loc),
                );
                value.insert(
                    "label".to_owned(),
                    json!(format!("{} effect {index}", display(name))),
                );
                value.insert("expr".to_owned(), expr.python_ast());
                value.insert("action".to_owned(), json!(action_id));
                value.insert("target".to_owned(), json!(root));
                self.add_node(value, false);
                self.add_edge(edge(&action_id, "has_effect", &id));
                self.add_writes(&action_id, root);
                self.add_writes(&id, root);
                self.add_reads(&id, &expr_reads(expr, &self.state));
            }
            action_reads.extend(statement_reads(&action.statements, &self.state));
            for (index, ensures) in action.ensures.iter().enumerate() {
                let id = format!("ensures:{name}:{index}");
                let mut value = node(
                    id.clone(),
                    "ensures",
                    Some(format!("{name}:{index}")),
                    action.ensure_spans.get(index).map(|span| span.python_loc()),
                );
                value.insert(
                    "label".to_owned(),
                    json!(format!("{} ensures {index}", display(name))),
                );
                value.insert("expr".to_owned(), ensures.python_ast());
                value.insert("action".to_owned(), json!(action_id));
                self.add_node(value, false);
                self.add_edge(edge(&action_id, "has_ensures", &id));
                let reads = expr_reads(ensures, &self.state);
                action_reads.extend(reads.clone());
                self.add_reads(&id, &reads);
            }
            self.add_reads(&action_id, &action_reads);
        }
        for (kind, items) in [
            ("invariant", &self.model.invariants),
            ("trans", &self.model.transitions),
            ("reachable", &self.model.reachables),
        ] {
            for property in items {
                let id = format!("{kind}:{}", property.name);
                let mut value = node(
                    id.clone(),
                    kind,
                    Some(property.name.clone()),
                    Some(property.span.python_loc()),
                );
                value.insert("expr".to_owned(), property.expr.python_ast());
                add_requirement_metadata(&mut value, &property.annotations, property.meta.as_ref());
                self.add_node(value, true);
                let reads = expr_reads(&property.expr, &self.state);
                self.add_reads(&id, &reads);
                self.add_checks(&id, &reads);
            }
        }
        for property in &self.model.leadstos {
            let id = format!("leadsTo:{}", property.name);
            let mut value = node(
                id.clone(),
                "leadsTo",
                Some(property.name.clone()),
                Some(property.span.python_loc()),
            );
            value.insert("P".to_owned(), property.before.python_ast());
            value.insert("Q".to_owned(), property.after.python_ast());
            add_requirement_metadata(&mut value, &property.annotations, property.meta.as_ref());
            if let Some(within) = property.within {
                value.insert("within".to_owned(), json!(within));
            }
            if let Some(decreases) = &property.decreases {
                value.insert("decreases".to_owned(), decreases.python_ast());
            }
            self.add_node(value, true);
            let mut reads = expr_reads(&property.before, &self.state);
            reads.extend(expr_reads(&property.after, &self.state));
            if let Some(decreases) = &property.decreases {
                reads.extend(expr_reads(decreases, &self.state));
            }
            self.add_reads(&id, &reads);
            self.add_checks(&id, &reads);
        }
        json!({"analysis":"structure","projection":"tsg","schema_version":"tsg.v0","nodes":self.nodes.into_values().collect::<Vec<_>>(),"edges":self.edges.into_values().collect::<Vec<_>>()})
    }
}

/// Build the deterministic `tsg.v0` structural projection.
#[must_use]
pub fn build_tsg(model: &KernelModel) -> Value {
    Builder::new(model).build()
}

/// Build either the raw TSG or one of its deterministic graph projections.
///
/// # Errors
///
/// Returns an error for unsupported projections, invalid focus usage, or an
/// unknown impact-graph focus node.
pub fn analyze_model(
    model: &KernelModel,
    projection: &str,
    focus: Option<&str>,
) -> Result<Value, String> {
    let tsg = build_tsg(model);
    if projection == "tsg" {
        if focus.is_some() {
            return Err("--focus is supported only with --projection impact_graph".to_owned());
        }
        Ok(tsg)
    } else {
        analysis_graph::project(&tsg, projection, focus)
    }
}

#[cfg(test)]
mod structural_review_tests {
    use super::*;

    fn checked_model(source: &str) -> KernelModel {
        let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
            .expect("lower source");
        fsl_core::build_model(kernel).expect("build model")
    }

    #[test]
    fn checked_model_yields_source_positioned_structural_findings() {
        let source = "spec Review { state { ready: Bool } init { ready = false } invariant stable { ready or not ready } }";
        let kernel = fsl_core::parse_kernel_source(source, &fsl_core::FsResolver::new("."))
            .expect("lower source");
        let model = fsl_core::build_model(kernel).expect("build model");
        let findings = structural_review_findings(&build_tsg(&model));
        let finding = findings
            .iter()
            .find(|finding| finding["finding_type"] == "unwritten_state")
            .expect("unwritten state finding");
        assert_eq!(finding["formal_status"], "not_a_violation");
        assert_eq!(finding["involved_nodes"][0], "state:ready");
    }

    #[test]
    fn conservation_candidates_preserve_exact_weights_action_order_and_wording() {
        let model = checked_model(
            r"spec ConservationCandidate {
  const STEP = 2
  state { stock: Int, reserved: Int, audit: Int }
  init { stock = 2 reserved = 0 audit = 0 }
  action reserve() { stock = stock - STEP reserved = reserved + 1 }
  action release() { stock = stock + STEP reserved = reserved - 1 }
  action audit_event() { audit = audit + 1 }
}",
        );

        let findings = conservation_review_findings(&model);
        assert_eq!(findings, conservation_review_findings(&model));
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding["finding_type"], "conservation_candidate");
        assert_eq!(finding["formal_status"], "not_a_violation");
        assert_eq!(finding["confidence"], 0.6);
        assert_eq!(
            finding["involved_nodes"],
            json!([
                "action:release",
                "action:reserve",
                "state:reserved",
                "state:stock"
            ])
        );
        assert_eq!(
            finding["witness"],
            json!({
                "kind":"weighted_sum_conservation_candidate",
                "expression":"2*reserved + stock",
                "weights":{"reserved":2,"stock":1},
                "action_net_effects":[
                    {"action":"action:release","deltas":{"reserved":-1,"stock":2},"weighted_sum_delta":0},
                    {"action":"action:reserve","deltas":{"reserved":1,"stock":-2},"weighted_sum_delta":0}
                ],
                "excluded_counters":[]
            })
        );
        assert!(
            finding["candidate_repairs"][0]["template"]
                .as_str()
                .is_some_and(|text| text.contains("fslc verify"))
        );
    }

    #[test]
    fn conservation_candidates_reject_insufficient_or_nested_evidence() {
        let one_counter = checked_model(
            r"spec OneCounter {
  state { value: Int }
  init { value = 0 }
  action increment() { value = value + 1 }
  action decrement() { value = value - 1 }
}",
        );
        assert!(conservation_review_findings(&one_counter).is_empty());

        let one_action = checked_model(
            r"spec OneAction {
  state { left: Int, right: Int }
  init { left = 1 right = 0 }
  action move() { left = left - 1 right = right + 1 }
}",
        );
        assert!(conservation_review_findings(&one_action).is_empty());

        let nested = checked_model(
            r"spec NestedEffects {
  state { left: Int, right: Int }
  init { left = 1 right = 0 }
  action move() { if true { left = left - 1 right = right + 1 } }
  action undo() { if true { left = left + 1 right = right - 1 } }
}",
        );
        assert!(conservation_review_findings(&nested).is_empty());

        let unsupported_arithmetic = checked_model(
            r"spec UnsupportedArithmetic {
  state { left: Int, right: Int }
  init { left = 1 right = 0 }
  action move() { left = left - 1 right = right + 1 }
  action undo() { left = left + 1 right = right - 1 }
  action scale() { left = left * 2 }
}",
        );
        assert!(conservation_review_findings(&unsupported_arithmetic).is_empty());
    }
}

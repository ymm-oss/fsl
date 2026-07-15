// SPDX-License-Identifier: Apache-2.0

//! Structural graph projection for standalone refinement mappings.

use std::collections::{BTreeMap, BTreeSet};

use fsl_syntax::{ActionTarget, Binder, Expr, RefinementItem, SurfaceRefinement};
use serde_json::{Value, json};

use crate::analysis_graph::graph_envelope;

#[allow(clippy::needless_pass_by_value)]
fn node(id: String, kind: &str, name: &str) -> Value {
    json!({"id":id,"kind":kind,"name":name,"label":name})
}

#[allow(clippy::needless_pass_by_value)]
fn located_node(id: String, kind: &str, name: &str, span: fsl_syntax::Span) -> Value {
    json!({"id":id,"kind":kind,"name":name,"label":name,"loc":span.python_loc()})
}

fn edge(from: &str, kind: &str, to: &str) -> Value {
    json!({"id":format!("edge:{from}:{kind}:{to}"),"kind":kind,"from":from,"to":to})
}

fn binder_name(binder: &Binder) -> &str {
    match binder {
        Binder::Typed { name, .. }
        | Binder::Range { name, .. }
        | Binder::Collection { name, .. } => name,
    }
}

fn expression_vars(expr: &Expr) -> BTreeSet<String> {
    fn visit(value: &Value, names: &mut BTreeSet<String>) {
        match value {
            Value::Array(parts) => {
                if parts.first().and_then(Value::as_str) == Some("var") {
                    if let Some(name) = parts.get(1).and_then(Value::as_str) {
                        names.insert(name.to_owned());
                    }
                    return;
                }
                for part in parts.iter().skip(1) {
                    visit(part, names);
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    visit(value, names);
                }
            }
            _ => {}
        }
    }
    let mut names = BTreeSet::new();
    visit(&expr.python_ast(), &mut names);
    names
}

fn insert_node(nodes: &mut BTreeMap<String, Value>, value: Value) {
    if let Some(id) = value["id"].as_str() {
        nodes.entry(id.to_owned()).or_insert(value);
    }
}

fn insert_edge(edges: &mut BTreeMap<String, Value>, value: Value) {
    if let Some(id) = value["id"].as_str() {
        edges.entry(id.to_owned()).or_insert(value);
    }
}

/// Build the deterministic `refinement_graph` projection used by `fslc analyze`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn analyze_refinement(refinement: &SurfaceRefinement) -> Value {
    let mut nodes = BTreeMap::new();
    let mut edges = BTreeMap::new();
    let ref_id = format!("refinement:{}", refinement.name);
    insert_node(
        &mut nodes,
        node(ref_id.clone(), "refinement", &refinement.name),
    );
    for item in &refinement.items {
        match item {
            RefinementItem::Impl(name) => {
                let id = format!("impl_spec:{name}");
                insert_node(&mut nodes, node(id.clone(), "impl_spec", name));
                insert_edge(&mut edges, edge(&ref_id, "implements", &id));
            }
            RefinementItem::Abs(name) => {
                let id = format!("abs_spec:{name}");
                insert_node(&mut nodes, node(id.clone(), "abs_spec", name));
                insert_edge(&mut edges, edge(&ref_id, "abstracts", &id));
            }
            RefinementItem::MapsAuto(_) => {
                let id = format!("maps_auto:{}", refinement.name);
                insert_node(&mut nodes, node(id.clone(), "maps_auto", "maps auto"));
                insert_edge(&mut edges, edge(&ref_id, "declares", &id));
            }
            RefinementItem::Map {
                name,
                binder,
                expr,
                span,
            } => {
                let binder = binder.as_ref().map(binder_name);
                let id = binder.map_or_else(
                    || format!("state_map:{name}"),
                    |binder| format!("state_map:{name}:{binder}"),
                );
                let mut value = located_node(id.clone(), "state_map", name, *span);
                if let Value::Object(object) = &mut value {
                    if let Some(binder) = binder {
                        object.insert("binder".to_owned(), json!(binder));
                    }
                    object.insert("expr".to_owned(), expr.python_ast());
                }
                insert_node(&mut nodes, value);
                insert_edge(&mut edges, edge(&ref_id, "declares", &id));
                let abs_id = format!("abs_state:{name}");
                insert_node(&mut nodes, node(abs_id.clone(), "abs_state", name));
                insert_edge(&mut edges, edge(&id, "maps_state", &abs_id));
                let mut reads = expression_vars(expr);
                if let Some(binder) = binder {
                    reads.remove(binder);
                }
                for read in reads {
                    let read_id = format!("map_expr_read:{read}");
                    insert_node(&mut nodes, node(read_id.clone(), "map_expr_read", &read));
                    insert_edge(&mut edges, edge(&id, "reads_impl_state", &read_id));
                }
            }
            RefinementItem::Action {
                name,
                params,
                target,
                origin,
                span,
            } => {
                let id = format!("action_map:{name}");
                let mut value = located_node(id.clone(), "action_map", name, *span);
                if let Value::Object(object) = &mut value {
                    object.insert(
                        "params".to_owned(),
                        json!(params.iter().map(|param| &param.name).collect::<Vec<_>>()),
                    );
                    object.insert("origin".to_owned(), json!(origin.as_str()));
                }
                insert_node(&mut nodes, value);
                insert_edge(&mut edges, edge(&ref_id, "declares", &id));
                let impl_id = format!("impl_action:{name}");
                insert_node(&mut nodes, node(impl_id.clone(), "impl_action", name));
                insert_edge(&mut edges, edge(&id, "maps_action", &impl_id));
                match target {
                    ActionTarget::Stutter => {
                        let stutter_id = format!("stutter_map:{name}");
                        insert_node(
                            &mut nodes,
                            located_node(stutter_id.clone(), "stutter_map", name, *span),
                        );
                        insert_edge(&mut edges, edge(&id, "stutters", &stutter_id));
                    }
                    ActionTarget::Action(abs_action, args) => {
                        let abs_id = format!("abs_action:{abs_action}");
                        insert_node(&mut nodes, node(abs_id.clone(), "abs_action", abs_action));
                        insert_edge(&mut edges, edge(&id, "abstracts", &abs_id));
                        let param_names = params
                            .iter()
                            .map(|param| param.name.as_str())
                            .collect::<BTreeSet<_>>();
                        let reads = args
                            .iter()
                            .flat_map(expression_vars)
                            .filter(|read| !param_names.contains(read.as_str()))
                            .collect::<BTreeSet<_>>();
                        for read in reads {
                            let read_id = format!("map_expr_read:{read}");
                            insert_node(&mut nodes, node(read_id.clone(), "map_expr_read", &read));
                            insert_edge(&mut edges, edge(&id, "reads_impl_state", &read_id));
                        }
                    }
                }
            }
            RefinementItem::PreserveProgress { responses, span } => {
                let id = format!("preserve_progress:{}", refinement.name);
                insert_node(
                    &mut nodes,
                    located_node(id.clone(), "preserve_progress", "preserve progress", *span),
                );
                insert_edge(&mut edges, edge(&ref_id, "preserves_progress", &id));
                for (leadsto, actions, response_span) in responses {
                    let response_id = format!("progress_response:{leadsto}");
                    insert_node(
                        &mut nodes,
                        located_node(
                            response_id.clone(),
                            "progress_response",
                            leadsto,
                            *response_span,
                        ),
                    );
                    insert_edge(&mut edges, edge(&id, "responds_by", &response_id));
                    let abs_id = format!("abs_leadsTo:{leadsto}");
                    insert_node(&mut nodes, node(abs_id.clone(), "abs_leadsTo", leadsto));
                    insert_edge(
                        &mut edges,
                        edge(&response_id, "preserves_progress", &abs_id),
                    );
                    let mut actions = actions.clone();
                    actions.sort();
                    for action in actions {
                        let action_id = format!("impl_action:{action}");
                        insert_node(&mut nodes, node(action_id.clone(), "impl_action", &action));
                        insert_edge(&mut edges, edge(&response_id, "responds_by", &action_id));
                    }
                }
            }
        }
    }
    let nodes = nodes.into_values().collect::<Vec<_>>();
    let edges = edges.into_values().collect::<Vec<_>>();
    graph_envelope("refinement_graph", &nodes, &edges)
}

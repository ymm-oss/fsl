// SPDX-License-Identifier: Apache-2.0

//! Deterministic graph projections and metrics over `tsg.v0`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Value, json};

fn string(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn pairs(edges: &[Value]) -> Vec<(String, String)> {
    edges
        .iter()
        .filter_map(|edge| {
            Some((
                edge.get("from")?.as_str()?.to_owned(),
                edge.get("to")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn adjacency(
    nodes: &[String],
    edges: &[Value],
    reverse: bool,
    undirected: bool,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut graph = nodes
        .iter()
        .map(|node| (node.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for (from, to) in pairs(edges) {
        graph.entry(from.clone()).or_default();
        graph.entry(to.clone()).or_default();
        if reverse {
            graph.entry(to).or_default().insert(from);
        } else {
            graph.entry(from.clone()).or_default().insert(to.clone());
            if undirected {
                graph.entry(to).or_default().insert(from);
            }
        }
    }
    graph
}

fn components(nodes: &[String], edges: &[Value]) -> Vec<Vec<String>> {
    let graph = adjacency(nodes, edges, false, true);
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for start in graph.keys() {
        if seen.contains(start) {
            continue;
        }
        let mut stack = vec![start.clone()];
        seen.insert(start.clone());
        let mut component = Vec::new();
        while let Some(current) = stack.pop() {
            component.push(current.clone());
            for next in graph[&current].iter().rev() {
                if seen.insert(next.clone()) {
                    stack.push(next.clone());
                }
            }
        }
        component.sort();
        result.push(component);
    }
    result.sort_by_key(|component| {
        (
            component.first().cloned().unwrap_or_default(),
            component.len(),
            component.clone(),
        )
    });
    result
}

fn sccs(nodes: &[String], edges: &[Value], singletons: bool) -> Vec<Vec<String>> {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        seen: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) {
        if !seen.insert(node.to_owned()) {
            return;
        }
        for next in &graph[node] {
            visit(next, graph, seen, order);
        }
        order.push(node.to_owned());
    }
    fn collect(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        seen: &mut BTreeSet<String>,
        out: &mut Vec<String>,
    ) {
        if !seen.insert(node.to_owned()) {
            return;
        }
        out.push(node.to_owned());
        for next in &graph[node] {
            collect(next, graph, seen, out);
        }
    }
    let graph = adjacency(nodes, edges, false, false);
    let reverse = adjacency(nodes, edges, true, false);
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    for node in graph.keys() {
        visit(node, &graph, &mut seen, &mut order);
    }
    seen.clear();
    let self_loops = pairs(edges)
        .into_iter()
        .filter(|(a, b)| a == b)
        .map(|(a, _)| a)
        .collect::<BTreeSet<_>>();
    let mut result = Vec::new();
    for node in order.into_iter().rev() {
        if seen.contains(&node) {
            continue;
        }
        let mut component = Vec::new();
        collect(&node, &reverse, &mut seen, &mut component);
        component.sort();
        if singletons || component.len() > 1 || self_loops.contains(&component[0]) {
            result.push(component);
        }
    }
    result.sort_by_key(|component| {
        (
            component.first().cloned().unwrap_or_default(),
            component.len(),
            component.clone(),
        )
    });
    result
}

fn representative_cycles(nodes: &[String], edges: &[Value]) -> Vec<Vec<String>> {
    fn find(
        current: &str,
        allowed: &BTreeSet<String>,
        graph: &BTreeMap<String, BTreeSet<String>>,
        path: &mut Vec<String>,
        positions: &mut BTreeMap<String, usize>,
    ) -> Option<Vec<String>> {
        positions.insert(current.to_owned(), path.len());
        path.push(current.to_owned());
        for next in &graph[current] {
            if !allowed.contains(next) {
                continue;
            }
            if let Some(position) = positions.get(next) {
                let mut cycle = path[*position..].to_vec();
                cycle.push(next.clone());
                return Some(cycle);
            }
            if let Some(found) = find(next, allowed, graph, path, positions) {
                return Some(found);
            }
        }
        path.pop();
        positions.remove(current);
        None
    }
    let graph = adjacency(nodes, edges, false, false);
    let mut result = Vec::new();
    for component in sccs(nodes, edges, false) {
        let allowed = component.iter().cloned().collect::<BTreeSet<_>>();
        for start in &allowed {
            if let Some(cycle) = find(
                start,
                &allowed,
                &graph,
                &mut Vec::new(),
                &mut BTreeMap::new(),
            ) {
                result.push(cycle);
                break;
            }
        }
    }
    result.sort_by_key(|cycle| {
        (
            cycle.first().cloned().unwrap_or_default(),
            cycle.len(),
            cycle.clone(),
        )
    });
    result
}

fn degrees(nodes: &[String], edges: &[Value]) -> Vec<Value> {
    let mut values = nodes
        .iter()
        .map(|node| (node.clone(), [0_usize; 3]))
        .collect::<BTreeMap<_, _>>();
    for (from, to) in pairs(edges) {
        let entry = values.entry(from).or_default();
        entry[1] += 1;
        entry[2] += 1;
        let entry = values.entry(to).or_default();
        entry[0] += 1;
        entry[2] += 1;
    }
    values
        .into_iter()
        .map(|(node, c)| json!({"node":node,"in":c[0],"out":c[1],"total":c[2]}))
        .collect()
}

fn metrics(
    nodes: &[String],
    edges: &[Value],
    components: &[Vec<String>],
    sccs: &[Vec<String>],
) -> Value {
    let degree = degrees(nodes, edges);
    let maximum = |key: &str| {
        let max = degree
            .iter()
            .filter_map(|item| item[key].as_u64())
            .max()
            .unwrap_or(0);
        let all = degree
            .iter()
            .filter(|item| item[key].as_u64() == Some(max))
            .filter_map(|item| item["node"].as_str())
            .collect::<Vec<_>>();
        json!({"value":max,"nodes":all.iter().take(5).collect::<Vec<_>>(),"truncated":all.len()>5})
    };
    let top = |key: &str| {
        let mut all = degree
            .iter()
            .map(|item| {
                (
                    item[key].as_u64().unwrap_or(0),
                    item["node"].as_str().unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        all.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        all.into_iter()
            .take(5)
            .map(|(value, node)| json!({"node":node,key:value}))
            .collect::<Vec<_>>()
    };
    let cycle_rank = edges
        .len()
        .saturating_add(components.len())
        .saturating_sub(nodes.len());
    json!({"node_count":nodes.len(),"edge_count":edges.len(),"component_count":components.len(),"scc_count":sccs.len(),"cycle_rank":cycle_rank,"max_fan_in":maximum("in"),"max_fan_out":maximum("out"),"top_fan_in":top("in"),"top_fan_out":top("out")})
}

fn projection_edge(from: &str, kind: &str, to: &str) -> Value {
    json!({"id":format!("edge:{from}:{kind}:{to}"),"kind":kind,"from":from,"to":to,"formal_status":"not_a_violation"})
}

fn select(tsg: &Value, ids: &BTreeSet<String>, edges: Vec<Value>) -> (Vec<Value>, Vec<Value>) {
    let mut nodes = tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|node| node["id"].as_str().is_some_and(|id| ids.contains(id)))
        .cloned()
        .collect::<Vec<_>>();
    nodes.sort_by_key(|node| (string(node, "kind"), string(node, "id")));
    let mut unique = BTreeMap::new();
    for edge in edges {
        unique.insert(string(&edge, "id"), edge);
    }
    let mut edges = unique.into_values().collect::<Vec<_>>();
    edges.sort_by_key(|edge| {
        (
            string(edge, "kind"),
            string(edge, "from"),
            string(edge, "to"),
            string(edge, "id"),
        )
    });
    (nodes, edges)
}

fn action_state(tsg: &Value) -> (Vec<Value>, Vec<Value>) {
    let action_ids = tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["kind"] == "action")
        .filter_map(|n| n["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let state_ids = tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["kind"] == "state")
        .filter_map(|n| n["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let by_id = tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| Some((n["id"].as_str()?.to_owned(), n)))
        .collect::<BTreeMap<_, _>>();
    let owner = |id: &str| {
        if action_ids.contains(id) {
            Some(id.to_owned())
        } else {
            by_id
                .get(id)
                .and_then(|node| node.get("action"))
                .and_then(Value::as_str)
                .filter(|id| action_ids.contains(*id))
                .map(str::to_owned)
        }
    };
    let mut edges = Vec::new();
    for edge in tsg["edges"].as_array().into_iter().flatten() {
        let from = edge["from"].as_str().unwrap_or_default();
        let to = edge["to"].as_str().unwrap_or_default();
        if edge["kind"] == "writes" && state_ids.contains(to) {
            if let Some(action) = owner(from) {
                edges.push(projection_edge(&action, "writes", to));
            }
        } else if edge["kind"] == "reads" && state_ids.contains(to) {
            if let Some(action) = owner(from) {
                edges.push(projection_edge(to, "read_by", &action));
            }
        }
    }
    select(tsg, &action_ids.union(&state_ids).cloned().collect(), edges)
}

fn action_dependency(tsg: &Value) -> (Vec<Value>, Vec<Value>) {
    let (nodes, edges) = action_state(tsg);
    let actions = nodes
        .iter()
        .filter(|n| n["kind"] == "action")
        .filter_map(|n| n["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let mut writers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut readers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for edge in &edges {
        if edge["kind"] == "writes" {
            writers
                .entry(string(edge, "to"))
                .or_default()
                .insert(string(edge, "from"));
        } else if edge["kind"] == "read_by" {
            readers
                .entry(string(edge, "from"))
                .or_default()
                .insert(string(edge, "to"));
        }
    }
    let mut result = Vec::new();
    for state in writers.keys().filter(|state| readers.contains_key(*state)) {
        for writer in &writers[state] {
            for reader in &readers[state] {
                if writer != reader {
                    let mut edge = projection_edge(writer, "enables", reader);
                    edge.as_object_mut()
                        .unwrap()
                        .insert("state".to_owned(), json!(state));
                    edge.as_object_mut()
                        .unwrap()
                        .insert("states".to_owned(), json!([state]));
                    result.push(edge);
                }
            }
        }
    }
    for (state, values) in &writers {
        let values = values.iter().collect::<Vec<_>>();
        for left in 0..values.len() {
            for right in left + 1..values.len() {
                let mut edge = projection_edge(values[left], "conflicts_with", values[right]);
                let object = edge.as_object_mut().unwrap();
                object.insert("state".to_owned(), json!(state));
                object.insert("states".to_owned(), json!([state]));
                object.insert("symmetric".to_owned(), json!(true));
                result.push(edge);
            }
        }
    }
    select(tsg, &actions, result)
}

fn requirement_property(tsg: &Value) -> (Vec<Value>, Vec<Value>) {
    let kinds = BTreeSet::from([
        "invariant",
        "trans",
        "leadsTo",
        "reachable",
        "acceptance",
        "forbidden",
        "requirement",
        "action",
        "kpi",
        "control",
    ]);
    let ids = tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["kind"].as_str().is_some_and(|kind| kinds.contains(kind)))
        .filter_map(|n| n["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let edges = tsg["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| {
            matches!(
                e["kind"].as_str(),
                Some("covers" | "precedes" | "starts_with")
            ) && ids.contains(e["from"].as_str().unwrap_or_default())
                && ids.contains(e["to"].as_str().unwrap_or_default())
        })
        .map(|e| {
            projection_edge(
                e["from"].as_str().unwrap_or_default(),
                e["kind"].as_str().unwrap_or_default(),
                e["to"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    select(tsg, &ids, edges)
}

fn property_state(tsg: &Value) -> (Vec<Value>, Vec<Value>) {
    let kinds = BTreeSet::from(["invariant", "trans", "leadsTo", "reachable", "state"]);
    let ids = tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| n["kind"].as_str().is_some_and(|kind| kinds.contains(kind)))
        .filter_map(|n| n["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let edges = tsg["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| {
            matches!(e["kind"].as_str(), Some("reads" | "checks"))
                && ids.contains(e["from"].as_str().unwrap_or_default())
                && ids.contains(e["to"].as_str().unwrap_or_default())
        })
        .map(|e| {
            projection_edge(
                e["from"].as_str().unwrap_or_default(),
                e["kind"].as_str().unwrap_or_default(),
                e["to"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    select(tsg, &ids, edges)
}

fn distances(focus: &str, graph: &BTreeMap<String, BTreeSet<String>>) -> BTreeMap<String, usize> {
    if !graph.contains_key(focus) {
        return BTreeMap::new();
    }
    let mut result = BTreeMap::from([(focus.to_owned(), 0)]);
    let mut queue = VecDeque::from([focus.to_owned()]);
    while let Some(current) = queue.pop_front() {
        for next in &graph[&current] {
            if !result.contains_key(next) {
                result.insert(next.clone(), result[&current] + 1);
                queue.push_back(next.clone());
            }
        }
    }
    result
}

fn impact(tsg: &Value, focus: &str) -> Result<(Vec<Value>, Vec<Value>), String> {
    let all = tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| n["id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if !all.iter().any(|id| id == focus) {
        return Err(format!("unknown analyze focus node: {focus}"));
    }
    let all_edges = tsg["edges"].as_array().cloned().unwrap_or_default();
    let down = distances(focus, &adjacency(&all, &all_edges, false, false));
    let up = distances(focus, &adjacency(&all, &all_edges, true, false));
    let ids = up
        .keys()
        .chain(down.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut nodes = Vec::new();
    for node in tsg["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|n| ids.contains(n["id"].as_str().unwrap_or_default()))
    {
        let id = node["id"].as_str().unwrap_or_default();
        let mut item = node.clone();
        let object = item.as_object_mut().unwrap();
        let (direction, directions) = if id == focus {
            ("focus", json!(["focus"]))
        } else if up.contains_key(id) && down.contains_key(id) {
            (
                if up[id] <= down[id] {
                    "upstream"
                } else {
                    "downstream"
                },
                json!(["upstream", "downstream"]),
            )
        } else if up.contains_key(id) {
            ("upstream", json!(["upstream"]))
        } else {
            ("downstream", json!(["downstream"]))
        };
        object.insert("direction".to_owned(), json!(direction));
        object.insert("directions".to_owned(), directions);
        object.insert(
            "focus_distance".to_owned(),
            json!(
                up.get(id)
                    .into_iter()
                    .chain(down.get(id))
                    .min()
                    .copied()
                    .unwrap_or(0)
            ),
        );
        if let Some(value) = up.get(id) {
            object.insert("upstream_distance".to_owned(), json!(value));
        }
        if let Some(value) = down.get(id) {
            object.insert("downstream_distance".to_owned(), json!(value));
        }
        nodes.push(item);
    }
    nodes.sort_by_key(|n| (string(n, "kind"), string(n, "id")));
    let mut edges = all_edges
        .into_iter()
        .filter(|e| {
            ids.contains(e["from"].as_str().unwrap_or_default())
                && ids.contains(e["to"].as_str().unwrap_or_default())
        })
        .collect::<Vec<_>>();
    for edge in &mut edges {
        edge.as_object_mut()
            .unwrap()
            .entry("formal_status".to_owned())
            .or_insert(json!("not_a_violation"));
    }
    edges.sort_by_key(|e| {
        (
            string(e, "kind"),
            string(e, "from"),
            string(e, "to"),
            string(e, "id"),
        )
    });
    Ok((nodes, edges))
}

pub(crate) fn project(tsg: &Value, projection: &str, focus: Option<&str>) -> Result<Value, String> {
    let (nodes, edges) = match projection {
        "action_state_graph" => action_state(tsg),
        "action_dependency_graph" => action_dependency(tsg),
        "requirement_property_graph" => requirement_property(tsg),
        "property_state_graph" => property_state(tsg),
        "impact_graph" => impact(
            tsg,
            focus
                .ok_or_else(|| "--projection impact_graph requires --focus <node-id>".to_owned())?,
        )?,
        _ => return Err(format!("unsupported projection: {projection}")),
    };
    Ok(graph_envelope(projection, &nodes, &edges))
}

pub(crate) fn graph_envelope(projection: &str, nodes: &[Value], edges: &[Value]) -> Value {
    let ids = nodes
        .iter()
        .filter_map(|n| n["id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let components = components(&ids, edges);
    let sccs = sccs(&ids, edges, false);
    let cycles = representative_cycles(&ids, edges);
    let degree = degrees(&ids, edges);
    let metrics = metrics(&ids, edges, &components, &sccs);
    json!({"analysis":"structure","projection":projection,"schema_version":"analysis-graph.v0","formal_status":"not_a_violation","nodes":nodes,"edges":edges,"components":components.into_iter().enumerate().map(|(index,nodes)|json!({"id":format!("component:{index}"),"nodes":nodes})).collect::<Vec<_>>(),"sccs":sccs.into_iter().enumerate().map(|(index,nodes)|json!({"id":format!("scc:{index}"),"nodes":nodes})).collect::<Vec<_>>(),"cycles":cycles.into_iter().enumerate().map(|(index,steps)|json!({"id":format!("cycle:{index}"),"steps":steps})).collect::<Vec<_>>(),"degree":degree,"metrics":metrics})
}

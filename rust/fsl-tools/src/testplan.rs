// SPDX-License-Identifier: Apache-2.0

//! JSON-only conformance test-plan selection from Public Kernel and conformance
//! vectors (issue #844 slice 1).

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use fsl_core::{KERNEL_SCHEMA_VERSION, TEST_PLAN_V1_SCHEMA_ID, TEST_PLAN_V1_SCHEMA_VERSION};
use serde_json::{Map, Value, json};

use crate::public_kernel::{public_kernel_v1_root, required_array, required_object, required_str};

const ALLOWED_OUTCOME_KINDS: &[&str] = &[
    "ok",
    "requires_failed",
    "partial_op",
    "type_bound",
    "invariant",
    "trans",
    "ensures",
];

const DO_NOT_ASSUME: &[&str] = &[
    "not proof of implementation correctness",
    "not exhaustive beyond the declared depth and finite scope",
    "selection coverage is not completeness",
    "does not replace verify, induction, replay, or refinement",
];

const LAYER_SELECTION_REQUIREMENT: &str = "pass the spec at the same FSL layer granularity as the implementation you are checking; from an upper layer reuse forbidden (negative) scenarios only";

/// Fail-closed semantic validation for a closed `test-plan.v1` document.
///
/// # Errors
///
/// Returns an error when schema identity, required fields, or semantic invariants
/// are violated.
pub fn validate_test_plan_v1(plan: &Value) -> Result<(), String> {
    let root = required_object(plan, "test-plan root")?;
    for key in root.keys() {
        if !matches!(
            key.as_str(),
            "$schema"
                | "schema_version"
                | "kernel_schema_version"
                | "fsl"
                | "result"
                | "spec"
                | "depth"
                | "initial"
                | "formal_result"
                | "assurance_effect"
                | "oracle"
                | "selection_coverage"
                | "do_not_assume"
                | "layer_selection"
                | "cases"
        ) {
            return Err(format!("unknown test-plan field '{key}'"));
        }
    }
    let schema = required_str(root, "$schema", "test-plan")?;
    if schema != TEST_PLAN_V1_SCHEMA_ID {
        return Err(format!(
            "unsupported test-plan $schema '{schema}'; expected '{TEST_PLAN_V1_SCHEMA_ID}'"
        ));
    }
    let version = required_str(root, "schema_version", "test-plan")?;
    if version != TEST_PLAN_V1_SCHEMA_VERSION {
        return Err(format!(
            "unsupported test-plan schema_version '{version}'; expected '{TEST_PLAN_V1_SCHEMA_VERSION}'"
        ));
    }
    let kernel_version = required_str(root, "kernel_schema_version", "test-plan")?;
    if kernel_version != KERNEL_SCHEMA_VERSION {
        return Err(format!(
            "unsupported test-plan kernel_schema_version '{kernel_version}'; expected '{KERNEL_SCHEMA_VERSION}'"
        ));
    }
    if root.get("result").and_then(Value::as_str) != Some("testplan") {
        return Err("test-plan result must be 'testplan'".to_owned());
    }
    if root.get("formal_result").and_then(Value::as_str) != Some("not_run") {
        return Err("test-plan formal_result must be 'not_run'".to_owned());
    }
    if root.get("assurance_effect").and_then(Value::as_str) != Some("none") {
        return Err("test-plan assurance_effect must be 'none'".to_owned());
    }
    let oracle = required_object(
        root.get("oracle").ok_or("test-plan oracle is required")?,
        "oracle",
    )?;
    if oracle.get("evaluator_reimplemented") != Some(&Value::Bool(false)) {
        return Err("test-plan oracle.evaluator_reimplemented must be false".to_owned());
    }
    let coverage = required_object(
        root.get("selection_coverage")
            .ok_or("test-plan selection_coverage is required")?,
        "selection_coverage",
    )?;
    let available = coverage
        .get("vectors_available")
        .and_then(Value::as_u64)
        .ok_or("selection_coverage.vectors_available must be a non-negative integer")?;
    let selected = coverage
        .get("vectors_selected")
        .and_then(Value::as_u64)
        .ok_or("selection_coverage.vectors_selected must be a non-negative integer")?;
    if selected > available {
        return Err("selection_coverage.vectors_selected exceeds vectors_available".to_owned());
    }
    let cases = required_array(root, "cases", "test-plan")?;
    if cases.is_empty() {
        return Err("test-plan cases must be non-empty".to_owned());
    }
    for (index, case) in cases.iter().enumerate() {
        validate_case(case, index)?;
    }
    Ok(())
}

fn validate_case(case: &Value, index: usize) -> Result<(), String> {
    let object = required_object(case, &format!("cases[{index}]"))?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "name" | "setup" | "target" | "expected" | "source_vector"
        ) {
            return Err(format!("unknown test-plan case field '{key}'"));
        }
    }
    let expected = required_object(
        object
            .get("expected")
            .ok_or_else(|| format!("cases[{index}].expected is required"))?,
        &format!("cases[{index}].expected"),
    )?;
    let kind = required_str(
        expected,
        "outcome_kind",
        &format!("cases[{index}].expected"),
    )?;
    if !ALLOWED_OUTCOME_KINDS.contains(&kind) {
        return Err(format!("unsupported outcome kind '{kind}'"));
    }
    Ok(())
}

/// Build a closed `test-plan.v1` document from one Public Kernel snapshot and
/// matching conformance vectors produced from the same checked model.
///
/// The planner selects conformance vectors only; it does not evaluate guards or
/// recompute expected states.
///
/// # Errors
///
/// Returns an error when inputs fail closed validation or selection cannot
/// proceed.
pub fn build_test_plan_v1(kernel: &Value, conformance: &Value) -> Result<Value, String> {
    let kernel_root = public_kernel_v1_root(kernel)?;
    let conformance_root = conformance_v1_root(conformance)?;
    let spec_object = required_object(
        kernel_root
            .get("spec")
            .ok_or("kernel spec object is required")?,
        "kernel spec",
    )?;
    let spec = required_str(spec_object, "name", "kernel spec")?;
    if required_str(conformance_root, "spec", "conformance")? != spec {
        return Err("kernel and conformance spec names must match".to_owned());
    }
    if required_str(conformance_root, "kernel_schema_version", "conformance")?
        != KERNEL_SCHEMA_VERSION
    {
        return Err("conformance kernel_schema_version must match public Kernel v1".to_owned());
    }
    let depth = conformance_root
        .get("depth")
        .and_then(Value::as_u64)
        .ok_or("conformance depth must be a non-negative integer")?;
    let states = required_array(conformance_root, "states", "conformance")?;
    let vectors = required_array(conformance_root, "vectors", "conformance")?;
    let state_values = load_state_values(states)?;
    let initial = state_values
        .get("s0")
        .ok_or("conformance must include initial state s0")?
        .clone();
    let paths = shortest_ok_paths(states, vectors, &state_values)?;
    let mut selected = BTreeSet::new();
    let mut cases = Vec::new();

    for boundary in boundary_cases(kernel_root, vectors, &state_values, &paths)? {
        let vector_id = boundary.vector_id;
        if selected.insert(vector_id.clone()) {
            cases.push(boundary.case);
        }
    }

    for (vector_id, case) in failure_cases(vectors, &state_values, &paths, &selected)? {
        selected.insert(vector_id);
        cases.push(case);
    }

    cases.sort_by(|left, right| {
        left["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["name"].as_str().unwrap_or_default())
    });

    let plan = json!({
        "$schema": TEST_PLAN_V1_SCHEMA_ID,
        "schema_version": TEST_PLAN_V1_SCHEMA_VERSION,
        "kernel_schema_version": KERNEL_SCHEMA_VERSION,
        "result": "testplan",
        "spec": spec,
        "depth": depth,
        "initial": initial,
        "formal_result": "not_run",
        "assurance_effect": "none",
        "oracle": {
            "producer": "fslc conformance",
            "evaluator_reimplemented": false
        },
        "selection_coverage": {
            "vectors_available": vectors.len(),
            "vectors_selected": selected.len(),
            "uncovered": uncovered_vector_ids(vectors.len(), &selected),
        },
        "do_not_assume": DO_NOT_ASSUME,
        "layer_selection": {
            "requirement": LAYER_SELECTION_REQUIREMENT
        },
        "cases": cases,
    });
    validate_test_plan_v1(&plan)?;
    Ok(plan)
}

/// Select every non-`ok` vector the boundary pass did not already claim as a
/// failure case, validating each vector's shape on the way. Every vector is
/// checked -- including the `ok` ones the plan does not select -- so a
/// corrupted outcome kind or a failure vector that mutated its input state
/// aborts planning rather than being silently skipped.
fn failure_cases(
    vectors: &[Value],
    state_values: &BTreeMap<String, Value>,
    paths: &BTreeMap<String, Vec<Value>>,
    already_selected: &BTreeSet<String>,
) -> Result<Vec<(String, Value)>, String> {
    let mut selected = Vec::new();
    for (index, vector) in vectors.iter().enumerate() {
        let vector_id = format!("v{index}");
        if already_selected.contains(&vector_id) {
            continue;
        }
        let object = required_object(vector, &format!("vectors[{index}]"))?;
        let outcome = required_object(
            object
                .get("outcome")
                .ok_or_else(|| format!("vectors[{index}].outcome is required"))?,
            &format!("vectors[{index}].outcome"),
        )?;
        let kind = required_str(outcome, "kind", &format!("vectors[{index}].outcome"))?;
        if !ALLOWED_OUTCOME_KINDS.contains(&kind) {
            return Err(format!("unsupported conformance outcome kind '{kind}'"));
        }
        let state_id = required_str(object, "state", &format!("vectors[{index}]"))?;
        let before = state_values
            .get(state_id)
            .ok_or_else(|| format!("unknown conformance state '{state_id}'"))?;
        let after = outcome
            .get("state")
            .ok_or_else(|| format!("vectors[{index}].outcome.state is required"))?;
        if kind == "ok" {
            continue;
        }
        if after != before {
            return Err(format!(
                "failure vector vectors[{index}] must retain the input state"
            ));
        }
        let name = format!("failure_{state_id}_{vector_id}");
        selected.push((
            vector_id,
            build_case(
                &name,
                paths.get(state_id).map_or(&[][..], Vec::as_slice),
                object,
                outcome,
                kind,
            )?,
        ));
    }
    Ok(selected)
}

struct BoundaryCase {
    vector_id: String,
    case: Value,
}

fn boundary_cases(
    kernel_root: &Map<String, Value>,
    vectors: &[Value],
    state_values: &BTreeMap<String, Value>,
    paths: &BTreeMap<String, Vec<Value>>,
) -> Result<Vec<BoundaryCase>, String> {
    let mut boundaries = Vec::new();
    let actions = required_array(kernel_root, "actions", "kernel")?;
    for action in actions {
        let action_object = required_object(action, "kernel action")?;
        let action_name = required_str(action_object, "name", "kernel action")?;
        for guard in required_array(action_object, "guards", "kernel action")? {
            let guard_object = required_object(guard, "guard")?;
            if guard_object.get("kind").and_then(Value::as_str) != Some("requires") {
                continue;
            }
            let expression = guard_object
                .get("expression")
                .ok_or("guard expression is required")?;
            let Some((param, operator, threshold)) = comparison_threshold(expression)? else {
                continue;
            };
            let (accept_value, reject_value) = match operator.as_str() {
                "<=" => (threshold, threshold + 1),
                "<" => (threshold.saturating_sub(1), threshold),
                ">=" => (threshold, threshold.saturating_sub(1)),
                ">" => (threshold + 1, threshold),
                _ => continue,
            };
            if let Some((index, vector)) =
                find_vector(vectors, "s0", action_name, &param, accept_value, "ok")
            {
                let object = required_object(vector, "accept vector")?;
                let outcome = required_object(
                    object.get("outcome").expect("outcome"),
                    "accept vector outcome",
                )?;
                boundaries.push(BoundaryCase {
                    vector_id: format!("v{index}"),
                    case: build_case(
                        &format!("boundary_accept_{param}"),
                        paths.get("s0").map_or(&[][..], Vec::as_slice),
                        object,
                        outcome,
                        "ok",
                    )?,
                });
            }
            if let Some((index, vector)) = find_vector(
                vectors,
                "s0",
                action_name,
                &param,
                reject_value,
                "requires_failed",
            ) {
                let object = required_object(vector, "reject vector")?;
                let outcome = required_object(
                    object.get("outcome").expect("outcome"),
                    "reject vector outcome",
                )?;
                boundaries.push(BoundaryCase {
                    vector_id: format!("v{index}"),
                    case: build_case(
                        &format!("boundary_reject_{param}"),
                        paths.get("s0").map_or(&[][..], Vec::as_slice),
                        object,
                        outcome,
                        "requires_failed",
                    )?,
                });
            }
        }
    }
    let _ = state_values;
    Ok(boundaries)
}

fn comparison_threshold(expression: &Value) -> Result<Option<(String, String, i64)>, String> {
    let object = required_object(expression, "comparison expression")?;
    if object.get("kind").and_then(Value::as_str) != Some("binary") {
        return Ok(None);
    }
    let operator = object
        .get("operator")
        .and_then(Value::as_str)
        .ok_or("comparison operator must be a string")?;
    if !matches!(operator, "<=" | "<" | ">=" | ">") {
        return Ok(None);
    }
    let left = object
        .get("left")
        .ok_or("comparison left operand is required")?;
    let right = object
        .get("right")
        .ok_or("comparison right operand is required")?;
    // Public Kernel v1 names a parameter/state reference `"var"` (the `name`
    // field carries the identifier); `"num"` is the integer literal.
    let (param, literal) = if left.get("kind").and_then(Value::as_str) == Some("var")
        && right.get("kind").and_then(Value::as_str) == Some("num")
    {
        (
            required_str(
                required_object(left, "comparison param")?,
                "name",
                "comparison param",
            )?
            .to_owned(),
            right
                .get("value")
                .and_then(Value::as_i64)
                .ok_or("comparison literal must be an integer")?,
        )
    } else if right.get("kind").and_then(Value::as_str) == Some("var")
        && left.get("kind").and_then(Value::as_str) == Some("num")
    {
        let literal = left
            .get("value")
            .and_then(Value::as_i64)
            .ok_or("comparison literal must be an integer")?;
        let param = required_str(
            required_object(right, "comparison param")?,
            "name",
            "comparison param",
        )?
        .to_owned();
        let flipped = match operator {
            "<=" => ">=",
            "<" => ">",
            ">=" => "<=",
            ">" => "<",
            _ => operator,
        };
        return Ok(Some((param, flipped.to_owned(), literal)));
    } else {
        return Ok(None);
    };
    Ok(Some((param, operator.to_owned(), literal)))
}

fn find_vector<'a>(
    vectors: &'a [Value],
    state_id: &str,
    action_name: &str,
    param: &str,
    value: i64,
    kind: &str,
) -> Option<(usize, &'a Value)> {
    vectors.iter().enumerate().find(|(_, vector)| {
        let Some(object) = vector.as_object() else {
            return false;
        };
        if object.get("state").and_then(Value::as_str) != Some(state_id) {
            return false;
        }
        let Some(action) = object.get("action").and_then(Value::as_object) else {
            return false;
        };
        if action.get("name").and_then(Value::as_str) != Some(action_name) {
            return false;
        }
        let Some(params) = action.get("params").and_then(Value::as_object) else {
            return false;
        };
        if params.get(param).and_then(Value::as_i64) != Some(value) {
            return false;
        }
        object
            .get("outcome")
            .and_then(|outcome| outcome.get("kind"))
            .and_then(Value::as_str)
            == Some(kind)
    })
}

fn build_case(
    name: &str,
    setup: &[Value],
    vector: &Map<String, Value>,
    outcome: &Map<String, Value>,
    kind: &str,
) -> Result<Value, String> {
    let action = required_object(
        vector.get("action").ok_or("vector action is required")?,
        "vector action",
    )?;
    let params = action.get("params").cloned().unwrap_or_else(|| json!({}));
    let mut expected = json!({
        "outcome_kind": kind,
        "state": outcome.get("state").cloned().unwrap_or_else(|| json!({})),
    });
    if kind != "ok"
        && let Some(object) = expected.as_object_mut()
    {
        object.insert("state_unchanged".to_owned(), Value::Bool(true));
    }
    Ok(json!({
        "name": name,
        "setup": setup,
        "target": {
            "action": required_str(action, "name", "vector action")?,
            "params": params,
            "expected": outcome.get("state").cloned().unwrap_or_else(|| json!({})),
        },
        "expected": expected,
        "source_vector": {
            "state": required_str(vector, "state", "vector")?,
            "action": {
                "name": required_str(action, "name", "vector action")?,
                "params": action.get("params").cloned().unwrap_or_else(|| json!({})),
            }
        }
    }))
}

fn uncovered_vector_ids(total: usize, selected: &BTreeSet<String>) -> Vec<String> {
    (0..total)
        .map(|index| format!("v{index}"))
        .filter(|id| !selected.contains(id))
        .collect()
}

fn conformance_v1_root(conformance: &Value) -> Result<&Map<String, Value>, String> {
    let root = required_object(conformance, "conformance root")?;
    let schema = required_str(root, "$schema", "conformance")?;
    if schema != "https://fsl.dev/schemas/fslc/kernel/conformance.v1.schema.json" {
        return Err(format!("unsupported conformance $schema '{schema}'"));
    }
    let version = required_str(root, "schema_version", "conformance")?;
    if version != "1.0.0" {
        return Err(format!(
            "unsupported conformance schema_version '{version}'"
        ));
    }
    if root.get("result").and_then(Value::as_str) != Some("conformance") {
        return Err("conformance result must be 'conformance'".to_owned());
    }
    Ok(root)
}

fn load_state_values(states: &[Value]) -> Result<BTreeMap<String, Value>, String> {
    let mut values = BTreeMap::new();
    for (index, state) in states.iter().enumerate() {
        let object = required_object(state, &format!("states[{index}]"))?;
        let id = required_str(object, "id", &format!("states[{index}]"))?;
        let value = object
            .get("state")
            .cloned()
            .ok_or_else(|| format!("states[{index}].state is required"))?;
        values.insert(id.to_owned(), value);
    }
    Ok(values)
}

fn shortest_ok_paths(
    states: &[Value],
    vectors: &[Value],
    state_values: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Vec<Value>>, String> {
    let mut adjacency: HashMap<String, Vec<(String, Value)>> = HashMap::new();
    for vector in vectors {
        let object = required_object(vector, "vector")?;
        if object
            .get("outcome")
            .and_then(|outcome| outcome.get("kind"))
            .and_then(Value::as_str)
            != Some("ok")
        {
            continue;
        }
        let from = required_str(object, "state", "vector")?.to_owned();
        let action = required_object(object.get("action").expect("action"), "vector action")?;
        let after = object
            .get("outcome")
            .and_then(|outcome| outcome.get("state"))
            .cloned()
            .ok_or("ok vector outcome.state is required")?;
        // `conformance` enumerates states up to the requested depth but still
        // emits every vector leaving the frontier, so at the boundary depth an
        // `ok` vector's successor is legitimately absent from `states[]` (at
        // `--depth 0`, `states[]` is the initial state alone). Such an edge can
        // never lie on a shortest path *to* an enumerated state, so skipping it
        // loses no path; treating it as an error rejected valid input instead.
        let Some(to) = state_values
            .iter()
            .find(|(_, value)| *value == &after)
            .map(|(id, _)| id.clone())
        else {
            continue;
        };
        let step = json!({
            "action": required_str(action, "name", "vector action")?,
            "params": action.get("params").cloned().unwrap_or_else(|| json!({})),
            "expected": after,
        });
        adjacency.entry(from).or_default().push((to, step));
    }

    let mut paths = BTreeMap::from([("s0".to_owned(), Vec::new())]);
    let mut queue = VecDeque::from(["s0".to_owned()]);
    while let Some(state_id) = queue.pop_front() {
        let prefix = paths.get(&state_id).cloned().unwrap_or_default();
        for (next, step) in adjacency.get(&state_id).cloned().unwrap_or_default() {
            if paths.contains_key(&next) {
                continue;
            }
            let mut extended = prefix.clone();
            extended.push(step);
            paths.insert(next.clone(), extended);
            queue.push_back(next);
        }
    }
    let _ = states;
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_unknown_outcome_kind() {
        let plan = json!({
            "$schema": TEST_PLAN_V1_SCHEMA_ID,
            "schema_version": TEST_PLAN_V1_SCHEMA_VERSION,
            "kernel_schema_version": KERNEL_SCHEMA_VERSION,
            "result": "testplan",
            "spec": "S",
            "depth": 0,
            "initial": {},
            "formal_result": "not_run",
            "assurance_effect": "none",
            "oracle": {"producer": "fslc conformance", "evaluator_reimplemented": false},
            "selection_coverage": {"vectors_available": 1, "vectors_selected": 1, "uncovered": []},
            "do_not_assume": DO_NOT_ASSUME,
            "layer_selection": {"requirement": LAYER_SELECTION_REQUIREMENT},
            "cases": [{
                "name": "bad",
                "setup": [],
                "target": {"action": "a", "params": {}, "expected": {}},
                "expected": {"outcome_kind": "guard_failed", "state": {}},
                "source_vector": {"state": "s0", "action": {"name": "a", "params": {}}}
            }]
        });
        let error = validate_test_plan_v1(&plan).expect_err("unknown outcome kind");
        assert!(error.contains("guard_failed"));
    }
}

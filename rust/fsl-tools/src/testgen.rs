// SPDX-License-Identifier: Apache-2.0

//! Alternate-language conformance-test emitters.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fsl_core::{
    KERNEL_SCHEMA_VERSION, TESTGEN_TRACE_V1_SCHEMA_ID, TESTGEN_TRACE_V1_SCHEMA_VERSION,
    display_name,
};
use serde_json::{Map, Value};

use crate::public_kernel::{public_kernel_v1_root, required_array, required_object, required_str};

#[derive(Clone, Debug, Eq, PartialEq)]
struct TestgenAction {
    name: String,
    params: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TestgenOutputContext {
    WithoutOutput,
    WithOutput { parent: Option<PathBuf> },
}

/// Explicit, delivery-normalized path input for generated test presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestgenPathContext {
    source_name: String,
    spec_path: PathBuf,
    output: TestgenOutputContext,
}

impl TestgenPathContext {
    /// Build context for content returned without an output file.
    ///
    /// # Errors
    ///
    /// Rejects a raw source path without a UTF-8 file name.
    pub fn without_output(source_path: &Path, spec_path: PathBuf) -> Result<Self, String> {
        Ok(Self {
            source_name: testgen_source_name(source_path)?,
            spec_path,
            output: TestgenOutputContext::WithoutOutput,
        })
    }

    /// Build context for content written to a caller-selected output file.
    ///
    /// # Errors
    ///
    /// Rejects a raw source path without a UTF-8 file name.
    pub fn with_output(
        source_path: &Path,
        spec_path: PathBuf,
        output_parent: Option<PathBuf>,
    ) -> Result<Self, String> {
        Ok(Self {
            source_name: testgen_source_name(source_path)?,
            spec_path,
            output: TestgenOutputContext::WithOutput {
                parent: output_parent,
            },
        })
    }
}

fn testgen_source_name(source_path: &Path) -> Result<String, String> {
    source_path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_owned)
        .ok_or_else(|| "testgen spec path must have a UTF-8 file name".to_owned())
}

/// Normalized, target-independent input consumed by every test generator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestgenInput {
    spec_name: String,
    path: TestgenPathContext,
    state_order: Vec<String>,
    actions: Vec<TestgenAction>,
    scenarios: Vec<Value>,
    walk: Value,
}

impl TestgenInput {
    /// Name of the checked specification.
    #[must_use]
    pub fn spec_name(&self) -> &str {
        &self.spec_name
    }
}

fn source_order(value: &Value, index: usize) -> (u64, u64, usize) {
    let span = value.get("span").and_then(Value::as_object);
    (
        span.and_then(|span| span.get("line"))
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX),
        span.and_then(|span| span.get("column"))
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX),
        index,
    )
}

fn validate_step(step: &Value, context: &str) -> Result<(), String> {
    let step = step
        .as_object()
        .ok_or_else(|| format!("{context} must be an object"))?;
    if !step.get("action").is_some_and(Value::is_string) {
        return Err(format!("{context}.action must be a string"));
    }
    if !step.get("params").is_some_and(Value::is_object) {
        return Err(format!("{context}.params must be an object"));
    }
    Ok(())
}

fn validate_closed_object(
    object: &Map<String, Value>,
    context: &str,
    allowed: &[&str],
) -> Result<(), String> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("{context} has unknown field '{key}'"));
    }
    Ok(())
}

fn validate_state_fields(
    state: &Value,
    context: &str,
    state_names: &BTreeSet<&str>,
    require_all: bool,
) -> Result<(), String> {
    let state = state
        .as_object()
        .ok_or_else(|| format!("{context} must be an object"))?;
    let actual = state.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if !actual.is_subset(state_names) {
        let unknown = actual.difference(state_names).copied().collect::<Vec<_>>();
        return Err(format!(
            "{context} has unknown state fields: {}",
            unknown.join(", ")
        ));
    }
    if require_all && actual != *state_names {
        return Err(format!(
            "{context} must contain every public Kernel state field"
        ));
    }
    Ok(())
}

fn validate_contract_step(
    step: &Value,
    context: &str,
    actions: &BTreeMap<&str, BTreeSet<&str>>,
) -> Result<(), String> {
    validate_step(step, context)?;
    let step = step.as_object().expect("validate_step checked the object");
    let action = step["action"]
        .as_str()
        .expect("validate_step checked the action");
    let expected_params = actions
        .get(action)
        .ok_or_else(|| format!("{context}.action names unknown action '{action}'"))?;
    let actual_params = step["params"]
        .as_object()
        .expect("validate_step checked params")
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if actual_params != *expected_params {
        return Err(format!(
            "{context}.params must match public Kernel parameters for '{action}'"
        ));
    }
    Ok(())
}

fn validate_scenarios(
    scenarios: &Value,
    spec_name: &str,
    state_names: &BTreeSet<&str>,
    actions: &BTreeMap<&str, BTreeSet<&str>>,
) -> Result<Vec<Value>, String> {
    let root = scenarios
        .as_object()
        .ok_or_else(|| "testgen scenarios root must be an object".to_owned())?;
    let scenario_spec = root
        .get("spec")
        .and_then(Value::as_str)
        .ok_or_else(|| "testgen scenarios.spec must be a string".to_owned())?;
    if scenario_spec != spec_name {
        return Err(format!(
            "testgen scenarios spec '{scenario_spec}' does not match public Kernel spec '{spec_name}'"
        ));
    }
    let scenarios = root
        .get("scenarios")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "testgen scenarios.scenarios must be an array".to_owned())?;
    for (index, scenario) in scenarios.iter().enumerate() {
        let context = format!("testgen scenarios.scenarios[{index}]");
        let scenario = scenario
            .as_object()
            .ok_or_else(|| format!("{context} must be an object"))?;
        if !scenario.get("name").is_some_and(Value::is_string) {
            return Err(format!("{context}.name must be a string"));
        }
        validate_state_fields(
            scenario
                .get("initial_state")
                .ok_or_else(|| format!("{context}.initial_state is required"))?,
            &format!("{context}.initial_state"),
            state_names,
            true,
        )?;
        let steps = scenario
            .get("steps")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{context}.steps must be an array"))?;
        let expected = scenario
            .get("expected_states")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{context}.expected_states must be an array"))?;
        if steps.len() != expected.len() {
            return Err(format!(
                "{context}.steps and {context}.expected_states must have equal lengths"
            ));
        }
        for (step_index, step) in steps.iter().enumerate() {
            validate_contract_step(step, &format!("{context}.steps[{step_index}]"), actions)?;
        }
        for (state_index, state) in expected.iter().enumerate() {
            validate_state_fields(
                state,
                &format!("{context}.expected_states[{state_index}]"),
                state_names,
                false,
            )?;
        }
        if let Some(step) = scenario.get("forbidden_step") {
            validate_contract_step(step, &format!("{context}.forbidden_step"), actions)?;
        }
    }
    Ok(scenarios)
}

fn validate_walk(
    walk: &Value,
    spec_name: &str,
    state_names: &BTreeSet<&str>,
    actions: &BTreeMap<&str, BTreeSet<&str>>,
) -> Result<(), String> {
    let root = walk
        .as_object()
        .ok_or_else(|| "testgen concrete vector must be an object".to_owned())?;
    validate_closed_object(
        root,
        "testgen concrete vector",
        &[
            "$schema",
            "schema_version",
            "kernel_schema_version",
            "result",
            "spec",
            "initial",
            "steps",
        ],
    )?;
    for (key, expected) in [
        ("$schema", TESTGEN_TRACE_V1_SCHEMA_ID),
        ("schema_version", TESTGEN_TRACE_V1_SCHEMA_VERSION),
        ("kernel_schema_version", KERNEL_SCHEMA_VERSION),
        ("result", "testgen_trace"),
        ("spec", spec_name),
    ] {
        let actual = root
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("testgen concrete vector.{key} must be a string"))?;
        if actual != expected {
            return Err(format!(
                "testgen concrete vector.{key} '{actual}' does not match '{expected}'"
            ));
        }
    }
    validate_state_fields(
        root.get("initial")
            .ok_or_else(|| "testgen concrete vector.initial is required".to_owned())?,
        "testgen concrete vector.initial",
        state_names,
        true,
    )?;
    let steps = root
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| "testgen concrete vector.steps must be an array".to_owned())?;
    if steps.len() > 100 {
        return Err("testgen concrete vector.steps must contain at most 100 items".to_owned());
    }
    for (index, step) in steps.iter().enumerate() {
        validate_contract_step(
            step,
            &format!("testgen concrete vector.steps[{index}]"),
            actions,
        )?;
        validate_closed_object(
            step.as_object()
                .expect("validate_contract_step checked the object"),
            &format!("testgen concrete vector.steps[{index}]"),
            &["action", "params", "expected"],
        )?;
        if !step.get("expected").is_some_and(Value::is_object) {
            return Err(format!(
                "testgen concrete vector.steps[{index}].expected must be an object"
            ));
        }
        validate_state_fields(
            &step["expected"],
            &format!("testgen concrete vector.steps[{index}].expected"),
            state_names,
            true,
        )?;
    }
    Ok(())
}

fn normalize_scenarios(
    mut scenarios: Vec<Value>,
    state_order: &[String],
    actions: &[TestgenAction],
) -> Vec<Value> {
    let action_order = actions
        .iter()
        .enumerate()
        .map(|(index, action)| (action.name.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    scenarios.sort_by_key(|scenario| {
        scenario
            .get("action")
            .and_then(Value::as_str)
            .and_then(|name| action_order.get(name))
            .copied()
            .unwrap_or(usize::MAX)
    });
    for scenario in &mut scenarios {
        let Some(scenario) = scenario.as_object_mut() else {
            continue;
        };
        if let Some(Value::Array(states)) = scenario.get_mut("expected_states") {
            for state in states {
                *state = ordered_object(state, state_order);
            }
        }
        if let Some(initial) = scenario.get_mut("initial_state") {
            *initial = ordered_object(initial, state_order);
        }
        if let Some(Value::Array(steps)) = scenario.get_mut("steps") {
            for step in steps {
                normalize_step(step, actions);
            }
        }
        if let Some(step) = scenario.get_mut("forbidden_step") {
            normalize_step(step, actions);
        }
    }
    scenarios
}

fn normalize_step(step: &mut Value, actions: &[TestgenAction]) {
    let Some(step) = step.as_object_mut() else {
        return;
    };
    let action = step
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let order = actions
        .iter()
        .find(|candidate| candidate.name == action)
        .map_or(&[][..], |candidate| candidate.params.as_slice());
    if let Some(params) = step.get_mut("params") {
        *params = ordered_object(params, order);
    }
}

fn build_input(
    spec_name: String,
    path: &TestgenPathContext,
    state_order: Vec<String>,
    actions: Vec<TestgenAction>,
    scenarios: &Value,
    walk: &Value,
) -> Result<TestgenInput, String> {
    let public_state_names = state_order
        .iter()
        .map(|name| display_name(name))
        .collect::<Vec<_>>();
    let state_names = public_state_names
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let action_params = actions
        .iter()
        .map(|action| {
            (
                action.name.as_str(),
                action
                    .params
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    validate_walk(walk, &spec_name, &state_names, &action_params)?;
    let scenarios = normalize_scenarios(
        validate_scenarios(scenarios, &spec_name, &state_names, &action_params)?,
        &state_order,
        &actions,
    );
    Ok(TestgenInput {
        spec_name,
        path: path.clone(),
        state_order,
        actions,
        scenarios,
        walk: walk.clone(),
    })
}

/// Adapt Public Kernel v1 plus generated scenarios and a concrete walk vector.
///
/// # Errors
///
/// Rejects incompatible or malformed contracts and mismatched specification names.
pub fn public_kernel_testgen_input(
    kernel: &Value,
    path: &TestgenPathContext,
    scenarios: &Value,
    walk: &Value,
) -> Result<TestgenInput, String> {
    let root = public_kernel_v1_root(kernel)?;
    let spec = required_object(
        root.get("spec")
            .ok_or_else(|| "public Kernel root.spec is required".to_owned())?,
        "root.spec",
    )?;
    let spec_name = required_str(spec, "name", "root.spec")?.to_owned();

    let mut state = required_array(root, "state", "root")?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let object = required_object(value, &format!("root.state[{index}]"))?;
            Ok((
                source_order(value, index),
                required_str(object, "name", &format!("root.state[{index}]"))?.to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    state.sort_by_key(|(order, _)| *order);
    let mut state_order = state.into_iter().map(|(_, name)| name).collect::<Vec<_>>();
    if let Some(initial) = walk.get("initial").and_then(Value::as_object)
        && !initial.is_empty()
    {
        state_order = initial.keys().cloned().collect();
    }

    let mut actions = required_array(root, "actions", "root")?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let context = format!("root.actions[{index}]");
            let object = required_object(value, &context)?;
            let params = required_array(object, "parameters", &context)?
                .iter()
                .enumerate()
                .map(|(param_index, param)| {
                    let context = format!("{context}.parameters[{param_index}]");
                    required_str(required_object(param, &context)?, "name", &context)
                        .map(str::to_owned)
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok((
                source_order(value, index),
                TestgenAction {
                    name: display_name(required_str(object, "name", &context)?),
                    params,
                },
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    actions.sort_by_key(|(order, _)| *order);
    let actions = actions.into_iter().map(|(_, action)| action).collect();
    build_input(spec_name, path, state_order, actions, scenarios, walk)
}

/// Explicit compose bridge while Public Kernel rejects multi-file provenance.
///
/// The caller supplies checked names and declaration order only; emitters still
/// consume the same normalized input and never receive a private model or AST.
///
/// # Errors
///
/// Rejects malformed scenarios or concrete vectors and mismatched spec names.
#[doc(hidden)]
pub fn compose_testgen_input(
    spec_name: &str,
    path: &TestgenPathContext,
    state_order: Vec<String>,
    actions: Vec<(String, Vec<String>)>,
    scenarios: &Value,
    walk: &Value,
) -> Result<TestgenInput, String> {
    build_input(
        spec_name.to_owned(),
        path,
        state_order,
        actions
            .into_iter()
            .map(|(name, params)| TestgenAction {
                name: display_name(&name),
                params,
            })
            .collect(),
        scenarios,
        walk,
    )
}

fn ordered_object(value: &Value, order: &[String]) -> Value {
    let Some(values) = value.as_object() else {
        return value.clone();
    };
    let mut result = Map::new();
    for key in order {
        if let Some(value) = values.get(key) {
            result.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in values {
        if !result.contains_key(key) {
            result.insert(key.clone(), value.clone());
        }
    }
    Value::Object(result)
}

fn python_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn python_literal(value: &Value, key_order: &[String]) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => python_string(value),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| python_literal(value, &[]))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(values) => {
            let ordered = key_order
                .iter()
                .filter(|key| values.contains_key(*key))
                .chain(values.keys().filter(|key| !key_order.contains(key)))
                .collect::<Vec<_>>();
            format!(
                "{{{}}}",
                ordered
                    .into_iter()
                    .map(|key| format!(
                        "{}: {}",
                        python_string(key),
                        python_literal(&values[key], &[])
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

fn portable_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

fn relative_spec_path(path: &TestgenPathContext) -> String {
    let TestgenOutputContext::WithOutput {
        parent: Some(parent),
    } = &path.output
    else {
        return portable_path(&path.spec_path);
    };
    let left = parent.components().collect::<Vec<_>>();
    let right = path.spec_path.components().collect::<Vec<_>>();
    let common = left
        .iter()
        .zip(&right)
        .take_while(|(left, right)| left == right)
        .count();
    let mut result = PathBuf::new();
    for _ in common..left.len() {
        result.push("..");
    }
    for component in &right[common..] {
        result.push(component.as_os_str());
    }
    portable_path(&result)
}

#[allow(clippy::too_many_lines)]
fn emit_pytest(input: &TestgenInput) -> String {
    let path_expr = match input.path.output {
        TestgenOutputContext::WithoutOutput => python_string(&relative_spec_path(&input.path)),
        TestgenOutputContext::WithOutput { .. } => format!(
            "Path(__file__).resolve().parent / {}",
            python_string(&relative_spec_path(&input.path))
        ),
    };
    let source_name = &input.path.source_name;
    let mut text = format!(
        r#""""Auto-generated conformance tests for FSL spec.
Source: {source_name}
Connect Adapter to your implementation, or use MonitorSelfAdapter for self-check.
"""
import random
from pathlib import Path

import pytest

from fslc.runtime import Monitor

SPEC_PATH = {path_expr}


class Adapter:
    """Connect your implementation to the spec actions/state.

    Wiring convention:
    - reset(): put implementation in the same initial state as spec init
    - step(action, params): drive one spec action on the implementation
    - observe(): return implementation state projected to spec logical state shape
    """

    def reset(self):
        raise NotImplementedError("wire your implementation reset")

    def step(self, action: str, params: dict):
        raise NotImplementedError("wire your implementation step")

    def observe(self) -> dict:
        raise NotImplementedError("wire your implementation observe")


def _adapter_ready(adapter):
    try:
        adapter.reset()
        adapter.observe()
        return True
    except NotImplementedError:
        return False


@pytest.fixture
def adapter():
    return Adapter()


def _assert_partial_expected(observed, expected):
    for key, val in expected.items():
        if isinstance(val, dict) and isinstance(observed.get(key), dict):
            _assert_partial_expected(observed[key], val)
        else:
            assert observed[key] == val


def _assert_rejected(result, expected_kind):
    assert isinstance(result, dict), 'forbidden adapter.step must return a result dict'
    assert result.get('ok') is False
    if expected_kind is not None:
        assert result.get('kind') == expected_kind
"#
    );
    let state_order = input
        .state_order
        .iter()
        .map(|name| display_name(name))
        .collect::<Vec<_>>();
    let mut seen = BTreeMap::new();
    for (scenario_index, scenario) in input.scenarios.iter().enumerate() {
        let name = scenario["name"].as_str().unwrap_or("scenario");
        let function = unique_ident(name, &mut seen, "test_scenario_");
        let separator = if scenario_index == 0 { "\n\n" } else { "\n" };
        let _ = write!(
            text,
            "{separator}def {function}(adapter):\n    {}\n    if not _adapter_ready(adapter):\n        pytest.skip('Adapter not implemented')\n    adapter.reset()\n",
            python_string(&format!("Scenario: {name}"))
        );
        let steps = scenario["steps"].as_array().cloned().unwrap_or_default();
        let states = scenario["expected_states"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for (index, step) in steps.iter().enumerate() {
            let action = step["action"].as_str().unwrap_or_default();
            let param_order = input
                .actions
                .iter()
                .find(|item| item.name == action)
                .map_or(&[][..], |item| item.params.as_slice());
            let _ = writeln!(
                text,
                "    adapter.step({}, {})",
                python_string(action),
                python_literal(&step["params"], param_order)
            );
            let expected = states
                .get(index)
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            let _ = writeln!(
                text,
                "    _assert_partial_expected(adapter.observe(), {})",
                python_literal(&expected, &state_order)
            );
        }
    }
    text.push_str(
        r#"
def test_random_walk_conformance(adapter):
    if not _adapter_ready(adapter):
        pytest.skip('Adapter not implemented')
    mon = Monitor(SPEC_PATH)
    mon.reset()
    adapter.reset()
    assert adapter.observe() == mon.state
    rng = random.Random(0)
    for _ in range(100):
        enabled = mon.enabled()
        if not enabled:
            break
        choice = enabled[rng.randrange(len(enabled))]
        action, params = choice['action'], dict(choice['params'])
        adapter.step(action, params)
        result = mon.step(action, params)
        if not result.get('ok'):
            pytest.fail(
                f'spec oracle violation at {action} {params}: '
                f"{result.get('kind')} {result.get('name', '')}"
            )
        assert adapter.observe() == mon.state

"#,
    );
    text
}

fn inline(value: &Value) -> String {
    match value {
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(inline).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_owned()),
                    inline(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned()),
    }
}

fn array(values: &[Value]) -> String {
    if values.is_empty() {
        return "[]".to_owned();
    }
    format!(
        "[\n{},\n]",
        values
            .iter()
            .map(|value| format!("  {}", inline(value)))
            .collect::<Vec<_>>()
            .join(",\n")
    )
}

fn vitest_scenario(scenario: &Value) -> String {
    let name = scenario["name"].as_str().unwrap_or_default();
    let mut lines = vec![
        format!(
            "scenario({}, () => {{",
            inline(&Value::String(format!("scenario: {name}")))
        ),
        "  adapter.reset();".to_owned(),
    ];
    let steps = scenario["steps"].as_array().cloned().unwrap_or_default();
    let states = scenario["expected_states"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let empty = Value::Object(serde_json::Map::new());
    for (index, step) in steps.iter().enumerate() {
        lines.push(format!(
            "  adapter.step({}, {});",
            inline(&step["action"]),
            inline(&step["params"])
        ));
        lines.push(format!(
            "  assertPartial(adapter.observe(), {});",
            inline(states.get(index).unwrap_or(&empty))
        ));
    }
    if scenario["kind"].as_str() == Some("forbidden") && scenario["forbidden_step"].is_object() {
        let step = &scenario["forbidden_step"];
        lines.push(format!(
            "  const result = adapter.step({}, {});",
            inline(&step["action"]),
            inline(&step["params"])
        ));
        lines.push(format!(
            "  assertRejected(result, {});",
            inline(&scenario["rejected_by"])
        ));
    }
    lines.push("});".to_owned());
    lines.join("\n")
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn testgen_template(template: &str, source_name: &str) -> String {
    normalize_newlines(template).replace("__SOURCE__", source_name)
}

/// Emit a standalone Vitest conformance scaffold.
#[must_use]
fn emit_vitest(source_name: &str, scenarios: &[Value], walk: &Value) -> String {
    let mut parts = vec![testgen_template(
        include_str!("testgen_vitest.txt"),
        source_name,
    )];
    parts.extend(scenarios.iter().map(vitest_scenario));
    let steps = walk["steps"].as_array().cloned().unwrap_or_default();
    parts.push(
        [
            "interface WalkStep {".to_owned(),
            "  action: string;".to_owned(),
            "  params: Record<string, unknown>;".to_owned(),
            "  expected: Record<string, unknown>;".to_owned(),
            "}".to_owned(),
            String::new(),
            format!(
                "const RANDOM_WALK_INITIAL: Record<string, unknown> = {};",
                inline(&walk["initial"])
            ),
            format!("const RANDOM_WALK: WalkStep[] = {};", array(&steps)),
            String::new(),
            "scenario(\"random-walk conformance (baked oracle trace)\", () => {".to_owned(),
            "  adapter.reset();".to_owned(),
            "  assertPartial(adapter.observe(), RANDOM_WALK_INITIAL);".to_owned(),
            "  for (const step of RANDOM_WALK) {".to_owned(),
            "    adapter.step(step.action, step.params);".to_owned(),
            "    assertPartial(adapter.observe(), step.expected);".to_owned(),
            "  }".to_owned(),
            "});".to_owned(),
        ]
        .join("\n"),
    );
    parts.join("\n\n") + "\n"
}

#[derive(Clone, Copy)]
enum Target {
    Swift,
    Kotlin,
    Dart,
    Php,
}

fn quoted(value: &str, target: Target) -> String {
    let quote = if matches!(target, Target::Dart | Target::Php) {
        '\''
    } else {
        '"'
    };
    let mut result = String::from(quote);
    for character in value.chars() {
        match (target, character) {
            (Target::Php, '\\') => result.push_str("\\\\"),
            (Target::Php | Target::Dart, '\'') => result.push_str("\\'"),
            (Target::Php, _) => result.push(character),
            (_, '"') if quote == '"' => result.push_str("\\\""),
            (_, '\\') => result.push_str("\\\\"),
            (Target::Kotlin | Target::Dart, '$') => result.push_str("\\$"),
            (_, '\n') => result.push_str("\\n"),
            (_, '\t') => result.push_str("\\t"),
            (_, '\r') => result.push_str("\\r"),
            (Target::Swift, '\0') => result.push_str("\\0"),
            (Target::Swift | Target::Dart, value) if value < ' ' => {
                let _ = write!(result, "\\u{{{:x}}}", u32::from(value));
            }
            (Target::Kotlin, value) if value < ' ' => {
                let _ = write!(result, "\\u{:04x}", u32::from(value));
            }
            (_, value) => result.push(value),
        }
    }
    result.push(quote);
    result
}

fn literal(value: &Value, target: Target) -> String {
    match value {
        Value::Null => match target {
            Target::Swift => "FSLNull.instance".to_owned(),
            _ => "null".to_owned(),
        },
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => quoted(value, target),
        Value::Array(values) if values.is_empty() => match target {
            Target::Swift | Target::Php => "[]".to_owned(),
            Target::Kotlin => "listOf<Any?>()".to_owned(),
            Target::Dart => "<dynamic>[]".to_owned(),
        },
        Value::Array(values) => {
            let items = values
                .iter()
                .map(|value| literal(value, target))
                .collect::<Vec<_>>()
                .join(", ");
            match target {
                Target::Kotlin => format!("listOf({items})"),
                _ => format!("[{items}]"),
            }
        }
        Value::Object(values) if values.is_empty() => match target {
            Target::Swift => "[String: Any]()".to_owned(),
            Target::Kotlin => "mapOf<String, Any?>()".to_owned(),
            Target::Dart => "<String, dynamic>{}".to_owned(),
            Target::Php => "[]".to_owned(),
        },
        Value::Object(values) => {
            let items = values
                .iter()
                .map(|(key, value)| match target {
                    Target::Swift | Target::Dart => {
                        format!("{}: {}", quoted(key, target), literal(value, target))
                    }
                    Target::Kotlin => {
                        format!("{} to {}", quoted(key, target), literal(value, target))
                    }
                    Target::Php => format!("{} => {}", quoted(key, target), literal(value, target)),
                })
                .collect::<Vec<_>>()
                .join(", ");
            match target {
                Target::Kotlin => format!("mapOf({items})"),
                Target::Dart => format!("{{{items}}}"),
                _ => format!("[{items}]"),
            }
        }
    }
}

fn safe_ident(value: &str) -> String {
    let mut result = String::new();
    let mut underscore = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            result.push(character);
            underscore = false;
        } else if !underscore {
            result.push('_');
            underscore = true;
        }
    }
    let result = result.trim_matches('_');
    if result.is_empty() {
        "scenario".to_owned()
    } else {
        result.to_owned()
    }
}

fn unique_ident(value: &str, seen: &mut BTreeMap<String, usize>, prefix: &str) -> String {
    let safe = safe_ident(value);
    let count = seen.entry(safe.clone()).or_default();
    *count += 1;
    if *count == 1 {
        format!("{prefix}{safe}")
    } else {
        format!("{prefix}{safe}_{count}")
    }
}

fn scenario_parts(scenario: &Value) -> (&str, Vec<Value>, Vec<Value>) {
    (
        scenario["name"].as_str().unwrap_or("scenario"),
        scenario["steps"].as_array().cloned().unwrap_or_default(),
        scenario["expected_states"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    )
}

fn append_forbidden(lines: &mut Vec<String>, scenario: &Value, target: Target, indent: &str) {
    if scenario["kind"].as_str() != Some("forbidden") || !scenario["forbidden_step"].is_object() {
        return;
    }
    let step = &scenario["forbidden_step"];
    let rejected = scenario
        .get("rejected_by")
        .filter(|value| !value.is_null())
        .map_or_else(|| "null".to_owned(), |value| literal(value, target));
    let action = literal(&step["action"], target);
    let params = literal(&step["params"], target);
    match target {
        Target::Swift => {
            lines.push(format!("{indent}let result = a.step({action}, {params})"));
            lines.push(format!("{indent}assertRejected(result, {rejected})"));
        }
        Target::Kotlin => {
            lines.push(format!("{indent}val result = a.step({action}, {params})"));
            lines.push(format!("{indent}assertRejected(result, {rejected})"));
        }
        Target::Dart => {
            lines.push(format!(
                "{indent}final result = a.step({action}, {params});"
            ));
            lines.push(format!("{indent}assertRejected(result, {rejected});"));
        }
        Target::Php => {
            lines.push(format!("{indent}$result = $a->step({action}, {params});"));
            lines.push(format!(
                "{indent}$this->assertRejected($result, {rejected});"
            ));
        }
    }
}

/// Emit a standalone Swift Testing conformance scaffold.
#[must_use]
fn emit_swift(source_name: &str, scenarios: &[Value], walk: &Value) -> String {
    let mut parts = vec![testgen_template(
        include_str!("testgen_swift.txt"),
        source_name,
    )];
    let mut seen = BTreeMap::new();
    for scenario in scenarios {
        let (name, steps, states) = scenario_parts(scenario);
        let function = unique_ident(name, &mut seen, "scenario_");
        let mut lines = vec![
            format!("@Test(.enabled(if: isAdapterWired())) func {function}() throws {{"),
            "    let a = try makeAdapter()".to_owned(),
            "    a.reset()".to_owned(),
        ];
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "    _ = a.step({}, {})",
                literal(&step["action"], Target::Swift),
                literal(&step["params"], Target::Swift)
            ));
            lines.push(format!(
                "    assertPartial(a.observe(), {})",
                literal(states.get(index).unwrap_or(&Value::Null), Target::Swift)
            ));
        }
        append_forbidden(&mut lines, scenario, Target::Swift, "    ");
        lines.push("}".to_owned());
        parts.push(lines.join("\n"));
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "        (action: {}, params: {}, expected: {})",
                literal(&step["action"], Target::Swift),
                literal(&step["params"], Target::Swift),
                literal(&step["expected"], Target::Swift)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "[]".to_owned()
    } else {
        format!("[\n{},\n    ]", rows.join(",\n"))
    };
    parts.push([
        "@Test(.enabled(if: isAdapterWired())) func randomWalkConformance() throws {".to_owned(),
        "    // random-walk conformance (baked oracle trace)".to_owned(),
        "    let a = try makeAdapter()".to_owned(), "    a.reset()".to_owned(),
        format!("    let initial: [String: Any] = {}", literal(&walk["initial"], Target::Swift)),
        "    assertPartial(a.observe(), initial)".to_owned(),
        format!("    let walk: [(action: String, params: [String: Any], expected: [String: Any])] = {walk_literal}"),
        "    for step in walk {".to_owned(), "        _ = a.step(step.action, step.params)".to_owned(),
        "        assertPartial(a.observe(), step.expected)".to_owned(), "    }".to_owned(), "}".to_owned(),
    ].join("\n"));
    parts.join("\n\n") + "\n"
}

/// Emit a standalone kotlin.test conformance scaffold.
#[must_use]
fn emit_kotlin(source_name: &str, spec_name: &str, scenarios: &[Value], walk: &Value) -> String {
    let preamble = testgen_template(include_str!("testgen_kotlin.txt"), source_name);
    let mut lines = vec![format!("class {spec_name}ConformanceTest {{")];
    let mut seen = BTreeMap::new();
    for scenario in scenarios {
        let (name, steps, states) = scenario_parts(scenario);
        lines.push(String::new());
        lines.push(format!(
            "    @Test fun {}() {{",
            unique_ident(name, &mut seen, "scenario_")
        ));
        lines.push("        val a = makeAdapter() ?: return".to_owned());
        lines.push("        a.reset()".to_owned());
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "        a.step({}, {})",
                literal(&step["action"], Target::Kotlin),
                literal(&step["params"], Target::Kotlin)
            ));
            lines.push(format!(
                "        assertPartial(a.observe(), {})",
                literal(states.get(index).unwrap_or(&Value::Null), Target::Kotlin)
            ));
        }
        append_forbidden(&mut lines, scenario, Target::Kotlin, "        ");
        lines.push("    }".to_owned());
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "            Triple({}, {}, {})",
                literal(&step["action"], Target::Kotlin),
                literal(&step["params"], Target::Kotlin),
                literal(&step["expected"], Target::Kotlin)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "listOf<Triple<String, Map<String, Any?>, Map<String, Any?>>>()".to_owned()
    } else {
        format!("listOf(\n{},\n        )", rows.join(",\n"))
    };
    lines.extend([
        String::new(),
        "    @Test fun randomWalkConformance() {".to_owned(),
        "        // random-walk conformance (baked oracle trace)".to_owned(),
        "        val a = makeAdapter() ?: return".to_owned(),
        "        a.reset()".to_owned(),
        format!(
            "        val initial: Map<String, Any?> = {}",
            literal(&walk["initial"], Target::Kotlin)
        ),
        "        assertPartial(a.observe(), initial)".to_owned(),
        format!(
            "        val walk: List<Triple<String, Map<String, Any?>, Map<String, Any?>>> = {walk_literal}"
        ),
        "        for ((action, params, expected) in walk) {".to_owned(),
        "            a.step(action, params)".to_owned(),
        "            assertPartial(a.observe(), expected)".to_owned(),
        "        }".to_owned(),
        "    }".to_owned(),
        "}".to_owned(),
    ]);
    format!("{preamble}\n\n{}\n", lines.join("\n"))
}

/// Emit a standalone package:test Dart conformance scaffold.
#[must_use]
fn emit_dart(source_name: &str, scenarios: &[Value], walk: &Value) -> String {
    let preamble = testgen_template(include_str!("testgen_dart.txt"), source_name);
    let mut lines = vec![
        "void main() {".to_owned(),
        "  final wired = _adapterWired();".to_owned(),
    ];
    for scenario in scenarios {
        let (name, steps, states) = scenario_parts(scenario);
        lines.push(String::new());
        lines.push(format!(
            "  test({}, () {{",
            quoted(&format!("scenario: {name}"), Target::Dart)
        ));
        lines.push("    final a = makeAdapter();".to_owned());
        lines.push("    a.reset();".to_owned());
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "    a.step({}, {});",
                literal(&step["action"], Target::Dart),
                literal(&step["params"], Target::Dart)
            ));
            lines.push(format!(
                "    assertPartial(a.observe(), {});",
                literal(states.get(index).unwrap_or(&Value::Null), Target::Dart)
            ));
        }
        append_forbidden(&mut lines, scenario, Target::Dart, "    ");
        lines.push("  }, skip: wired ? null : 'Adapter not wired');".to_owned());
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "      {{'action': {}, 'params': {}, 'expected': {}}}",
                literal(&step["action"], Target::Dart),
                literal(&step["params"], Target::Dart),
                literal(&step["expected"], Target::Dart)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "<Map<String, dynamic>>[]".to_owned()
    } else {
        format!("<Map<String, dynamic>>[\n{},\n    ]", rows.join(",\n"))
    };
    lines.extend([
        String::new(),
        "  test('random-walk conformance (baked oracle trace)', () {".to_owned(),
        "    final a = makeAdapter();".to_owned(),
        "    a.reset();".to_owned(),
        format!(
            "    final initial = {};",
            literal(&walk["initial"], Target::Dart)
        ),
        "    assertPartial(a.observe(), Map<String, dynamic>.from(initial));".to_owned(),
        format!("    final walk = {walk_literal};"),
        "    for (final step in walk) {".to_owned(),
        "      a.step(step['action'] as String, step['params'] as Map<String, dynamic>);"
            .to_owned(),
        "      assertPartial(a.observe(), step['expected'] as Map<String, dynamic>);".to_owned(),
        "    }".to_owned(),
        "  }, skip: wired ? null : 'Adapter not wired');".to_owned(),
        "}".to_owned(),
    ]);
    format!("{preamble}\n\n{}\n", lines.join("\n"))
}

/// Emit a standalone `PHPUnit` conformance scaffold.
#[must_use]
fn emit_phpunit(source_name: &str, spec_name: &str, scenarios: &[Value], walk: &Value) -> String {
    let preamble = testgen_template(include_str!("testgen_php.txt"), source_name);
    let mut lines = vec![
        format!("final class {spec_name}ConformanceTest extends TestCase"),
        "{".to_owned(),
        normalize_newlines(include_str!("testgen_php_helpers.txt"))
            .trim_end()
            .to_owned(),
    ];
    let mut seen = BTreeMap::new();
    for scenario in scenarios {
        let (name, steps, states) = scenario_parts(scenario);
        lines.push(String::new());
        lines.push(format!(
            "    public function {}(): void",
            unique_ident(name, &mut seen, "testScenario_")
        ));
        lines.push("    {".to_owned());
        lines.push("        $a = $this->adapter;".to_owned());
        lines.push("        $a->reset();".to_owned());
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "        $a->step({}, {});",
                literal(&step["action"], Target::Php),
                literal(&step["params"], Target::Php)
            ));
            lines.push(format!(
                "        $this->assertPartial({}, $a->observe());",
                literal(states.get(index).unwrap_or(&Value::Null), Target::Php)
            ));
        }
        append_forbidden(&mut lines, scenario, Target::Php, "        ");
        lines.push("    }".to_owned());
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "        ['action' => {}, 'params' => {}, 'expected' => {}]",
                literal(&step["action"], Target::Php),
                literal(&step["params"], Target::Php),
                literal(&step["expected"], Target::Php)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "[]".to_owned()
    } else {
        format!("[\n{},\n    ]", rows.join(",\n"))
    };
    lines.extend([
        String::new(),
        format!(
            "    private const INITIAL = {};",
            literal(&walk["initial"], Target::Php)
        ),
        format!("    private const WALK = {walk_literal};"),
        String::new(),
        "    public function testRandomWalkConformance(): void".to_owned(),
        "    {".to_owned(),
        "        // random-walk conformance (baked oracle trace)".to_owned(),
        "        $a = $this->adapter;".to_owned(),
        "        $a->reset();".to_owned(),
        "        $this->assertPartial(self::INITIAL, $a->observe());".to_owned(),
        "        foreach (self::WALK as $step) {".to_owned(),
        "            $a->step($step['action'], $step['params']);".to_owned(),
        "            $this->assertPartial($step['expected'], $a->observe());".to_owned(),
        "        }".to_owned(),
        "    }".to_owned(),
        "}".to_owned(),
    ]);
    format!("{preamble}\n\n{}\n", lines.join("\n"))
}

/// Generate one target scaffold from the normalized testgen boundary.
///
/// # Errors
///
/// Rejects unknown targets instead of selecting an implicit fallback.
pub fn generate_testgen(input: &TestgenInput, target: &str) -> Result<String, String> {
    let content = match target {
        "pytest" => emit_pytest(input),
        "vitest" => emit_vitest(&input.path.source_name, &input.scenarios, &input.walk),
        "swift" => emit_swift(&input.path.source_name, &input.scenarios, &input.walk),
        "kotlin" => emit_kotlin(
            &input.path.source_name,
            &input.spec_name,
            &input.scenarios,
            &input.walk,
        ),
        "dart" => emit_dart(&input.path.source_name, &input.scenarios, &input.walk),
        "phpunit" => emit_phpunit(
            &input.path.source_name,
            &input.spec_name,
            &input.scenarios,
            &input.walk,
        ),
        _ => {
            return Err(format!(
                "unknown testgen target '{target}'; expected pytest, vitest, swift, kotlin, dart, or phpunit"
            ));
        }
    };
    Ok(content)
}

#[cfg(test)]
mod tests {
    use fsl_core::{KERNEL_SCHEMA_ID, KERNEL_SCHEMA_VERSION};
    use serde_json::json;

    use super::*;

    fn contracts() -> (Value, Value, Value) {
        let kernel = json!({
            "$schema": KERNEL_SCHEMA_ID,
            "schema_version": KERNEL_SCHEMA_VERSION,
            "spec": {"name": "Demo"},
            "state": [
                {"name":"alpha","span":{"line":2,"column":1}},
                {"name":"zeta","span":{"line":1,"column":1}}
            ],
            "actions": [
                {"name":"finish","span":{"line":4,"column":1},"parameters":[]},
                {"name":"begin","span":{"line":3,"column":1},"parameters":[{"name":"id"}]}
            ]
        });
        let scenarios = json!({"spec":"Demo","scenarios":[]});
        let walk = json!({
            "$schema": TESTGEN_TRACE_V1_SCHEMA_ID,
            "schema_version": TESTGEN_TRACE_V1_SCHEMA_VERSION,
            "kernel_schema_version": KERNEL_SCHEMA_VERSION,
            "result": "testgen_trace",
            "spec": "Demo",
            "initial":{"zeta":0,"alpha":0},
            "steps":[]
        });
        (kernel, scenarios, walk)
    }

    fn path_context() -> TestgenPathContext {
        TestgenPathContext::without_output(Path::new("demo.fsl"), PathBuf::from("demo.fsl"))
            .expect("build test path context")
    }

    #[test]
    fn public_and_explicit_compose_metadata_normalize_to_one_input() {
        let (kernel, scenarios, walk) = contracts();
        let public = public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &walk)
            .expect("adapt public Kernel");
        let compose = compose_testgen_input(
            "Demo",
            &path_context(),
            vec!["zeta".to_owned(), "alpha".to_owned()],
            vec![
                ("begin".to_owned(), vec!["id".to_owned()]),
                ("finish".to_owned(), vec![]),
            ],
            &scenarios,
            &walk,
        )
        .expect("adapt explicit compose metadata");

        assert_eq!(public, compose);
    }

    #[test]
    fn explicit_path_context_is_the_only_generated_path_input() {
        let (kernel, scenarios, walk) = contracts();
        let first_context = TestgenPathContext::with_output(
            Path::new("demo.fsl"),
            PathBuf::from("/workspace/first/demo.fsl"),
            Some(PathBuf::from("/workspace/generated")),
        )
        .expect("build first path context");
        let second_context = TestgenPathContext::with_output(
            Path::new("demo.fsl"),
            PathBuf::from("/workspace/second/demo.fsl"),
            Some(PathBuf::from("/workspace/generated")),
        )
        .expect("build second path context");
        let first = public_kernel_testgen_input(&kernel, &first_context, &scenarios, &walk)
            .expect("adapt first path context");
        let second = public_kernel_testgen_input(&kernel, &second_context, &scenarios, &walk)
            .expect("adapt second path context");

        let first_pytest = generate_testgen(&first, "pytest").expect("emit first pytest");
        let second_pytest = generate_testgen(&second, "pytest").expect("emit second pytest");
        assert!(
            first_pytest
                .contains("SPEC_PATH = Path(__file__).resolve().parent / '../first/demo.fsl'")
        );
        assert!(
            second_pytest
                .contains("SPEC_PATH = Path(__file__).resolve().parent / '../second/demo.fsl'")
        );
        assert_ne!(first_pytest, second_pytest);
        for target in ["vitest", "swift", "kotlin", "dart", "phpunit"] {
            assert_eq!(
                generate_testgen(&first, target).expect("emit first non-pytest target"),
                generate_testgen(&second, target).expect("emit second non-pytest target"),
                "{target} must not observe path-context changes"
            );
        }
    }

    #[test]
    fn missing_and_parentless_paths_keep_the_explicit_fallback_spelling() {
        let (kernel, scenarios, walk) = contracts();
        let missing = TestgenPathContext::without_output(
            Path::new("missing.fsl"),
            PathBuf::from("missing/spec.fsl"),
        )
        .expect("build missing path context");
        let input = public_kernel_testgen_input(&kernel, &missing, &scenarios, &walk)
            .expect("adapt missing path context");
        let pytest = generate_testgen(&input, "pytest").expect("emit missing-path pytest");
        assert!(pytest.contains("Source: missing.fsl"));
        assert!(pytest.contains("SPEC_PATH = 'missing/spec.fsl'"));

        let parentless = TestgenPathContext::with_output(
            Path::new("demo.fsl"),
            PathBuf::from("/workspace/demo.fsl"),
            None,
        )
        .expect("build parentless path context");
        let input = public_kernel_testgen_input(&kernel, &parentless, &scenarios, &walk)
            .expect("adapt parentless output context");
        let pytest = generate_testgen(&input, "pytest").expect("emit parentless-path pytest");
        assert!(
            pytest.contains("SPEC_PATH = Path(__file__).resolve().parent / '/workspace/demo.fsl'")
        );
    }

    #[test]
    fn generator_source_has_no_filesystem_or_canonicalization_dependency() {
        let source = include_str!("testgen.rs");
        let filesystem_api = ["std", "::", "fs"].concat();
        let canonicalization_api = ["canonical", "ize"].concat();
        let cwd_api = ["current", "_dir"].concat();
        assert!(!source.contains(&filesystem_api));
        assert!(!source.contains(&canonicalization_api));
        assert!(!source.contains(&cwd_api));
    }

    #[cfg(unix)]
    #[test]
    fn path_context_preserves_non_utf8_source_name_rejection() {
        use std::os::unix::ffi::OsStringExt;

        let source = PathBuf::from(std::ffi::OsString::from_vec(vec![0xff]));
        let error = TestgenPathContext::without_output(&source, source.clone())
            .expect_err("reject a non-UTF-8 source name");
        assert_eq!(error, "testgen spec path must have a UTF-8 file name");
    }

    #[test]
    fn checked_in_templates_emit_the_same_text_after_windows_checkout() {
        let unix = "Source: __SOURCE__\nbody\n";
        let windows = "Source: __SOURCE__\r\nbody\r\n";
        assert_eq!(
            testgen_template(unix, "demo.fsl"),
            testgen_template(windows, "demo.fsl")
        );
        assert_eq!(
            portable_path(&Path::new("parent").join("child")),
            "parent/child"
        );
    }

    #[test]
    fn public_adapter_fails_closed_on_schema_version_and_spec_mismatch() {
        let (mut kernel, scenarios, walk) = contracts();
        kernel["$schema"] = json!("https://example.com/kernel.schema.json");
        let error = public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &walk)
            .expect_err("reject unsupported schema identifier");
        assert!(error.contains("unsupported public Kernel $schema"));

        let (mut kernel, scenarios, walk) = contracts();
        kernel["schema_version"] = json!("2.0.0");
        let error = public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &walk)
            .expect_err("reject unsupported version");
        assert!(error.contains("unsupported public Kernel schema_version"));

        let (kernel, scenarios, mut walk) = contracts();
        walk["schema_version"] = json!("2.0.0");
        let error = public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &walk)
            .expect_err("reject unsupported trace version");
        assert!(error.contains("concrete vector.schema_version"));

        let (kernel, mut scenarios, walk) = contracts();
        scenarios["spec"] = json!("Other");
        let error = public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &walk)
            .expect_err("reject mismatched spec");
        assert!(error.contains("does not match public Kernel spec"));
    }

    #[test]
    fn testgen_rejects_malformed_vectors_and_unknown_targets() {
        let (kernel, scenarios, mut malformed_walk) = contracts();
        malformed_walk["steps"] = json!([{"action":"begin"}]);
        let error =
            public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &malformed_walk)
                .expect_err("reject malformed vector");
        assert!(error.contains("steps[0].params must be an object"));

        let (_, _, mut malformed_walk) = contracts();
        malformed_walk["extra"] = json!(true);
        let error =
            public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &malformed_walk)
                .expect_err("reject unknown trace field");
        assert!(error.contains("unknown field 'extra'"));

        let (_, _, mut malformed_walk) = contracts();
        malformed_walk["steps"] = json!([{
            "action":"finish",
            "params":{},
            "expected":{"zeta":0,"alpha":0},
            "extra":true
        }]);
        let error =
            public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &malformed_walk)
                .expect_err("reject unknown trace step field");
        assert!(error.contains("steps[0] has unknown field 'extra'"));

        let malformed_scenarios = json!({
            "spec":"Demo",
            "scenarios":[{
                "name":"broken",
                "initial_state":{"zeta":0,"alpha":0},
                "steps":[{"action":"begin","params":{"id":0}}],
                "expected_states":[]
            }]
        });
        let (_, _, walk) = contracts();
        let error =
            public_kernel_testgen_input(&kernel, &path_context(), &malformed_scenarios, &walk)
                .expect_err("reject inconsistent scenario vector");
        assert!(error.contains("must have equal lengths"));

        let (_, _, mut walk) = contracts();
        walk["steps"] = json!([{
            "action":"missing",
            "params":{},
            "expected":{"zeta":0,"alpha":0}
        }]);
        let error = public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &walk)
            .expect_err("reject unknown vector action");
        assert!(error.contains("unknown action 'missing'"));

        let (_, _, mut walk) = contracts();
        walk["steps"] = json!([{
            "action":"begin",
            "params":{},
            "expected":{"zeta":0,"alpha":0}
        }]);
        let error = public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &walk)
            .expect_err("reject mismatched vector parameters");
        assert!(error.contains("must match public Kernel parameters"));

        let (_, _, walk) = contracts();
        let input = public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &walk)
            .expect("adapt valid input");
        assert!(generate_testgen(&input, "unknown").is_err());
    }

    #[test]
    fn pytest_sanitizes_scenario_names_without_losing_display_names() {
        let (kernel, mut scenarios, walk) = contracts();
        scenarios["scenarios"] = json!([
            {
                "name":"reach_bank.Settled",
                "initial_state":{"zeta":0,"alpha":0},
                "steps":[],
                "expected_states":[]
            },
            {
                "name":"reach_bank-Settled",
                "initial_state":{"zeta":0,"alpha":0},
                "steps":[],
                "expected_states":[]
            }
        ]);
        let input = public_kernel_testgen_input(&kernel, &path_context(), &scenarios, &walk)
            .expect("adapt valid scenarios");

        let output = generate_testgen(&input, "pytest").expect("emit pytest");

        assert!(output.contains("def test_scenario_reach_bank_Settled(adapter):"));
        assert!(output.contains("def test_scenario_reach_bank_Settled_2(adapter):"));
        assert!(output.contains("'Scenario: reach_bank.Settled'"));
        assert!(output.contains("'Scenario: reach_bank-Settled'"));
    }
}

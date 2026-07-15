// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Versioned external replay-trace input and the explicit legacy adapter.

use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplayTraceContract {
    Legacy,
    V1 {
        schema_version: String,
        kernel_schema_version: String,
        spec: String,
        initial: Map<String, Value>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayEvent {
    pub tick: Option<u64>,
    pub timestamp: Option<String>,
    pub step: ReplayStep,
    pub state: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplayStep {
    Action {
        name: String,
        params: Map<String, Value>,
    },
    Stutter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayTraceInput {
    pub contract: ReplayTraceContract,
    pub events: Vec<ReplayEvent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayTraceV1 {
    #[serde(rename = "$schema")]
    schema: String,
    schema_version: String,
    kernel_schema_version: String,
    spec: String,
    initial: Map<String, Value>,
    events: Vec<ReplayEventV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayEventV1 {
    tick: u64,
    #[serde(default)]
    timestamp: Option<String>,
    action: Value,
    params: Map<String, Value>,
    state: Map<String, Value>,
}

/// Parse a public replay trace or the pre-v1 action-only compatibility shape.
///
/// Presence of any version marker selects the public parser and fails closed;
/// malformed versioned input never falls through to the legacy adapter.
///
/// # Errors
///
/// Returns an error for malformed events, unsupported versions, or non-canonical ticks.
pub fn parse_replay_trace(data: Value) -> Result<ReplayTraceInput, String> {
    if data.as_object().is_some_and(|object| {
        object.contains_key("$schema")
            || object.contains_key("schema_version")
            || object.contains_key("kernel_schema_version")
            || object.contains_key("spec")
            || object.contains_key("initial")
    }) {
        return parse_v1(data);
    }
    let events = match data {
        Value::Array(events) => events,
        Value::Object(mut object) => {
            match object.remove("events") {
                Some(Value::Array(events)) => events,
                _ => {
                    return Err("trace JSON must be a public replay trace, an array, or {\"events\": [...]}".to_owned());
                }
            }
        }
        _ => {
            return Err(
                "trace JSON must be a public replay trace, an array, or {\"events\": [...]}"
                    .to_owned(),
            );
        }
    };
    Ok(ReplayTraceInput {
        contract: ReplayTraceContract::Legacy,
        events: events
            .into_iter()
            .map(parse_legacy_event)
            .collect::<Result<_, _>>()?,
    })
}

fn parse_v1(data: Value) -> Result<ReplayTraceInput, String> {
    let trace: ReplayTraceV1 = serde_json::from_value(data)
        .map_err(|error| format!("invalid replay trace v1: {error}"))?;
    if trace.schema != fsl_core::REPLAY_TRACE_V1_SCHEMA_ID {
        return Err(format!(
            "unsupported replay trace schema '{}'; expected '{}'",
            trace.schema,
            fsl_core::REPLAY_TRACE_V1_SCHEMA_ID
        ));
    }
    if !matches!(
        trace.schema_version.as_str(),
        fsl_core::REPLAY_TRACE_V1_INITIAL_SCHEMA_VERSION
            | fsl_core::REPLAY_TRACE_V1_STUTTER_SCHEMA_VERSION
            | fsl_core::REPLAY_TRACE_V1_SCHEMA_VERSION
    ) {
        return Err(format!(
            "unsupported replay trace schema_version '{}'; expected '{}', '{}', or '{}'",
            trace.schema_version,
            fsl_core::REPLAY_TRACE_V1_INITIAL_SCHEMA_VERSION,
            fsl_core::REPLAY_TRACE_V1_STUTTER_SCHEMA_VERSION,
            fsl_core::REPLAY_TRACE_V1_SCHEMA_VERSION
        ));
    }
    if !matches!(
        trace.kernel_schema_version.as_str(),
        fsl_core::KERNEL_V1_SCHEMA_VERSION | fsl_core::KERNEL_V2_SCHEMA_VERSION
    ) {
        return Err(format!(
            "unsupported replay trace kernel_schema_version '{}'",
            trace.kernel_schema_version
        ));
    }
    if trace.spec.is_empty() {
        return Err("replay trace spec must not be empty".to_owned());
    }
    let events = trace
        .events
        .into_iter()
        .enumerate()
        .map(|(index, event)| {
            let expected_tick = index as u64 + 1;
            if event.tick != expected_tick {
                return Err(format!(
                    "replay trace event {index} has tick {}; expected {expected_tick}",
                    event.tick
                ));
            }
            let step = parse_v1_step(event.action, event.params, index, &trace.schema_version)?;
            if event.timestamp.as_ref().is_some_and(String::is_empty) {
                return Err(format!(
                    "replay trace event {index} timestamp must not be empty"
                ));
            }
            Ok(ReplayEvent {
                tick: Some(event.tick),
                timestamp: event.timestamp,
                step,
                state: Some(event.state),
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(ReplayTraceInput {
        contract: ReplayTraceContract::V1 {
            schema_version: trace.schema_version,
            kernel_schema_version: trace.kernel_schema_version,
            spec: trace.spec,
            initial: trace.initial,
        },
        events,
    })
}

fn parse_v1_step(
    action: Value,
    params: Map<String, Value>,
    index: usize,
    schema_version: &str,
) -> Result<ReplayStep, String> {
    match action {
        Value::String(action) if !action.is_empty() => Ok(ReplayStep::Action {
            name: action,
            params,
        }),
        Value::String(_) => Err(format!(
            "replay trace event {index} action must not be empty"
        )),
        Value::Null
            if matches!(
                schema_version,
                fsl_core::REPLAY_TRACE_V1_STUTTER_SCHEMA_VERSION
                    | fsl_core::REPLAY_TRACE_V1_SCHEMA_VERSION
            ) && params.is_empty() =>
        {
            Ok(ReplayStep::Stutter)
        }
        Value::Null if schema_version == fsl_core::REPLAY_TRACE_V1_INITIAL_SCHEMA_VERSION => {
            Err(format!(
                "replay trace event {index} stutter requires schema_version '{}'",
                fsl_core::REPLAY_TRACE_V1_STUTTER_SCHEMA_VERSION
            ))
        }
        Value::Null => Err(format!(
            "replay trace event {index} stutter params must be empty"
        )),
        _ => Err(format!(
            "replay trace event {index} action must be a string or null"
        )),
    }
}

fn parse_legacy_event(event: Value) -> Result<ReplayEvent, String> {
    let Value::Object(mut event) = event else {
        return Err("trace event must be an object".to_owned());
    };
    let action = event
        .remove("action")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| "trace event requires string action".to_owned())?;
    let params = match event.remove("params") {
        None => Map::new(),
        Some(Value::Object(params)) => params,
        Some(_) => return Err("trace params must be an object".to_owned()),
    };
    Ok(ReplayEvent {
        tick: None,
        timestamp: None,
        step: ReplayStep::Action {
            name: action,
            params,
        },
        state: None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{ReplayStep, ReplayTraceContract, parse_replay_trace};

    fn versioned() -> Value {
        json!({
            "$schema":fsl_core::REPLAY_TRACE_V1_SCHEMA_ID,
            "schema_version":fsl_core::REPLAY_TRACE_V1_SCHEMA_VERSION,
            "kernel_schema_version":fsl_core::KERNEL_V1_SCHEMA_VERSION,
            "spec":"S",
            "initial":{"ready":false},
            "events":[{"tick":1,"action":"advance","params":{},"state":{"ready":true}}],
        })
    }

    #[test]
    fn legacy_array_and_object_use_only_the_explicit_compatibility_adapter() {
        for input in [
            json!([{"action":"advance"}]),
            json!({"events":[{"action":"advance","params":{}}]}),
        ] {
            let parsed = parse_replay_trace(input).expect("legacy trace");
            assert_eq!(parsed.contract, ReplayTraceContract::Legacy);
            assert_eq!(parsed.events.len(), 1);
            assert!(matches!(
                &parsed.events[0].step,
                ReplayStep::Action { name, .. } if name == "advance"
            ));
        }
        assert!(
            parse_replay_trace(json!({"spec":"S","events":[]}))
                .expect_err("reserved marker must select v1")
                .contains("invalid replay trace v1")
        );
        assert!(
            parse_replay_trace(json!({"initial":{},"events":[]}))
                .expect_err("reserved marker must select v1")
                .contains("invalid replay trace v1")
        );
    }

    #[test]
    fn v1_fails_closed_on_versions_ticks_closed_shapes_and_empty_identity() {
        let cases = [
            ("$schema", json!("https://example.com/trace.json")),
            ("schema_version", json!("2.0.0")),
            ("kernel_schema_version", json!("3.0.0")),
            ("spec", json!("")),
        ];
        for (key, value) in cases {
            let mut input = versioned();
            input[key] = value;
            assert!(parse_replay_trace(input).is_err(), "{key}");
        }

        for tick in [0, 2] {
            let mut input = versioned();
            input["events"][0]["tick"] = json!(tick);
            assert!(parse_replay_trace(input).is_err(), "tick {tick}");
        }
        let mut input = versioned();
        input["events"][0]["timestamp"] = json!("");
        assert!(parse_replay_trace(input).is_err());

        let mut input = versioned();
        input["extra"] = json!(true);
        assert!(parse_replay_trace(input).is_err());
        let mut input = versioned();
        input["events"][0]["extra"] = json!(true);
        assert!(parse_replay_trace(input).is_err());
        for missing in ["params", "state"] {
            let mut input = versioned();
            input["events"][0]
                .as_object_mut()
                .expect("event")
                .remove(missing);
            assert!(parse_replay_trace(input).is_err(), "{missing}");
        }
    }

    #[test]
    fn v1_accepts_both_published_kernel_majors() {
        for version in [
            fsl_core::KERNEL_V1_SCHEMA_VERSION,
            fsl_core::KERNEL_V2_SCHEMA_VERSION,
        ] {
            let mut input = versioned();
            input["kernel_schema_version"] = json!(version);
            let parsed = parse_replay_trace(input).expect("supported Kernel version");
            assert!(matches!(parsed.contract, ReplayTraceContract::V1 { .. }));
        }
    }

    #[test]
    fn v1_1_adds_explicit_stutter_without_reserving_an_action_name() {
        let mut stutter = versioned();
        stutter["schema_version"] = json!(fsl_core::REPLAY_TRACE_V1_STUTTER_SCHEMA_VERSION);
        stutter["events"][0]["action"] = Value::Null;
        let parsed = parse_replay_trace(stutter.clone()).expect("v1.1 stutter");
        assert_eq!(parsed.events[0].step, ReplayStep::Stutter);

        let mut current = stutter.clone();
        current["schema_version"] = json!(fsl_core::REPLAY_TRACE_V1_SCHEMA_VERSION);
        assert!(parse_replay_trace(current).is_ok());

        stutter["schema_version"] = json!(fsl_core::REPLAY_TRACE_V1_INITIAL_SCHEMA_VERSION);
        assert!(parse_replay_trace(stutter).is_err());

        let mut params = versioned();
        params["events"][0]["action"] = Value::Null;
        params["events"][0]["params"] = json!({"unexpected":true});
        assert!(parse_replay_trace(params).is_err());

        let mut named = versioned();
        named["events"][0]["action"] = json!("stutter");
        let parsed = parse_replay_trace(named).expect("ordinary action name");
        assert!(matches!(
            &parsed.events[0].step,
            ReplayStep::Action { name, .. } if name == "stutter"
        ));
    }
}

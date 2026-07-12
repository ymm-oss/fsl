// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: fsl-bmc SPEC [DEPTH]");
    let depth = args
        .next()
        .map_or(Ok(4_usize), |value| value.parse::<usize>())
        .expect("depth must be a non-negative integer");
    match run(&path, depth) {
        Ok(result) => println!(
            "{}",
            serde_json::to_string(&json!({
                "spec": result.spec,
                "depth": result.depth,
                "violation": result.violation.as_ref().map(|violation| json!({
                    "kind": violation.kind,
                    "name": violation.name,
                    "step": violation.step,
                })),
                "reachables": result.reachables.iter().map(|(name, witness)| (
                    name.clone(),
                    witness.as_ref().map(|witness| witness.step),
                )).collect::<std::collections::BTreeMap<_, _>>(),
                "deadlock_step": result.deadlock_step,
                "action_coverage": result.action_coverage,
                "witnesses": {
                    "violation": result.violation.as_ref().map(|violation| trace_json(&violation.trace)),
                    "reachables": result.reachables.iter().filter_map(|(name, witness)| {
                        witness.as_ref().map(|witness| (name.clone(), trace_json(&witness.trace)))
                    }).collect::<std::collections::BTreeMap<_, _>>(),
                    "deadlock": result.deadlock_trace.as_ref().map(|trace| trace_json(trace)),
                },
            }))
            .expect("serialize BMC result")
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

fn run(path: &str, depth: usize) -> Result<fsl_verifier::BmcResult, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let base = std::path::Path::new(path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel =
        fsl_core::parse_kernel_source(&source, &resolver).map_err(|error| error.to_string())?;
    let model = fsl_core::build_model(kernel).map_err(|error| error.to_string())?;
    let mut solver = fsl_solver_z3::Z3Solver::new().map_err(|error| error.to_string())?;
    let result = block_on_native(fsl_verifier::verify_bounded(&model, &mut solver, depth))
        .map_err(|error| error.to_string())?;
    replay_witnesses(&model, &result)?;
    Ok(result)
}

fn replay_witnesses(
    model: &fsl_core::KernelModel,
    result: &fsl_verifier::BmcResult,
) -> Result<(), String> {
    if let Some(violation) = &result.violation {
        fsl_runtime::replay_trace(model.clone(), &violation.trace)
            .map_err(|error| format!("counterexample replay failed: {error}"))?;
    }
    for (name, witness) in &result.reachables {
        if let Some(witness) = witness {
            fsl_runtime::replay_trace(model.clone(), &witness.trace)
                .map_err(|error| format!("reachable '{name}' replay failed: {error}"))?;
        }
    }
    if let Some(trace) = &result.deadlock_trace {
        fsl_runtime::replay_trace(model.clone(), trace)
            .map_err(|error| format!("deadlock replay failed: {error}"))?;
    }
    Ok(())
}

fn trace_json(trace: &[fsl_core::TraceStep]) -> serde_json::Value {
    serde_json::Value::Array(
        trace
            .iter()
            .map(|entry| {
                let mut value = serde_json::Map::new();
                value.insert("step".to_owned(), json!(entry.step));
                value.insert(
                    "state".to_owned(),
                    serde_json::Value::Object(
                        entry
                            .state
                            .iter()
                            .map(|(name, value)| (name.clone(), fsl_value_json(value)))
                            .collect(),
                    ),
                );
                if let Some(action) = &entry.action {
                    value.insert(
                        "action".to_owned(),
                        json!({
                            "name": action.name,
                            "params": action.params.iter().map(|(name, value)| (
                                name.clone(), fsl_value_json(value)
                            )).collect::<serde_json::Map<_, _>>(),
                        }),
                    );
                    value.insert(
                        "changes".to_owned(),
                        serde_json::Value::Object(
                            entry
                                .changes
                                .iter()
                                .map(|(name, change)| {
                                    (
                                        name.clone(),
                                        json!({
                                            "from": fsl_value_json(&change.from),
                                            "to": fsl_value_json(&change.to),
                                        }),
                                    )
                                })
                                .collect(),
                        ),
                    );
                }
                serde_json::Value::Object(value)
            })
            .collect(),
    )
}

fn fsl_value_json(value: &fsl_core::FslValue) -> serde_json::Value {
    match value {
        fsl_core::FslValue::Int(value) => json!(value),
        fsl_core::FslValue::Bool(value) => json!(value),
        fsl_core::FslValue::Enum { member, .. } => json!(member),
        fsl_core::FslValue::None => serde_json::Value::Null,
        fsl_core::FslValue::Some(value) => fsl_value_json(value),
        fsl_core::FslValue::Struct { fields, .. } => serde_json::Value::Object(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), fsl_value_json(value)))
                .collect(),
        ),
        fsl_core::FslValue::Map(entries) => serde_json::Value::Object(
            entries
                .iter()
                .map(|(key, value)| (map_key(key), fsl_value_json(value)))
                .collect(),
        ),
        fsl_core::FslValue::Set(values) => {
            serde_json::Value::Array(values.iter().map(fsl_value_json).collect())
        }
        fsl_core::FslValue::Seq(values) => {
            serde_json::Value::Array(values.iter().map(fsl_value_json).collect())
        }
        fsl_core::FslValue::Relation(values) => serde_json::Value::Array(
            values
                .iter()
                .map(|(source, target)| json!([fsl_value_json(source), fsl_value_json(target)]))
                .collect(),
        ),
    }
}

fn map_key(value: &fsl_core::FslValue) -> String {
    match value {
        fsl_core::FslValue::Int(value) => value.to_string(),
        fsl_core::FslValue::Bool(value) => value.to_string(),
        fsl_core::FslValue::Enum { member, .. } => member.clone(),
        _ => format!("{value:?}"),
    }
}

fn block_on_native<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("native Z3 backend unexpectedly yielded Pending"),
    }
}

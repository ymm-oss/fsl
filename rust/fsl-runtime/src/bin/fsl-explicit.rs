// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! A minimal driver for [`fsl_runtime::verify_explicit`], the same shape as
//! `fsl-bfs`. Exists so a memory-ceiling regression test can exercise the
//! explicit-state lane's own `(State, usize)`/`BTreeSet<State>` frontier in
//! isolation, without the `fslc verify` CLI's Z3-backed vacuity checks
//! (which run regardless of `--engine` and would fold solver memory into
//! the measurement, `fsl-runtime` itself never links a solver at all --
//! see `rust-verifier.md`'s dependency-direction rule).

use std::fs;

use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: fsl-explicit SPEC [DEPTH] [MAX_STATES]");
    let depth = args
        .next()
        .map_or(Ok(4_usize), |value| value.parse::<usize>())
        .expect("depth must be a non-negative integer");
    let max_states = args
        .next()
        .map_or(Ok(1_000_000_usize), |value| value.parse::<usize>())
        .expect("max_states must be a positive integer");
    let result = fs::read_to_string(&path)
        .map_err(|error| error.to_string())
        .and_then(|source| {
            let base = std::path::Path::new(&path)
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."));
            let resolver = fsl_core::FsResolver::new(base);
            fsl_core::parse_kernel_source(&source, &resolver).map_err(|error| error.to_string())
        })
        .and_then(|kernel| fsl_core::build_model(kernel).map_err(|error| error.to_string()))
        .and_then(|model| {
            fsl_runtime::verify_explicit(model, depth, max_states)
                .map_err(|error| error.to_string())
        });
    match result {
        Ok(result) => println!(
            "{}",
            serde_json::to_string(&json!({
                "spec": result.spec,
                "depth": result.depth,
                "depth_reached": result.depth_reached,
                "states_explored": result.states_explored,
                "max_frontier_width": result.max_frontier_width,
                "closure": result.closure,
                "budget_exceeded": result.budget_exceeded,
                "violation": result.violation.as_ref().map(|violation| json!({
                    "kind": violation.violation.kind,
                    "name": violation.violation.name,
                    "step": violation.violation.step,
                })),
                "deadlock_step": result.deadlock_step,
                "action_coverage": result.action_coverage,
            }))
            .expect("serialize explicit-engine result")
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

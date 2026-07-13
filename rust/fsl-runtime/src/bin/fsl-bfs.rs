// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::fs;

use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: fsl-bfs SPEC [DEPTH]");
    let depth = args
        .next()
        .map_or(Ok(4_usize), |value| value.parse::<usize>())
        .expect("depth must be a non-negative integer");
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
        .and_then(|model| fsl_runtime::bfs(model, depth).map_err(|error| error.to_string()));
    match result {
        Ok(result) => println!(
            "{}",
            serde_json::to_string(&json!({
                "spec": result.spec,
                "depth": result.depth,
                "states_explored": result.states_explored,
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
            }))
            .expect("serialize BFS result")
        ),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

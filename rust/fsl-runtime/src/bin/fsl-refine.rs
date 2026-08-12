// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! A minimal driver for [`fsl_runtime::check_refinement`], the same shape as
//! `fsl-bfs`/`fsl-explicit`. Exists so a memory-ceiling regression test can
//! exercise `check_refinement`'s own correspondence walk in isolation,
//! without the `fslc refine` CLI's Z3-backed `progress` check (which runs
//! whenever the mapping declares one and would fold solver memory into the
//! measurement -- `fsl-runtime` itself never links a solver at all, see
//! `rust-verifier.md`'s dependency-direction rule).

use std::fs;
use std::path::Path;

use serde_json::json;

fn load_model(path: &str) -> Result<fsl_core::KernelModel, String> {
    let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let base = Path::new(path).parent().unwrap_or_else(|| Path::new("."));
    let resolver = fsl_core::FsResolver::new(base);
    let kernel =
        fsl_core::parse_kernel_source(&source, &resolver).map_err(|error| error.to_string())?;
    fsl_core::build_model(kernel).map_err(|error| error.to_string())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: fsl-refine IMPL ABS MAPPING [DEPTH]";
    let implementation_path = args.next().expect(usage);
    let abstraction_path = args.next().expect(usage);
    let mapping_path = args.next().expect(usage);
    let depth = args
        .next()
        .map_or(Ok(8_usize), |value| value.parse::<usize>())
        .expect("depth must be a non-negative integer");
    let result = load_model(&implementation_path).and_then(|implementation| {
        let abstraction = load_model(&abstraction_path)?;
        let mapping_source =
            fs::read_to_string(&mapping_path).map_err(|error| error.to_string())?;
        let mapping = fsl_core::parse_refinement(&mapping_source, &implementation, &abstraction)
            .map_err(|error| error.message)?;
        fsl_runtime::check_refinement(&implementation, &abstraction, &mapping, depth)
            .map_err(|error| error.to_string())
    });
    match result {
        Ok(checked) => {
            let (verdict, kind) = if let Some((violation, _)) = &checked.impl_violation {
                ("impl_violation", Some(violation.kind.clone()))
            } else if let Some(failure) = &checked.failure {
                ("refinement_failed", Some(failure.kind.clone()))
            } else {
                ("refines", None)
            };
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "implementation": checked.implementation,
                    "abstraction": checked.abstraction,
                    "depth": checked.depth,
                    "verdict": verdict,
                    "kind": kind,
                }))
                .expect("serialize refinement result")
            );
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

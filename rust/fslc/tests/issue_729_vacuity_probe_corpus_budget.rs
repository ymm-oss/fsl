// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita
//
// Corpus conservation control for issue #729's shared vacuity-reachability
// probe, mirroring `rust/fslc/tests/issue_697_corpus_probe_budget.rs`'s
// control for `CONCRETE_PROBE_BUDGET` on the concrete boundary pre-pass.
//
// `verification_warnings`'s `vacuous_implication`/`vacuous_leadsto` lanes
// now share one budgeted BFS (`fsl_runtime::expression_reachability`) over
// every implication antecedent and leadsTo trigger candidate, budgeted at
// the same `CONCRETE_PROBE_BUDGET` `find_boundary_violation` uses. Reaching
// the budget before every candidate resolves degrades a real
// `vacuous_implication`/`vacuous_leadsto` finding to `vacuity_probe_truncated`
// -- sound (fail-closed, not fail-open), but a UX regression for any spec
// that would otherwise get a clean vacuity verdict. This control fails
// loudly if any spec in the maintained corpus would lose that clean verdict
// to truncation, exactly like the `#697` control it mirrors.

use std::path::{Path, PathBuf};

fn kernel_specs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut out = Vec::new();
    for dir in ["specs", "examples"] {
        let mut stack = vec![root.join(dir)];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|s| s.to_str()) == Some("fsl") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

#[test]
fn no_corpus_spec_exhausts_the_shared_vacuity_reachability_budget() {
    let budget = fsl_runtime::CONCRETE_PROBE_BUDGET;
    let mut truncated: Vec<String> = Vec::new();
    let mut probed = 0usize;
    let mut probed_with_candidates = 0usize;

    for path in kernel_specs() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(document) = fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new("."))
        else {
            continue;
        };
        let Ok(model) = fsl_core::build_model(document) else {
            continue;
        };
        let implication_candidates = fsl_runtime::vacuous_implication_candidates(&model);
        let leadsto_candidates = fsl_runtime::vacuous_leadsto_candidates(&model);
        let mut expressions: Vec<_> = implication_candidates
            .iter()
            .map(|(_, expr)| expr.clone())
            .collect();
        expressions.extend(leadsto_candidates.iter().cloned());
        if expressions.is_empty() {
            continue;
        }
        // Depth 8 matches this corpus control's `#697` counterpart and the
        // product default (`DEFAULT_DEPTH` in `rust/fslc/src/main.rs`).
        let Ok(results) = fsl_runtime::expression_reachability(&model, &expressions, 8, budget)
        else {
            continue;
        };
        probed += 1;
        probed_with_candidates += 1;
        if results
            .iter()
            .any(|result| matches!(result, fsl_runtime::Reachability::Exhausted))
        {
            truncated.push(path.display().to_string());
        }
    }

    println!(
        "probed {probed} corpus specs at depth 8, budget {budget}, \
         {probed_with_candidates} carried at least one vacuity-reachability candidate"
    );
    assert!(
        truncated.is_empty(),
        "these corpus specs would degrade a vacuous_implication/vacuous_leadsto finding to \
         vacuity_probe_truncated under the shared budget: {truncated:#?}"
    );
}

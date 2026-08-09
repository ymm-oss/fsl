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
//
// Every corpus `.fsl` is accounted for by exactly one bucket below --
// `parse_kernel_source`/`build_model` failing (e.g. a domain/requirements/
// business-dialect spec `fsl_core::parse_kernel_source` does not lower on
// its own) is expected and not itself evidence of anything, but this
// repository's discipline is not to build an implicit exclusion: the count
// is printed, not swallowed silently (review of an earlier version of this
// file found `probed`/`probed_with_candidates` incremented in the same
// place, making them tautologically equal, and the corpus's actual total
// `.fsl` count -- 214 at review time -- going unreported).

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
    let total = kernel_specs().len();
    let mut unreadable = 0usize;
    let mut not_a_kernel_source = 0usize;
    let mut model_build_failed = 0usize;
    let mut no_candidates = 0usize;
    let mut probe_errored = 0usize;
    let mut probed = 0usize;

    for path in kernel_specs() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            unreadable += 1;
            continue;
        };
        let Ok(document) = fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new("."))
        else {
            // Expected for a non-kernel-dialect spec (domain/requirements/
            // business documents do not lower through this entrypoint on
            // their own); counted, not silently excluded.
            not_a_kernel_source += 1;
            continue;
        };
        let Ok(model) = fsl_core::build_model(document) else {
            model_build_failed += 1;
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
            no_candidates += 1;
            continue;
        }
        // Depth 8 matches this corpus control's `#697` counterpart and the
        // product default (`DEFAULT_DEPTH` in `rust/fslc/src/main.rs`).
        let Ok(results) = fsl_runtime::expression_reachability(&model, &expressions, 8, budget)
        else {
            probe_errored += 1;
            continue;
        };
        probed += 1;
        if results
            .iter()
            .any(|result| matches!(result, fsl_runtime::Reachability::Exhausted))
        {
            truncated.push(path.display().to_string());
        }
    }

    let accounted = unreadable
        + not_a_kernel_source
        + model_build_failed
        + no_candidates
        + probe_errored
        + probed;
    println!(
        "{total} corpus .fsl files total; unreadable={unreadable} \
         not_a_kernel_source={not_a_kernel_source} model_build_failed={model_build_failed} \
         no_candidates={no_candidates} probe_errored={probe_errored} probed={probed} \
         (depth 8, budget {budget})"
    );
    assert_eq!(
        accounted, total,
        "every corpus .fsl must land in exactly one bucket -- a mismatch means this loop silently \
         dropped a file instead of accounting for it"
    );
    assert!(
        probed > 0,
        "no corpus spec exercised the probe at all -- this control would pass vacuously"
    );
    assert!(
        truncated.is_empty(),
        "these corpus specs would degrade a vacuous_implication/vacuous_leadsto finding to \
         vacuity_probe_truncated under the shared budget: {truncated:#?}"
    );
}

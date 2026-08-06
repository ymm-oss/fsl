// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita
//
// Corpus conservation control for `CONCRETE_PROBE_BUDGET` (#697).
//
// The budget exists so the concrete boundary pre-pass cannot consume unbounded
// memory. Exhausting it is sound -- the run falls through to the symbolic
// engine, which fails closed on the one class the pre-pass uniquely covers (a
// reachable over-capacity `Seq`) -- but it is a UX regression: a spec whose
// violation today comes back as `violated` with a concrete replayable trace
// would instead come back as an error. So the budget must stay generous enough
// that no spec in the maintained corpus exhausts it.
//
// Measured when the budget was set (167 corpus specs, depth 8, budget 50_000):
// the largest pre-pass explored 23_409 states, in `examples/named_predicate.fsl`
// -- about 2.1x of headroom. That margin is thin enough that this control, not
// the margin, is what protects the property: a newly added spec that would
// exhaust the budget fails here loudly instead of silently losing its concrete
// evidence. When it fails, raise `CONCRETE_PROBE_BUDGET` deliberately and
// record the new measured maximum above, or establish that the new spec should
// not be probed at all.

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
fn no_corpus_spec_exhausts_the_concrete_probe_budget() {
    let budget = fsl_runtime::CONCRETE_PROBE_BUDGET;
    let mut max_states = 0usize;
    let mut max_path = String::new();
    let mut exhausted: Vec<String> = Vec::new();
    let mut probed = 0usize;

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
        if fsl_runtime::deterministic_initial_state(&model).is_err() {
            continue;
        }
        let Ok(probe) = fsl_runtime::find_boundary_violation(&model, 8, budget) else {
            continue;
        };
        probed += 1;
        if probe.states_explored > max_states {
            max_states = probe.states_explored;
            max_path = path.display().to_string();
        }
        if probe.exhausted {
            exhausted.push(format!(
                "{} ({} states)",
                path.display(),
                probe.states_explored
            ));
        }
    }

    println!("probed {probed} corpus specs at depth 8, budget {budget}");
    println!("max states_explored = {max_states} in {max_path}");
    // Integer tenths, so the report needs no lossy float cast (clippy denies
    // `cast_precision_loss` in this workspace).
    let headroom_tenths = budget.saturating_mul(10) / max_states.max(1);
    println!(
        "headroom factor = {}.{}x",
        headroom_tenths / 10,
        headroom_tenths % 10
    );
    assert!(
        exhausted.is_empty(),
        "these corpus specs exhaust the budget, so their concrete evidence would \
         degrade to the fail-closed symbolic error: {exhausted:#?}"
    );
}

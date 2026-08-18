// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Regression coverage for issue #730's `verify_explicit_selected` lane:
//! its frontier used to be `BTreeMap<State, Monitor>`, cloning the whole
//! `KernelModel` per live frontier state per level
//! (`rust/fsl-runtime/src/explicit.rs`, before this fix). The fix carries a
//! bare `BTreeSet<State>` instead and re-points one scratch `Monitor` at
//! each frontier state.
//!
//! Unlike the `bfs` lane (see `issue_730_bfs_memory_ceiling.rs`), this
//! lane's memory is not dominated by the model clone alone: level-
//! synchronous exploration additionally holds one `Vec<EnabledAction>` per
//! *simultaneously live* frontier state in `enabled_by_state`
//! (`rust/fsl-runtime/src/explicit.rs`), each carrying its own
//! `bindings`/`params` maps -- a cost this fix does not touch and #730
//! never claimed to. Measured directly with `fsl-explicit` (a driver for
//! `fsl_runtime::verify_explicit`, the same shape as `fsl-bfs`, added
//! alongside this test specifically so the measurement is not diluted by
//! `fslc verify`'s own Z3-backed vacuity checks, which run regardless of
//! `--engine`): peak memory footprint on the `LabelCoreRepro` reproducer
//! (`support::LABEL_CORE_REPRO_SOURCE`, shared with
//! `issue_730_bfs_memory_ceiling.rs` and `issue_783_refine_memory_ceiling.rs`)
//! (release build, depth 3, 16,290 states, max frontier width
//! 15,424) dropped from ~1,783 MB (pre-fix) to ~954 MB (post-fix) --
//! real and state-count-identical, but a much smaller *relative* drop than
//! `bfs`'s ~4x, because `enabled_by_state`'s cost is unaffected and sets
//! the floor both before and after.
//!
//! Same platform caveat as `issue_730_bfs_memory_ceiling.rs`: macOS does
//! not enforce `RLIMIT_AS`, so this only runs on Linux (where CI actually
//! executes it), and the calibration below crosses from a macOS RSS-style
//! measurement to a Linux `ulimit -v` (`RLIMIT_AS`) ceiling -- a
//! monotonicity assumption, not a second direct measurement on the
//! enforcing platform. Because this lane's post/pre ratio (~0.54) leaves
//! much less headroom than `bfs`'s (~0.25), the ceiling below sits closer
//! to the post-fix baseline than `issue_730_bfs_memory_ceiling.rs`'s does;
//! if it flakes, widen `CEILING_KB` rather than tightening this comment's
//! claim.
#![cfg(target_os = "linux")]

use std::process::Command;

use serde_json::Value;

#[path = "support/mod.rs"]
mod support;
use support::{Fixture, LABEL_CORE_REPRO_SOURCE};

#[test]
fn explicit_stays_under_a_calibrated_ceiling_for_the_branching_reproducer() {
    // ~954 MB measured post-fix baseline, ~1,783 MB measured pre-fix, in KiB.
    const CEILING_KB: u64 = 1_300 * 1024;

    let fixture = Fixture::new("explicit-ceiling", LABEL_CORE_REPRO_SOURCE);
    let command = format!(
        "ulimit -v {CEILING_KB} && exec {:?} {:?} 3",
        env!("CARGO_BIN_EXE_fsl-explicit"),
        fixture.text(),
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .expect("run capped fsl-explicit");
    assert!(
        output.status.success(),
        "expected the explicit engine to finish inside a {CEILING_KB} KiB address-space \
         ceiling (calibrated between the measured post-fix and pre-fix peaks); status={:?} \
         stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON under the capped run: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(value["states_explored"], 16290, "{value:#}");
    assert_eq!(value["max_frontier_width"], 15424, "{value:#}");
}

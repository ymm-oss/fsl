// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative control for #512: `fslc refine` must classify a zero divisor in
//! an action-correspondence *argument* expression (`go2(a) -> go(a / c)`,
//! where `c` is an impl state variable that can be zero) as a located
//! `result:"refinement_failed"` / `kind:"map_partial_op"` finding, matching
//! the documented closed set of refinement failure kinds
//! (`docs/DESIGN-refinement.md`, `docs/LANGUAGE.md`). Before the fix this
//! surfaced as an unclassified internal error: `result:"error"`,
//! `kind:"type"`, `message:"division by zero"` -- neither of the two
//! documented `/0` treatments (`docs/DESIGN-divmod.md` §2.1/§2.3
//! totalization or §2.2 `partial_op`), and not a member of the refinement
//! contract's kind set at all.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn run(args: &[String]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn write(path: &Path, source: &str) {
    std::fs::write(path, source).expect("write fixture");
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fslc-issue-512-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    dir
}

const ABS: &str = "spec DivMapAbs { state { q: 0..10 } init { q = 0 } \
     action go(v in 0..10) { q = v } }";
const IMPL: &str = "spec DivMapImpl { state { q: 0..10, c: 0..5 } init { q = 0  c = 1 } \
     action set_c(v in 0..5) { c = v } \
     action go2(a in 0..10) { q = a } }";
const MAPPING: &str = "refinement M { impl DivMapImpl abs DivMapAbs map q = q \
     action set_c(v) -> stutter action go2(a) -> go(a / c) }";

/// `fslc refine` on a correspondence argument that can divide by zero must
/// report `refinement_failed`/exit 1 with `kind:"map_partial_op"`, not an
/// unclassified `error`/`kind:"type"`.
#[test]
fn refine_reports_map_partial_op_for_a_zero_divisor_in_a_correspondence_argument() {
    let dir = scratch("refine");
    let implementation = dir.join("impl.fsl");
    let abstraction = dir.join("abs.fsl");
    let mapping = dir.join("map.fsl");
    write(&implementation, IMPL);
    write(&abstraction, ABS);
    write(&mapping, MAPPING);

    let (output, status) = run(&[
        "refine".to_owned(),
        implementation.display().to_string(),
        abstraction.display().to_string(),
        mapping.display().to_string(),
        "--depth".to_owned(),
        "3".to_owned(),
    ]);
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["result"], "refinement_failed");
    assert_eq!(output["kind"], "map_partial_op");
    assert_ne!(output["result"], "error");
}

/// Regression control: the same correspondence argument division, guarded
/// so the divisor is never zero on any reachable impl step, must still
/// `refines`/exit 0.
#[test]
fn refine_still_refines_when_the_correspondence_divisor_is_always_guarded() {
    let dir = scratch("refine-guarded");
    let implementation = dir.join("impl.fsl");
    let abstraction = dir.join("abs.fsl");
    let mapping = dir.join("map.fsl");
    write(
        &implementation,
        "spec DivMapImplGuarded { state { q: 0..10, c: 0..5 } init { q = 0  c = 1 } \
         action set_c(v in 0..5) { requires v != 0  c = v } \
         action go2(a in 0..10) { q = a / c } }",
    );
    write(&abstraction, ABS);
    write(
        &mapping,
        "refinement M { impl DivMapImplGuarded abs DivMapAbs map q = q \
         action set_c(v) -> stutter action go2(a) -> go(a / c) }",
    );

    let (output, status) = run(&[
        "refine".to_owned(),
        implementation.display().to_string(),
        abstraction.display().to_string(),
        mapping.display().to_string(),
        "--depth".to_owned(),
        "3".to_owned(),
    ]);
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "refines");
}

// SPDX-License-Identifier: Apache-2.0

//! Native CLI coverage for `fslc refine`'s multi-link chain route (issue #850).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT_SCRATCH: AtomicUsize = AtomicUsize::new(0);

struct ChainFiles {
    implementation: PathBuf,
    middle: PathBuf,
    top: PathBuf,
    impl_to_middle: PathBuf,
    middle_to_top: PathBuf,
    broken_middle_to_top: PathBuf,
}

fn scratch(label: &str) -> PathBuf {
    let ordinal = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "fslc-issue-850-{label}-{}-{ordinal}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

fn write(path: &Path, source: &str) {
    std::fs::write(path, source).expect("write fixture");
}

fn chain_files(directory: &Path) -> ChainFiles {
    let implementation = directory.join("impl.fsl");
    let middle = directory.join("middle.fsl");
    let top = directory.join("top.fsl");
    let impl_to_middle = directory.join("impl-middle.fsl");
    let middle_to_top = directory.join("middle-top.fsl");
    let broken_middle_to_top = directory.join("middle-top-broken.fsl");

    // The three layers deliberately use distinct enum names. `refine` merges
    // type metadata by name, so reusing a name would let this fixture hide a
    // layer's members instead of exercising the complete chain.
    write(
        &implementation,
        r"spec ImplLayer {
  enum ImplPhase { ImplIdle, ImplDone }
  state { impl_phase: ImplPhase }
  init { impl_phase = ImplIdle }
  action advance() { requires impl_phase == ImplIdle impl_phase = ImplDone }
  action ping() { requires impl_phase == ImplIdle impl_phase = ImplIdle }
}
",
    );
    write(
        &middle,
        r"spec MiddleLayer {
  enum MiddlePhase { MiddleWaiting, MiddleComplete }
  state { middle_phase: MiddlePhase }
  init { middle_phase = MiddleWaiting }
  action advance_middle() { requires middle_phase == MiddleWaiting middle_phase = MiddleComplete }
  action ping_middle() { requires middle_phase == MiddleWaiting middle_phase = MiddleWaiting }
}
",
    );
    write(
        &top,
        r"spec TopLayer {
  enum TopPhase { TopOpen, TopClosed }
  state { top_phase: TopPhase }
  init { top_phase = TopOpen }
  action advance_top() { requires top_phase == TopOpen top_phase = TopClosed }
}
",
    );
    write(
        &impl_to_middle,
        r"refinement ImplToMiddle {
  impl ImplLayer
  abs MiddleLayer
  enum conversion impl_middle ImplPhase -> MiddlePhase {
    ImplIdle -> MiddleWaiting
    ImplDone -> MiddleComplete
  }
  map middle_phase = convert(impl_middle, impl_phase)
  action advance() -> advance_middle()
  action ping() -> ping_middle()
}
",
    );
    write(
        &middle_to_top,
        r"refinement MiddleToTop {
  impl MiddleLayer
  abs TopLayer
  enum conversion middle_top MiddlePhase -> TopPhase {
    MiddleWaiting -> TopOpen
    MiddleComplete -> TopClosed
  }
  map top_phase = convert(middle_top, middle_phase)
  action advance_middle() -> advance_top()
  action ping_middle() -> stutter
}
",
    );
    write(
        &broken_middle_to_top,
        r"refinement BrokenMiddleToTop {
  impl MiddleLayer
  abs TopLayer
  enum conversion middle_top MiddlePhase -> TopPhase {
    MiddleWaiting -> TopOpen
    MiddleComplete -> TopClosed
  }
  map top_phase = TopOpen
  action advance_middle() -> advance_top()
  action ping_middle() -> stutter
}
",
    );

    ChainFiles {
        implementation,
        middle,
        top,
        impl_to_middle,
        middle_to_top,
        broken_middle_to_top,
    }
}

fn run(args: &[String]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc {}`: {error}; stderr={}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let status = output
        .status
        .code()
        .expect("native CLI terminated without a signal");
    (value, status)
}

fn chain_args(files: &ChainFiles, final_mapping: &Path) -> Vec<String> {
    vec![
        "refine".to_owned(),
        files.implementation.display().to_string(),
        files.middle.display().to_string(),
        files.impl_to_middle.display().to_string(),
        files.top.display().to_string(),
        final_mapping.display().to_string(),
        "--depth".to_owned(),
        "2".to_owned(),
    ]
}

#[test]
fn refine_chain_composes_action_maps_across_three_distinct_layers() {
    let files = chain_files(&scratch("success"));
    let (result, status) = run(&chain_args(&files, &files.middle_to_top));

    assert_eq!(status, 0, "{result}");
    assert_eq!(result["result"], "refines", "{result}");
    assert_eq!(
        result["chain"],
        json!(["ImplLayer", "MiddleLayer", "TopLayer"]),
        "{result}"
    );
    assert_eq!(
        result["action_map"],
        json!({"advance": "advance_top", "ping": "stutter"}),
        "{result}"
    );
}

#[test]
fn refine_chain_reports_the_intermediate_link_that_fails() {
    let files = chain_files(&scratch("intermediate-failure"));
    let (result, status) = run(&chain_args(&files, &files.broken_middle_to_top));

    assert_eq!(status, 1, "{result}");
    assert_eq!(result["result"], "refinement_failed", "{result}");
    assert_eq!(
        result["failed_link"],
        json!({
            "from": "MiddleLayer",
            "to": "TopLayer",
            "kind": "abs_state_mismatch",
        }),
        "{result}"
    );
}

#[test]
fn refine_chain_rejects_an_unpaired_final_operand() {
    let files = chain_files(&scratch("unpaired-operand"));
    let args = vec![
        "refine".to_owned(),
        files.implementation.display().to_string(),
        files.middle.display().to_string(),
        files.impl_to_middle.display().to_string(),
        files.top.display().to_string(),
        "--depth".to_owned(),
        "2".to_owned(),
    ];
    let (result, status) = run(&args);

    assert_eq!(status, 2, "{result}");
    assert_eq!(result["result"], "error", "{result}");
    assert_eq!(result["kind"], "io", "{result}");
    assert_eq!(
        result["message"], "refine chain must list (abs map) pairs after the first mapping",
        "{result}"
    );
}

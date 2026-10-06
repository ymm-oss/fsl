// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `check_refinement`'s correspondence walk stops at
//! `fsl_runtime::IMPLEMENTS_SEARCH_BUDGET` (50,000 states, #1041) for every
//! caller, but only inline `implements` read the cutoff. `fslc refine` (single
//! and chain), the `fslc chain` refine layer and governance preservations
//! reported the cut-off walk as `refines` / exit 0. Each entry point gets a
//! cutoff case (fails if that entry stops reading the cutoff) and a control on
//! the same files at a depth below the budget (fails if the entry reports the
//! cutoff when there is none).
//!
//! The fixture is the smallest shape that crosses the real budget: `Seq<V, 6>`
//! over six values has `sum_{i=0}^{6} 6^i = 55,987` reachable states, all of
//! them by depth 6, and 1,555 by depth 4. Like
//! `issue_1041_implements_budget.rs`, the files are written to a scratch
//! directory instead of the corpus, so the all-corpus `check` sweep does not
//! pay for them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

const BUDGET: u64 = 50_000;

fn wide_spec(name: &str, value_type: &str) -> String {
    format!(
        "spec {name} {{\n  type {value_type} = 0..5\n  state {{ seq: Seq<{value_type}, 6> }}\n  \
         init {{ seq = Seq {{}} }}\n  action push(v: {value_type}) {{\n    \
         requires seq.size() < 6\n    seq = seq.push(v)\n  }}\n}}\n"
    )
}

fn identity_mapping(name: &str, implementation: &str, abstraction: &str) -> String {
    format!("refinement {name} {{ impl {implementation} abs {abstraction} maps auto }}\n")
}

/// A scratch directory under `rust/target/` (gitignored) holding the three
/// layers `WideImpl` -> `WideMid` -> `WideAbs` and their identity mappings.
fn fixture(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/refine-budget-unknown-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    for (file, source) in [
        ("impl.fsl", wide_spec("WideImpl", "IV")),
        ("mid.fsl", wide_spec("WideMid", "MV")),
        ("abs.fsl", wide_spec("WideAbs", "AV")),
        (
            "impl_refines_mid.fsl",
            identity_mapping("ImplRefinesMid", "WideImpl", "WideMid"),
        ),
        (
            "mid_refines_abs.fsl",
            identity_mapping("MidRefinesAbs", "WideMid", "WideAbs"),
        ),
    ] {
        fs::write(dir.join(file), source).expect("write fixture file");
    }
    dir
}

fn fslc(dir: &Path, args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(dir)
        .env("FSLC_CACHE_DIR", dir.join("cache"))
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}); stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn assert_states_explored_reached_the_budget(value: &Value) {
    assert!(
        value["states_explored"]
            .as_u64()
            .is_some_and(|states| states >= BUDGET),
        "{value:#}"
    );
}

#[test]
fn refine_reports_unknown_budget_when_the_correspondence_walk_is_cut_off() {
    let dir = fixture("refine-cutoff");

    let (value, status) = fslc(
        &dir,
        &[
            "refine",
            "impl.fsl",
            "mid.fsl",
            "impl_refines_mid.fsl",
            "--depth",
            "6",
        ],
    );

    assert_eq!(status, 1, "{value:#}");
    assert_eq!(value["result"], "unknown_budget", "{value:#}");
    assert_eq!(value["impl"], "WideImpl", "{value:#}");
    assert_eq!(value["abs"], "WideMid", "{value:#}");
    assert_states_explored_reached_the_budget(&value);
    // Nothing was checked to the requested depth, so nothing claims it was.
    assert!(value.get("checked_to_depth").is_none(), "{value:#}");
    assert!(value.get("action_map").is_none(), "{value:#}");
}

#[test]
fn refine_still_refines_the_same_files_below_the_budget() {
    let dir = fixture("refine-control");

    let (value, status) = fslc(
        &dir,
        &[
            "refine",
            "impl.fsl",
            "mid.fsl",
            "impl_refines_mid.fsl",
            "--depth",
            "4",
        ],
    );

    assert_eq!(status, 0, "{value:#}");
    assert_eq!(value["result"], "refines", "{value:#}");
    assert_eq!(value["checked_to_depth"], 4, "{value:#}");
}

#[test]
fn refine_chain_stops_at_the_cut_off_link() {
    let dir = fixture("chain-cutoff");

    let (value, status) = fslc(
        &dir,
        &[
            "refine",
            "impl.fsl",
            "mid.fsl",
            "impl_refines_mid.fsl",
            "abs.fsl",
            "mid_refines_abs.fsl",
            "--depth",
            "6",
        ],
    );

    assert_eq!(status, 1, "{value:#}");
    assert_eq!(value["result"], "unknown_budget", "{value:#}");
    assert_eq!(
        value["failed_link"],
        serde_json::json!({"from": "WideImpl", "to": "WideMid", "kind": null}),
        "{value:#}"
    );
    assert_states_explored_reached_the_budget(&value);
}

#[test]
fn refine_chain_still_refines_the_same_files_below_the_budget() {
    let dir = fixture("chain-control");

    let (value, status) = fslc(
        &dir,
        &[
            "refine",
            "impl.fsl",
            "mid.fsl",
            "impl_refines_mid.fsl",
            "abs.fsl",
            "mid_refines_abs.fsl",
            "--depth",
            "4",
        ],
    );

    assert_eq!(status, 0, "{value:#}");
    assert_eq!(value["result"], "refines", "{value:#}");
    assert_eq!(
        value["chain"],
        serde_json::json!(["WideImpl", "WideMid", "WideAbs"]),
        "{value:#}"
    );
}

/// `[requirements]` and `[design]` are verified at depth 1 (cheap); only the
/// design -> requirements refine link uses `refine_depth`.
fn chain_manifest(refine_depth: u32) -> String {
    format!(
        "[requirements]\nfile = \"mid.fsl\"\ndepth = 1\n\n\
         [design]\nfile = \"impl.fsl\"\ndepth = 1\n\
         refine_against = \"requirements\"\nmapping = \"impl_refines_mid.fsl\"\n\
         refine_depth = {refine_depth}\n"
    )
}

fn refine_layer(value: &Value) -> &Value {
    value["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .find(|layer| layer["kind"] == "refine")
        .unwrap_or_else(|| panic!("no refine layer: {value:#}"))
}

#[test]
fn chain_manifest_fails_a_cut_off_refine_layer() {
    let dir = fixture("manifest-cutoff");
    fs::write(dir.join("fsl-project.toml"), chain_manifest(6)).expect("write manifest");

    let (value, status) = fslc(&dir, &["chain", "fsl-project.toml"]);

    let layer = refine_layer(&value);
    assert_eq!(layer["status"], "failed", "{value:#}");
    assert_eq!(layer["result"], "unknown_budget", "{value:#}");
    assert_eq!(layer["exit_code"], 1, "{value:#}");
    assert_states_explored_reached_the_budget(&layer["detail"]);
    assert_eq!(status, 1, "{value:#}");
}

#[test]
fn chain_manifest_still_passes_the_refine_layer_below_the_budget() {
    let dir = fixture("manifest-control");
    fs::write(dir.join("fsl-project.toml"), chain_manifest(4)).expect("write manifest");

    let (value, _status) = fslc(&dir, &["chain", "fsl-project.toml"]);

    let layer = refine_layer(&value);
    assert_eq!(layer["status"], "passed", "{value:#}");
    assert_eq!(layer["result"], "refines", "{value:#}");
    assert_eq!(layer["exit_code"], 0, "{value:#}");
}

/// Governance runs each preservation's refinement at depth 8, which covers
/// the whole 55,987-state fixture, so the cutoff case needs no depth knob.
fn governance(before: &str, after: &str, mapping: &str) -> String {
    format!(
        "governance WideControls {{\n  control CTRL-WIDE \"The sequence is preserved\"\n\n  \
         preservation WidePreserved {{\n    before {before}\n    after {after}\n    \
         preserve CTRL-WIDE\n    checked_by refinement \"{mapping}\"\n  }}\n}}\n"
    )
}

#[test]
fn governance_reports_unknown_budget_for_a_cut_off_preservation() {
    let dir = fixture("governance-cutoff");
    fs::write(
        dir.join("governance.fsl"),
        governance(
            "WideMid from \"mid.fsl\"",
            "WideImpl from \"impl.fsl\"",
            "impl_refines_mid.fsl",
        ),
    )
    .expect("write governance");

    let (value, status) = fslc(&dir, &["check", "governance.fsl"]);

    assert_eq!(
        value["governance"]["preservations"][0]["result"], "unknown_budget",
        "{value:#}"
    );
    // A preservation result does not fold into `check`'s own verdict; the
    // same holds for `refinement_failed` (cli_regression.rs).
    assert_eq!(status, 0, "{value:#}");
}

const SMALL_ABS: &str = "spec SmallAbs { type AQty = 0..3 state { n: AQty } init { n = 0 } \
     action bump() { requires n < 3  n = n + 1 } }\n";
const SMALL_IMPL: &str = "spec SmallImpl { type IQty = 0..3 state { n: IQty } init { n = 0 } \
     action bump() { requires n < 3  n = n + 1 } }\n";
const SMALL_SELF_VIOLATING: &str = "spec SmallBroken { type IQty = 0..3 state { n: IQty } \
     init { n = 0 } action bump() { n = n + 1 } }\n";

fn small_fixture(dir: &Path) {
    fs::write(dir.join("small_abs.fsl"), SMALL_ABS).expect("write");
    fs::write(dir.join("small_impl.fsl"), SMALL_IMPL).expect("write");
    fs::write(dir.join("small_broken.fsl"), SMALL_SELF_VIOLATING).expect("write");
    fs::write(
        dir.join("small_refines.fsl"),
        identity_mapping("SmallRefines", "SmallImpl", "SmallAbs"),
    )
    .expect("write");
    fs::write(
        dir.join("broken_refines.fsl"),
        identity_mapping("BrokenRefines", "SmallBroken", "SmallAbs"),
    )
    .expect("write");
}

#[test]
fn governance_still_refines_a_small_preservation() {
    let dir = fixture("governance-control");
    small_fixture(&dir);
    fs::write(
        dir.join("governance.fsl"),
        governance(
            "SmallAbs from \"small_abs.fsl\"",
            "SmallImpl from \"small_impl.fsl\"",
            "small_refines.fsl",
        ),
    )
    .expect("write governance");

    let (value, status) = fslc(&dir, &["check", "governance.fsl"]);

    assert_eq!(status, 0, "{value:#}");
    assert_eq!(
        value["governance"]["preservations"][0]["result"], "refines",
        "{value:#}"
    );
}

/// Native reads a self-violating `after` spec off `refine`'s own `violated`
/// verdict; the Worker must report the same value (`fsl-wasm` unit tests use
/// the same sources).
#[test]
fn governance_reports_violated_for_a_self_violating_after_spec() {
    let dir = fixture("governance-violated");
    small_fixture(&dir);
    fs::write(
        dir.join("governance.fsl"),
        governance(
            "SmallAbs from \"small_abs.fsl\"",
            "SmallBroken from \"small_broken.fsl\"",
            "broken_refines.fsl",
        ),
    )
    .expect("write governance");

    let (value, status) = fslc(&dir, &["check", "governance.fsl"]);

    assert_eq!(status, 0, "{value:#}");
    assert_eq!(
        value["governance"]["preservations"][0]["result"], "violated",
        "{value:#}"
    );
}

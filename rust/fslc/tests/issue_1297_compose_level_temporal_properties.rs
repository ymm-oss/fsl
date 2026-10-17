// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native contract for #1297: a compose-level `leadsTo`, `until`, or `unless`
//! reads component state through `alias.var`, as compose-level `invariant`,
//! `trans`, and `reachable` already do (docs/manual/LANGUAGE.md "Compose").
//!
//! Before the fix `rewrite_compose_item` resolved aliases only for the three
//! state/step properties, so `a.on` in a temporal property reached the Kernel
//! as a field access on an undeclared `a` and every such compose stopped with
//! exit 2 (`public Kernel cannot type identifier 'a'`). Each test asserts the
//! verdict the property really has -- verified where it holds, violated where
//! it does not -- so a fix that merely silenced the error would not pass.
//!
//! Every test here fails on `origin/main` (ade95232).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn scratch(files: &[(&str, &str)]) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace")
        .join(format!("target/issue-1297-{}-{id}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    for (name, source) in files {
        std::fs::write(dir.join(name), source).expect("write fixture");
    }
    dir
}

fn verify(dir: &Path, file: &str) -> (Value, i32) {
    let output: Output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["verify", file, "--depth", "4", "--no-cache"])
        .current_dir(dir)
        .output()
        .expect("run native fslc");
    let json = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (json, output.status.code().expect("exit status"))
}

/// `flip` is the only action, so `on` always becomes true again.
const ALWAYS_FLIPS: &str = "spec A {
  state { on: Bool }
  init { on = false }
  fair action flip() { on = not on }
}
";

/// `stay` can keep `on` false forever.
const CAN_STAY_OFF: &str = "spec A {
  state { on: Bool }
  init { on = false }
  action flip() { on = not on }
  action stay() { requires not on  on = false }
}
";

fn compose_with(property: &str) -> String {
    format!("compose C {{\n  use A as a from \"a.fsl\"\n  {property}\n}}\n")
}

/// The issue's reproduction: a cross-component `leadsTo` that holds verifies
/// and is reported under its compose-level name.
#[test]
fn compose_level_leads_to_over_component_state_verifies() {
    let dir = scratch(&[
        ("a.fsl", ALWAYS_FLIPS),
        (
            "c.fsl",
            &compose_with("leadsTo Flips { a.on == false ~> a.on == true }"),
        ),
    ]);
    let (json, code) = verify(&dir, "c.fsl");
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["result"], "verified", "{json}");
    assert!(json["leads_to"]["Flips"].is_object(), "{json}");
}

/// The same `leadsTo` over a component that can stay off is violated: the
/// property really reads `a.on`.
#[test]
fn compose_level_leads_to_over_component_state_detects_violation() {
    let dir = scratch(&[
        ("a.fsl", CAN_STAY_OFF),
        (
            "c.fsl",
            &compose_with("leadsTo Flips { a.on == false ~> a.on == true }"),
        ),
    ]);
    let (json, code) = verify(&dir, "c.fsl");
    assert_eq!(code, 1, "{json}");
    assert_eq!(json["result"], "violated", "{json}");
    assert_eq!(json["violation_kind"], "leadsTo", "{json}");
}

/// Compose-level `until` / `unless` over component state hold where the
/// component makes them hold.
#[test]
fn compose_level_until_and_unless_over_component_state_verify() {
    for property in [
        "until U { not a.on until a.on }",
        "unless U { not a.on unless a.on }",
    ] {
        let dir = scratch(&[("a.fsl", ALWAYS_FLIPS), ("c.fsl", &compose_with(property))]);
        let (json, code) = verify(&dir, "c.fsl");
        assert_eq!(code, 0, "{property}: {json}");
        assert_eq!(json["result"], "verified", "{property}: {json}");
    }
}

/// ... and are violated where the component breaks them: `until`'s progress
/// half fails when `a.stay` can loop, `unless` fails on the first `a.flip`.
#[test]
fn compose_level_until_and_unless_over_component_state_detect_violation() {
    for property in [
        "until U { not a.on until a.on }",
        "unless U { not a.on unless false }",
    ] {
        let dir = scratch(&[("a.fsl", CAN_STAY_OFF), ("c.fsl", &compose_with(property))]);
        let (json, code) = verify(&dir, "c.fsl");
        assert_eq!(code, 1, "{property}: {json}");
        assert_eq!(json["result"], "violated", "{property}: {json}");
    }
}

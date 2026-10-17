// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Native contract for #1197: a compose component's `until` / `unless` is
//! rewritten like the component's other properties -- its name gets the
//! `alias.` prefix and its operands read the component's own state.
//!
//! Before the fix both declarations passed through `rewrite_component_item`
//! untouched. A compose using such a component failed with exit 2
//! (`cannot type identifier 'held'`), two components declaring the same
//! property name collided (`duplicate property name 'U'`), and -- the false
//! pass -- when the compose declared state with the same name, the property
//! silently checked the compose's variable and reported `verified` for a
//! component whose own `unless` / `until` is violated.
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
        .join(format!("target/issue-1197-{}-{id}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    for (name, source) in files {
        std::fs::write(dir.join(name), source).expect("write fixture");
    }
    dir
}

fn verify(dir: &Path, file: &str, depth: &str) -> (Value, i32) {
    let output: Output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["verify", file, "--depth", depth, "--no-cache"])
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

/// A component whose `until` / `unless` holds on its own.
fn holding_component(kind: &str) -> String {
    format!(
        "spec P {{
  state {{ held: Bool, done: Bool }}
  init {{ held = true  done = false }}
  fair action release() {{ requires held  held = false  done = true }}
  {kind} U {{ held {kind} done }}
}}
"
    )
}

/// A component whose `until` / `unless` is violated on its own: `drop`
/// releases without ever reaching `done`.
fn violating_component(kind: &str) -> String {
    format!(
        "spec Bad {{
  state {{ held: Bool, done: Bool }}
  init {{ held = true  done = false }}
  action drop() {{ requires held  held = false }}
  {kind} W {{ held {kind} done }}
}}
"
    )
}

/// A compose of one component carrying `until` / `unless` verifies, and the
/// property is reported under its public `alias.name` spelling.
#[test]
fn component_until_and_unless_verify_under_alias_names() {
    for kind in ["until", "unless"] {
        let dir = scratch(&[
            ("p.fsl", &holding_component(kind)),
            ("one.fsl", "compose One {\n  use P as p from \"p.fsl\"\n}\n"),
        ]);
        let (json, code) = verify(&dir, "one.fsl", "4");
        assert_eq!(code, 0, "{kind}: {json}");
        assert_eq!(json["result"], "verified", "{kind}: {json}");
        let safety = json["transitions_checked"].to_string();
        assert!(safety.contains("\"p.U"), "{kind}: {json}");
    }
}

/// Two components declaring the same `until` name no longer collide: each is
/// checked under its own alias.
#[test]
fn same_until_name_in_two_components_does_not_collide() {
    let dir = scratch(&[
        ("p.fsl", &holding_component("until")),
        (
            "two.fsl",
            "compose Two {\n  use P as p1 from \"p.fsl\"\n  use P as p2 from \"p.fsl\"\n}\n",
        ),
    ]);
    let (json, code) = verify(&dir, "two.fsl", "4");
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["result"], "verified", "{json}");
    assert!(json["leads_to"]["p1.U"].is_object(), "{json}");
    assert!(json["leads_to"]["p2.U"].is_object(), "{json}");
}

/// The false pass: the compose declares its own `held` / `done`, which the
/// unrewritten property used to read. The component's property is violated
/// by `b.drop`, and the compose must say so.
#[test]
fn component_property_is_not_captured_by_same_named_compose_state() {
    for kind in ["until", "unless"] {
        let dir = scratch(&[
            ("bad.fsl", &violating_component(kind)),
            (
                "cap.fsl",
                "compose Cap {
  use Bad as b from \"bad.fsl\"
  state { held: Bool, done: Bool }
  init { held = false  done = false }
}
",
            ),
        ]);
        let (json, code) = verify(&dir, "cap.fsl", "3");
        assert_eq!(code, 1, "{kind}: {json}");
        assert_eq!(json["result"], "violated", "{kind}: {json}");
        assert_eq!(json["last_action"]["name"], "b.drop", "{kind}: {json}");
        let property = json["trans"].as_str().unwrap_or_default();
        assert!(property.starts_with("b.W"), "{kind}: {json}");
    }
}

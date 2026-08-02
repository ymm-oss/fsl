// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fsl-issue-650-{name}-{}-{nonce}.fsl",
            std::process::id()
        ));
        std::fs::write(&path, source).expect("write fixture");
        Self(path)
    }

    fn verify(&self, depth: &str) -> (Value, i32) {
        self.verify_with(&["--depth", depth, "--deadlock", "ignore"])
    }

    fn verify_with(&self, options: &[&str]) -> (Value, i32) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(["verify", self.0.to_str().expect("UTF-8 fixture path")])
            .args(options)
            .args(["--no-cache"])
            .output()
            .expect("run native CLI");
        let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "invalid JSON: {error}; stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (value, output.status.code().expect("native exit status"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn invariant_seq_head_is_a_located_partial_operation() {
    let fixture = Fixture::new(
        "head-invariant",
        r#"
spec PropertyHead {
  type Item = 0..2
  state { queue: Seq<Item, 2> }
  init { queue = Seq {} }
  action stay() { queue = queue }
  invariant HeadRead "REQ-HEAD: read only a live head" { queue.head() >= 0 }
}
"#,
    );
    let (output, status) = fixture.verify("1");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["violation_kind"], "partial_op", "{output:#}");
    assert_eq!(
        output["invariant"], "_partial_property_HeadRead",
        "{output:#}"
    );
    assert_eq!(output["violated_at_step"], 0, "{output:#}");
    assert!(output["loc"].is_object(), "{output:#}");
    assert_eq!(output["requirement"]["id"], "REQ-HEAD", "{output:#}");
}

#[test]
fn reachable_seq_index_cannot_create_a_phantom_witness() {
    let fixture = Fixture::new(
        "index-reachable",
        r"
spec PropertyIndex {
  type Item = 0..2
  state { queue: Seq<Item, 2> }
  init { queue = Seq {} }
  action stay() { queue = queue }
  reachable Phantom { queue[0] == 1 }
}
",
    );
    let (output, status) = fixture.verify("1");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["violation_kind"], "partial_op", "{output:#}");
    assert_eq!(
        output["invariant"], "_partial_property_Phantom",
        "{output:#}"
    );
    assert_eq!(output["violated_at_step"], 0, "{output:#}");
}

#[test]
fn property_division_by_zero_remains_totalized() {
    let fixture = Fixture::new(
        "total-division",
        r"
spec PropertyDivision {
  state { x: Int }
  init { x = 0 }
  action stay() { x = x }
  invariant TotalDivision { 7 / x == 0 and 7 % x == 0 }
}
",
    );
    let (output, status) = fixture.verify("1");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
}

#[test]
fn pattern_binder_is_visible_to_rhs_definedness() {
    let fixture = Fixture::new(
        "pattern-binder-definedness",
        r"
spec PropertyPatternBinder {
  type Item = 0..1
  type Stock = 0..2
  state { stock: Map<Item, Stock>, selected: Option<Item> }
  init {
    forall i: Item { stock[i] = 1 }
    selected = none
  }
  action stay() { selected = selected }
  invariant SelectionHasStock { selected is some(i) => stock[i] > 0 }
}
",
    );
    let (output, status) = fixture.verify("1");
    assert_eq!(status, 0, "{output:#}");
    assert_eq!(output["result"], "verified", "{output:#}");
}

#[test]
fn terminal_seq_head_is_partial_in_symbolic_and_explicit_engines() {
    let fixture = Fixture::new(
        "head-terminal",
        r"
spec PropertyTerminal {
  type Item = 0..2
  state { queue: Seq<Item, 2> }
  init { queue = Seq {} }
  action blocked() {
    requires false
    queue = queue
  }
  terminal { queue.head() >= 0 }
}
",
    );
    for engine in ["bmc", "explicit"] {
        let (output, status) =
            fixture.verify_with(&["--engine", engine, "--depth", "1", "--deadlock", "ignore"]);
        assert_eq!(status, 1, "engine={engine}: {output:#}");
        assert_eq!(
            output["violation_kind"], "partial_op",
            "engine={engine}: {output:#}"
        );
        assert_eq!(
            output["invariant"], "_partial_property_terminal",
            "engine={engine}: {output:#}"
        );
        assert_eq!(output["violated_at_step"], 0, "engine={engine}: {output:#}");
        assert!(output["loc"].is_object(), "engine={engine}: {output:#}");
    }
}

#[test]
fn leadsto_seq_head_cannot_make_the_trigger_vacuously_false() {
    let fixture = Fixture::new(
        "head-leadsto",
        r#"
spec PropertyLeadsto {
  type Item = 0..2
  state { queue: Seq<Item, 2> }
  init { queue = Seq {} }
  action stay() { queue = queue }
  leadsTo HeadEventuallyNonnegative "REQ-LIVE-HEAD: only inspect a live head" {
    queue.head() >= 0 ~> queue.size() == 0
  }
}
"#,
    );
    let (output, status) = fixture.verify("1");
    assert_eq!(status, 1, "{output:#}");
    assert_eq!(output["violation_kind"], "partial_op", "{output:#}");
    assert_eq!(
        output["invariant"], "_partial_property_HeadEventuallyNonnegative",
        "{output:#}"
    );
    assert_eq!(output["violated_at_step"], 0, "{output:#}");
    assert!(output["loc"].is_object(), "{output:#}");
    assert_eq!(output["requirement"]["id"], "REQ-LIVE-HEAD", "{output:#}");
}

#[test]
fn terminal_division_by_zero_remains_totalized_in_both_engines() {
    let fixture = Fixture::new(
        "total-terminal-division",
        r"
spec PropertyTerminalDivision {
  state { x: Int }
  init { x = 0 }
  action blocked() {
    requires false
    x = x
  }
  terminal { 7 / x == 0 and 7 % x == 0 }
}
",
    );
    for engine in ["bmc", "explicit"] {
        let (output, status) =
            fixture.verify_with(&["--engine", engine, "--depth", "1", "--deadlock", "ignore"]);
        assert_eq!(status, 0, "engine={engine}: {output:#}");
        assert_ne!(output["result"], "violated", "engine={engine}: {output:#}");
    }
}

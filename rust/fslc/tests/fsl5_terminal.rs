// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The `terminal { }` a `fsl 5` requirements document derives from its
//! lifecycles, through the native CLI behind the `fsl5` gate.
//!
//! A business document has always derived its terminal states from the sink
//! stages of its processes. A requirements document of this edition derives
//! them the same way from its entities: an instance has stopped as intended
//! when it is in a final stage of its lifecycle, a stage no `lifecycle` edge
//! leaves, and the document has stopped as intended when every instance of
//! every entity with a lifecycle has. An entity without a lifecycle and a
//! relation's role add nothing to it. A `terminal { }` the document writes
//! replaces the derived one.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// A scratch directory for the documents of one test.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("fsl5_terminal")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        Self(directory)
    }

    fn run(&self, source: &str, args: &[&str]) -> Value {
        std::fs::write(self.0.join("doc.fsl"), source).expect("write scratch source");
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .arg("doc.fsl")
            .current_dir(&self.0)
            .env(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE, "fsl5")
            .output()
            .expect("run fslc");
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "fslc {args:?}: invalid JSON ({error}): {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }

    /// The kernel's terminal condition, without source spans.
    fn terminal(&self, source: &str) -> Value {
        strip_spans(
            &self.run(source, &["kernel", "--kernel-version", "2"])["properties"]["terminal"],
        )
    }

    /// The `deadlock` warnings of `verify --depth 8`, which must verify.
    fn deadlocks(&self, source: &str) -> Vec<Value> {
        let output = self.run(source, &["verify", "--depth", "8"]);
        assert_eq!(output["result"], "verified", "{output:#}");
        output["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|warning| warning["kind"] == "deadlock")
            .cloned()
            .collect()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn strip_spans(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| key.as_str() != "span")
                .map(|(key, value)| (key.clone(), strip_spans(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(strip_spans).collect()),
        scalar => scalar.clone(),
    }
}

/// A return is requested, then approved and refunded or rejected: every
/// run ends with each return in `Refunded` or `Rejected`. `{terminal}` is
/// written just before the closing brace.
fn returns(terminal: &str) -> String {
    format!(
        "fsl 5\nrequirements Returns {{\n  actor Customer, Manager\n  entity Return {{\n    \
         lifecycle Draft -> Requested -> Approved -> Refunded\n    \
         lifecycle Requested -> Rejected\n  }}\n  \
         event request = Customer requests Return Draft -> Requested\n  \
         event approve = Manager approves Return Requested -> Approved\n  \
         event reject = Manager rejects Return Requested -> Rejected\n  \
         event refund = Manager refunds Return Approved -> Refunded\n  \
         invariant Typed {{ true }}\n{terminal}}}\nverify {{\n  instances Return = 2\n}}\n"
    )
}

/// The derived terminal is the one a person would write for the final
/// stages, so a document that ends normally verifies without a `deadlock`
/// warning, as its business layer does.
#[test]
fn the_final_stages_of_a_lifecycle_are_the_terminal_states() {
    let scratch = Scratch::new("final-stages");
    let written =
        "  terminal { forall r: Return { stage(r) == Refunded or stage(r) == Rejected } }\n";
    let derived = scratch.terminal(&returns(""));
    assert_ne!(derived, Value::Null, "a terminal is derived");
    assert_eq!(derived, scratch.terminal(&returns(written)));
    assert_eq!(scratch.deadlocks(&returns("")), Vec::<Value>::new());
}

/// A stage a `lifecycle` edge leaves is not final even when no event moves
/// along that edge (`lifecycle_edge_without_event`): an instance stuck there
/// has not finished, so the stop is still a `deadlock`.
#[test]
fn a_stage_a_lifecycle_edge_leaves_is_not_final() {
    let scratch = Scratch::new("edge-without-event");
    let source = returns("").replace(
        "  event refund = Manager refunds Return Approved -> Refunded\n",
        "",
    );
    let deadlocks = scratch.deadlocks(&source);
    assert_eq!(deadlocks.len(), 1, "{deadlocks:#?}");
    assert!(
        deadlocks[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Approved")),
        "{deadlocks:#?}"
    );
}

/// With two entities, the document has stopped as intended when each has:
/// one condition per entity with a lifecycle, joined by `and`. The entity
/// without a lifecycle and the relation's role add nothing.
#[test]
fn every_entity_with_a_lifecycle_must_be_in_a_final_stage() {
    let scratch = Scratch::new("two-entities");
    let source = |terminal: &str| {
        format!(
            "fsl 5\nrequirements Support {{\n  actor Agent\n  entity Agent\n  \
             entity Ticket {{\n    lifecycle Open -> Assigned -> Closed\n    \
             lifecycle Assigned -> Escalated\n  }}\n  \
             Ticket belongs to Agent as assignee via take\n  \
             entity Shift {{\n    lifecycle Planned -> Worked\n  }}\n  \
             event take = Agent takes Ticket Open -> Assigned\n  \
             event close = Agent closes Ticket Assigned -> Closed\n  \
             event escalate = Agent escalates Ticket Assigned -> Escalated\n  \
             event work = Agent works Shift Planned -> Worked\n  \
             invariant Typed {{ true }}\n{terminal}}}\nverify {{\n  instances Agent = 1\n  \
             instances Ticket = 1\n  instances Shift = 1\n}}\n"
        )
    };
    let written = "  terminal { (forall t: Ticket { stage(t) == Closed or stage(t) == Escalated }) \
                   and (forall s: Shift { stage(s) == Worked }) }\n";
    assert_eq!(
        scratch.terminal(&source("")),
        scratch.terminal(&source(written))
    );
    assert_eq!(scratch.deadlocks(&source("")), Vec::<Value>::new());
}

/// A `terminal { }` the document writes is the terminal: nothing derived
/// widens it, so a stop it does not name is still a `deadlock`.
#[test]
fn a_written_terminal_replaces_the_derived_one() {
    let scratch = Scratch::new("written");
    let narrower = returns("  terminal { forall r: Return { stage(r) == Refunded } }\n");
    let deadlocks = scratch.deadlocks(&narrower);
    assert_eq!(deadlocks.len(), 1, "{deadlocks:#?}");
    assert!(
        deadlocks[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Rejected")),
        "{deadlocks:#?}"
    );
}

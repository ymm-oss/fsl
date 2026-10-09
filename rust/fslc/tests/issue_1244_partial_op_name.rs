// SPDX-License-Identifier: Apache-2.0

//! Issue #1244: a partial operation reached in an action's guard is one
//! failure, so every command that names it uses one name. `verify` (BMC,
//! explicit, induction) calls it `_partial_<action>`; the Monitor's
//! `attempt`, which `conformance` and `replay` use, called it
//! `_partial_op_<action>`, so a conformance vector could not be matched
//! against the verify result for the same spec.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// A guard that divides by zero in the initial state.
const GUARD_DIVISION: &str = r"
spec GuardDivision {
  type Small = 0..2
  state { x: Small, y: Int }
  init { x = 0  y = 0 }
  action div() {
    requires 2 / x >= 0
    y = 1
  }
}
";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("fsl-issue-1244-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        Self(dir)
    }

    fn write(&self, name: &str, text: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, text).expect("write scratch file");
        path.to_str().expect("UTF-8 scratch path").to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fslc(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

#[test]
fn guard_partial_operation_has_one_name_in_verify_conformance_and_replay() {
    let scratch = Scratch::new();
    let spec = scratch.write("guard_division.fsl", GUARD_DIVISION);

    let mut names = Vec::new();
    for engine in ["bmc", "explicit", "induction"] {
        let (value, code) = fslc(&[
            "verify",
            &spec,
            "--engine",
            engine,
            "--depth",
            "2",
            "--no-cache",
        ]);
        assert_eq!(code, 1, "{engine}: {value:#}");
        assert_eq!(value["violation_kind"], "partial_op", "{engine}: {value:#}");
        names.push((
            format!("verify --engine {engine}"),
            value["invariant"].clone(),
        ));
    }

    let (value, code) = fslc(&["conformance", &spec, "--depth", "2"]);
    assert_eq!(code, 0, "{value:#}");
    let outcome = value["vectors"]
        .as_array()
        .expect("conformance vectors")
        .iter()
        .find(|vector| vector["action"]["name"] == "div")
        .map(|vector| vector["outcome"].clone())
        .expect("a vector for div");
    assert_eq!(outcome["kind"], "partial_op", "{value:#}");
    names.push(("conformance".to_owned(), outcome["name"].clone()));

    let trace = scratch.write(
        "trace.json",
        &json!({"events":[{"action":"div","params":{}}]}).to_string(),
    );
    let (value, code) = fslc(&["replay", &spec, "--trace", &trace]);
    assert_eq!(code, 1, "{value:#}");
    assert_eq!(value["violation"]["kind"], "partial_op", "{value:#}");
    names.push(("replay".to_owned(), value["violation"]["name"].clone()));

    for (command, name) in &names {
        assert_eq!(name, &json!("_partial_div"), "{command}: {names:?}");
    }
}

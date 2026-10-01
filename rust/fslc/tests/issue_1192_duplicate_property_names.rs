// SPDX-License-Identifier: Apache-2.0

//! Regression for issue #1192: a spec with two `leadsTo L` passed `check`, and
//! `verify --engine induction` reported `proved`/`unbounded` because the
//! results keyed by name collapsed the false second `L` into the ranked first.
//! Both commands must now refuse the spec with the located `semantics` error.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

const DUP: &str = "spec Dup {
  state { x: 0..1, y: 0..5 }
  init { x = 0  y = 0 }
  action flip() { requires y == 5 x = 1 - x }
  action inc() { requires y < 5 y = y + 1 }
  leadsTo L { y < 5 ~> y == 5 decreases 5 - y }
  leadsTo L { x == 0 ~> x == 2 }
}
";

fn fixture() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fslc-issue-1192-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch directory");
    let path = dir.join("dup.fsl");
    std::fs::write(&path, DUP).expect("write fixture");
    path
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

fn assert_refused(value: &Value, code: i32) {
    assert_eq!(code, 2, "{value}");
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "semantics", "{value}");
    assert_eq!(
        value["message"],
        "duplicate property name 'L': leadsTo reuses the name of the leadsTo declared at 6:3"
    );
    assert_eq!(value["loc"]["line"], 7, "{value}");
    assert_eq!(value["loc"]["column"], 3, "{value}");
}

#[test]
fn check_and_verify_refuse_a_duplicate_leadsto_name() {
    let path = fixture();
    let path = path.display().to_string();
    let (value, code) = run(&["check", &path]);
    assert_refused(&value, code);
    for engine in ["induction", "bmc"] {
        let (value, code) = run(&["verify", &path, "--engine", engine, "--depth", "3"]);
        assert_refused(&value, code);
    }
}

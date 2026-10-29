// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use serde_json::{Value, json};
use std::process::Command;

const ROLE: &str = "fsl 5\nrequirements R {\n  actor User\n  entity User\n  entity Claim {\n    lifecycle Draft -> Approved\n  }\n  Claim belongs to User as NAME via approve\n  event approve = User approves Claim Draft -> Approved\n}\nverify {\n  instances User = 1\n  instances Claim = 1\n}\n";
const RESOURCE: &str = "fsl 5\nrequirements R {\n  number Amount\n  resource NAME: Amount = 0\n  actor User\n  entity Claim {\n    lifecycle Draft -> Approved\n  }\n  event approve = User approves Claim Draft -> Approved\n}\nverify {\n  instances Claim = 1\n  values Amount = 0..1\n}\n";
const MEASURE: &str = "fsl 5\nrequirements R {\n  actor User\n  entity Claim {\n    lifecycle Draft -> Approved\n  }\n  event approve = User approves Claim Draft -> Approved\n  measure NAME = count Claim in Approved\n}\nverify {\n  instances Claim = 1\n}\n";

fn check(source: &str, position: &str, name: &str, line: u32, column: u32) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target")
        .join(format!("issue-1600-{}-{name}.fsl", std::process::id()));
    std::fs::write(&path, source.replace("NAME", name)).expect("write fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("check")
        .arg(&path)
        .env("FSLC_UNSTABLE", "fsl5")
        .env("FSLC_CACHE", "off")
        .output()
        .expect("run fslc");
    let value: Value = serde_json::from_slice(&output.stdout).expect("CLI JSON");
    if name.contains("__") {
        assert_eq!(output.status.code(), Some(2), "{name}: {value:#}");
        assert_eq!(value["kind"], "name", "{name}: {value:#}");
        let prefix = format!(
            "{position} name '{name}' contains '__', which is reserved for the names FSL generates"
        );
        assert!(
            value["message"].as_str().unwrap().starts_with(&prefix),
            "{value:#}"
        );
        assert_eq!(value["loc"], json!({"line": line, "column": column}));
    } else {
        assert_eq!(output.status.code(), Some(0), "{name}: {value:#}");
        assert_eq!(value["result"], "ok", "{name}: {value:#}");
    }
}

macro_rules! case {
    ($test:ident, $source:ident, $position:literal, $name:literal, $line:literal, $column:literal) => {
        #[test]
        fn $test() {
            check($source, $position, $name, $line, $column);
        }
    };
}

// Detectors: each must fail on the named pre-fix base, d37a215e.
case!(role_leading_dunder, ROLE, "relation role", "__owner", 8, 28);
case!(role_mid_dunder, ROLE, "relation role", "owner__x", 8, 28);
case!(resource_dunder, RESOURCE, "resource", "__r", 4, 12);
case!(measure_dunder, MEASURE, "measure", "__m", 8, 11);
// Preservation controls: accepted both before and after the fix.
case!(role_control, ROLE, "relation role", "owner", 8, 28);
case!(resource_control, RESOURCE, "resource", "r", 4, 12);
case!(measure_control, MEASURE, "measure", "m", 8, 11);

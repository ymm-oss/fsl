// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `fslc lint` warns with `shared_requirement_id` where a current-edition `requirements`
//! document gives one requirement ID two owners: a `requirement` repeated, or an
//! `acceptance` and a `requirement` sharing the ID. `check` accepts both and the ledger
//! keeps one row for the ID; `check` stays unchanged.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

/// Line 5 starts `requirement`, line 11 `acceptance`, line 15 `forbidden`; an extra
/// declaration starts at line 20.
fn document(extra: &str) -> String {
    format!(
        "requirements R {{
  type K = 0..1
  state {{ n: K }}
  init {{ n = 0 }}
  requirement REQ-CART-001 \"go sets n\" {{
    action go() {{
      requires n == 0
      n = 1
    }}
  }}
  acceptance AC-CART-001 \"go reaches one\" {{
    go()
    expect n == 1
  }}
  forbidden FB-CART-001 \"go runs once\" {{
    go()
    go()
    expect rejected
  }}
{extra}}}
"
    )
}

const SECOND_REQUIREMENT: &str = "  requirement REQ-CART-001 \"dup\" {
    invariant small { n <= 1 }
  }
";
const ACCEPTANCE_OF_REQUIREMENT: &str = "  acceptance REQ-CART-001 \"dup\" {
    go()
    expect n == 1
  }
";
const REQUIREMENT_OF_ACCEPTANCE: &str = "  requirement AC-CART-001 \"dup\" {
    invariant small { n <= 1 }
  }
";

/// A scratch directory holding `doc.fsl`.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str, source: &str) -> Self {
        let directory = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("lint_shared_requirement_id")
            .join(label);
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create scratch directory");
        std::fs::write(directory.join("doc.fsl"), source).expect("write source");
        Self(directory)
    }

    fn fslc(&self, args: &[&str]) -> (i32, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env("FSLC_CACHE", "off")
            .env_remove(fslc_rust::unstable_gate::ENVIRONMENT_VARIABLE)
            .output()
            .expect("run fslc");
        let packet = serde_json::from_slice(&output.stdout).expect("JSON stdout");
        (output.status.code().expect("exit code"), packet)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The exit of `check`, the exit of `lint` with `extra_args`, and lint's
/// `shared_requirement_id` findings.
fn run(label: &str, extra: &str, extra_args: &[&str]) -> (i32, i32, Vec<Value>) {
    let scratch = Scratch::new(label, &document(extra));
    let (check, _) = scratch.fslc(&["check", "doc.fsl"]);
    let mut args = vec!["lint", "doc.fsl"];
    args.extend_from_slice(extra_args);
    let (lint, packet) = scratch.fslc(&args);
    let findings = packet["files"][0]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|finding| finding["code"] == "shared_requirement_id")
        .cloned()
        .collect();
    (check, lint, findings)
}

fn finding(kind: &str, id: &str, earlier: &str, at: (u32, u32), column: u32) -> Value {
    json!({
        "code": "shared_requirement_id",
        "kind": "shared_requirement_id",
        "taxonomy": "ambiguous_intent",
        "severity": "warning",
        "edition": "current",
        "message": format!(
            "requirement ID '{id}': this {kind} reuses the ID of the {earlier} at {}:{}; the \
             ledger keeps one row for the two",
            at.0, at.1
        ),
        "loc": {
            "file": "doc.fsl",
            "line": 20,
            "column": column,
            "end_line": 20,
            "end_column": column + u32::try_from(id.len()).expect("short ID"),
        },
        "canonical_replacement": "rename one of the two IDs, or merge the two declarations",
        "machine_applicable": false,
        "edits": [],
    })
}

#[test]
fn a_second_owner_of_a_requirement_id_is_a_lint_warning() {
    let cases = [
        (
            "requirement_twice",
            SECOND_REQUIREMENT,
            finding("requirement", "REQ-CART-001", "requirement", (5, 15), 15),
        ),
        (
            "acceptance_of_requirement",
            ACCEPTANCE_OF_REQUIREMENT,
            finding("acceptance", "REQ-CART-001", "requirement", (5, 15), 14),
        ),
        (
            "requirement_of_acceptance",
            REQUIREMENT_OF_ACCEPTANCE,
            finding("requirement", "AC-CART-001", "acceptance", (11, 14), 15),
        ),
    ];
    for (label, extra, expected) in cases {
        let (check, lint, findings) = run(label, extra, &[]);
        assert_eq!(check, 0, "{label}: check is unchanged");
        assert_eq!(lint, 1, "{label}");
        assert_eq!(findings, vec![expected], "{label}");
    }
}

/// `--edition next` reports it at the same severity: the warning is about the document,
/// not about an edition's rule.
#[test]
fn the_warning_keeps_its_severity_for_the_next_edition() {
    let (_, lint, findings) = run("next", SECOND_REQUIREMENT, &["--edition", "next"]);
    assert_eq!(lint, 1);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["severity"], "warning");
    assert_eq!(findings[0]["edition"], "next");
}

/// One owner per ID gives no finding; two `acceptance`s are refused by `check` and left
/// to it; a `forbidden` reusing a requirement ID is not in this warning's scope.
#[test]
fn other_documents_give_no_shared_requirement_id() {
    let second_acceptance = "  acceptance AC-CART-001 \"dup\" {
    go()
    expect n == 1
  }
";
    let forbidden_of_requirement = "  forbidden REQ-CART-001 \"dup\" {
    go()
    go()
    expect rejected
  }
";
    let (check, lint, findings) = run("control", "", &[]);
    assert_eq!((check, lint, findings.len()), (0, 0, 0));
    let (check, _, findings) = run("acceptance_twice", second_acceptance, &[]);
    assert_eq!((check, findings.len()), (2, 0));
    let (check, _, findings) = run("forbidden_of_requirement", forbidden_of_requirement, &[]);
    assert_eq!((check, findings.len()), (0, 0));
}

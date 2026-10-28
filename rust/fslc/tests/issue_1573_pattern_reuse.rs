// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1573: collision sets follow evaluator binding lifetime, without broadening
//! lexical name visibility. Renaming repairs not/if and where/body collisions.

#[path = "support/issue_1573_forms.rs"]
mod forms;
#[path = "support/scratch_identity.rs"]
mod scratch_identity;

use forms::{Case, Expected};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    file: &'static str,
    property: &'static str,
}

impl Fixture {
    fn new(case: &Case, composed: bool) -> Self {
        let root = scratch_identity::unique(std::env::temp_dir().join(format!(
            "fslc-1573-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        std::fs::create_dir(&root).expect("create fresh #1573 fixture");
        std::fs::write(root.join("a.fsl"), &case.source).expect("write direct spec");
        std::fs::write(
            root.join("case.json"),
            json!({"name": case.name, "composed": composed, "expected": format!("{:?}", case.expected)}).to_string(),
        )
        .expect("retain case identity");
        let (file, property, action) = if composed {
            let name = case.source.split_whitespace().nth(1).expect("spec name");
            std::fs::write(
                root.join("s.fsl"),
                format!("compose C {{\n  use {name} as a from \"a.fsl\"\n}}\n"),
            )
            .expect("write composition");
            ("s.fsl", "a.P", "a.stay")
        } else {
            ("a.fsl", "P", "stay")
        };
        std::fs::write(
            root.join("trace.json"),
            json!([{"action": action, "params": case.params}]).to_string(),
        )
        .expect("write one-step Monitor trace");
        Self {
            root,
            file,
            property,
        }
    }

    fn observe(&self, label: &str, consumer: &str) -> (Value, i32) {
        let binary = std::env::var_os("FSL_MATRIX_BINARY")
            .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_fslc")), PathBuf::from);
        let binary = std::fs::canonicalize(binary).expect("selected native binary");
        let args = match consumer {
            "check" => vec!["check", self.file],
            "Monitor" => vec!["replay", self.file, "--trace", "trace.json"],
            _ => vec![
                "verify",
                self.file,
                "--engine",
                consumer,
                "--depth",
                "4",
                "--no-cache",
            ],
        };
        let output = Command::new(&binary)
            .args(&args)
            .env("FSLC_CACHE", "off")
            .current_dir(&self.root)
            .output()
            .expect("run native fslc");
        assert!(output.stderr.is_empty(), "{label}/{consumer}: {output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).expect("native JSON");
        let exit = output.status.code().expect("native exit");
        // libtest status/panic output can interleave with stderr JSON. Keep
        // byte-complete CLI outputs in separate files, not only a mixed log.
        std::fs::write(
            self.root.join(format!("{consumer}.stdout.json")),
            &output.stdout,
        )
        .expect("retain full native output");
        std::fs::write(
            self.root.join(format!("{consumer}.record.json")),
            json!({"binary": binary, "args": args, "exit": exit}).to_string(),
        )
        .expect("retain invocation and exit");
        eprintln!(
            "OBS {label}/{consumer} exit={exit} result={}",
            value["result"]
        );
        (value, exit)
    }
}

fn collision(value: &Value, exit: i32, line: u32, column: u32) -> bool {
    // Measured direct and compose errors both carry numeric coordinates only.
    let message = format!(
        "invalid model expression: pattern binding 'v' in 'is some(v)' shadows a name already in scope (an action parameter, quantifier binder, let, or earlier pattern binding) of the same name; rename the pattern variable at {line}:{column}"
    );
    exit == 2
        && value["fsl"] == "1.0"
        && value["result"] == "error"
        && value["kind"] == "semantics"
        && value["message"] == message
        && value["loc"] == json!({"line": line, "column": column})
}

fn accepted(value: &Value, exit: i32) -> bool {
    exit == 0 && value["result"] == "ok" && no_error(value)
}

fn no_error(value: &Value) -> bool {
    ["kind", "message", "error"]
        .iter()
        .all(|key| value.get(*key).is_none())
}

fn verdict(value: &Value, exit: i32, consumer: &str, truth: bool, property: &str) -> bool {
    // Independent per-consumer contracts, not an equality comparison of native
    // envelopes: observed proof/closure/cost fields genuinely differ by engine.
    let result = match (consumer, truth) {
        ("Monitor", true) => "conformant",
        ("Monitor", false) => "nonconformant",
        ("bmc", true) => "verified",
        (_, true) => "proved",
        (_, false) => "violated",
    };
    if exit != i32::from(!truth) || value["result"] != result || !no_error(value) {
        return false;
    }
    if truth {
        [
            "violation",
            "violation_kind",
            "invariant",
            "failed_at_event",
        ]
        .iter()
        .all(|key| value.get(*key).is_none())
            && (consumer != "Monitor" || value["steps_checked"] == 1)
    } else if consumer == "Monitor" {
        value["failed_at_event"] == 0
            && value["violation"]["kind"] == "invariant"
            && value["violation"]["name"] == property
    } else {
        value["violation_kind"] == "invariant"
            && value["invariant"] == property
            && value["violated_at_step"].is_number()
    }
}

fn matrix(cases: Vec<Case>) {
    eprintln!(
        "AXIS visibility: unchanged, no name-export contract is granted by collision-set rows"
    );
    let mut failures = Vec::new();
    let mut count = 0;
    for case in cases {
        for composed in [false, true] {
            count += 1;
            let label = format!("{}/compose={composed}", case.name);
            let fixture = Fixture::new(&case, composed);
            eprintln!("CELL {label} expected={:?}", case.expected);
            eprintln!("FIXTURE {label} root={}", fixture.root.display());
            let (value, exit) = fixture.observe(&label, "check");
            let matches = match case.expected {
                Expected::Reject(line, column) => collision(&value, exit, line, column),
                Expected::Check | Expected::Truth(_) => accepted(&value, exit),
            };
            if !matches {
                failures.push(format!(
                    "{label}/check: expected {:?}, got {exit}: {value}",
                    case.expected
                ));
            }
            if let Expected::Truth(truth) = case.expected {
                for consumer in ["bmc", "induction", "explicit", "Monitor"] {
                    let (value, exit) = fixture.observe(&label, consumer);
                    if !verdict(&value, exit, consumer, truth, fixture.property) {
                        failures.push(format!(
                            "{label}/{consumer}: expected truth={truth}, got {exit}: {value}"
                        ));
                    }
                }
            }
        }
    }
    eprintln!(
        "MATRIX collision-set cells={count} failures={}",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn issue_table_direct_and_composed() {
    matrix(forms::issue_table());
}

#[test]
fn hidden_bindings_collide_at_each_later_position() {
    matrix(forms::positions());
}

#[test]
fn conditional_arms_in_invariants_arguments_and_definitions() {
    matrix(forms::branch_arms());
}

#[test]
fn conditional_arms_with_both_conditions_and_payload_orders() {
    matrix(forms::branch_variants());
}

#[test]
fn where_bindings_collide_with_body_and_of() {
    eprintln!(
        "IMPOSSIBLE count/unique/exactlyOne where-to-body/of: no independent body/of in grammar; two-match filter controls instead (3 kinds x 2 names x 2 composition modes = 12 cells)"
    );
    matrix(forms::binders());
}

#[test]
fn candidate_local_names_do_not_collide_with_outer_siblings() {
    matrix(forms::isolation());
}

#[test]
fn existing_collision_diagnostics_are_unchanged() {
    matrix(forms::diagnostics());
}

#[test]
fn statement_if_bindings_collide_in_arms_and_later_statements() {
    matrix(forms::statements());
}

#[test]
fn statement_if_arms_remain_independent_unlike_expression_arms() {
    matrix(forms::independent_statement_arms());
}

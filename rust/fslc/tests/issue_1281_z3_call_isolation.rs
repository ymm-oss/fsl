// SPDX-License-Identifier: Apache-2.0

//! Mutation BMC results must not depend on earlier queries on the same thread.
//! The Seq-init-removal case from #1251 exposes this on pre-isolation builds.
//! At f5f3f194, preceding built-ins change `_bounds_q` to `build_spec`. At
//! 08d9656c, that order control passes, but repeating the same full mutant model
//! still changes the killer (including `killers`) within one process.
//! Compare the entire external record, not a selected set of outcome fields.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const SPEC: &str = "spec Q {\n  type V = 0..2\n  state { q: Seq<V, 2>, n: Int }\n  \
     init { q = Seq {} n = 0 }\n  \
     action push(v: V) { requires q.size() < 2 q = q.push(v) n = 1 }\n  \
     action pop() { requires q.size() > 0 q = q.pop() }\n}\n";

fn fixture(label: &str, record: &Value) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/issue-1281-z3-call-isolation-{}/{label}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    std::fs::write(dir.join("q.fsl"), SPEC).expect("write spec");
    std::fs::write(dir.join("mutants.jsonl"), format!("{record}\n")).expect("write catalog");
    dir
}

fn mutate(dir: &Path, cap: &str, attribution: bool) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fslc"));
    command
        .args([
            "mutate",
            "q.fsl",
            "--depth",
            "1",
            "--max-mutants",
            cap,
            "--from",
            "mutants.jsonl",
        ])
        .current_dir(dir)
        .env("FSLC_CACHE", "off");
    if attribution {
        command.arg("--oracle-attribution");
    }
    let output = command.output().expect("run native mutate");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).expect("mutate JSON");
    assert_eq!(value["result"], "mutated", "{value}");
    assert_eq!(value["baseline"], "verified", "{value}");
    value
}

fn external(output: &Value) -> &Value {
    let external = output["mutants"]
        .as_array()
        .expect("mutants")
        .iter()
        .filter(|mutant| mutant["source"] == "external")
        .collect::<Vec<_>>();
    assert_eq!(external.len(), 1, "{output}");
    external[0]
}

fn assert_history_independent(attribution: bool) {
    let dir = fixture(
        if attribution {
            "attribution"
        } else {
            "default"
        },
        &json!({"id": "rm-q-init", "replace": {"target": "q = Seq {}", "replacement": ""}}),
    );
    let alone = mutate(&dir, "0", attribution);
    let after_catalog = mutate(&dir, "200", attribution);
    assert_eq!(alone["mutants"].as_array().unwrap().len(), 1);
    // A surviving built-in must have reached BMC, so this genuinely changes
    // the query history rather than comparing two baseline-only refusals.
    assert!(
        after_catalog["mutants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mutant| mutant["source"] == "builtin" && mutant["status"] == "survived"),
        "{after_catalog}"
    );
    let alone = external(&alone);
    let after_catalog = external(&after_catalog);
    for mutant in [alone, after_catalog] {
        assert_eq!(mutant["status"], "killed", "{mutant}");
        assert!(mutant["killed_by"].is_string(), "{mutant}");
        if attribution {
            assert!(!mutant["killers"].as_array().expect("killers").is_empty());
        } else {
            assert!(mutant.get("killers").is_none(), "{mutant}");
        }
    }
    // Same JSONL record at line 1 in both runs: even catalog metadata is
    // identical, so no exclusions or normalization are necessary.
    assert_eq!(alone, after_catalog, "earlier BMC calls changed the mutant");
}

#[test]
fn external_mutant_is_independent_of_builtin_bmc_history() {
    assert_history_independent(false);
}

#[test]
fn oracle_attribution_is_independent_of_builtin_bmc_history() {
    assert_history_independent(true);
}

fn assert_repeated_history_independent(attribution: bool) {
    let mutant_source = SPEC.replace("q = Seq {}", "");
    let records = (0..12)
        .map(|index| {
            json!({
                "id": format!("probe-{index}"),
                "description": "identical model",
                "spec": mutant_source,
            })
        })
        .collect::<Vec<_>>();
    let dir = fixture(
        if attribution {
            "repeated-attribution"
        } else {
            "repeated-default"
        },
        &records[0],
    );
    let mut catalog = String::new();
    for record in &records {
        writeln!(catalog, "{record}").expect("write catalog record");
    }
    std::fs::write(dir.join("mutants.jsonl"), catalog).expect("write repeated models");
    let repeated = mutate(&dir, "0", attribution);
    let mutants = repeated["mutants"].as_array().expect("mutants");
    assert_eq!(mutants.len(), records.len(), "{repeated}");
    for (index, (mutant, record)) in mutants.iter().zip(&records).enumerate() {
        assert_eq!(mutant["source"], "external", "{mutant}");
        assert_eq!(mutant["status"], "killed", "{mutant}");
        assert_eq!(mutant["id"], record["id"], "{mutant}");
        assert_eq!(mutant["line"], index + 1, "{mutant}");
        if attribution {
            assert!(!mutant["killers"].as_array().expect("killers").is_empty());
        } else {
            assert!(mutant.get("killers").is_none(), "{mutant}");
        }
        // IDs must be unique in a catalog. Keep each copy's entire returned
        // record comparable by using that same ID and JSONL line when alone.
        // Blank lines add no earlier mutation queries. No fields are stripped.
        std::fs::write(
            dir.join("mutants.jsonl"),
            format!("{}{record}\n", "\n".repeat(index)),
        )
        .expect("write same record alone at the same line");
        let alone = mutate(&dir, "0", attribution);
        assert_eq!(
            external(&alone),
            mutant,
            "earlier identical mutant queries changed record {index}"
        );
    }
}

#[test]
fn repeated_external_mutant_is_independent_of_earlier_identical_models() {
    assert_repeated_history_independent(false);
}

#[test]
fn repeated_oracle_attribution_is_independent_of_earlier_identical_models() {
    assert_repeated_history_independent(true);
}

#[test]
fn unmodified_external_model_survives() {
    let dir = fixture("healthy", &json!({"id": "healthy", "spec": SPEC}));
    let output = mutate(&dir, "0", true);
    let mutant = external(&output);
    assert_eq!(mutant["status"], "survived", "{mutant}");
    assert_eq!(mutant["killed_by"], Value::Null, "{mutant}");
    assert!(mutant.get("killers").is_none(), "{mutant}");
}

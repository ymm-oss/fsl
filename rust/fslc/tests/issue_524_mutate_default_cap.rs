// SPDX-License-Identifier: Apache-2.0

//! Regression coverage for issue #524: native `fslc mutate` defaulted to 100
//! built-in mutants while its own published CLI contract
//! (`rust/fslc/cli-contract.json`), `docs/DESIGN-mutate.md`,
//! `skills/fsl/references/commands.md`, and the frozen `src/fslc/mutate.py`
//! (`DEFAULT_MAX_MUTANTS`) all specify 200. For a model with more than 100
//! candidates that silently evaluated a different mutant set and reported a
//! different kill count, making native reports incomparable with the
//! documented baseline.
//!
//! Every pre-existing native mutate test passed an explicit `--max-mutants`,
//! which is exactly why none of them saw the default.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/issue_524_mutant_cardinality.fsl")
}

fn find_command<'a>(node: &'a Value, name: &str) -> Option<&'a Value> {
    if node["path"]
        .as_array()
        .is_some_and(|path| path.last().and_then(Value::as_str) == Some(name))
    {
        return Some(node);
    }
    node["commands"]
        .as_array()?
        .iter()
        .find_map(|child| find_command(child, name))
}

/// The `max_mutants` default the published CLI contract advertises. The
/// expectation is read from the contract rather than restated as a second
/// literal, so a future contract edit cannot drift away from the runtime
/// default unnoticed. `native_integration.rs` already pins the checked-in
/// contract to the one the binary publishes.
fn contract_default_max_mutants() -> u64 {
    let contract: Value = serde_json::from_str(include_str!("../cli-contract.json"))
        .expect("checked-in CLI contract");
    let mutate = find_command(&contract["root"], "mutate").expect("mutate command in contract");
    mutate["actions"]
        .as_array()
        .expect("mutate actions")
        .iter()
        .find(|action| action["dest"] == "max_mutants")
        .and_then(|action| action["default"].as_u64())
        .expect("published max_mutants default")
}

fn mutate(extra: &[&str]) -> Value {
    let mut args = vec![
        "mutate".to_owned(),
        fixture().display().to_string(),
        "--depth".to_owned(),
        "2".to_owned(),
    ];
    args.extend(extra.iter().map(|value| (*value).to_owned()));
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(&args)
        .current_dir(workspace_root())
        .output()
        .expect("run native CLI");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(value["result"], "mutated", "{value}");
    value
}

#[test]
fn the_published_contract_still_advertises_two_hundred() {
    assert_eq!(contract_default_max_mutants(), 200);
}

#[test]
fn a_default_run_evaluates_the_same_mutant_set_as_the_contract_default() {
    let limit = contract_default_max_mutants().to_string();
    let implicit = mutate(&[]);
    let explicit = mutate(&["--max-mutants", &limit]);
    assert_eq!(implicit["summary"], explicit["summary"], "{implicit}");
    assert_eq!(implicit["mutants"], explicit["mutants"]);
    assert_eq!(implicit["notes"], explicit["notes"]);
    // The fixture has more candidates than the cap, so the run really is
    // truncated: an equal-but-uncapped comparison would prove nothing.
    assert_eq!(implicit["summary"]["total"], 200, "{}", implicit["summary"]);
    assert!(
        implicit["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .any(|note| note.as_str() == Some("mutant cap 200 reached: 374 dropped")),
        "{}",
        implicit["notes"]
    );
}

#[test]
fn an_explicit_lower_limit_still_truncates() {
    // The pre-fix default; explicit `--max-mutants` behavior is unchanged, and
    // it must stay distinguishable from the default or the test above would
    // pass for the wrong reason.
    let explicit = mutate(&["--max-mutants", "100"]);
    assert_eq!(explicit["summary"]["total"], 100, "{}", explicit["summary"]);
    assert!(
        explicit["notes"]
            .as_array()
            .expect("notes")
            .iter()
            .any(|note| note.as_str() == Some("mutant cap 100 reached: 474 dropped")),
        "{}",
        explicit["notes"]
    );
    let implicit = mutate(&[]);
    assert_ne!(implicit["summary"], explicit["summary"]);
}

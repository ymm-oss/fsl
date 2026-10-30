// SPDX-License-Identifier: Apache-2.0

//! Independent CLI acceptance tests for simultaneous indexed progress substitution.
//! Crashing cases must stay in subprocesses, never in the integration-test process.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PROGRESS: &str = "  preserve progress { respond L by go }\n";

struct Fixture(PathBuf);

impl Fixture {
    fn new(
        name: &str,
        implementation: &str,
        abstraction: &str,
        maps: &str,
        progress: bool,
    ) -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.codex/tmp/issue-1613")
            .join(format!("{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mapping = format!(
            "// SPDX-License-Identifier: Apache-2.0\nrefinement R {{\n  impl Impl\n  abs Abs\n{maps}  action go(k) -> step(k)\n{}}}\n",
            if progress { PROGRESS } else { "" }
        );
        for (file, source) in [
            ("impl.fsl", implementation),
            ("abs.fsl", abstraction),
            ("map.fsl", mapping.as_str()),
        ] {
            std::fs::write(dir.join(file), source).unwrap();
        }
        Self(dir)
    }

    fn run(&self, args: &[&str]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env_remove("FSLC_CACHE")
            .env_remove("FSLC_CACHE_VERIFY")
            .env("FSLC_CACHE_DIR", self.0.join("cache"))
            .output()
            .expect("execute native fslc subprocess");
        println!(
            "OBS fixture={} args={args:?} status={}\nstdout={}\nstderr={}",
            self.0.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn check_specs(&self) {
        for file in ["impl.fsl", "abs.fsl"] {
            let output = self.run(&["check", file]);
            let value = envelope(&output, 0);
            assert_eq!(value["result"], "ok", "{value}");
        }
    }

    fn refine(&self) -> Output {
        self.run(&["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "4"])
    }
}

fn envelope(output: &Output, exit: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit),
        "expected exit {exit}; produced status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).expect("complete CLI JSON envelope");
    assert_eq!(value["fsl"], "1.0", "{value}");
    value
}

fn progress_lost(output: &Output, line: u32, keyed: bool) -> Value {
    let value = envelope(output, 1);
    assert_eq!(value["result"], "refinement_failed", "{value}");
    assert_eq!(value["kind"], "progress_lost", "{value}");
    assert_eq!(value["violation_kind"], "leadsTo", "{value}");
    assert_eq!(value["invariant"], "L", "{value}");
    assert_eq!(value["impl"], "Impl", "{value}");
    assert_eq!(value["abs"], "Abs", "{value}");
    assert_eq!(
        value["bindings"],
        if keyed { json!({"k": 0}) } else { json!({}) },
        "{value}"
    );
    assert_eq!(value["loc"], json!({"line": line, "column": 3}), "{value}");
    assert_eq!(
        value["progress"],
        json!({"leadsTo": "L", "actions": ["go"]}),
        "{value}"
    );
    assert_eq!(value["trace_type"], "refinement", "{value}");
    assert_eq!(
        value["faithfulness_class"], "liveness_not_refined",
        "{value}"
    );
    assert_eq!(value["pending_since"], 0, "{value}");
    assert!(
        !value["impl_trace"].as_array().unwrap().is_empty(),
        "{value}"
    );
    assert!(value["stutter"].is_boolean(), "{value}");
    assert!(
        matches!(
            value["progress_failure"].as_str(),
            Some("deadlock_or_stall_blocks_progress" | "lasso_blocks_progress")
        ),
        "{value}"
    );
    value
}

fn identity(renamed: bool, progress: bool) -> Fixture {
    let state = if renamed { "n" } else { "m" };
    Fixture::new(
        &format!("identity-{renamed}-{progress}"),
        &format!(
            "// SPDX-License-Identifier: Apache-2.0\nspec Impl {{\n  type K = 0..1\n  state {{ {state}: Map<K, Int> }}\n  init {{ forall k: K {{ {state}[k] = 0 }} }}\n  fair action go(k: K) {{ requires {state}[k] == 0 and k == 1 {state}[k] = 1 }}\n}}\n"
        ),
        "// SPDX-License-Identifier: Apache-2.0\nspec Abs {\n  type K = 0..1\n  state { m: Map<K, Int> }\n  init { forall k: K { m[k] = 0 } }\n  fair action step(k: K) { requires m[k] == 0 m[k] = 1 }\n  leadsTo L { forall k: K { m[k] == 0 ~> m[k] == 1 } }\n}\n",
        &format!("  map m[i: K] = {state}[i]\n"),
        progress,
    )
}

fn pair(shape: &str, renamed: bool, progress: bool) -> Fixture {
    let (left, right, target, guard) = match (shape, renamed) {
        ("swap", false) => ("a", "b", "b", " and k == 1"),
        ("swap", true) => ("x", "y", "y", " and k == 1"),
        ("chain", false) => ("b", "c", "c", ""),
        ("chain", true) => ("e", "c", "c", ""),
        _ => unreachable!(),
    };
    let implementation = format!(
        "// SPDX-License-Identifier: Apache-2.0\nspec Impl {{\n  type K = 0..1\n  state {{ {left}: Map<K, Int>, {right}: Map<K, Int> }}\n  init {{ forall k: K {{ {left}[k] = 0 }} forall k: K {{ {right}[k] = 0 }} }}\n  fair action go(k: K) {{ requires {target}[k] == 0{guard} {target}[k] = 1 }}\n}}\n"
    );
    let (step, tick, first, second) = if shape == "swap" {
        ("a", "", right, left)
    } else {
        (
            "b",
            "  fair action tick(k: K) { requires a[k] == 0 a[k] = 1 }\n",
            left,
            right,
        )
    };
    let abstraction = format!(
        "// SPDX-License-Identifier: Apache-2.0\nspec Abs {{\n  type K = 0..1\n  state {{ a: Map<K, Int>, b: Map<K, Int> }}\n  init {{ forall k: K {{ a[k] = 0 }} forall k: K {{ b[k] = 0 }} }}\n  fair action step(k: K) {{ requires {step}[k] == 0 {step}[k] = 1 }}\n{tick}  leadsTo L {{ forall k: K {{ a[k] == 0 ~> a[k] == 1 }} }}\n}}\n"
    );
    Fixture::new(
        &format!("{shape}-{renamed}-{progress}"),
        &implementation,
        &abstraction,
        &format!("  map a[i: K] = {first}[i]\n  map b[j: K] = {second}[j]\n"),
        progress,
    )
}

fn run_progress(fixture: &Fixture, line: u32, repeat: bool) {
    fixture.check_specs();
    let first = progress_lost(&fixture.refine(), line, true);
    if repeat {
        // Same fixtures and process environment: compare complete JSON, no exclusions.
        assert_eq!(first, progress_lost(&fixture.refine(), line, true));
    }
}

#[test]
fn self_identity_reports_progress_lost_not_stack_overflow() {
    run_progress(&identity(false, true), 7, false);
}

#[test]
fn swap_reports_progress_lost_not_stack_overflow() {
    run_progress(&pair("swap", false, true), 7, false);
}

#[test]
fn chain_reports_progress_lost_not_false_refines() {
    run_progress(&pair("chain", false, true), 8, false);
}

#[test]
fn renamed_identity_preserves_progress_lost_twice() {
    run_progress(&identity(true, true), 7, true);
}

#[test]
fn renamed_swap_preserves_progress_lost_twice() {
    run_progress(&pair("swap", true, true), 7, true);
}

#[test]
fn renamed_chain_preserves_progress_lost_twice() {
    run_progress(&pair("chain", true, true), 8, true);
}

fn simulation(fixture: &Fixture) {
    fixture.check_specs();
    let mut previous = None;
    for _ in 0..2 {
        let value = envelope(&fixture.refine(), 0);
        assert_eq!(value["result"], "refines", "{value}");
        assert_eq!(value["checked_to_depth"], 4, "{value}");
        assert_eq!(value["action_map"], json!({"go": "step"}), "{value}");
        assert!(value.get("progress").is_none(), "{value}");
        if let Some(previous) = previous {
            assert_eq!(value, previous);
        }
        previous = Some(value);
    }
}

#[test]
fn identity_without_preserve_still_refines_twice() {
    simulation(&identity(false, false));
}

#[test]
fn swap_without_preserve_still_refines_twice() {
    simulation(&pair("swap", false, false));
}

#[test]
fn chain_without_preserve_still_refines_twice() {
    simulation(&pair("chain", false, false));
}

#[test]
fn scalar_chain_keeps_simultaneous_substitution_twice() {
    let fixture = Fixture::new(
        "scalar-chain",
        "// SPDX-License-Identifier: Apache-2.0\nspec Impl { type K = 0..1 state { b: Int = 0, c: Int = 0 } fair action go(k: K) { requires c == 0 c = 1 } }\n",
        "// SPDX-License-Identifier: Apache-2.0\nspec Abs {\n  type K = 0..1\n  state { a: Int = 0, b: Int = 0 }\n  fair action step(k: K) { requires b == 0 b = 1 }\n  fair action tick(k: K) { requires a == 0 a = 1 }\n  leadsTo L { a == 0 ~> a == 1 }\n}\n",
        "  map a = b\n  map b = c\n",
        true,
    );
    fixture.check_specs();
    assert_eq!(
        progress_lost(&fixture.refine(), 7, false),
        progress_lost(&fixture.refine(), 7, false)
    );
}

#[test]
fn unpullable_whole_map_read_is_a_located_mapping_error() {
    let fixture = Fixture::new(
        "whole-map",
        "// SPDX-License-Identifier: Apache-2.0\nspec Impl { type K = 0..1 state { n: Map<K, Int> } init { forall k: K { n[k] = 0 } } fair action go(k: K) { requires n[k] == 0 n[k] = 1 } }\n",
        "// SPDX-License-Identifier: Apache-2.0\nspec Abs { type K = 0..1 state { m: Map<K, Int> } init { forall k: K { m[k] = 0 } } fair action step(k: K) { requires m[k] == 0 m[k] = 1 } leadsTo L { m == m ~> false } }\n",
        "  map m[i: K] = n[i]\n",
        true,
    );
    fixture.check_specs();
    let value = envelope(&fixture.refine(), 2);
    assert_eq!(value["result"], "error", "{value}");
    assert_eq!(value["kind"], "type", "{value}");
    // Pull errors use the mapping's respond declaration span, as in issue #1595.
    assert_eq!(value["loc"], json!({"line": 7, "column": 23}), "{value}");
    assert!(
        value["message"].as_str().is_some_and(|m| !m.is_empty()),
        "{value}"
    );
    for key in ["impl_trace", "trace", "trace_type"] {
        assert!(value.get(key).is_none(), "{key}: {value}");
    }
}

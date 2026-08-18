// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative control (a) for issue #729: `--vacuity ignore` is now allowed to
//! *skip* the budgeted reachability probe entirely (`fsl_runtime::
//! verification_warnings`'s `skip_vacuity_probe`) rather than compute it and
//! filter the result, as it did before. Skipping the computation must never
//! change what the CLI reports beyond the `warnings` array itself: this test
//! proves the `--vacuity ignore` envelope is exactly the `--vacuity warn`
//! envelope with every `is_vacuity_kind` entry removed from `warnings`
//! (`--cost`/`elapsed_s`/`cost.solver.check_elapsed_s` excluded from the
//! comparison, since skipping genuinely does less work and so legitimately
//! takes less time).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn run_cli(arguments: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(arguments)
        .current_dir(repository_root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={arguments:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

const FIXTURE: &str = "rust/fslc/tests/fixtures/vacuous_leadsto.fsl";

/// Timing/cost fields legitimately move when the probe is skipped -- they
/// are not part of the "did skipping change anything else" claim.
const TIMING_ONLY_KEYS: &[&str] = &["cost"];

fn without_timing(mut envelope: Value) -> Value {
    if let Value::Object(object) = &mut envelope {
        for key in TIMING_ONLY_KEYS {
            object.remove(*key);
        }
    }
    envelope
}

fn without_vacuity_warnings(mut envelope: Value) -> Value {
    if let Value::Object(object) = &mut envelope
        && let Some(Value::Array(warnings)) = object.get_mut("warnings")
    {
        warnings.retain(|warning| {
            !warning
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| fsl_core::VACUITY_KINDS.contains(&kind))
        });
    }
    envelope
}

#[test]
fn vacuity_ignore_skip_produces_the_same_envelope_as_warn_with_vacuity_kinds_filtered() {
    let (warn_output, warn_status) = run_cli(&[
        "verify",
        FIXTURE,
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--vacuity",
        "warn",
        "--no-cache",
    ]);
    let (ignore_output, ignore_status) = run_cli(&[
        "verify",
        FIXTURE,
        "--depth",
        "3",
        "--deadlock",
        "ignore",
        "--vacuity",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(warn_status, 0, "{warn_output:#}");
    assert_eq!(ignore_status, 0, "{ignore_output:#}");

    // Sanity: the fixture must actually exercise a vacuity finding, or this
    // test would pass vacuously (no `warnings` difference to filter at all).
    assert!(
        warn_output["warnings"]
            .as_array()
            .expect("warnings array")
            .iter()
            .any(|warning| warning["kind"] == "vacuous_leadsto"),
        "fixture must produce a vacuous_leadsto warning under --vacuity warn: {warn_output:#}"
    );
    assert!(
        !ignore_output["warnings"]
            .as_array()
            .expect("warnings array")
            .iter()
            .any(|warning| warning["kind"] == "vacuous_leadsto"),
        "--vacuity ignore must still suppress the vacuity finding in its own output: {ignore_output:#}"
    );

    let filtered_warn = without_timing(without_vacuity_warnings(warn_output));
    let ignore_normalized = without_timing(ignore_output);
    assert_eq!(
        filtered_warn, ignore_normalized,
        "skipping the probe (--vacuity ignore) must produce exactly the envelope warn-mode \
         would produce with vacuity-kind warnings filtered out -- any other difference means \
         skipping silently changed observable behavior beyond the warnings array"
    );
}

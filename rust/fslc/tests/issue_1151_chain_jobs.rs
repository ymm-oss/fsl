// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! `fslc chain --jobs N` (issue #1151) must not change what a chain reports.
//!
//! Every manifest here puts a deliberately slow layer first, so at `--jobs 4`
//! the later layers finish before it. An implementation that emitted layers,
//! picked the first failure, or decided the skipped set in completion order
//! would therefore disagree with `--jobs 1`; manifest-order aggregation is
//! what makes the two runs agree. The comparisons are over the whole stdout
//! JSON, the whole stderr table, and the exit code. The only fields removed
//! are the two the job count is allowed to move: wall-clock `*elapsed_s`, and
//! the solver's `memory_mb`, which Z3 keeps per process and so includes other
//! workers' memory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

/// A fresh directory under `rust/target/` (gitignored), unique per call.
fn scratch_dir(name: &str) -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = repo_root().join(format!(
        "rust/target/issue-1151-{name}-{}-{id}",
        std::process::id()
    ));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("clean stale scratch dir");
    }
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// A layer that takes long enough to finish after every other layer here.
const SLOW_LAYER: &str = include_str!("fixtures/issue_1151_slow_layer.fsl");
/// Violated first at step 6, the manifest's depth for the slow layer, so the
/// run pays for every step before it fails.
const SLOW_BROKEN_INVARIANT: &str = "\n  invariant NotYetSix { level[0] + level[1] < 6 }\n";

/// The project: the committed `tests/fixtures/chain` layers, plus the slow
/// layer in `slow.fsl` and its violating variant in `slow_broken.fsl`.
fn project() -> PathBuf {
    let dir = scratch_dir("project");
    for entry in fs::read_dir(repo_root().join("tests/fixtures/chain")).expect("chain fixtures") {
        let path = entry.expect("fixture entry").path();
        if path.extension().is_some_and(|extension| extension == "fsl") {
            fs::copy(&path, dir.join(path.file_name().expect("file name"))).expect("copy");
        }
    }
    fs::write(dir.join("slow.fsl"), SLOW_LAYER).expect("write slow layer");
    let broken = SLOW_LAYER
        .rsplit_once('}')
        .map(|(body, _)| format!("{body}{SLOW_BROKEN_INVARIANT}}}\n"))
        .expect("slow layer ends with a closing brace");
    fs::write(dir.join("slow_broken.fsl"), broken).expect("write slow broken layer");
    dir
}

/// `[impl]` runs this test binary's own `fslc`: no interpreter dependency.
fn impl_command() -> String {
    let fslc = env!("CARGO_BIN_EXE_fslc").replace('\\', "\\\\");
    format!("command = \"{fslc} check business.fsl\"")
}

/// Four independent verification steps -- business, requirements, design,
/// and the design->requirements refine link -- then `[impl]`.
fn manifest(business: &str, requirements: &str) -> String {
    format!(
        "[business]\nfile = \"{business}\"\ndepth = 6\n\n\
         [requirements]\nfile = \"{requirements}\"\ndepth = 1\n\n\
         [design]\nfile = \"design.fsl\"\ndepth = 2\n\
         refine_against = \"requirements\"\nmapping = \"design_refines_requirements.fsl\"\n\n\
         [impl]\n{}\n",
        impl_command()
    )
}

struct Run {
    code: Option<i32>,
    stdout: Value,
    stderr: String,
}

fn chain(dir: &Path, cache: &Path, manifest: &str, args: &[&str]) -> Run {
    fs::write(dir.join("fsl-project.toml"), manifest).expect("write manifest");
    let output: Output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg("chain")
        .arg("fsl-project.toml")
        .args(args)
        .current_dir(dir)
        .env("FSLC_CACHE_DIR", cache)
        .env_remove("FSLC_CACHE")
        .env_remove("FSLC_CACHE_VERIFY")
        .output()
        .expect("run native fslc");
    Run {
        code: output.status.code(),
        stdout: serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "chain stdout is not JSON ({error}): {}",
                String::from_utf8_lossy(&output.stderr)
            )
        }),
        stderr: String::from_utf8(output.stderr).expect("UTF-8 stderr"),
    }
}

/// The stdout JSON without the two fields the job count may move.
fn comparable(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| !key.ends_with("elapsed_s") && key.as_str() != "memory_mb")
                .map(|(key, value)| (key.clone(), comparable(value)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(comparable).collect()),
        other => other.clone(),
    }
}

fn layer_names(value: &Value) -> Vec<String> {
    value["layers"]
        .as_array()
        .expect("layers")
        .iter()
        .map(|layer| {
            format!(
                "{}:{}",
                layer["layer"].as_str().expect("layer name"),
                layer["status"].as_str().expect("layer status")
            )
        })
        .collect()
}

/// Runs the manifest cold at `--jobs 1` and `--jobs 4`, each against its own
/// empty cache, then again warm, and asserts all four agree pairwise.
fn assert_jobs_agree(manifest: &str, extra: &[&str]) -> Run {
    let dir = project();
    let serial_cache = scratch_dir("cache-serial");
    let parallel_cache = scratch_dir("cache-parallel");
    let mut serial_args = extra.to_vec();
    serial_args.extend(["--jobs", "1"]);
    let mut parallel_args = extra.to_vec();
    parallel_args.extend(["--jobs", "4"]);

    let serial = chain(&dir, &serial_cache, manifest, &serial_args);
    let parallel = chain(&dir, &parallel_cache, manifest, &parallel_args);
    assert_eq!(serial.code, parallel.code, "exit code differs");
    assert_eq!(serial.stderr, parallel.stderr, "stderr table differs");
    assert_eq!(
        comparable(&serial.stdout),
        comparable(&parallel.stdout),
        "stdout differs between --jobs 1 and --jobs 4"
    );

    // Warm: every verify layer is now a cache hit, and the `cache`
    // annotation (content-addressed key included) must agree too.
    let serial_warm = chain(&dir, &serial_cache, manifest, &serial_args);
    let parallel_warm = chain(&dir, &parallel_cache, manifest, &parallel_args);
    assert_eq!(serial_warm.code, parallel_warm.code);
    assert_eq!(serial_warm.stderr, parallel_warm.stderr);
    assert_eq!(
        comparable(&serial_warm.stdout),
        comparable(&parallel_warm.stdout),
        "warm stdout differs between --jobs 1 and --jobs 4"
    );
    serial
}

#[test]
fn passing_chain_is_identical_at_jobs_1_and_4() {
    let serial = assert_jobs_agree(&manifest("slow.fsl", "requirements.fsl"), &[]);
    assert_eq!(serial.code, Some(0), "{}", serial.stderr);
    assert_eq!(
        layer_names(&serial.stdout),
        [
            "business:passed",
            "requirements:passed",
            "design:passed",
            "design->requirements:passed",
            "impl:passed",
        ]
    );
}

#[test]
fn failing_middle_layer_is_identical_at_jobs_1_and_4() {
    // `business_broken.fsl` violates at step 0, long before the slow first
    // layer finishes.
    let serial = assert_jobs_agree(&manifest("slow.fsl", "business_broken.fsl"), &[]);
    assert_eq!(serial.code, Some(1), "{}", serial.stderr);
    assert_eq!(serial.stdout["failed"], serde_json::json!(["requirements"]));
    assert_eq!(
        layer_names(&serial.stdout),
        [
            "business:passed",
            "requirements:failed",
            "design:skipped",
            "design->requirements:skipped",
            "impl:skipped",
        ]
    );
}

#[test]
fn failing_middle_layer_with_keep_going_is_identical_at_jobs_1_and_4() {
    let serial = assert_jobs_agree(
        &manifest("slow.fsl", "business_broken.fsl"),
        &["--keep-going"],
    );
    // The refine link targets the broken `[requirements]` layer, so it fails
    // too (an error, hence exit 2): two failures, both reported in order.
    assert_eq!(serial.code, Some(2), "{}", serial.stderr);
    assert_eq!(
        serial.stdout["failed"],
        serde_json::json!(["requirements", "design->requirements"])
    );
    assert_eq!(
        layer_names(&serial.stdout),
        [
            "business:passed",
            "requirements:failed",
            "design:passed",
            "design->requirements:failed",
            "impl:passed",
        ]
    );
}

#[test]
fn first_failure_is_attributed_in_manifest_order_not_completion_order() {
    // Two failing layers: the slow first one, and a fast second one that
    // finishes (and fails) first at `--jobs 4`. The serial chain stops at the
    // first, so the second must be `skipped`, not `failed`.
    let serial = assert_jobs_agree(&manifest("slow_broken.fsl", "business_broken.fsl"), &[]);
    assert_eq!(serial.code, Some(1), "{}", serial.stderr);
    assert_eq!(serial.stdout["failed"], serde_json::json!(["business"]));
    assert_eq!(
        layer_names(&serial.stdout),
        [
            "business:failed",
            "requirements:skipped",
            "design:skipped",
            "design->requirements:skipped",
            "impl:skipped",
        ]
    );
}

#[test]
fn layers_with_identical_files_keep_their_serial_cache_interaction() {
    // `[business]` and `[requirements]` name byte-identical files at the same
    // options, so they share one content-addressed cache key: serially, the
    // second is a hit on the entry the first just wrote. Run concurrently it
    // would miss, so the two must stay in one worker, in manifest order.
    let dir = project();
    fs::copy(dir.join("business.fsl"), dir.join("business_copy.fsl")).expect("copy");
    let manifest = format!(
        "[business]\nfile = \"business.fsl\"\ndepth = 1\n\n\
         [requirements]\nfile = \"business_copy.fsl\"\ndepth = 1\n\n\
         [design]\nfile = \"slow.fsl\"\ndepth = 6\n\n[impl]\n{}\n",
        impl_command()
    );
    let serial = chain(
        &dir,
        &scratch_dir("same-serial"),
        &manifest,
        &["--jobs", "1"],
    );
    let parallel = chain(
        &dir,
        &scratch_dir("same-parallel"),
        &manifest,
        &["--jobs", "4"],
    );
    assert_eq!(
        serial.stdout["layers"][1]["detail"]["cache"]["hit"],
        Value::Bool(true),
        "precondition: serially the second identical layer is a cache hit"
    );
    assert_eq!(comparable(&serial.stdout), comparable(&parallel.stdout));
    assert_eq!(serial.stderr, parallel.stderr);
}

#[test]
fn jobs_rejects_a_missing_zero_or_non_integer_value() {
    let dir = project();
    let cache = scratch_dir("usage-cache");
    fs::write(
        dir.join("fsl-project.toml"),
        manifest("business.fsl", "requirements.fsl"),
    )
    .expect("write manifest");
    for (args, message) in [
        (vec!["--jobs"], "--jobs requires a value"),
        (vec!["--jobs", "0"], "--jobs must be a positive integer"),
        (vec!["--jobs", "two"], "--jobs must be a positive integer"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .arg("chain")
            .args(&args)
            .current_dir(&dir)
            .env("FSLC_CACHE_DIR", &cache)
            .output()
            .expect("run native fslc");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(text.contains(message), "{args:?}: {text}");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["chain", "fsl-project.toml", "other.toml"])
        .current_dir(&dir)
        .env("FSLC_CACHE_DIR", &cache)
        .output()
        .expect("run native fslc");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("unexpected chain argument 'other.toml'"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    // The path may follow the options, as the published contract says.
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["chain", "--jobs", "2", "--keep-going", "fsl-project.toml"])
        .current_dir(&dir)
        .env("FSLC_CACHE_DIR", &cache)
        .output()
        .expect("run native fslc");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `fslc verify <file> --depth <d>` with the same wall-clock/memory fields
/// removed, and without `versions`, which a chain layer's detail does not
/// carry.
fn standalone_verify(dir: &Path, file: &str, depth: &str) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["verify", file, "--depth", depth, "--no-cache"])
        .current_dir(dir)
        .output()
        .expect("run native fslc");
    let mut value: Value = serde_json::from_slice(&output.stdout).expect("verify JSON");
    value
        .as_object_mut()
        .expect("verify envelope")
        .remove("versions");
    comparable(&value)
}

#[test]
fn every_layer_reports_what_a_standalone_verify_reports() {
    // z3 0.20 keeps one context per thread. Run on the thread that had just
    // verified `agentic_rag_business.fsl`, the design layer's solver counts
    // (conflicts/decisions/propagations) differed from a standalone
    // `fslc verify` of the same file at the same depth -- measured, and at
    // depth 8 the `reachable_failed` witness itself differed. Each step now
    // gets a fresh thread, so the layer is the standalone result at any job
    // count, and the same as what the verify cache would serve.
    let dir = scratch_dir("fresh-context");
    let examples = repo_root().join("examples/agentic_rag");
    for file in ["agentic_rag_business.fsl", "agentic_rag_design.fsl"] {
        fs::copy(examples.join(file), dir.join(file)).expect("copy example");
    }
    let manifest = "[business]\nfile = \"agentic_rag_business.fsl\"\ndepth = 1\n\n\
                    [design]\nfile = \"agentic_rag_design.fsl\"\ndepth = 1\n";
    let standalone = standalone_verify(&dir, "agentic_rag_design.fsl", "1");
    assert_eq!(standalone["result"], "reachable_failed", "{standalone}");
    for jobs in ["1", "4"] {
        let run = chain(
            &dir,
            &scratch_dir("fresh-context-cache"),
            manifest,
            &["--keep-going", "--jobs", jobs],
        );
        assert_eq!(
            comparable(&run.stdout["layers"][1]["detail"]),
            standalone,
            "--jobs {jobs}: the design layer differs from `fslc verify`"
        );
    }
}

/// The published cache entries under `cache` (temporary files excluded).
fn cache_entries(cache: &Path) -> Vec<String> {
    fn walk(dir: &Path, found: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("cache entry").path();
            let name = path
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                walk(&path, found);
            } else if !name.starts_with('.') {
                found.push(name);
            }
        }
    }
    let mut found = Vec::new();
    walk(cache, &mut found);
    found.sort();
    found
}

#[test]
fn an_early_failure_does_not_wait_for_a_slow_later_layer() {
    // `[business]` fails at step 0; `[design]` is the slow layer at a depth
    // that takes seconds. Serially the design layer never starts. At
    // `--jobs 4` it starts at once, and the run must still end as soon as the
    // business failure decides the report -- not after the discarded design
    // layer finishes. A run that waited would have stored the design
    // layer's cache entry before exiting; one that did not leaves exactly the
    // serial run's entries.
    let dir = project();
    let manifest = format!(
        "[business]\nfile = \"business_broken.fsl\"\ndepth = 1\n\n\
         [design]\nfile = \"slow.fsl\"\ndepth = 9\n\n[impl]\n{}\n",
        impl_command()
    );
    let serial_cache = scratch_dir("early-serial");
    let parallel_cache = scratch_dir("early-parallel");
    let serial = chain(&dir, &serial_cache, &manifest, &["--jobs", "1"]);
    let started = std::time::Instant::now();
    let parallel = chain(&dir, &parallel_cache, &manifest, &["--jobs", "4"]);
    let parallel_wall = started.elapsed();
    assert_eq!(serial.code, Some(1), "{}", serial.stderr);
    assert_eq!(serial.code, parallel.code);
    assert_eq!(serial.stderr, parallel.stderr);
    assert_eq!(comparable(&serial.stdout), comparable(&parallel.stdout));
    assert_eq!(
        layer_names(&serial.stdout),
        ["business:failed", "design:skipped", "impl:skipped"]
    );
    assert_eq!(
        cache_entries(&parallel_cache),
        cache_entries(&serial_cache),
        "--jobs 4 waited for the discarded design layer (wall {parallel_wall:?})"
    );
}

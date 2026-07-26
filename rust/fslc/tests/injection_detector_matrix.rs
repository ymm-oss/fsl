// SPDX-License-Identifier: Apache-2.0

//! Native primary/blind negative-control detector matrix for
//! `examples/gallery/injected/`. See issue #485 and
//! `docs/DESIGN-conformance-harness.md`: the injected corpus is only a valid
//! detector calibration if it is measured against the authoritative native
//! CLI, not only the frozen Python reference (`tests/test_injection_bench.py`,
//! which now defaults to measuring this same native binary too).
//!
//! For every injected spec, the detector named by its `expect-detector`
//! header (the *primary* detector) must catch the injected defect, and the
//! detector this defect type is designed to be invisible to (the *blind*
//! detector) must not. A green positive path alone is not evidence a
//! detector works (AGENTS.md); this test is the negative control.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const DEPTH: &str = "4";
const DIFF_MUTATE_DEPTH: &str = "3";
const DIFF_MUTATE_MAX: &str = "30";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn injected_dir() -> PathBuf {
    root().join("examples/gallery/injected")
}

fn run_cli(args: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native CLI");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON for `fslc {}`: {error}; stderr={}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn primary_detector(inject: &str) -> &'static str {
    match inject {
        "omission" => "strict_tags_requirements",
        "boundary-flip" | "guard-weakening" => "forbidden_acceptance",
        "invariant-weakening" => "mutate",
        "unreachable-antecedent" => "vacuity",
        "fabricated-constraint" => "strict_tags",
        "over-strengthened-guard" => "verify",
        other => panic!("unknown injection type: {other}"),
    }
}

fn blind_detector(inject: &str) -> &'static str {
    match inject {
        "omission"
        | "boundary-flip"
        | "guard-weakening"
        | "unreachable-antecedent"
        | "over-strengthened-guard" => "strict_tags",
        "invariant-weakening" => "verify",
        "fabricated-constraint" => "vacuity",
        other => panic!("unknown injection type: {other}"),
    }
}

fn baseline_for(base: &str) -> PathBuf {
    match base {
        "specs/bank.fsl" => root().join("specs/bank.fsl"),
        "specs/order_workflow.fsl" => root().join("specs/order_workflow.fsl"),
        "examples/layers/return_system.fsl" => root().join("examples/layers/return_system.fsl"),
        other => panic!("unknown base spec: {other}"),
    }
}

/// Parse the `// key: value` header comments in the first 8 lines, the same
/// convention `tests/test_injection_bench.py::_headers` reads.
fn headers(path: &Path) -> BTreeMap<String, String> {
    let source = std::fs::read_to_string(path).expect("read injected spec");
    let mut out = BTreeMap::new();
    for line in source.lines().take(8) {
        let trimmed = line.trim();
        let Some(body) = trimmed.strip_prefix("//") else {
            continue;
        };
        let body = body.trim();
        if let Some((key, value)) = body.split_once(':') {
            out.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    out
}

fn warning_names(value: &Value, kind: &str) -> Vec<String> {
    value["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|warning| warning["kind"] == kind)
        .map(|warning| warning["name"].as_str().unwrap_or_default().to_owned())
        .collect()
}

struct Cell {
    caught: bool,
    signal: String,
}

fn cell(caught: bool, signal: impl Into<String>) -> Cell {
    Cell {
        caught,
        signal: signal.into(),
    }
}

/// Write a per-domain requirement-id registry derived from `ids.txt`, the
/// same derivation `tests/test_injection_bench.py::_registry_for` uses for
/// `strict_tags_requirements`.
fn registry_for(expect_signal: &str, scratch: &Path) -> PathBuf {
    let expected = expect_signal.rsplit(':').next().unwrap_or(expect_signal);
    let prefix = format!("{}-", expected.split('-').next().unwrap_or(expected));
    let ids = std::fs::read_to_string(injected_dir().join("ids.txt")).expect("read ids.txt");
    let matching = ids
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with(&prefix))
        .collect::<Vec<_>>()
        .join("\n");
    let path = scratch.join(format!(
        "{}_ids.txt",
        prefix.to_lowercase().trim_matches('-')
    ));
    std::fs::write(&path, format!("{matching}\n")).expect("write registry");
    path
}

fn scratch_dir() -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "fslc-injection-detector-matrix-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("create scratch dir");
    directory
}

fn forbidden_acceptance_cell(path_str: &str) -> Cell {
    let check = run_cli(&["check", path_str]);
    if check["result"] == "error"
        && matches!(check["kind"].as_str(), Some("acceptance" | "forbidden"))
    {
        cell(
            true,
            format!("{}:{}", check["kind"].as_str().unwrap_or(""), check["id"]),
        )
    } else {
        cell(false, check["result"].to_string())
    }
}

fn verify_cell(path_str: &str) -> Cell {
    let verify = run_cli(&["verify", path_str, "--depth", DEPTH, "--deadlock", "ignore"]);
    if verify["result"] == "reachable_failed" {
        let names = verify["unreached"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| item["name"].as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(",");
        cell(true, format!("reachable_failed:{names}"))
    } else if verify["result"] == "violated" {
        cell(true, format!("violated:{}", verify["violation_kind"]))
    } else {
        cell(false, verify["result"].to_string())
    }
}

fn vacuity_cell(path_str: &str) -> Cell {
    let vacuity = run_cli(&[
        "verify",
        path_str,
        "--depth",
        DEPTH,
        "--deadlock",
        "ignore",
        "--vacuity",
        "error",
    ]);
    let vacuity_kind = vacuity["kind"].as_str().unwrap_or_default();
    if vacuity["result"] == "error" && vacuity_kind.starts_with("vacuous_") {
        let names = vacuity["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| f["name"].as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join(",");
        cell(true, format!("{vacuity_kind}:{names}"))
    } else {
        cell(false, vacuity_kind.to_owned())
    }
}

fn strict_tags_cell(path_str: &str) -> Cell {
    let strict = run_cli(&["check", path_str, "--strict-tags"]);
    let untagged = warning_names(&strict, "untagged");
    if untagged.is_empty() {
        cell(false, strict["result"].to_string())
    } else {
        cell(true, format!("untagged:{}", untagged.join(",")))
    }
}

fn strict_tags_requirements_cell(path_str: &str, expect_signal: &str, scratch: &Path) -> Cell {
    let registry = registry_for(expect_signal, scratch);
    let registry_str = registry.to_str().expect("utf8 registry path");
    let strict_req = run_cli(&[
        "check",
        path_str,
        "--strict-tags",
        "--requirements",
        registry_str,
    ]);
    let unreferenced = warning_names(&strict_req, "unreferenced_requirement");
    if unreferenced.is_empty() {
        cell(false, strict_req["result"].to_string())
    } else {
        cell(
            true,
            format!("unreferenced_requirement:{}", unreferenced.join(",")),
        )
    }
}

/// Only meaningful for `invariant-weakening`: a single-spec `mutate` run
/// only ever shows survivors, so detection requires a baseline differential
/// against the unmodified base spec (`tests/test_injection_bench.py`'s
/// `_measure_case` docstring-equivalent comment).
fn mutate_differential_cell(path_str: &str, base: &str) -> Cell {
    let baseline = run_cli(&[
        "mutate",
        baseline_for(base).to_str().expect("utf8"),
        "--depth",
        DIFF_MUTATE_DEPTH,
        "--max-mutants",
        DIFF_MUTATE_MAX,
    ]);
    let injected = run_cli(&[
        "mutate",
        path_str,
        "--depth",
        DIFF_MUTATE_DEPTH,
        "--max-mutants",
        DIFF_MUTATE_MAX,
    ]);
    let base_survived = baseline["summary"]["survived"].as_i64().unwrap_or(0);
    let injected_survived = injected["summary"]["survived"].as_i64().unwrap_or(0);
    cell(
        injected_survived > base_survived,
        format!("survivors {base_survived}->{injected_survived}"),
    )
}

fn measure(
    path: &Path,
    headers: &BTreeMap<String, String>,
    scratch: &Path,
) -> BTreeMap<&'static str, Cell> {
    let path_str = path.to_str().expect("utf8 path");
    let mut cells = BTreeMap::new();
    cells.insert("forbidden_acceptance", forbidden_acceptance_cell(path_str));
    cells.insert("verify", verify_cell(path_str));
    cells.insert("vacuity", vacuity_cell(path_str));
    cells.insert("strict_tags", strict_tags_cell(path_str));
    cells.insert(
        "strict_tags_requirements",
        strict_tags_requirements_cell(path_str, &headers["expect-signal"], scratch),
    );
    cells.insert(
        "mutate",
        if headers["inject"] == "invariant-weakening" {
            mutate_differential_cell(path_str, &headers["base"])
        } else {
            cell(false, "not-differential")
        },
    );
    cells
}

#[test]
fn native_injected_corpus_primary_blind_matrix() {
    let mut paths = std::fs::read_dir(injected_dir())
        .expect("read injected dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "fsl"))
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths.len(), 21, "expected 21 injected specs");

    let scratch = scratch_dir();
    let mut failures = Vec::new();

    for path in &paths {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let case_headers = headers(path);
        let inject = case_headers["inject"].as_str();
        let cells = measure(path, &case_headers, &scratch);

        let primary = primary_detector(inject);
        if !cells[primary].caught {
            failures.push(format!(
                "{file_name}: primary {primary} did not catch {inject}: {}",
                cells[primary].signal
            ));
        }

        let blind = blind_detector(inject);
        if cells[blind].caught {
            failures.push(format!(
                "{file_name}: blind detector {blind} unexpectedly caught {inject}: {}",
                cells[blind].signal
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

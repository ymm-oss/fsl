// SPDX-License-Identifier: Apache-2.0

//! #1344: timing-free HTML is opt-in, HTML-only, and independent of delivery.
//! No existing `.fsl` under specs/examples/native fixtures names `elapsed_s`.
//! Domain/source same-name preservation therefore remains an implementation-unit
//! control, not a claim made by these existing-fixture CLI tests.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const CLEAN: &str = "rust/fslc/tests/fixtures/sweep_clean.fsl";
const VIOLATED: &str = "rust/fslc/tests/fixtures/sweep_violating.fsl";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
}

struct ReportRun(PathBuf);

impl ReportRun {
    fn new(label: &str) -> Self {
        let parent = root().join(".codex/tmp");
        std::fs::create_dir_all(&parent).expect("worktree-local scratch parent");
        let path = parent.join(format!(
            "issue-1344-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos(),
        ));
        std::fs::create_dir(&path).expect("unique worktree-local report directory");
        Self(path)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(root())
            .env("FSLC_CACHE_DIR", self.0.join("cache"))
            .env_remove("FSLC_CACHE")
            .env_remove("FSLC_CACHE_VERIFY")
            .output()
            .expect("run native CLI")
    }
}

fn assert_html(output: &Output, status: i32) {
    assert_eq!(
        output.status.code(),
        Some(status),
        "expected HTML/exit {status}; stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        output.stdout.starts_with(b"<!doctype html>"),
        "raw HTML delivery"
    );
}

fn embedded_json(output: &Output, label: &str) -> Value {
    let html = std::str::from_utf8(&output.stdout).expect("UTF-8 HTML");
    let marker = format!("<details><summary>{label}</summary><div><div class=\"code-block\"><pre>");
    let escaped = html
        .split_once(&marker)
        .expect("named raw JSON section")
        .1
        .split_once("</pre>")
        .expect("raw JSON closing pre")
        .0;
    let json = escaped
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    serde_json::from_str(&json).expect("embedded JSON")
}

fn assert_four_timing_locations(verification: &Value, explained: &Value, present: bool) {
    let cost = verification["cost"]
        .as_object()
        .expect("cost block is retained");
    assert_eq!(
        cost.contains_key("elapsed_s"),
        present,
        "verification.cost.elapsed_s"
    );
    let solver = cost["solver"]
        .as_object()
        .expect("solver block is retained");
    assert!(
        solver.contains_key("checks"),
        "do not discard stable check counts"
    );
    assert!(solver.contains_key("memory_mb"), "not a timing field");
    assert_eq!(
        solver.contains_key("check_elapsed_s"),
        present,
        "verification.cost.solver.check_elapsed_s"
    );
    let properties = cost["properties"]
        .as_array()
        .expect("property costs retained");
    assert!(
        !properties.is_empty(),
        "property timing exclusion must be live"
    );
    for property in properties {
        assert!(property.get("kind").is_some() && property.get("name").is_some());
        assert!(property.get("checks").is_some(), "property counts retained");
        assert_eq!(
            property.get("elapsed_s").is_some(),
            present,
            "verification.cost.properties[].elapsed_s: {property}"
        );
    }
    let counterfactuals = explained["counterfactuals"]
        .as_array()
        .expect("counterfactuals");
    let violations = counterfactuals
        .iter()
        .filter_map(|item| item.get("violation"))
        .collect::<Vec<_>>();
    assert!(
        !violations.is_empty(),
        "counterfactual timing exclusion must be live"
    );
    for violation in violations {
        let cost = violation["cost"]
            .as_object()
            .expect("counterfactual cost retained");
        assert_eq!(
            cost.contains_key("elapsed_s"),
            present,
            "explained.counterfactuals[].violation.cost.elapsed_s: {violation}"
        );
    }
}

fn assert_stable_stdout(spec: &str, status: i32) {
    let run = ReportRun::new("stdout");
    let first = run.run(&[
        "html",
        "--no-timing",
        spec,
        "--depth",
        "2",
        "--deadlock",
        "ignore",
    ]);
    let second = run.run(&[
        "html",
        spec,
        "--depth",
        "2",
        "--no-timing",
        "--deadlock",
        "ignore",
    ]);
    assert_html(&first, status);
    assert_html(&second, status);
    assert_eq!(
        first.stdout, second.stdout,
        "compare all HTML bytes, with no normalization"
    );
}

#[test]
fn no_timing_stdout_is_byte_stable_for_a_clean_spec() {
    assert_stable_stdout(CLEAN, 0);
}

#[test]
fn no_timing_stdout_is_byte_stable_and_preserves_the_violated_exit() {
    assert_stable_stdout(VIOLATED, 1);
}

#[test]
fn no_timing_stdout_matches_both_output_aliases() {
    let run = ReportRun::new("files");
    let stdout = run.run(&[
        "html",
        "--no-timing",
        CLEAN,
        "--depth",
        "2",
        "--deadlock",
        "ignore",
    ]);
    assert_html(&stdout, 0);
    for (index, flag) in ["-o", "--output"].into_iter().enumerate() {
        let path = run.0.join(format!("report-{index}.html"));
        let path_text = path.to_str().expect("UTF-8 output path");
        let output = if index == 0 {
            run.run(&[
                "html",
                CLEAN,
                flag,
                path_text,
                "--no-timing",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
            ])
        } else {
            run.run(&[
                "html",
                "--no-timing",
                flag,
                path_text,
                CLEAN,
                "--depth",
                "2",
                "--deadlock",
                "ignore",
            ])
        };
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let envelope: Value = serde_json::from_slice(&output.stdout).expect("-o JSON envelope");
        assert_eq!(envelope["result"], "generated");
        assert_eq!(envelope["kind"], "html_report");
        assert_eq!(envelope["spec"], "SweepClean");
        assert_eq!(envelope["output"], path_text);
        assert!(
            envelope.get("content").is_none(),
            "file delivery has no content member"
        );
        assert_eq!(std::fs::read(&path).expect("generated HTML"), stdout.stdout);
    }
}

#[test]
fn default_html_preserves_all_four_observed_timing_locations() {
    let run = ReportRun::new("default");
    let output = run.run(&["html", CLEAN, "--depth", "2", "--deadlock", "ignore"]);
    assert_html(&output, 0);
    assert_four_timing_locations(
        &embedded_json(&output, "verify JSON"),
        &embedded_json(&output, "explain JSON"),
        true,
    );
}

#[test]
fn no_timing_removes_only_the_four_execution_timing_locations() {
    let run = ReportRun::new("locations");
    let default = run.run(&["html", CLEAN, "--depth", "2", "--deadlock", "ignore"]);
    assert_html(&default, 0);
    assert_four_timing_locations(
        &embedded_json(&default, "verify JSON"),
        &embedded_json(&default, "explain JSON"),
        true,
    );
    let stable = run.run(&[
        "html",
        CLEAN,
        "--no-timing",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
    ]);
    assert_html(&stable, 0);
    assert_four_timing_locations(
        &embedded_json(&stable, "verify JSON"),
        &embedded_json(&stable, "explain JSON"),
        false,
    );
}

#[test]
fn no_timing_is_rejected_by_testgen_and_ledger_in_both_positions() {
    let run = ReportRun::new("html-only");
    for command in ["testgen", "ledger"] {
        for args in [
            [command, "--no-timing", CLEAN],
            [command, CLEAN, "--no-timing"],
        ] {
            let output = run.run(&args);
            assert_eq!(output.status.code(), Some(2));
            let envelope: Value = serde_json::from_slice(&output.stdout).expect("usage JSON");
            assert_eq!(envelope["result"], "error");
            assert_eq!(envelope["kind"], "usage");
            assert_eq!(
                envelope["message"],
                format!("unknown {command} option '--no-timing'")
            );
        }
    }
}

#[test]
fn html_contract_advertises_no_timing_in_help_and_actions() {
    let contract: Value =
        serde_json::from_str(include_str!("../cli-contract.json")).expect("native CLI contract");
    let html = contract["root"]["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .find(|node| node["path"] == serde_json::json!(["html"]))
        .expect("html command");
    assert!(
        html["help"].as_str().expect("help").contains("--no-timing"),
        "missing HTML help option"
    );
    assert!(
        html["actions"]
            .as_array()
            .expect("actions")
            .iter()
            .any(|action| {
                action["flags"]
                    .as_array()
                    .is_some_and(|flags| flags.iter().any(|flag| flag == "--no-timing"))
            }),
        "missing HTML actions option"
    );
}

// SPDX-License-Identifier: Apache-2.0

//! Issues #1250 and #1301: every usage line in `rust/fslc/cli-contract.json`
//! lists options before positionals (`fslc verify [--depth DEPTH] ... file`),
//! so the native CLI must accept that order and give the same envelope as the
//! spec-first order. A value-taking option consumes its value (`--depth 3`
//! never makes `3` a path), and an unknown or value-less option stays a
//! `usage` error instead of an `io` error about a file named `--x`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
}

/// Exit code plus stdout; a JSON envelope loses its `cost` members (solver
/// timing, the one field the issue excludes from the comparison).
fn run(args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native CLI");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut value = serde_json::from_str(&stdout).unwrap_or(Value::String(stdout));
    strip_cost(&mut value);
    (output.status.code().expect("exit status"), value)
}

fn strip_cost(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("cost");
            object.values_mut().for_each(strip_cost);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_cost),
        _ => {}
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = root().join("rust/target/issue-1250-cli-argument-order");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join(name)
}

/// The usage order (options first) must give exactly the envelope and exit
/// code of the spec-first order, and that envelope must not be a usage error.
/// The spec-first run goes first once more so that a verification-cache miss
/// on the very first run (`cache.hit`) cannot differ between the two.
fn assert_order_insensitive(options_first: &[&str], options_last: &[&str]) {
    run(options_last);
    let first = run(options_first);
    let last = run(options_last);
    assert_ne!(
        last.1["kind"], "usage",
        "the spec-first baseline must itself be a real run: {options_last:?} -> {}",
        last.1
    );
    assert_eq!(
        first, last,
        "{options_first:?} and {options_last:?} must give the same envelope"
    );
}

fn assert_usage_error(args: &[&str], message: &str) {
    let (status, value) = run(args);
    assert_eq!(status, 2, "{args:?} -> {value}");
    assert_eq!(value["result"], "error", "{args:?} -> {value}");
    assert_eq!(value["kind"], "usage", "{args:?} -> {value}");
    assert_eq!(value["message"], message, "{args:?} -> {value}");
}

const SPEC: &str = "specs/cart_v1.fsl";

#[test]
fn verify_and_scenarios_accept_options_before_the_spec() {
    let spec = "examples/conditional_expressions.fsl";
    assert_order_insensitive(
        &[
            "verify",
            "--depth",
            "3",
            "--deadlock",
            "ignore",
            "--no-cache",
            spec,
        ],
        &[
            "verify",
            spec,
            "--depth",
            "3",
            "--deadlock",
            "ignore",
            "--no-cache",
        ],
    );
    // An option between the spec and another option, as argparse allows.
    assert_order_insensitive(
        &[
            "verify",
            "--depth",
            "3",
            spec,
            "--no-cache",
            "--deadlock",
            "ignore",
        ],
        &[
            "verify",
            spec,
            "--depth",
            "3",
            "--deadlock",
            "ignore",
            "--no-cache",
        ],
    );
    let typed = "rust/fslc/tests/fixtures/typed_annotation_outputs.fsl";
    assert_order_insensitive(
        &["scenarios", "--depth", "2", "--deadlock", "ignore", typed],
        &["scenarios", typed, "--depth", "2", "--deadlock", "ignore"],
    );
}

#[test]
fn issue_1250_repro_depth_value_is_not_the_spec() {
    // The issue's first repro: `--depth 3` before the file was `unknown verify
    // option '3'`.
    let (status, value) = run(&[
        "verify",
        "--depth",
        "3",
        "--no-cache",
        "examples/collection_aggregates.fsl",
    ]);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "verified", "{value}");
    assert_eq!(value["depth"], 3, "{value}");
}

#[test]
fn check_and_sweep_accept_options_before_the_spec() {
    // #1345: compare a genuinely cold options-first sweep with its warm
    // spec-first peer, not two runs made equal by a hidden warm-up. Leave the
    // other commands' historical `run` / `strip_cost` behavior untouched.
    fn strip_metadata(verification: &mut Value, expect_hit: bool) {
        let envelope = verification.as_object_mut().expect("verification object");
        assert!(
            envelope.remove("cost").is_some(),
            "dead sweep cost exclusion"
        );
        let cache = envelope.remove("cache");
        if expect_hit {
            let cache = cache.expect("dead warm sweep cache exclusion");
            assert_eq!(cache["hit"], true);
            assert_eq!(cache["source"], "exact");
        } else {
            assert!(cache.is_none(), "first sweep must start cold: {cache:?}");
        }
    }
    assert_order_insensitive(
        &["check", "--strict-tags", "--edition", "current", SPEC],
        &["check", SPEC, "--strict-tags", "--edition", "current"],
    );
    let cache = scratch(&format!(
        "sweep-cache-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos(),
    ));
    std::fs::create_dir(&cache).expect("fresh worktree-local sweep cache");
    let run_sweep = |args: &[&str], expect_hit: bool| {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(root())
            .env("FSLC_CACHE_DIR", &cache)
            .env_remove("FSLC_CACHE")
            .env_remove("FSLC_CACHE_VERIFY")
            .output()
            .expect("run sweep without warm-up");
        let mut value: Value = serde_json::from_slice(&output.stdout).expect("sweep JSON");
        let grid = value["sweep"]
            .as_object_mut()
            .expect("sweep grid, not usage");
        let cells = grid
            .get_mut("results")
            .and_then(Value::as_array_mut)
            .expect("sweep cells");
        assert!(!cells.is_empty(), "sweep exclusions must be exercised");
        for cell in cells {
            strip_metadata(
                cell.get_mut("verification").expect("verification"),
                expect_hit,
            );
        }
        let minimal = grid.get_mut("minimal_counterexample").expect("minimal key");
        if !minimal.is_null() {
            strip_metadata(
                minimal.get_mut("verification").expect("verification"),
                expect_hit,
            );
        }
        (output.status.code().expect("sweep exit"), value)
    };
    let first = run_sweep(
        &["sweep", "--depth", "0..1", "--deadlock", "ignore", SPEC],
        false,
    );
    let last = run_sweep(
        &["sweep", SPEC, "--depth", "0..1", "--deadlock", "ignore"],
        true,
    );
    assert_eq!(
        first, last,
        "full stable sweep envelope and exit must agree"
    );
}

#[test]
fn explain_mutate_typestate_accept_options_before_the_spec() {
    assert_order_insensitive(
        &["explain", "--depth", "2", SPEC],
        &["explain", SPEC, "--depth", "2"],
    );
    assert_order_insensitive(
        &["mutate", "--depth", "2", "--max-mutants", "3", SPEC],
        &["mutate", SPEC, "--depth", "2", "--max-mutants", "3"],
    );
    assert_order_insensitive(
        &["typestate", "--ts", "specs/order_workflow.fsl"],
        &["typestate", "specs/order_workflow.fsl", "--ts"],
    );
}

#[test]
fn testgen_html_ledger_analyze_accept_options_before_the_spec() {
    assert_order_insensitive(
        &["testgen", "--depth", "2", "--target", "vitest", SPEC],
        &["testgen", SPEC, "--depth", "2", "--target", "vitest"],
    );
    // The raw page embeds solver timing, so compare the `-o` envelope.
    let clean = "rust/fslc/tests/fixtures/sweep_clean.fsl";
    let report = scratch("report.html");
    let report = report.to_str().expect("utf-8 path");
    assert_order_insensitive(
        &[
            "html",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "-o",
            report,
            clean,
        ],
        &[
            "html",
            clean,
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "-o",
            report,
        ],
    );
    assert_order_insensitive(
        &["ledger", "--depth", "2", SPEC],
        &["ledger", SPEC, "--depth", "2"],
    );
    assert_order_insensitive(
        &["analyze", "--projection", "tsg", "--format", "dot", SPEC],
        &["analyze", SPEC, "--projection", "tsg", "--format", "dot"],
    );
    // `analyze` takes `file [file ...]`: options may sit between the files.
    assert_order_insensitive(
        &[
            "analyze",
            "specs/order_workflow.fsl",
            "--projection",
            "tsg",
            SPEC,
        ],
        &[
            "analyze",
            "specs/order_workflow.fsl",
            SPEC,
            "--projection",
            "tsg",
        ],
    );
}

#[test]
fn issue_1301_refine_accepts_depth_before_and_between_the_positionals() {
    let positionals = [
        "specs/cart_impl.fsl",
        "specs/cart_v1.fsl",
        "specs/cart_refines.fsl",
    ];
    let mut last = vec!["refine"];
    last.extend(positionals);
    last.extend(["--depth", "3"]);
    let mut first = vec!["refine", "--depth", "3"];
    first.extend(positionals);
    let between = [
        "refine",
        positionals[0],
        "--depth",
        "3",
        positionals[1],
        positionals[2],
    ];
    assert_order_insensitive(&first, &last);
    assert_order_insensitive(&between, &last);
    let (status, value) = run(&first);
    assert_eq!(status, 0, "{value}");
    assert_eq!(value["result"], "refines", "{value}");
}

#[test]
fn replay_and_counterexample_accept_options_before_the_spec() {
    let spec = "rust/fslc/tests/fixtures/replay_trace.fsl";
    let trace = "rust/fslc/tests/fixtures/replay_trace.valid.v1.json";
    assert_order_insensitive(
        &["replay", "--trace", trace, spec],
        &["replay", spec, "--trace", trace],
    );
    let output = scratch("counterexample.json");
    let output = output.to_str().expect("utf-8 path");
    let violating = "rust/fslc/tests/fixtures/sweep_violating.fsl";
    assert_order_insensitive(
        &[
            "counterexample",
            "export",
            "--depth",
            "4",
            "-o",
            output,
            violating,
        ],
        &[
            "counterexample",
            "export",
            violating,
            "--depth",
            "4",
            "-o",
            output,
        ],
    );
}

#[test]
fn approval_and_document_accept_options_before_the_spec() {
    // A missing record is a deterministic, non-usage outcome in both orders.
    let record = "rust/target/issue-1250-cli-argument-order/missing-record.json";
    assert_order_insensitive(
        &["approval", "check", "--record", record, SPEC],
        &["approval", "check", SPEC, "--record", record],
    );
    assert_order_insensitive(
        &["approval", "diff", "--depth", "2", "--record", record, SPEC],
        &["approval", "diff", SPEC, "--depth", "2", "--record", record],
    );
    let pm = "examples/pm/cancel_system.fsl";
    assert_order_insensitive(
        &["document", "generate", "--lang", "en", pm],
        &["document", "generate", pm, "--lang", "en"],
    );
    assert_order_insensitive(
        &["document", "claims", "--view", "requirements", pm],
        &["document", "claims", pm, "--view", "requirements"],
    );
    let artifact = scratch("cancel_system.md");
    let (status, value) = run(&[
        "document",
        "generate",
        pm,
        "-o",
        artifact.to_str().expect("utf-8 path"),
    ]);
    assert_eq!(status, 0, "{value}");
    let artifact = artifact.to_str().expect("utf-8 path");
    assert_order_insensitive(
        &["document", "check", "--glossary", SPEC, pm, artifact],
        &["document", "check", pm, artifact, "--glossary", SPEC],
    );
}

#[test]
fn db_and_compat_accept_options_before_the_spec() {
    let kernel = "rust/fslc/tests/fixtures/issue_600_db_inconclusive_kernel.fsl";
    assert_order_insensitive(
        &["db", "check", "--depth", "4", "--engine", "bmc", kernel],
        &["db", "check", kernel, "--depth", "4", "--engine", "bmc"],
    );
    let target = "examples/db/runtime_observation_target.fsl";
    let trace = "examples/db/runtime_observation_mismatch.json";
    assert_order_insensitive(
        &["db", "observe", "--trace", trace, target],
        &["db", "observe", target, "--trace", trace],
    );
    let sql = "examples/db/minimal_import.sql";
    assert_order_insensitive(
        &["db", "import", "--name", "ImportedDb", sql],
        &["db", "import", sql, "--name", "ImportedDb"],
    );
    let compat = "examples/db/safe_ai_artifact_compat.fsl";
    assert_order_insensitive(
        &["compat", "check", "--include-ai", compat],
        &["compat", "check", compat, "--include-ai"],
    );
}

#[test]
fn ai_subcommands_accept_options_before_the_file() {
    let refund = "examples/ai/refund_agent_tool_safety.fsl";
    assert_order_insensitive(
        &["ai", "check", "--depth", "4", refund],
        &["ai", "check", refund, "--depth", "4"],
    );
    assert_order_insensitive(
        &["ai", "compat", "--environment", "prod", refund],
        &["ai", "compat", refund, "--environment", "prod"],
    );
    let component = "rust/fslc/tests/fixtures/issue_470_replay_component.fsl";
    let events = "rust/fslc/tests/fixtures/issue_470_replay_events.jsonl";
    assert_order_insensitive(
        &["ai", "replay", "--logs", events, component],
        &["ai", "replay", component, "--logs", events],
    );
    let quality = "examples/ai/support_answer_quality.fsl";
    assert_order_insensitive(
        &["ai", "eval", "--property", "LooseQuality", quality],
        &["ai", "eval", quality, "--property", "LooseQuality"],
    );
    let migration = "rust/fslc/tests/fixtures/issue_509_migration_custom_threshold.fsl";
    let regress = [
        "--before-records",
        "examples/ai/support_eval_v7.jsonl",
        "--after-records",
        "examples/ai/support_eval_v8_regressed.jsonl",
        "--migration",
        "LooseAccuracyMigration",
    ];
    let mut first = vec!["ai", "regress"];
    first.extend(regress);
    first.push(migration);
    let mut last = vec!["ai", "regress", migration];
    last.extend(regress);
    assert_order_insensitive(&first, &last);
    let project = "rust/fslc/tests/fixtures/error_envelope_literate_ai_project.md";
    let logs = "rust/fslc/tests/fixtures/error_envelope_empty_records.json";
    assert_order_insensitive(
        &["ai", "drift", "--logs", logs, project],
        &["ai", "drift", project, "--logs", logs],
    );
}

#[test]
fn domain_subcommands_accept_options_before_the_file() {
    let clean = "rust/fslc/tests/fixtures/issue_641_domain_clean.fsl";
    assert_order_insensitive(
        &["domain", "check", "--depth", "4", clean],
        &["domain", "check", clean, "--depth", "4"],
    );
    let saga = "rust/fslc/tests/fixtures/domain_characterization/effect_saga_valid.fsl";
    let expanded = scratch("expanded.fsl");
    let expanded = expanded.to_str().expect("utf-8 path");
    assert_order_insensitive(
        &["domain", "expand", "-o", expanded, saga],
        &["domain", "expand", saga, "-o", expanded],
    );
    assert_order_insensitive(
        &[
            "domain", "testgen", "--depth", "2", "--target", "vitest", saga,
        ],
        &[
            "domain", "testgen", saga, "--depth", "2", "--target", "vitest",
        ],
    );
    let generated = scratch("generated");
    let generated = generated.to_str().expect("utf-8 path");
    assert_order_insensitive(
        &[
            "domain",
            "generate",
            "--target",
            "typescript",
            "-o",
            generated,
            saga,
        ],
        &[
            "domain",
            "generate",
            saga,
            "--target",
            "typescript",
            "-o",
            generated,
        ],
    );
    let effect = "examples/domain/order_async_effect.fsl";
    let replay = "examples/domain/order_async_effect_replay.jsonl";
    assert_order_insensitive(
        &["domain", "replay", "--logs", replay, effect],
        &["domain", "replay", effect, "--logs", replay],
    );
}

#[test]
fn causal_subcommands_accept_options_before_the_file() {
    let retention = "examples/causal/subscription_retention.fsl";
    assert_order_insensitive(
        &["causal", "analyze", "--profile", "causal-review", retention],
        &["causal", "analyze", retention, "--profile", "causal-review"],
    );
    let incident = "examples/causal/incident_response.fsl";
    assert_order_insensitive(
        &["causal", "verify-expectations", "--depth", "4", incident],
        &["causal", "verify-expectations", incident, "--depth", "4"],
    );
    assert_order_insensitive(
        &["causal", "ledger", "--format", "json", retention],
        &["causal", "ledger", retention, "--format", "json"],
    );
    assert_order_insensitive(
        &["causal", "diff", "--format", "json", retention, retention],
        &["causal", "diff", retention, retention, "--format", "json"],
    );
    let observe = [
        "--from-log",
        "examples/causal/evidence/incident-observation-log.jsonl",
        "--mapping",
        "examples/causal/evidence/incident-log-mapping.fsl",
        "--scope",
        "examples/causal/evidence/incident-replay-scope.json",
        "--period-start",
        "2026-01-01",
        "--period-end",
        "2026-03-31",
    ];
    let mut first = vec!["causal", "observe-expectations"];
    first.extend(observe);
    first.push(incident);
    let mut last = vec!["causal", "observe-expectations", incident];
    last.extend(observe);
    assert_order_insensitive(&first, &last);
}

#[test]
fn options_without_a_spec_or_unknown_options_stay_usage_errors() {
    // The issue's `check` repro was an `io` error about a file named
    // `--no-cache`; `check` has no such option, so it is a usage error.
    assert_usage_error(
        &["check", "--no-cache", SPEC],
        "unknown check option '--no-cache'",
    );
    assert_usage_error(
        &["verify", "--bogus", SPEC],
        "unknown verify option '--bogus'",
    );
    // Options but no spec: the command's own missing-spec usage message,
    // never an `io` error about `--depth`.
    assert_usage_error(
        &["verify", "--depth", "3"],
        "usage: fslc verify SPEC [options]",
    );
    assert_usage_error(&["mutate", "--depth", "3"], "fslc mutate requires a spec");
    assert_usage_error(
        &["refine", "--depth", "3", "specs/cart_impl.fsl"],
        "refine requires IMPL ABS MAPPING",
    );
    // A trailing value-taking option without its value.
    assert_usage_error(&["verify", "--depth"], "--depth requires a value");
    assert_usage_error(&["analyze", SPEC, "--focus"], "--focus requires a value");
    // Two specs keep the pre-#1250 wording and exit code.
    assert_usage_error(
        &["verify", SPEC, "--depth", "2", SPEC],
        "unknown verify option 'specs/cart_v1.fsl'",
    );
}

#[test]
fn double_dash_ends_options() {
    // argparse semantics: every token after `--` is a positional, so a spec
    // can follow it (and a path starting with `-` can be named).
    let spec = "examples/conditional_expressions.fsl";
    assert_order_insensitive(
        &["verify", "--depth", "2", "--no-cache", "--", spec],
        &["verify", spec, "--depth", "2", "--no-cache"],
    );
    let (status, value) = run(&["check", "--", "-missing.fsl"]);
    assert_eq!(status, 2, "{value}");
    assert_eq!(value["kind"], "io", "{value}");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.starts_with("-missing.fsl")),
        "{value}"
    );
}

// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_cases.rs"]
pub(crate) mod cases;

use cases::{Results, Scratch, value, without_versions};
use serde_json::{Value, json};

fn run(scratch: &Scratch, template: &str) -> std::process::Output {
    cases::command(env!("CARGO_BIN_EXE_fslc"), &scratch.0, template, true)
}

fn git(scratch: &Scratch, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Edition Test",
            "-c",
            "user.email=edition@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .current_dir(&scratch.0)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn dialect_commands() -> Vec<Vec<String>> {
    // All paths are real; the same-pair wrong-dialect controls in the primary
    // adapter do not stand in for these successful dialect/sidecar executions.
    const COMMANDS: &str = "\
check {repo}/examples/e2e/1_business.fsl
check {repo}/examples/e2e/2_requirements.fsl
check {repo}/examples/ai/recursive_support_agent.fsl
check {repo}/examples/refinement_liveness/governance_preserves_progress.fsl
check compose.fsl
db check {repo}/examples/db/safe_add_nullable_column.fsl --depth 2
db observe {repo}/examples/db/safe_add_nullable_column.fsl --trace {fixtures}/error_envelope_empty_records.json
compat check {repo}/examples/db/safe_add_nullable_column.fsl
ai check {repo}/examples/ai/refund_agent_tool_safety.fsl --depth 2
ai compat {repo}/examples/ai/refund_agent_tool_safety.fsl
ai replay {repo}/examples/ai/refund_agent_tool_safety.fsl --logs {fixtures}/error_envelope_empty_records.json
ai eval {repo}/examples/ai/support_answer_quality.fsl --property LooseQuality --records {repo}/examples/ai/support_eval_v3.jsonl
domain replay {fixtures}/issue_518_domain_replay.fsl --logs {fixtures}/issue_518_clean.jsonl
replay {fixtures}/replay_trace.fsl --trace {fixtures}/replay_trace.valid.v1.json
causal check {repo}/examples/causal/incident_response.fsl
causal analyze {repo}/examples/causal/incident_response.fsl --projection causal_graph
causal diff {repo}/examples/causal/incident_response.fsl {repo}/examples/causal/incident_response.fsl
causal observe-expectations {repo}/examples/causal/incident_response.fsl --from-log {repo}/examples/causal/evidence/incident-observation-log.jsonl --mapping {repo}/examples/causal/evidence/incident-log-mapping.fsl --scope {repo}/examples/causal/evidence/incident-replay-scope.json --period-start 2026-01-01 --period-end 2026-03-31 --out observations.json";
    let replacements = [
        ("{repo}", cases::root()),
        ("{fixtures}", cases::root().join("rust/fslc/tests/fixtures")),
    ];
    COMMANDS
        .lines()
        .map(|line| cases::expand(line, &replacements))
        .collect()
}

#[test]
pub(crate) fn actual_dialects_and_valid_sidecars_are_preserved_twice() {
    let mut results = Results::default();
    let scratch = Scratch::new();
    scratch.write("dep.fsl", "fsl 5\nspec Dep { state { ready: Bool = false } invariant Typed { forall i in 0..1: ready == true or ready == false } }\n");
    scratch.write(
        "compose.fsl",
        "compose C { use Dep as d from \"dep.fsl\" }\n",
    );
    let commands = dialect_commands();
    for repeat in 0..2 {
        // Q2 is root-only: a dependency rejected on its own is deliberately
        // still accepted through compose. This is preservation, not a detector
        // for the separately tracked dependency-admission defect.
        let dependency = run(&scratch, "check dep.fsl");
        results.check(
            &format!("preserved/dependency-scope/{repeat}"),
            value(&dependency),
            "standalone unsupported_in_edition",
            dependency.status.code() == Some(2)
                && value(&dependency)["kind"] == "unsupported_in_edition",
        );
        for command in &commands {
            let output = cases::invoke(env!("CARGO_BIN_EXE_fslc"), command, &scratch.0, true);
            let actual = value(&output);
            results.check(
                &format!("preserved/dialect/{repeat}/{}", command.join(" ")),
                (output.status.code(), &actual),
                "successful real dialect and sidecar",
                output.status.code() == Some(0) && actual["result"] != "error",
            );
        }
        let document = format!("fsl 5\n{}", cases::DOCUMENT_LEGACY);
        let document = fslc_rust::migration::plan_migration(
            &document,
            "spec.fsl",
            fsl_syntax::Edition::Fsl5,
            &cases::gated(),
        )
        .unwrap()
        .migrated_source
        .unwrap();
        scratch.write("spec.fsl", &document);
        for command in [
            "document generate spec.fsl -o document.md",
            "document claims spec.fsl",
            "document check spec.fsl document.md",
        ] {
            let output = run(&scratch, command);
            let actual = value(&output);
            results.check(
                &format!("preserved/document/{repeat}/{command}"),
                (output.status.code(), &actual),
                0,
                output.status.code() == Some(0),
            );
        }
    }
    results.finish();
}

#[test]
pub(crate) fn approval_records_and_git_roots_refuse_before_output_writes() {
    let mut results = Results::default();
    let scratch = Scratch::new();
    let rejected = cases::rejected_metadata();
    let canonical = fslc_rust::migration::plan_migration(
        &rejected,
        "spec.fsl",
        fsl_syntax::Edition::Fsl5,
        &cases::gated(),
    )
    .unwrap()
    .migrated_source
    .unwrap();
    scratch.write("spec.fsl", cases::WARNING_CASES[1].2);
    git(&scratch, &["init", "-q"]);
    git(&scratch, &["add", "spec.fsl"]);
    git(&scratch, &["commit", "-qm", "canonical fixture"]);
    let canonical_sha = git(&scratch, &["rev-parse", "HEAD"]);
    let create = "approval create spec.fsl --kind ledger --artifact ledger.md --approver edition-test --depth 2 -o record.json";
    for command in [
        "ledger spec.fsl --depth 2 -o ledger.md",
        create,
        "approval check spec.fsl --record record.json",
        "approval diff spec.fsl --record record.json --depth 2",
    ] {
        let output = run(&scratch, command);
        let actual = value(&output);
        results.check(
            &format!("preserved/{command}"),
            (output.status.code(), &actual),
            0,
            output.status.code() == Some(0),
        );
    }
    scratch.write("spec.fsl", &rejected);
    let oracle = cases::refusal(
        env!("CARGO_BIN_EXE_fslc"),
        &scratch.0,
        std::path::Path::new("spec.fsl"),
    );
    for command in [
        "approval check spec.fsl --record record.json",
        "approval diff spec.fsl --record record.json --depth 2",
        "approval create spec.fsl --kind ledger --artifact ledger.md --approver edition-test --depth 2 -o refused-record.json",
    ] {
        let output = run(&scratch, command);
        results.error_envelope(
            &format!("approval/current/{command}"),
            &output,
            &oracle,
            !scratch.0.join("refused-record.json").exists(),
        );
    }
    git(&scratch, &["add", "spec.fsl"]);
    git(&scratch, &["commit", "-qm", "rejected edition fixture"]);
    let rejected_sha = git(&scratch, &["rev-parse", "HEAD"]);
    scratch.write("spec.fsl", &canonical);
    git(&scratch, &["add", "spec.fsl"]);
    git(&scratch, &["commit", "-qm", "canonical current fixture"]);
    let current_sha = git(&scratch, &["rev-parse", "HEAD"]);
    for (label, range) in [
        ("old", format!("{rejected_sha}..{current_sha}")),
        ("new", format!("{canonical_sha}..{rejected_sha}")),
    ] {
        let output = run(&scratch, &format!("diff --git {range} spec.fsl --depth 2"));
        let actual = without_versions(value(&output));
        results.check(
            &format!("git/{label}"),
            (output.status.code(), &actual),
            "unsupported_in_edition with complete finding",
            output.status.code() == Some(2)
                && actual["kind"] == "unsupported_in_edition"
                && full_materialized_finding(&actual, &rejected, &rejected_sha),
        );
    }
    // A v1 record is unsigned. Its baseline model digest is unchanged by the
    // metadata/header spelling, so rebinding to that git root is a valid input
    // for admission priority (not a fabricated missing/invalid record).
    let mut record: Value =
        serde_json::from_str(&std::fs::read_to_string(scratch.0.join("record.json")).unwrap())
            .unwrap();
    record["spec"]["git_commit"] = json!(rejected_sha);
    scratch.write("old-record.json", &record.to_string());
    let output = run(
        &scratch,
        "approval diff spec.fsl --record old-record.json --depth 2",
    );
    let actual = without_versions(value(&output));
    results.check(
        "approval/old-root",
        (output.status.code(), &actual),
        "unsupported_in_edition before baseline digest/diff",
        output.status.code() == Some(2)
            && actual["kind"] == "unsupported_in_edition"
            && full_materialized_finding(&actual, &rejected, &rejected_sha),
    );
    results.finish();
}

fn full_materialized_finding(output: &Value, source: &str, commit: &str) -> bool {
    // The git materialization directory is allocated per invocation. Its path
    // is ambient, not discarded: validate the live locator, then compare every
    // diagnostic field including its exact file identity within this result.
    let Some(path) = output
        .pointer("/findings/0/loc/file")
        .and_then(Value::as_str)
    else {
        return false;
    };
    let materialized = path.starts_with(cases::root().join(".codex/tmp").to_str().unwrap())
        && path.ends_with("/spec.fsl");
    if path != format!("{commit}:spec.fsl") && !materialized {
        return false;
    }
    let findings = cases::findings(source, path, fsl_syntax::Edition::Fsl5);
    *output
        == json!({"fsl":"1.0","result":"error","kind":"unsupported_in_edition","edition":"next","fsl_edition":5,"findings":findings})
}

#[test]
fn approval_only_report_renderers_do_not_deliver_stderr_warnings() {
    let mut results = Results::default();
    for kind in ["html", "ledger"] {
        let scratch = Scratch::new();
        scratch.write("spec.fsl", cases::WARNING_CASES[1].2);
        git(&scratch, &["init", "-q"]);
        git(&scratch, &["add", "spec.fsl"]);
        git(&scratch, &["commit", "-qm", "report fixture"]);
        for (phase, command) in [
            (
                "generate",
                format!("{kind} spec.fsl --depth 2 -o artifact.out"),
            ),
            (
                "create",
                format!(
                    "approval create spec.fsl --kind {kind} --artifact artifact.out --approver edition-test --depth 2 -o record.json"
                ),
            ),
            (
                "check",
                "approval check spec.fsl --record record.json".into(),
            ),
            (
                "overlay",
                "ledger spec.fsl --depth 2 --approval record.json -o approved.md".into(),
            ),
        ] {
            let output = run(&scratch, &command);
            let actual = value(&output);
            // Until approval admission changes, the flow has no envelope or
            // stderr warning delivery. Report baselines still embed theirs.
            let warning_count = actual
                .to_string()
                .matches("\"code\":\"legacy_string_metadata\"")
                .count();
            let produced = json!([
                output.status.code(),
                output.stderr,
                actual.get("warnings"),
                warning_count
            ]);
            let expected = json!([0, [], null, 0]);
            results.check(
                &format!("preserved/approval-render/{kind}/{phase}"),
                &produced,
                &expected,
                produced == expected,
            );
        }
    }
    results.finish();
}

// SPDX-License-Identifier: Apache-2.0

//! An artifact output target that is the same file as an input of the command
//! is refused before anything is written (`docs/design/DESIGN-atomic-artifact-output.md`).
//! Identity is the file, not its spelling: `./s.fsl`, a symlink and a hard link
//! to `s.fsl` are all `s.fsl`.

#![cfg(unix)]

#[path = "support/atomic_output_cli.rs"]
#[allow(dead_code)]
mod cli;

use cli::{root, run, scratch, state};
use serde_json::Value;
use std::path::Path;
use std::process::Output;

const REFUSAL: &str = "output target is an input of this command";
const NONREGULAR: &str = "output target is not a regular file";

const PROBE: &str = r#"spec OutputProbe {
  state { x: Bool = false }
  action stay() { x = x }
  @requirement("REQ-OUT-001", "x remains false")
  invariant Safe { not x }
}
"#;

const EVIDENCE: &str = r#"{"result":"conformant","formal_result":"not_run","evidence":{"kind":"runtime_replay"},"requirements":["REQ-OUT-001"]}
"#;

const HTML: [&str; 7] = [
    "html",
    "s.fsl",
    "--no-timing",
    "--depth",
    "2",
    "--deadlock",
    "ignore",
];

fn copy(home: &Path, name: &str, repository_path: &str) {
    let destination = home.join("out").join(name);
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::copy(root().join(repository_path), destination).unwrap();
}

fn put(home: &Path, name: &str, content: &str) {
    std::fs::write(home.join("out").join(name), content).unwrap();
}

fn with_target(arguments: &[&str], flag: &str, target: &str) -> Vec<String> {
    arguments
        .iter()
        .copied()
        .chain([flag, target])
        .map(str::to_owned)
        .collect()
}

fn envelope(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON envelope")
}

// Runs in `out/`, so relative spellings are relative to the inputs.
fn refused(home: &Path, argv: &[String]) -> Output {
    let directory = home.join("out");
    let before = state(&directory);
    let output = run(home, &directory, argv, None);
    assert_eq!(output.status.code(), Some(2), "{argv:?}: {output:?}");
    let value = envelope(&output);
    assert_eq!(value["result"], "error", "{value}");
    let message = value["message"].as_str().expect("message");
    assert!(message.ends_with(REFUSAL), "{argv:?}: {message}");
    assert_eq!(
        state(&directory),
        before,
        "{argv:?}: input bytes, inodes, modes, links and no scratch file"
    );
    output
}

/// The refusal keeps the envelope of the existing non-regular-target refusal:
/// the same exit, stderr and JSON, with only the message naming the reason.
fn same_envelope_as_nonregular(home: &Path, arguments: &[&str], flag: &str, target: &str) {
    let refusal = refused(home, &with_target(arguments, flag, target));
    std::fs::create_dir(home.join("out/nonregular")).unwrap();
    let directory = run(
        home,
        &home.join("out"),
        &with_target(arguments, flag, "nonregular"),
        None,
    );
    std::fs::remove_dir(home.join("out/nonregular")).unwrap();
    assert_eq!(refusal.status.code(), directory.status.code());
    assert_eq!(refusal.stderr, directory.stderr);
    let (mut refusal, mut directory) = (envelope(&refusal), envelope(&directory));
    let (refused_message, directory_message) = (
        refusal["message"].take().as_str().unwrap().to_owned(),
        directory["message"].take().as_str().unwrap().to_owned(),
    );
    assert!(
        directory_message.ends_with(NONREGULAR),
        "{directory_message}"
    );
    assert_eq!(
        refused_message.replace(REFUSAL, NONREGULAR),
        directory_message.replace("nonregular", target),
        "{arguments:?}"
    );
    assert_eq!(refusal, directory, "{arguments:?}");
    if flag == "-o" && arguments[0] != "testgen" {
        assert_eq!(refusal["kind"], "io", "{refusal}");
    }
}

#[test]
fn same_path_is_refused_for_every_single_output_command() {
    let fixtures: [(&str, &str, &[&str]); 9] = [
        ("html", "rust/fslc/tests/fixtures/sweep_clean.fsl", &HTML),
        (
            "ledger",
            "rust/fslc/tests/fixtures/sweep_clean.fsl",
            &["ledger", "s.fsl", "--depth", "2", "--deadlock", "ignore"],
        ),
        (
            "testgen",
            "specs/cart_v1.fsl",
            &["testgen", "s.fsl", "--depth", "3", "--target", "pytest"],
        ),
        (
            "counterexample-export",
            "rust/fslc/tests/fixtures/sweep_violating.fsl",
            &["counterexample", "export", "s.fsl", "--depth", "4"],
        ),
        (
            "document-generate",
            "examples/pm/cancel_system.fsl",
            &["document", "generate", "s.fsl"],
        ),
        (
            "document-claims",
            "examples/pm/cancel_system.fsl",
            &["document", "claims", "s.fsl"],
        ),
        ("domain-expand", cli::DOMAIN, &["domain", "expand", "s.fsl"]),
        (
            "domain-testgen",
            cli::DOMAIN,
            &["domain", "testgen", "s.fsl", "--target", "vitest"],
        ),
        (
            "db-import",
            "examples/db/minimal_import.sql",
            &["db", "import", "s.fsl", "--name", "ImportedDb"],
        ),
    ];
    for (label, fixture, arguments) in fixtures {
        let home = scratch(&format!("input-target-{label}"));
        copy(&home, "s.fsl", fixture);
        same_envelope_as_nonregular(&home, arguments, "-o", "s.fsl");
    }
}

#[test]
fn spelling_symlink_and_hard_link_name_the_same_input() {
    let home = scratch("input-target-spelling");
    copy(&home, "s.fsl", "rust/fslc/tests/fixtures/sweep_clean.fsl");
    refused(&home, &with_target(&HTML, "-o", "./s.fsl"));
    let mut dotted = HTML;
    dotted[1] = "./s.fsl";
    refused(&home, &with_target(&dotted, "-o", "s.fsl"));
    refused(&home, &with_target(&HTML, "-o", "../out/s.fsl"));
    std::os::unix::fs::symlink("s.fsl", home.join("out/link.fsl")).unwrap();
    refused(&home, &with_target(&HTML, "-o", "link.fsl"));
    let mut through_link = HTML;
    through_link[1] = "link.fsl";
    refused(&home, &with_target(&through_link, "-o", "s.fsl"));
    std::fs::hard_link(home.join("out/s.fsl"), home.join("out/hard.fsl")).unwrap();
    refused(&home, &with_target(&HTML, "-o", "hard.fsl"));
    let mut through_hard_link = HTML;
    through_hard_link[1] = "hard.fsl";
    refused(&home, &with_target(&through_hard_link, "-o", "s.fsl"));
}

#[test]
fn a_different_target_is_still_written() {
    let home = scratch("input-target-positive");
    copy(&home, "s.fsl", "rust/fslc/tests/fixtures/sweep_clean.fsl");
    std::fs::hard_link(home.join("out/s.fsl"), home.join("out/hard.fsl")).unwrap();
    let input = std::fs::read(home.join("out/s.fsl")).unwrap();
    let output = run(
        &home,
        &home.join("out"),
        &with_target(&HTML, "-o", "report.html"),
        None,
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(envelope(&output)["output"], "report.html");
    let report = std::fs::read_to_string(home.join("out/report.html")).unwrap();
    assert!(report.starts_with("<!doctype html>"), "{report}");
    assert_eq!(std::fs::read(home.join("out/s.fsl")).unwrap(), input);
    assert_eq!(std::fs::read(home.join("out/hard.fsl")).unwrap(), input);
}

#[test]
fn explicit_file_options_are_inputs() {
    let home = scratch("input-target-options");
    put(&home, "s.fsl", PROBE);
    put(&home, "evidence.json", EVIDENCE);
    same_envelope_as_nonregular(
        &home,
        &["ledger", "s.fsl", "--evidence", "evidence.json"],
        "-o",
        "evidence.json",
    );
    same_envelope_as_nonregular(
        &home,
        &[
            "document",
            "generate",
            "s.fsl",
            "--evidence",
            "evidence.json",
        ],
        "-o",
        "evidence.json",
    );
    let approval = [
        "approval",
        "create",
        "s.fsl",
        "--kind",
        "html",
        "--artifact",
        "report.html",
        "--approver",
        "alice",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
    ];
    // A solver approval binds the spec to a Git commit.
    for git in [
        &["init", "-q"][..],
        &["add", "out/s.fsl"],
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "spec",
        ],
    ] {
        let status = std::process::Command::new("git")
            .args(git)
            .current_dir(&home)
            .status()
            .unwrap();
        assert!(status.success(), "git {git:?}");
    }
    let html = ["html", "s.fsl", "--depth", "2", "--deadlock", "ignore"];
    let generated = run(
        &home,
        &home.join("out"),
        &with_target(&html, "-o", "report.html"),
        None,
    );
    assert_eq!(generated.status.code(), Some(0), "{generated:?}");
    let created = run(
        &home,
        &home.join("out"),
        &with_target(&approval, "-o", "record.json"),
        None,
    );
    assert_eq!(
        created.status.code(),
        Some(0),
        "positive control: {created:?}"
    );
    same_envelope_as_nonregular(&home, &approval, "-o", "report.html");
    same_envelope_as_nonregular(&home, &approval, "-o", "s.fsl");
}

#[test]
fn imported_specs_are_inputs() {
    let home = scratch("input-target-import");
    copy(&home, "order_system.fsl", "specs/order_system.fsl");
    copy(&home, "cart_v1.fsl", "specs/cart_v1.fsl");
    copy(&home, "payment.fsl", "specs/payment.fsl");
    let mut html = HTML;
    html[1] = "order_system.fsl";
    same_envelope_as_nonregular(&home, &html, "-o", "payment.fsl");
    let ledger = ["ledger", "order_system.fsl", "--depth", "2"];
    same_envelope_as_nonregular(&home, &ledger, "-o", "cart_v1.fsl");
}

// This model yields two observation artifacts, so `--out base.jsonl` writes
// `base.<evidence id>.jsonl` for each. Each input is placed at one of those
// names in turn: whichever is published second proves that nothing, not even
// the first artifact, is written before the refusal.
#[test]
fn causal_observation_outputs_refuse_inputs_before_any_is_written() {
    let home = scratch("input-target-causal");
    for name in ["incident_response.fsl", "incident_system.fsl"] {
        copy(&home, name, &format!("examples/causal/{name}"));
    }
    for name in [
        "incident-observation-log.jsonl",
        "incident-log-mapping.fsl",
        "incident-replay-scope.json",
    ] {
        copy(
            &home,
            &format!("evidence/{name}"),
            &format!("examples/causal/evidence/{name}"),
        );
    }
    let arguments = |log: &str, mapping: &str| {
        [
            "causal",
            "observe-expectations",
            "incident_response.fsl",
            "--from-log",
            log,
            "--mapping",
            mapping,
            "--scope",
            "evidence/incident-replay-scope.json",
            "--period-start",
            "2026-01-01",
            "--period-end",
            "2026-03-31",
            "--out",
            "evidence/out.jsonl",
        ]
        .map(str::to_owned)
    };
    let log = "evidence/incident-observation-log.jsonl";
    let mapping = "evidence/incident-log-mapping.fsl";
    for evidence_id in [
        "OBS_E_AlertQualityImproves_IncidentResponse",
        "OBS_E_GuardrailsVisible_IncidentResponse",
    ] {
        let collision = format!("evidence/out.{evidence_id}.jsonl");
        std::fs::rename(
            home.join("out").join(log),
            home.join("out").join(&collision),
        )
        .unwrap();
        let refusal = refused(&home, &arguments(&collision, mapping));
        let message = envelope(&refusal)["message"].as_str().unwrap().to_owned();
        assert_eq!(message, format!("cannot write {collision}: {REFUSAL}"));
        assert_eq!(envelope(&refusal)["kind"], "io");
        std::fs::rename(
            home.join("out").join(&collision),
            home.join("out").join(log),
        )
        .unwrap();
        std::fs::rename(
            home.join("out").join(mapping),
            home.join("out").join(&collision),
        )
        .unwrap();
        refused(&home, &arguments(log, &collision));
        std::fs::rename(
            home.join("out").join(&collision),
            home.join("out").join(mapping),
        )
        .unwrap();
    }
    // Positive control: the same invocation writes both artifacts.
    let written = run(&home, &home.join("out"), &arguments(log, mapping), None);
    assert_eq!(written.status.code(), Some(0), "{written:?}");
    for evidence_id in [
        "OBS_E_AlertQualityImproves_IncidentResponse",
        "OBS_E_GuardrailsVisible_IncidentResponse",
    ] {
        assert!(
            home.join(format!("out/evidence/out.{evidence_id}.jsonl"))
                .is_file()
        );
    }
}

#[test]
fn domain_generate_refuses_an_input_before_any_file_is_written() {
    let home = scratch("input-target-domain-generate");
    copy(&home, "s.fsl", cli::DOMAIN);
    let arguments = ["domain", "generate", "s.fsl", "--target", "typescript"];
    let manifest = run(
        &home,
        &home.join("out"),
        &arguments.map(str::to_owned),
        None,
    );
    assert_eq!(manifest.status.code(), Some(0), "{manifest:?}");
    let files = envelope(&manifest)["files"].as_array().unwrap().clone();
    assert!(files.len() >= 2, "multi-file publication");
    let last = files.last().unwrap()["path"].as_str().unwrap().to_owned();
    // The input sits where the last generated file would go.
    let input = Path::new("generated").join(&last);
    std::fs::create_dir_all(home.join("out").join(input.parent().unwrap())).unwrap();
    std::fs::rename(home.join("out/s.fsl"), home.join("out").join(&input)).unwrap();
    let mut argv = arguments.map(str::to_owned);
    argv[2] = input.to_str().unwrap().to_owned();
    let argv = with_target(
        &argv.iter().map(String::as_str).collect::<Vec<_>>(),
        "-o",
        "generated",
    );
    refused(&home, &argv);
}

// `document` never reads the abstraction an `implements ... from` names, yet
// `-o` onto it would replace the user's spec with the artifact (#1753). The
// fixture has requirement IDs, so `generate` reaches the write.
#[test]
fn document_refuses_the_implements_target() {
    let home = scratch("input-target-implements");
    copy(&home, "cancel_system.fsl", cli::PM);
    copy(&home, "cancel_flow.fsl", "examples/pm/cancel_flow.fsl");
    std::os::unix::fs::symlink("cancel_flow.fsl", home.join("out/flow_link.fsl")).unwrap();
    std::fs::hard_link(
        home.join("out/cancel_flow.fsl"),
        home.join("out/flow_hard.fsl"),
    )
    .unwrap();
    let abstraction = std::fs::read(home.join("out/cancel_flow.fsl")).unwrap();
    let commands: [&[&str]; 2] = [
        &["document", "claims", "cancel_system.fsl"],
        &["document", "generate", "cancel_system.fsl"],
    ];
    for arguments in commands {
        same_envelope_as_nonregular(&home, arguments, "-o", "cancel_flow.fsl");
        for target in ["./cancel_flow.fsl", "flow_link.fsl", "flow_hard.fsl"] {
            refused(&home, &with_target(arguments, "-o", target));
        }
        // The path resolves against the spec's directory, not the working one.
        let mut from_parent: Vec<String> = arguments.iter().map(|&a| a.to_owned()).collect();
        from_parent[2] = "out/cancel_system.fsl".to_owned();
        from_parent.extend(["-o".to_owned(), "out/cancel_flow.fsl".to_owned()]);
        let before = state(&home.join("out"));
        let output = run(&home, &home, &from_parent, None);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let message = envelope(&output)["message"].as_str().unwrap().to_owned();
        assert!(message.ends_with(REFUSAL), "{message}");
        assert_eq!(state(&home.join("out")), before);
    }
    assert_eq!(
        std::fs::read(home.join("out/cancel_flow.fsl")).unwrap(),
        abstraction
    );
    // Positive control: an unrelated path is still written, and the
    // abstraction and its other names keep their bytes.
    for (arguments, target, head) in [
        (commands[0], "claims.json", "{"),
        (commands[1], "requirements.md", "---"),
    ] {
        let output = run(
            &home,
            &home.join("out"),
            &with_target(arguments, "-o", target),
            None,
        );
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(envelope(&output)["output"], target);
        let written = std::fs::read_to_string(home.join("out").join(target)).unwrap();
        assert!(written.starts_with(head), "{target}: {written}");
    }
    for name in ["cancel_flow.fsl", "flow_link.fsl", "flow_hard.fsl"] {
        assert_eq!(
            std::fs::read(home.join("out").join(name)).unwrap(),
            abstraction,
            "{name}"
        );
    }
}

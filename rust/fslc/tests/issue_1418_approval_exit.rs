// SPDX-License-Identifier: Apache-2.0

//! #1418 real CLI detectors. Full same-invocation tuple qualification uses the
//! separately pinned temporary capture mutation; normal runs preserve stderr.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde_json::{Value, json};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CAPTURE: &str = "FSLC_1418_DOWNSTREAM ";

struct Repository(PathBuf);

impl Repository {
    fn new() -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "issue-1418-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // A fresh leaf per test; retain raw observations after assertion failure.
        assert!(
            !path.exists(),
            "scratch directory must be fresh: {}",
            path.display()
        );
        std::fs::create_dir_all(&path).unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/approval.fsl");
        std::fs::copy(fixture, path.join("spec.fsl")).unwrap();
        let repo = Self(path);
        repo.git(&["init", "-q"]);
        repo.commit("valid baseline");
        repo
    }

    fn git(&self, args: &[&str]) -> Output {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "spec.fsl"]);
        self.git(&[
            "-c",
            "user.name=Acceptance Test",
            "-c",
            "user.email=acceptance@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            message,
        ]);
    }

    fn replace_spec(&self, source: &str) {
        std::fs::write(self.0.join("spec.fsl"), source).unwrap();
        self.commit("replace the spec");
    }

    fn break_invariant(&self) {
        let path = self.0.join("spec.fsl");
        let source = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            source.matches("approved = true").count(),
            2,
            "the action also assigns ever_approved; use exact indentation below"
        );
        let from = "    approved = true\n";
        assert_eq!(source.matches(from).count(), 1);
        let broken = source.replacen(from, "    approved = false\n", 1);
        assert_ne!(source, broken);
        std::fs::write(path, broken).unwrap();
        self.commit("valid syntax with invariant-breaking action");
    }

    fn run(&self, args: &[&str]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
            .args(args)
            .current_dir(&self.0)
            .env("FSLC_CACHE", "off")
            .env_remove("FSLC_CACHE_VERIFY")
            .env_remove("FSLC_UNSTABLE")
            .output()
            .unwrap();
        let dir = self.0.join("observations");
        std::fs::create_dir_all(&dir).unwrap();
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        std::fs::write(dir.join(format!("{id}.stdout")), &output.stdout).unwrap();
        std::fs::write(dir.join(format!("{id}.stderr")), &output.stderr).unwrap();
        std::fs::write(
            dir.join(format!("{id}.argv.json")),
            serde_json::to_vec(args).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join(format!("{id}.status")),
            format!("{:?}\n", output.status.code()),
        )
        .unwrap();
        output
    }

    fn render_and_approve(&self, kind: &str, signed: bool) -> Value {
        let rendered = match kind {
            "scenarios" => self.run(&[
                "scenarios",
                "spec.fsl",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
            ]),
            "html" => self.run(&[
                "html",
                "spec.fsl",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
                "-o",
                "review.html",
            ]),
            _ => panic!("bounded target roster"),
        };
        assert_status(&rendered, 0);
        if kind == "scenarios" {
            assert_eq!(
                rendered.stdout.as_slice(),
                include_bytes!("fixtures/issue_1418_success_scenarios.json"),
                "successful canonical scenarios and their migration data must stay byte-identical"
            );
        }
        let artifact = if kind == "scenarios" {
            std::fs::write(self.0.join("review.json"), &rendered.stdout).unwrap();
            "review.json"
        } else {
            "review.html"
        };
        let mut args = vec![
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            kind,
            "--artifact",
            artifact,
            "--approver",
            "alice",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "-o",
            "approval.json",
        ];
        if signed {
            self.write_keys();
            args.extend(["--signing-key", "private.pem"]);
        }
        let created = self.run(&args);
        assert_status(&created, 0);
        let record = value(&created)["record"].clone();
        assert_eq!(record["approval"]["requirements"], json!(["REQ-190"]));
        if kind == "scenarios" {
            assert_eq!(
                record["target"]["digest_algorithm"],
                "fsl-rendered-artifact-v1+sha256"
            );
            assert_eq!(
                record["target"]["digest"],
                "sha256:ef2e880d98567185daa491e894cf626af57ff97279583c8c7992180490ed8f3c",
                "success warning data must retain the pre-W5 canonical digest"
            );
            assert_eq!(
                record["schema"],
                if signed {
                    "fslc.approval.v6"
                } else {
                    "fslc.approval.v5"
                }
            );
        }
        record
    }

    #[allow(clippy::default_trait_access)]
    fn write_keys(&self) {
        // Deterministic fixture material only; no developer key or trust store.
        let key = SigningKey::from_bytes(&[7; 32]);
        std::fs::write(
            self.0.join("private.pem"),
            key.to_pkcs8_pem(Default::default()).unwrap().as_bytes(),
        )
        .unwrap();
        std::fs::write(
            self.0.join("public.pem"),
            key.verifying_key()
                .to_public_key_pem(Default::default())
                .unwrap(),
        )
        .unwrap();
    }

    fn write_record(&self, record: &Value) {
        std::fs::write(
            self.0.join("approval.json"),
            serde_json::to_vec_pretty(record).unwrap(),
        )
        .unwrap();
    }
}

fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON {error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn assert_status(output: &Output, status: i32) {
    assert_eq!(
        output.status.code(),
        Some(status),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_aborted(output: &Output, outer_root_warnings: bool) -> Value {
    // Original e816 exit RED is produced2/expected1 here. Pre-W5 healthy
    // passes status1 and then exposes the separate four-versus-two warning RED.
    assert_status(output, 1);
    let actual = value(output);
    assert!(
        actual.get("status").is_none(),
        "aborted evaluation: {actual:#}"
    );
    // Adopted W5: the two authored root findings belong to the outer owner.
    // This exact array is the OLDhealthyR2 semantic RED: actual four vs expected two.
    if outer_root_warnings {
        assert_eq!(
            actual.get("warnings"),
            Some(&expected_legacy_warnings("spec.fsl", true)),
            "each distinct authored root finding must occur exactly once"
        );
    } else {
        assert!(
            actual.get("warnings").is_none(),
            "generated ledger HTML overlay stays undecorated"
        );
    }
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    if std::env::var_os("FSLC_1418_EXPECT_CAPTURE").is_some() {
        let records = stderr
            .lines()
            .filter_map(|line| line.strip_prefix(CAPTURE))
            .collect::<Vec<_>>();
        assert_eq!(
            records.len(),
            1,
            "one downstream call per failed invocation: {stderr}"
        );
        let (mut expected, status): (Value, i32) = serde_json::from_str(records[0]).unwrap();
        assert_eq!(status, 1);
        // The capture remains INNER, before faults. Apply only the existing
        // authored outer decoration; retain every inner field/warning in order.
        if outer_root_warnings {
            append_expected_root_warnings(&mut expected, "spec.fsl", true);
        }
        assert_eq!(
            actual, expected,
            "full tuple plus exact outer decoration must survive"
        );
        assert_eq!(stderr.lines().count(), 1, "unexpected non-capture stderr");
    } else {
        assert!(output.stderr.is_empty(), "unexpected stderr: {stderr}");
    }
    actual
}

fn scenario_failure(check: bool) {
    let repo = Repository::new();
    repo.render_and_approve("scenarios", false);
    let record_before = std::fs::read(repo.0.join("approval.json")).unwrap();
    repo.break_invariant();
    let output = if check {
        repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"])
    } else {
        repo.run(&[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "scenarios",
            "--artifact",
            "review.json",
            "--approver",
            "alice",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "-o",
            "failed.json",
        ])
    };
    let actual = assert_aborted(&output, true);
    assert_eq!(actual["result"], "violated");
    assert_eq!(actual["violation_kind"], "invariant");
    assert_eq!(actual["invariant"], "ApprovalPersists");
    // Replayable state/action evidence, not merely a nonempty trace.
    assert_eq!(actual["violated_at_step"], 1);
    let trace = actual["trace"].as_array().expect("counterexample states");
    assert_eq!(
        trace.first().unwrap()["state"],
        json!({"approved":false,"ever_approved":false})
    );
    assert_eq!(
        trace.last().unwrap()["state"],
        json!({"approved":false,"ever_approved":true})
    );
    assert_eq!(trace.last().unwrap()["action"]["name"], "approve");
    assert_eq!(trace.last().unwrap()["action"]["params"], json!({}));
    assert!(!repo.0.join("failed.json").exists());
    assert_eq!(
        std::fs::read(repo.0.join("approval.json")).unwrap(),
        record_before
    );
}

fn html_failure(check: bool) {
    let repo = Repository::new();
    repo.render_and_approve("html", false);
    let record_before = std::fs::read(repo.0.join("approval.json")).unwrap();
    let reviewed_before = std::fs::read(repo.0.join("review.html")).unwrap();
    repo.break_invariant();
    let output = if check {
        repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"])
    } else {
        repo.run(&[
            "approval",
            "create",
            "spec.fsl",
            "--kind",
            "html",
            "--artifact",
            "review.html",
            "--approver",
            "alice",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "-o",
            "failed.json",
        ])
    };
    let actual = assert_aborted(&output, true);
    assert_eq!(actual["result"], "generated");
    assert_eq!(actual["kind"], "html_report");
    assert_eq!(actual["spec"], "ApprovalReview");
    assert_eq!(actual["output"], "spec.html");
    let html = actual["content"].as_str().expect("in-memory HTML content");
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("ApprovalPersists"));
    assert!(
        !repo.0.join("spec.html").exists(),
        "default output is metadata only"
    );
    assert!(!repo.0.join("failed.json").exists());
    assert_eq!(
        std::fs::read(repo.0.join("approval.json")).unwrap(),
        record_before
    );
    assert_eq!(
        std::fs::read(repo.0.join("review.html")).unwrap(),
        reviewed_before
    );
}

#[test]
fn scenarios_create_propagates_violation_without_writing_a_record() {
    scenario_failure(false);
}
#[test]
fn scenarios_check_propagates_violation_without_replacing_the_record() {
    scenario_failure(true);
}
#[test]
fn html_create_propagates_in_memory_report_without_writing_html_or_record() {
    html_failure(false);
}
#[test]
fn html_check_propagates_in_memory_report_without_writing_html_or_record() {
    html_failure(true);
}

#[test]
fn direct_html_writes_a_real_failed_report_and_write_error_outranks_verdict() {
    let repo = Repository::new();
    repo.break_invariant();
    let output = repo.run(&[
        "html",
        "spec.fsl",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "-o",
        "direct.html",
    ]);
    assert_status(&output, 1);
    assert!(output.stderr.is_empty());
    assert_eq!(
        value(&output),
        json!({"fsl":"1.0","result":"generated","kind":"html_report",
        "spec":"ApprovalReview","output":"direct.html"})
    );
    let html = std::fs::read_to_string(repo.0.join("direct.html")).unwrap();
    assert!(html.contains("<!doctype html>"));
    assert!(html.contains("ApprovalPersists"));
    std::fs::create_dir(repo.0.join("unwritable.html")).unwrap();
    let output = repo.run(&[
        "html",
        "spec.fsl",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "-o",
        "unwritable.html",
    ]);
    assert_status(&output, 2);
    assert!(output.stderr.is_empty());
    // Publication refuses a non-regular target itself, on every platform.
    assert_eq!(
        value(&output),
        json!({"fsl":"1.0","result":"error","kind":"io","message":"output target is not a regular file","warnings": expected_legacy_warnings("spec.fsl", true)})
    );
    assert!(repo.0.join("unwritable.html").is_dir());
}

fn expected_check(record: &Value, signed: bool) -> Value {
    json!({"fsl":"1.0","result":"approval_check","status":"approved","reasons":[],
        "record":"approval.json","target_kind":record["target"]["kind"],
        "approver":record["approval"]["approver"],"approved_at":record["approval"]["approved_at"],
        "requirements":record["approval"]["requirements"],"baseline_digest":record["spec"]["digest"],
        "current_digest":record["spec"]["digest"],"artifact_digest":record["target"]["digest"],
        "current_artifact_digest":record["target"]["digest"],"diff_base":record["spec"]["git_commit"],
        "semantic_diff_command":"fslc approval diff spec.fsl --record approval.json",
        "signature_status":if signed {"signed"} else {"unsigned"},
        "key_id":if signed {record["signature"]["key_id"].clone()} else {Value::Null},"warnings": expected_legacy_warnings("spec.fsl", false)})
}

#[test]
fn completed_approved_and_drifted_checks_keep_the_complete_zero_exit_envelope() {
    let repo = Repository::new();
    let record = repo.render_and_approve("scenarios", false);
    let approved = repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"]);
    assert_status(&approved, 0);
    assert!(approved.stderr.is_empty());
    assert_eq!(value(&approved), expected_check(&record, false));
    let mut changed = record.clone();
    changed["target"]["generator_version"] = json!("deliberate-old-renderer");
    repo.write_record(&changed);
    let drifted = repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"]);
    assert_status(&drifted, 0);
    assert!(drifted.stderr.is_empty());
    let mut expected = expected_check(&record, false);
    expected["status"] = json!("drifted");
    expected["reasons"] = json!(["renderer_changed"]);
    assert_eq!(value(&drifted), expected);
}

#[test]
fn signature_invalid_and_missing_trust_preserve_distinct_complete_envelopes() {
    let repo = Repository::new();
    let record = repo.render_and_approve("scenarios", true);
    let checked = repo.run(&[
        "approval",
        "check",
        "spec.fsl",
        "--record",
        "approval.json",
        "--trust-key",
        "public.pem",
    ]);
    assert_status(&checked, 0);
    assert_eq!(value(&checked), expected_check(&record, true));
    let missing = repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"]);
    assert_status(&missing, 2);
    assert_eq!(
        value(&missing),
        json!({"fsl":"1.0","result":"error","kind":"io",
        "message":format!("no trusted Ed25519 public key matches '{}'", record["signature"]["key_id"].as_str().unwrap()),"warnings": expected_legacy_warnings("spec.fsl", false)})
    );
    let mut tampered = record.clone();
    tampered["approval"]["approver"] = json!("mallory");
    repo.write_record(&tampered);
    let invalid = repo.run(&[
        "approval",
        "check",
        "spec.fsl",
        "--record",
        "approval.json",
        "--trust-key",
        "public.pem",
    ]);
    assert_status(&invalid, 1);
    assert!(invalid.stderr.is_empty());
    assert_eq!(
        value(&invalid),
        json!({"fsl":"1.0","result":"approval_check","status":"signature-invalid",
        "signature_status":"signature-invalid","key_id":record["signature"]["key_id"],"reasons":["signature_invalid"],
        "record":"approval.json","target_kind":"scenarios","approver":"mallory",
        "approved_at":record["approval"]["approved_at"],"requirements":record["approval"]["requirements"],
        "baseline_digest":record["spec"]["digest"],"warnings": expected_legacy_warnings("spec.fsl", false)})
    );
}

#[test]
fn pre_evaluation_errors_exit_two_without_an_approval_status() {
    let repo = Repository::new();
    let record = repo.render_and_approve("scenarios", false);
    let mut malformed = record.clone();
    malformed["spec"]["digest"] = json!("sha256:not-a-digest");
    repo.write_record(&malformed);
    let output = repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"]);
    assert_status(&output, 2);
    assert_eq!(
        value(&output),
        json!({"fsl":"1.0","result":"error","kind":"io",
        "message":"approval record contains an invalid SHA-256 digest","warnings": expected_legacy_warnings("spec.fsl", false)})
    );
    repo.write_record(&record);
    std::fs::copy(repo.0.join("spec.fsl"), repo.0.join("other.fsl")).unwrap();
    repo.git(&["add", "other.fsl"]);
    repo.commit("track the other valid target");
    let output = repo.run(&[
        "approval",
        "check",
        "other.fsl",
        "--record",
        "approval.json",
    ]);
    assert_status(&output, 2);
    assert_eq!(
        value(&output),
        json!({"fsl":"1.0","result":"error","kind":"semantics",
        "message":"approval record targets 'spec.fsl' but current spec is 'other.fsl'","warnings": expected_legacy_warnings("other.fsl", false)})
    );
    let mut missing = record;
    let commit = "0".repeat(40);
    missing["spec"]["git_commit"] = json!(commit);
    repo.write_record(&missing);
    let failed_git = Command::new("git")
        .args(["cat-file", "-e", &format!("{commit}^{{commit}}")])
        .current_dir(&repo.0)
        .output()
        .unwrap();
    assert!(!failed_git.status.success());
    let message = String::from_utf8(failed_git.stderr)
        .unwrap()
        .trim()
        .to_owned();
    let output = repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"]);
    assert_status(&output, 2);
    assert_eq!(
        value(&output),
        json!({"fsl":"1.0","result":"error","kind":"io","message":message,"warnings": expected_legacy_warnings("spec.fsl", false)})
    );
}

#[test]
fn captured_counterexample_preserves_verification_tuple_and_writes_valid_artifact() {
    let repo = Repository::new();
    repo.break_invariant();
    let output = repo.run(&[
        "counterexample",
        "export",
        "spec.fsl",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "-o",
        "reproducer.json",
    ]);
    assert_status(&output, 1);
    let actual = value(&output);
    assert_eq!(actual["result"], "violated");
    assert_eq!(
        actual["warnings"],
        expected_legacy_warnings("spec.fsl", true),
        "counterexample export already has correct once-only ownership"
    );
    assert_eq!(
        actual["reproducer"],
        json!({"schema":fsl_core::REPRODUCER_V1_SCHEMA_ID,
        "exported_to":"reproducer.json"})
    );
    if std::env::var_os("FSLC_1418_EXPECT_CAPTURE").is_some() {
        let stderr = std::str::from_utf8(&output.stderr).unwrap();
        let lines = stderr.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "{stderr}");
        let (mut expected, status): (Value, i32) = serde_json::from_str(
            lines[0]
                .strip_prefix("FSLC_1418_CEX ")
                .expect("live captured verification tuple"),
        )
        .unwrap();
        assert_eq!(status, 1);
        assert!(expected.get("reproducer").is_none());
        expected["reproducer"] = json!({"schema":fsl_core::REPRODUCER_V1_SCHEMA_ID,
            "exported_to":"reproducer.json"});
        // Export verification is already open; its root owner adds two once.
        append_expected_root_warnings(&mut expected, "spec.fsl", true);
        assert_eq!(
            actual, expected,
            "only established reproducer and authored outer root decoration are allowed"
        );
    } else {
        assert!(output.stderr.is_empty());
    }
    let bytes = std::fs::read(repo.0.join("reproducer.json")).expect("actual exported bytes");
    let artifact: Value = serde_json::from_slice(&bytes).expect("actual exported JSON");
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/fslc/kernel/reproducer.v1.schema.json");
    let schema: Value = serde_json::from_slice(&std::fs::read(schema_path).unwrap()).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&artifact)
        .expect("reproducer schema");
    assert_eq!(
        artifact["schema_version"],
        fsl_core::REPRODUCER_V1_SCHEMA_VERSION
    );
    assert_eq!(artifact["result"], "reproducer");
    assert_eq!(
        artifact["canonical_steps"],
        json!([{"action":"approve","params":{}}])
    );
    assert_eq!(artifact["trace"], actual["trace"]);
}

fn ledger_overlay_refused(kind: &str) {
    let repo = Repository::new();
    repo.render_and_approve(kind, false);
    let record_before = std::fs::read(repo.0.join("approval.json")).unwrap();
    let artifact = if kind == "html" {
        "review.html"
    } else {
        "review.json"
    };
    let reviewed_before = std::fs::read(repo.0.join(artifact)).unwrap();
    repo.break_invariant();
    std::fs::write(repo.0.join("ledger.md"), "previous ledger\n").unwrap();
    let output = repo.run(&[
        "ledger",
        "spec.fsl",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--approval",
        "approval.json",
        "-o",
        "ledger.md",
    ]);
    assert_status(&output, 2);
    let actual = value(&output);
    assert_eq!(actual["result"], "error", "{actual:#}");
    assert_eq!(actual["kind"], "semantics", "{actual:#}");
    assert!(
        actual["message"]
            .as_str()
            .is_some_and(|message| message.contains("'approval.json'")),
        "the refusal names the record: {actual:#}"
    );
    assert_eq!(
        actual.get("warnings"),
        Some(&expected_legacy_warnings("spec.fsl", true)),
        "each distinct authored root finding must occur exactly once"
    );
    assert_eq!(
        std::fs::read(repo.0.join("ledger.md")).unwrap(),
        b"previous ledger\n",
        "a stale ledger must not look newly written"
    );
    assert!(!repo.0.join("spec.html").exists());
    assert_eq!(
        std::fs::read(repo.0.join("approval.json")).unwrap(),
        record_before
    );
    assert_eq!(
        std::fs::read(repo.0.join(artifact)).unwrap(),
        reviewed_before
    );
}

#[test]
fn ledger_approval_overlay_refuses_a_red_scenarios_record_and_keeps_the_ledger() {
    ledger_overlay_refused("scenarios");
}

#[test]
fn ledger_approval_overlay_refuses_a_red_html_record_and_keeps_the_ledger() {
    ledger_overlay_refused("html");
}

/// Negative control: a ledger record's rerender always completes, so the ledger
/// is written and shows the drift, with the verification's own exit.
#[test]
fn ledger_approval_overlay_with_a_red_ledger_record_writes_the_drifted_ledger() {
    let repo = Repository::new();
    let inputs = ["--depth", "2", "--deadlock", "ignore"];
    let direct = repo.run(&[&["ledger", "spec.fsl", "-o", "review.md"], &inputs[..]].concat());
    assert_status(&direct, 0);
    let created = repo.run(
        &[
            &[
                "approval",
                "create",
                "spec.fsl",
                "--kind",
                "ledger",
                "--artifact",
                "review.md",
                "--approver",
                "alice",
                "-o",
                "approval.json",
            ],
            &inputs[..],
        ]
        .concat(),
    );
    assert_status(&created, 0);
    repo.break_invariant();
    std::fs::write(repo.0.join("ledger.md"), "previous ledger\n").unwrap();
    let output = repo.run(
        &[
            &[
                "ledger",
                "spec.fsl",
                "--approval",
                "approval.json",
                "-o",
                "ledger.md",
            ],
            &inputs[..],
        ]
        .concat(),
    );
    assert_status(&output, 1);
    let actual = value(&output);
    assert_eq!(actual["result"], "generated", "{actual:#}");
    assert_eq!(actual["kind"], "audit_ledger", "{actual:#}");
    assert_eq!(actual["output"], "ledger.md", "{actual:#}");
    let ledger = std::fs::read_to_string(repo.0.join("ledger.md")).unwrap();
    assert!(ledger.contains("drifted"), "{ledger}");
}

/// Green stays as it was: the same html record on its green baseline overlays.
#[test]
fn ledger_approval_overlay_with_a_green_html_record_writes_the_ledger() {
    let repo = Repository::new();
    repo.render_and_approve("html", false);
    let output = repo.run(&[
        "ledger",
        "spec.fsl",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--approval",
        "approval.json",
        "-o",
        "ledger.md",
    ]);
    assert_status(&output, 0);
    assert_eq!(value(&output)["kind"], "audit_ledger");
    assert!(repo.0.join("ledger.md").exists());
}

// Fixed e8163bc6 warning literals, calibrated by the four archived BASE probes.
// No output field or migration implementation constructs these expectations.
// Only the authored request filename changes loc.file; the source bytes fix all offsets.
fn expected_legacy_warnings(file: &str, broken_action: bool) -> Value {
    let mut warnings: Value = serde_json::from_str(if broken_action {
        APPROVAL_BROKEN_ACTION_WARNINGS
    } else {
        APPROVAL_BASELINE_WARNINGS
    })
    .expect("fixed complete legacy warning literals");
    for warning in warnings.as_array_mut().expect("fixed warning array") {
        warning["loc"]["file"] = json!(file);
    }
    warnings
}

const APPROVAL_BASELINE_WARNINGS: &str = r#"[
  {
    "code": "legacy_string_metadata",
    "kind": "legacy_string_metadata",
    "taxonomy": "deprecated",
    "severity": "warning",
    "edition": "current",
    "message": "legacy string metadata is deprecated; use a typed annotation",
    "loc": {
      "file": "spec.fsl",
      "line": 12,
      "column": 20,
      "end_line": 12,
      "end_column": 63
    },
    "canonical_replacement": "@requirement(\"REQ-190\", \"a pending review can be approved\")",
    "machine_applicable": true,
    "edits": [
      {
        "start": 228,
        "end": 228,
        "replacement": "@requirement(\"REQ-190\", \"a pending review can be approved\")\n  "
      },
      {
        "start": 245,
        "end": 288,
        "replacement": ""
      }
    ]
  },
  {
    "code": "legacy_string_metadata",
    "kind": "legacy_string_metadata",
    "taxonomy": "deprecated",
    "severity": "warning",
    "edition": "current",
    "message": "legacy string metadata is deprecated; use a typed annotation",
    "loc": {
      "file": "spec.fsl",
      "line": 18,
      "column": 30,
      "end_line": 18,
      "end_column": 76
    },
    "canonical_replacement": "@requirement(\"REQ-190\", \"an approved review remains approved\")",
    "machine_applicable": true,
    "edits": [
      {
        "start": 369,
        "end": 369,
        "replacement": "@requirement(\"REQ-190\", \"an approved review remains approved\")\n  "
      },
      {
        "start": 396,
        "end": 442,
        "replacement": ""
      }
    ]
  }
]"#;

const APPROVAL_BROKEN_ACTION_WARNINGS: &str = r#"[
  {
    "code": "legacy_string_metadata",
    "kind": "legacy_string_metadata",
    "taxonomy": "deprecated",
    "severity": "warning",
    "edition": "current",
    "message": "legacy string metadata is deprecated; use a typed annotation",
    "loc": {
      "file": "spec.fsl",
      "line": 12,
      "column": 20,
      "end_line": 12,
      "end_column": 63
    },
    "canonical_replacement": "@requirement(\"REQ-190\", \"a pending review can be approved\")",
    "machine_applicable": true,
    "edits": [
      {
        "start": 228,
        "end": 228,
        "replacement": "@requirement(\"REQ-190\", \"a pending review can be approved\")\n  "
      },
      {
        "start": 245,
        "end": 288,
        "replacement": ""
      }
    ]
  },
  {
    "code": "legacy_string_metadata",
    "kind": "legacy_string_metadata",
    "taxonomy": "deprecated",
    "severity": "warning",
    "edition": "current",
    "message": "legacy string metadata is deprecated; use a typed annotation",
    "loc": {
      "file": "spec.fsl",
      "line": 18,
      "column": 30,
      "end_line": 18,
      "end_column": 76
    },
    "canonical_replacement": "@requirement(\"REQ-190\", \"an approved review remains approved\")",
    "machine_applicable": true,
    "edits": [
      {
        "start": 370,
        "end": 370,
        "replacement": "@requirement(\"REQ-190\", \"an approved review remains approved\")\n  "
      },
      {
        "start": 397,
        "end": 443,
        "replacement": ""
      }
    ]
  }
]"#;

// Explicit expected outer decoration, never producer-output filtering or dedup.
fn append_expected_root_warnings(expected: &mut Value, file: &str, broken_action: bool) {
    let findings = expected_legacy_warnings(file, broken_action);
    let warnings = expected
        .as_object_mut()
        .expect("captured open envelope")
        .entry("warnings")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("existing captured warnings array");
    warnings.extend(
        findings
            .as_array()
            .expect("literal findings")
            .iter()
            .cloned(),
    );
}

#[test]
fn semantic_and_multiple_root_findings_retain_their_order_and_complete_scenarios() {
    let repo = Repository::new();
    std::fs::write(
        repo.0.join("spec.fsl"),
        include_str!("fixtures/issue_1418_semantic_warning.fsl"),
    )
    .unwrap();
    repo.commit("supported never-enabled action after root metadata");
    let output = repo.run(&[
        "scenarios",
        "spec.fsl",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
    ]);
    assert_status(&output, 0);
    assert!(output.stderr.is_empty());
    let mut expected: Value =
        serde_json::from_slice(include_bytes!("fixtures/issue_1418_success_scenarios.json"))
            .unwrap();
    let mut warnings = vec![json!({
        "kind":"never_enabled_action","name":"blocked","loc":{"line":21,"column":3},
        "message":"action 'blocked' is never enabled within depth 2 — the spec may be vacuous (check its requires clauses)",
        "hint":"these requires clauses are unsatisfiable at every step up to depth 2; weaken one of them, add an action that establishes them, or increase --depth",
        "blocking_requires":[]
    })];
    warnings.extend(
        expected_legacy_warnings("spec.fsl", false)
            .as_array()
            .unwrap()
            .iter()
            .cloned(),
    );
    expected["warnings"] = json!(warnings);
    assert_eq!(
        value(&output),
        expected,
        "semantic finding precedes both distinct root findings"
    );
    std::fs::write(repo.0.join("review.json"), &output.stdout).unwrap();
    let created = repo.run(&[
        "approval",
        "create",
        "spec.fsl",
        "--kind",
        "scenarios",
        "--artifact",
        "review.json",
        "--approver",
        "alice",
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "-o",
        "approval.json",
    ]);
    assert_status(&created, 0);
    assert!(created.stderr.is_empty());
    let record = value(&created)["record"].clone();
    assert_eq!(
        record["target"]["digest"],
        "sha256:8bc9ad8fb6b5a4378f4eadd498b772aa5c246a617fb1199ac9354a46010cd888",
        "the semantic finding and both root findings remain canonical data"
    );
    let checked = repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"]);
    assert_status(&checked, 0);
    assert!(checked.stderr.is_empty());
    assert_eq!(value(&checked), expected_check(&record, false));
}

// W6: exact measured root/provider metadata, with no cross-run cost oracle.
fn dependency_run(repo: &Repository, args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(&repo.0)
        .env("FSLC_CACHE", "off")
        .env_remove("FSLC_CACHE_VERIFY")
        .env("FSLC_UNSTABLE", "fsl5")
        .output()
        .unwrap();
    let dir = repo.0.join("observations");
    std::fs::create_dir_all(&dir).unwrap();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    std::fs::write(dir.join(format!("{id}.stdout")), &output.stdout).unwrap();
    std::fs::write(dir.join(format!("{id}.stderr")), &output.stderr).unwrap();
    std::fs::write(
        dir.join(format!("{id}.argv.json")),
        serde_json::to_vec(args).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join(format!("{id}.status")),
        format!("{:?}\n", output.status.code()),
    )
    .unwrap();
    output
}

fn dependency_outer_findings() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/issue_1418_dependency_outer_warnings.json"
    ))
    .expect("four measured complete root/distinct-provider findings")
}

fn append_dependency_outer_findings(expected: &mut Value) {
    let findings = dependency_outer_findings();
    expected
        .as_object_mut()
        .expect("open inner packet")
        .entry("warnings")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("inner findings stay ordered")
        .extend(findings.as_array().unwrap().iter().cloned());
}

fn dependency_capture(output: &Output, expected_status: i32) -> Value {
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let lines = stderr.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        1,
        "one INNER packet, no unrelated stderr: {stderr}"
    );
    let (inner, status): (Value, i32) = serde_json::from_str(
        lines[0]
            .strip_prefix(CAPTURE)
            .expect("exact same-invocation INNER marker"),
    )
    .unwrap();
    assert_eq!(status, expected_status);
    inner
}

fn dependency_repository() -> Repository {
    let repo = Repository::new();
    std::fs::create_dir_all(repo.0.join("dir")).unwrap();
    std::fs::write(
        repo.0.join("root.fsl"),
        include_bytes!("fixtures/dependency-root.fsl"),
    )
    .unwrap();
    for path in ["dep.fsl", "dir/dep.fsl"] {
        std::fs::write(
            repo.0.join(path),
            include_bytes!("fixtures/dependency-leaf-valid.fsl"),
        )
        .unwrap();
    }
    repo.git(&["add", "root.fsl", "dep.fsl", "dir/dep.fsl"]);
    repo.commit("measured distinct dependency providers");
    let rendered = dependency_run(
        &repo,
        &[
            "scenarios",
            "root.fsl",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
        ],
    );
    assert_status(&rendered, 0);
    assert!(rendered.stderr.is_empty());
    assert_eq!(
        rendered.stdout.as_slice(),
        include_bytes!("fixtures/issue_1418_dependency_success_scenarios.json")
    );
    std::fs::write(repo.0.join("review.json"), &rendered.stdout).unwrap();
    let created = dependency_run(
        &repo,
        &[
            "approval",
            "create",
            "root.fsl",
            "--kind",
            "scenarios",
            "--artifact",
            "review.json",
            "--approver",
            "dependency-test",
            "--depth",
            "2",
            "--deadlock",
            "ignore",
            "-o",
            "approval.json",
        ],
    );
    assert_status(&created, 0);
    if std::env::var_os("FSLC_1418_EXPECT_CAPTURE").is_some() {
        assert_eq!(
            dependency_capture(&created, 0),
            serde_json::from_slice::<Value>(include_bytes!(
                "fixtures/issue_1418_dependency_success_scenarios.json"
            ))
            .unwrap()
        );
    } else {
        assert!(created.stderr.is_empty());
    }
    let record = value(&created)["record"].clone();
    assert_eq!(
        record["target"]["digest_algorithm"],
        "fsl-rendered-artifact-v1+sha256"
    );
    assert_eq!(
        record["target"]["digest"],
        "sha256:b98d4bf077f5bdfa34faa8bbf8d3c2e8facf5dd72a1aaf0a7701cbbdc320e5c8"
    );
    assert_eq!(
        record["spec"]["digest"],
        "sha256:95af98ab05b55d8eb74816ebf9dd0bb43b8d82098805d8a316c28ea7cc5c81f7"
    );
    assert_eq!(
        record["approval"]["requirements"],
        json!(["REQ-DEP-001", "REQ-ROOT-001", "REQ-ROOT-002"])
    );
    repo
}

#[allow(clippy::too_many_lines)] // Keep the complete dependency failure protocol together.
fn dependency_failure(operation: &str) {
    let repo = dependency_repository();
    let record_before = std::fs::read(repo.0.join("approval.json")).unwrap();
    let artifact_before = std::fs::read(repo.0.join("review.json")).unwrap();
    for path in ["dep.fsl", "dir/dep.fsl"] {
        std::fs::write(
            repo.0.join(path),
            include_bytes!("fixtures/dependency-leaf-broken.fsl"),
        )
        .unwrap();
    }
    repo.git(&["add", "dep.fsl", "dir/dep.fsl"]);
    repo.commit("same measured invariant break in both provider resources");
    let output = match operation {
        "create" => dependency_run(
            &repo,
            &[
                "approval",
                "create",
                "root.fsl",
                "--kind",
                "scenarios",
                "--artifact",
                "review.json",
                "--approver",
                "dependency-test",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
                "-o",
                "failed.json",
            ],
        ),
        "check" => dependency_run(
            &repo,
            &["approval", "check", "root.fsl", "--record", "approval.json"],
        ),
        "ledger" => dependency_run(
            &repo,
            &[
                "ledger",
                "root.fsl",
                "--depth",
                "2",
                "--deadlock",
                "ignore",
                "--approval",
                "approval.json",
                "-o",
                "failed-ledger.md",
            ],
        ),
        _ => panic!("finite dependency consumer roster"),
    };
    if operation == "ledger" {
        assert_status(&output, 2);
        let actual = value(&output);
        assert_eq!(actual["result"], "error", "{actual:#}");
        assert_eq!(
            actual["warnings"],
            dependency_outer_findings(),
            "root001/root002/dep.fsl/dir/dep.fsl each occur once, in delivered order"
        );
        assert!(!repo.0.join("failed-ledger.md").exists());
        return;
    }
    assert_status(&output, 1);
    let actual = value(&output);
    assert!(actual.get("status").is_none());
    assert_eq!(
        actual["warnings"],
        dependency_outer_findings(),
        "root001/root002/dep.fsl/dir/dep.fsl each occur once, in delivered order"
    );
    if std::env::var_os("FSLC_1418_EXPECT_CAPTURE").is_some() {
        let mut expected = dependency_capture(&output, 1);
        append_dependency_outer_findings(&mut expected);
        assert_eq!(
            actual, expected,
            "whole INNER plus literal outer metadata, including every live cost field"
        );
    } else {
        assert!(output.stderr.is_empty());
    }
    // The actual failed renderer emits no semantic warning; do not invent one.
    assert_eq!(actual["result"], "violated");
    assert_eq!(actual["spec"], "C");
    assert_eq!(actual["violation_kind"], "invariant");
    assert_eq!(actual["invariant"], "a.Typed");
    assert_eq!(
        actual["requirement"],
        json!({"id":"REQ-DEP-001","text":"日本語😀"})
    );
    assert_eq!(actual["loc"], json!({"line":2,"column":71}));
    assert_eq!(actual["violated_at_step"], 1);
    assert_eq!(
        actual["last_action"],
        json!({"name":"a.finish","params":{},"loc":{"line":2,"column":39}})
    );
    assert_eq!(
        actual["trace"],
        serde_json::from_str::<Value>(include_str!(
            "fixtures/issue_1418_dependency_failed_trace.json"
        ))
        .unwrap()
    );
    assert_eq!(
        std::fs::read(repo.0.join("approval.json")).unwrap(),
        record_before
    );
    assert_eq!(
        std::fs::read(repo.0.join("review.json")).unwrap(),
        artifact_before
    );
    assert!(!repo.0.join("failed.json").exists());
    assert!(!repo.0.join("failed-ledger.md").exists());
}

#[test]
fn dependency_scenarios_create_propagates_each_root_and_provider_once() {
    dependency_failure("create");
}

#[test]
fn dependency_scenarios_check_propagates_each_root_and_provider_once() {
    dependency_failure("check");
}

#[test]
fn dependency_ledger_overlay_refuses_with_each_root_and_provider_once() {
    dependency_failure("ledger");
}

fn install_original_baseline_objects(repo: &Repository) {
    // Original compressed objects, not fabricated commits or rewritten records.
    for (oid, bytes) in [
        (
            "ee30d3c83e07541a93c7f68aa0e7cb817063027a",
            &include_bytes!(
                "fixtures/prior-git-objects/ee30d3c83e07541a93c7f68aa0e7cb817063027a.zlib"
            )[..],
        ),
        (
            "eaedd7c7d80d9d4915c12c0a619ff88e271f513a",
            &include_bytes!(
                "fixtures/prior-git-objects/eaedd7c7d80d9d4915c12c0a619ff88e271f513a.zlib"
            )[..],
        ),
        (
            "1035c597e293b3bab37dc89d07ab79f0e3a9d07a",
            &include_bytes!(
                "fixtures/prior-git-objects/1035c597e293b3bab37dc89d07ab79f0e3a9d07a.zlib"
            )[..],
        ),
        (
            "55a2b49ca451e724fb3ad152712671409c88a43d",
            &include_bytes!(
                "fixtures/prior-git-objects/55a2b49ca451e724fb3ad152712671409c88a43d.zlib"
            )[..],
        ),
    ] {
        let dir = repo.0.join(".git/objects").join(&oid[..2]);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(&oid[2..]);
        if path.exists() {
            assert_eq!(std::fs::read(&path).unwrap().as_slice(), bytes);
        } else {
            std::fs::write(path, bytes).unwrap();
        }
    }
}

#[test]
fn original_unsigned_and_signed_records_keep_complete_approved_packets_and_raw_bytes() {
    for (signed, bytes) in [
        (
            false,
            &include_bytes!("fixtures/prior-unsigned-approval.json")[..],
        ),
        (
            true,
            &include_bytes!("fixtures/prior-signed-approval.json")[..],
        ),
    ] {
        let repo = Repository::new();
        install_original_baseline_objects(&repo);
        std::fs::write(repo.0.join("approval.json"), bytes).unwrap();
        std::fs::write(
            repo.0.join("review.json"),
            include_bytes!("fixtures/issue_1418_success_scenarios.json"),
        )
        .unwrap();
        std::fs::write(
            repo.0.join("public.pem"),
            include_bytes!("fixtures/prior-public.pem"),
        )
        .unwrap();
        let record: Value = serde_json::from_slice(bytes).unwrap();
        let args = if signed {
            vec![
                "approval",
                "check",
                "spec.fsl",
                "--record",
                "approval.json",
                "--trust-key",
                "public.pem",
            ]
        } else {
            vec!["approval", "check", "spec.fsl", "--record", "approval.json"]
        };
        let checked = repo.run(&args);
        assert_status(&checked, 0);
        assert!(checked.stderr.is_empty());
        assert_eq!(value(&checked), expected_check(&record, signed));
        assert_eq!(
            record["target"]["digest"],
            "sha256:ef2e880d98567185daa491e894cf626af57ff97279583c8c7992180490ed8f3c"
        );
        assert_eq!(
            std::fs::read(repo.0.join("approval.json"))
                .unwrap()
                .as_slice(),
            bytes
        );
        assert_eq!(
            std::fs::read(repo.0.join("review.json"))
                .unwrap()
                .as_slice(),
            include_bytes!("fixtures/issue_1418_success_scenarios.json")
        );
        assert_eq!(
            std::fs::read(repo.0.join("public.pem")).unwrap().as_slice(),
            include_bytes!("fixtures/prior-public.pem")
        );
    }
}

// Valid syntax whose `check` fails: the model still loads and renders.
const CHECK_FAILING_SPEC: &str = "spec V {
  state { a: Bool }
  init { a = false
    a = true }
  action go() \"REQ-1: go\" { a = true }
  invariant I \"REQ-1: inv\" { a or not a }
}
";

const CTI_SPEC: &str = "spec Cti {
  state { a: Bool, b: Bool }
  init { a = false
    b = false }
  action step() \"REQ-1: step\" {
    a = b
    b = false
  }
  invariant Imp \"REQ-1: a implies b\" { a => b }
}
";

const REACHABLE_FAILED_SPEC: &str = "spec Reach {
  state { a: Bool }
  init { a = false }
  action go() \"REQ-1: go\" { a = true }
  reachable Never \"REQ-2: unreachable goal\" { a and not a }
  invariant I \"REQ-1: inv\" { a or not a }
}
";

/// Approve a ledger whose verification is red and return `approval check`'s
/// output. The record binds digests, so `approved` is not a verification pass.
fn approve_red_ledger(repo: &Repository, engine: &str, verdict: &str) -> Output {
    let inputs = ["--depth", "4", "--deadlock", "ignore", "--engine", engine];
    let direct = repo.run(&[&["ledger", "spec.fsl", "-o", "review.md"], &inputs[..]].concat());
    assert_status(&direct, 1);
    let created = repo.run(
        &[
            &[
                "approval",
                "create",
                "spec.fsl",
                "--kind",
                "ledger",
                "--artifact",
                "review.md",
                "--approver",
                "alice",
                "-o",
                "approval.json",
            ],
            &inputs[..],
        ]
        .concat(),
    );
    assert_status(&created, 0);
    let verified = repo.run(&[&["verify", "spec.fsl"], &inputs[..]].concat());
    assert_status(&verified, 1);
    assert_eq!(value(&verified)["result"], verdict);
    repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"])
}

fn assert_approved_without_a_verdict(checked: &Output) {
    assert_status(checked, 0);
    let actual = value(checked);
    assert_eq!(actual["result"], "approval_check", "{actual:#}");
    assert_eq!(actual["status"], "approved", "{actual:#}");
    for verdict_field in ["verification", "verdict", "invariants_checked"] {
        assert!(actual.get(verdict_field).is_none(), "{actual:#}");
    }
}

#[test]
fn ledger_approval_of_a_violated_baseline_confirms_digests_not_a_pass() {
    let repo = Repository::new();
    repo.break_invariant();
    assert_approved_without_a_verdict(&approve_red_ledger(&repo, "bmc", "violated"));
}

#[test]
fn ledger_approval_of_a_cti_baseline_confirms_digests_not_a_pass() {
    let repo = Repository::new();
    repo.replace_spec(CTI_SPEC);
    assert_approved_without_a_verdict(&approve_red_ledger(&repo, "induction", "unknown_cti"));
}

#[test]
fn ledger_approval_of_a_failed_reachable_baseline_confirms_digests_not_a_pass() {
    let repo = Repository::new();
    repo.replace_spec(REACHABLE_FAILED_SPEC);
    assert_approved_without_a_verdict(&approve_red_ledger(&repo, "bmc", "reachable_failed"));
}

/// The real error, not `drifted` at exit 0: the spec must fail `check` only
/// after the record exists, so its model still loads and could still render.
#[test]
fn ledger_approval_check_reports_the_verification_error_instead_of_drift() {
    let repo = Repository::new();
    let inputs = ["--depth", "2", "--deadlock", "ignore"];
    let direct = repo.run(&[&["ledger", "spec.fsl", "-o", "review.md"], &inputs[..]].concat());
    assert_status(&direct, 0);
    let created = repo.run(
        &[
            &[
                "approval",
                "create",
                "spec.fsl",
                "--kind",
                "ledger",
                "--artifact",
                "review.md",
                "--approver",
                "alice",
                "-o",
                "approval.json",
            ],
            &inputs[..],
        ]
        .concat(),
    );
    assert_status(&created, 0);
    let record = std::fs::read(repo.0.join("approval.json")).unwrap();
    repo.replace_spec(CHECK_FAILING_SPEC);
    let expected = repo.run(&[&["ledger", "spec.fsl"], &inputs[..]].concat());
    assert_status(&expected, 2);
    assert_eq!(value(&expected)["result"], "error");

    let checked = repo.run(&["approval", "check", "spec.fsl", "--record", "approval.json"]);
    assert_status(&checked, 2);
    assert_eq!(value(&checked), value(&expected));
    assert!(value(&checked).get("status").is_none());
    assert_eq!(
        std::fs::read(repo.0.join("approval.json")).unwrap(),
        record,
        "a failed evaluation must not replace the record"
    );
}

#[test]
fn ledger_approval_create_reports_the_verification_error_instead_of_a_mismatch() {
    let repo = Repository::new();
    repo.replace_spec(CHECK_FAILING_SPEC);
    std::fs::write(repo.0.join("wrong.md"), "not the rendered ledger\n").unwrap();
    let inputs = ["--depth", "2", "--deadlock", "ignore"];
    let expected = repo.run(&[&["ledger", "spec.fsl"], &inputs[..]].concat());
    assert_status(&expected, 2);
    let created = repo.run(
        &[
            &[
                "approval",
                "create",
                "spec.fsl",
                "--kind",
                "ledger",
                "--artifact",
                "wrong.md",
                "--approver",
                "alice",
                "-o",
                "approval.json",
            ],
            &inputs[..],
        ]
        .concat(),
    );
    assert_status(&created, 2);
    assert_eq!(value(&created), value(&expected));
    assert!(!repo.0.join("approval.json").exists());
}

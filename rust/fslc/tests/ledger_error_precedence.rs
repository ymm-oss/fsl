// SPDX-License-Identifier: Apache-2.0

#[path = "support/report_cli.rs"]
mod cli;
#[path = "support/ledger_fixtures.rs"]
mod support;

const MISSING: &str = "rust/fslc/tests/fixtures/governance_missing_dependency.fsl";

fn assert_precedence(option: &str, to_file: bool) {
    let (mut expected, expected_status) =
        cli::run_json("verify", MISSING, 3, &["--deadlock", "ignore"]);
    assert_eq!(expected["result"], "error", "{expected}");
    assert_eq!(expected["kind"], "type", "{expected}");
    assert_eq!(expected_status, 2);
    let root = support::directory("error-precedence");
    let absent = root.join("absent.json");
    let artifact = root.join("review.md");
    assert!(!absent.exists());
    assert!(!artifact.exists(), "output must start absent");
    let mut extra = vec!["--deadlock", "ignore", option, absent.to_str().unwrap()];
    if to_file {
        extra.extend(["-o", artifact.to_str().unwrap()]);
    }
    let (actual, status) = cli::run_json("ledger", MISSING, 3, &extra);
    assert!(!artifact.exists(), "verification error wrote an artifact");
    assert_eq!(status, expected_status, "{actual}");
    // Observed CLI-only asymmetry: verify dispatch adds versions; ledger does not.
    // Requiring its one-sided presence prevents a dead exclusion on both outputs.
    assert!(actual.get("versions").is_none(), "{actual}");
    let versions = expected
        .as_object_mut()
        .unwrap()
        .remove("versions")
        .expect("live verify-only versions");
    assert!(versions.is_object(), "{versions}");
    assert_eq!(
        actual, expected,
        "complete verification error must precede ancillary I/O error"
    );
}

#[test]
fn verification_error_precedes_missing_impl_log() {
    assert_precedence("--impl-log", false);
}
#[test]
fn verification_error_precedes_missing_evidence() {
    assert_precedence("--evidence", false);
}
#[test]
fn missing_impl_log_with_verification_error_never_writes_output() {
    assert_precedence("--impl-log", true);
}
#[test]
fn missing_evidence_with_verification_error_never_writes_output() {
    assert_precedence("--evidence", true);
}

#[test]
fn successful_verification_still_reports_missing_impl_log_as_io() {
    let fixture = "examples/gallery/valid/tiny_bounded_counter.fsl";
    let (verified, status) = cli::run_json("verify", fixture, 3, &["--deadlock", "ignore"]);
    assert_eq!(status, 0, "{verified}");
    assert_eq!(verified["result"], "verified", "{verified}");
    let absent = support::directory("io-control").join("absent.json");
    assert!(!absent.exists());
    let (actual, status) = cli::run_json(
        "ledger",
        fixture,
        3,
        &[
            "--deadlock",
            "ignore",
            "--impl-log",
            absent.to_str().unwrap(),
        ],
    );
    assert_eq!(status, 2, "{actual}");
    assert_eq!(actual["result"], "error", "{actual}");
    assert_eq!(actual["kind"], "io", "{actual}");
    assert_eq!(
        actual["message"], "No such file or directory (os error 2)",
        "{actual}"
    );
}

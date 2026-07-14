// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .output()
        .expect("run native CLI");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("native exit status"))
}

fn path_text(path: &Path) -> String {
    path.to_str().expect("UTF-8 fixture path").to_owned()
}

#[test]
fn legacy_union_warns_with_stable_code_span_and_canonical_fix() {
    let legacy = fixture("domain_legacy_enum_union.fsl");
    for command in ["check", "verify"] {
        let mut args = vec![command.to_owned(), path_text(&legacy)];
        if command == "verify" {
            args.extend([
                "--depth".to_owned(),
                "1".to_owned(),
                "--no-cache".to_owned(),
            ]);
        }
        let borrowed = args.iter().map(String::as_str).collect::<Vec<_>>();
        let (output, status) = run(&borrowed);
        assert_eq!(status, 0, "{command}: {output}");
        let warning = output["warnings"]
            .as_array()
            .and_then(|warnings| {
                warnings
                    .iter()
                    .find(|warning| warning["code"] == "deprecated_domain_enum_union")
            })
            .unwrap_or_else(|| panic!("missing deprecation warning from {command}: {output}"));
        assert_eq!(warning["severity"], "warning");
        assert_eq!(warning["loc"]["line"], 3);
        assert_eq!(warning["loc"]["column"], 3);
        assert_eq!(
            warning["canonical_replacement"],
            "enum Status { Pending, Approved }"
        );
        assert_eq!(
            warning["suggestion"]["replacement"],
            warning["canonical_replacement"]
        );
        if command == "check" {
            let mut migrated = std::fs::read_to_string(&legacy).expect("read legacy fixture");
            let start = usize::try_from(
                warning["suggestion"]["span"]["start"]
                    .as_u64()
                    .expect("byte start"),
            )
            .expect("start fits usize");
            let end = usize::try_from(
                warning["suggestion"]["span"]["end"]
                    .as_u64()
                    .expect("byte end"),
            )
            .expect("end fits usize");
            migrated.replace_range(
                start..end,
                warning["suggestion"]["replacement"]
                    .as_str()
                    .expect("replacement text"),
            );
            assert!(migrated.contains("// This comment and the declaration span must survive"));
            assert!(migrated.contains("enum Status { Pending, Approved }"));
            assert!(!migrated.contains("type Status = Pending | Approved"));
        }
    }

    let legacy_text = path_text(&legacy);
    let (output, status) = run(&["domain", "check", &legacy_text, "--depth", "1"]);
    assert_eq!(status, 0, "{output}");
    assert!(output["warnings"].as_array().is_some_and(|warnings| {
        warnings
            .iter()
            .any(|warning| warning["code"] == "deprecated_domain_enum_union")
    }));
}

#[test]
fn canonical_and_legacy_sources_have_the_same_verdict_without_canonical_warning() {
    let canonical = path_text(&fixture("domain_canonical_enum.fsl"));
    let legacy = path_text(&fixture("domain_legacy_enum_union.fsl"));
    let (canonical_output, canonical_status) = run(&[
        "verify",
        &canonical,
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    let (legacy_output, legacy_status) = run(&[
        "verify",
        &legacy,
        "--depth",
        "2",
        "--deadlock",
        "ignore",
        "--no-cache",
    ]);
    assert_eq!(canonical_status, legacy_status);
    assert_eq!(canonical_output["result"], legacy_output["result"]);
    assert_eq!(canonical_output["spec"], legacy_output["spec"]);
    assert!(
        !canonical_output["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings
                .iter()
                .any(|warning| warning["code"] == "deprecated_domain_enum_union"))
    );
}

#[test]
fn next_edition_rejects_legacy_union_and_accepts_canonical_enum() {
    let canonical = path_text(&fixture("domain_canonical_enum.fsl"));
    let legacy = path_text(&fixture("domain_legacy_enum_union.fsl"));

    for prefix in [vec!["check"], vec!["domain", "check"]] {
        let mut canonical_args = prefix.clone();
        canonical_args.extend([canonical.as_str(), "--edition", "next"]);
        let (accepted, accepted_status) = run(&canonical_args);
        assert_eq!(accepted_status, 0, "{accepted}");
        assert_eq!(accepted["edition"], "next");

        let mut legacy_args = prefix;
        legacy_args.extend([legacy.as_str(), "--edition", "next"]);
        let (rejected, rejected_status) = run(&legacy_args);
        assert_eq!(rejected_status, 2, "{rejected}");
        assert_eq!(rejected["result"], "error");
        assert_eq!(rejected["kind"], "deprecated_domain_enum_union");
        assert_eq!(rejected["findings"][0]["severity"], "error");
    }

    let (rejected, status) = run(&[
        "verify",
        &legacy,
        "--edition",
        "next",
        "--depth",
        "1",
        "--no-cache",
    ]);
    assert_eq!(status, 2, "{rejected}");
    assert_eq!(rejected["kind"], "deprecated_domain_enum_union");

    let invalid = path_text(&fixture(
        "domain_characterization/invalid_unknown_member.fsl",
    ));
    let (current_error, current_status) = run(&["check", &invalid]);
    assert_eq!(current_status, 2, "{current_error}");
    assert!(
        current_error["warnings"]
            .as_array()
            .is_some_and(|warnings| {
                warnings
                    .iter()
                    .any(|warning| warning["code"] == "deprecated_domain_enum_union")
            })
    );

    let (next_error, next_status) = run(&["check", &invalid, "--edition", "next"]);
    assert_eq!(next_status, 2, "{next_error}");
    assert_eq!(next_error["kind"], "deprecated_domain_enum_union");

    let (domain_current, domain_current_status) =
        run(&["domain", "check", &invalid, "--depth", "1"]);
    assert_eq!(domain_current_status, 2, "{domain_current}");
    assert!(
        domain_current["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings
                .iter()
                .any(|warning| warning["code"] == "deprecated_domain_enum_union"))
    );

    let (domain_next, domain_next_status) = run(&[
        "domain",
        "check",
        &invalid,
        "--depth",
        "1",
        "--edition",
        "next",
    ]);
    assert_eq!(domain_next_status, 2, "{domain_next}");
    assert_eq!(domain_next["kind"], "deprecated_domain_enum_union");
}

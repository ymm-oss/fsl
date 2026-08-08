// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Negative controls for #726: `fslc domain analyze` parsed a raw
//! `DomainSpec` without ever routing it through
//! `fsl_core::validate_lowerable_constructs`, so it was the last consumer
//! that still accepted the three constructs #710/#711/#712 rejected
//! fail-closed everywhere else (a top-level `await`, an `on_stale` policy,
//! and a `value_object` invariant). The same fixture would report
//! `result:"analyzed"`/exit 0 from `analyze` while `check` and
//! `domain expand` rejected it with `kind:"semantics"`/exit 2 -- a
//! cross-command accept/reject split on one spec.
//!
//! `analyze` must now reject the same three fixtures with the same
//! diagnostic shape (`kind`, `loc`, exit code) its guarded sibling
//! `domain expand` already produces for them, and it must keep accepting a
//! well-formed domain spec exactly as before.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn run(args: &[&str]) -> (Value, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(args)
        .current_dir(root())
        .output()
        .expect("run native fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON: {error}; args={args:?}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.code().expect("exit status"))
}

struct RejectedFixtureCase {
    fixture: &'static str,
    expected_message: &'static str,
    expected_line: i64,
    expected_column: i64,
}

const REJECTED_FIXTURE_CASES: &[RejectedFixtureCase] = &[
    RejectedFixtureCase {
        fixture: "rust/fslc/tests/fixtures/domain_await_routing_rejected.fsl",
        expected_message: "top-level await 'PaymentResult' has no executable lowering; use a saga step's awaits",
        expected_line: 26,
        expected_column: 3,
    },
    RejectedFixtureCase {
        fixture: "rust/fslc/tests/fixtures/domain_stale_policy_rejected.fsl",
        expected_message: "on_stale 'Approved' has no executable lowering; stale policies are not supported",
        expected_line: 30,
        expected_column: 5,
    },
    RejectedFixtureCase {
        fixture: "rust/fslc/tests/fixtures/domain_value_object_invariant_rejected.fsl",
        expected_message: "value_object invariant 'AuditStamp.nonNegative' has no executable \
            lowering; value-object invariants are not supported",
        expected_line: 9,
        expected_column: 5,
    },
];

/// Negative control: each #710/#711/#712 fixture must now be rejected by
/// `analyze` with the same `kind`/location/exit code as `check`, instead of
/// silently succeeding with an incomplete projection.
#[test]
fn domain_analyze_rejects_unlowerable_constructs_like_check() {
    for case in REJECTED_FIXTURE_CASES {
        let (analyze, analyze_status) = run(&["domain", "analyze", case.fixture]);
        assert_eq!(analyze_status, 2, "{}: {analyze:#}", case.fixture);
        assert_eq!(analyze["result"], "error", "{}: {analyze:#}", case.fixture);
        assert_eq!(
            analyze["kind"], "semantics",
            "{}: {analyze:#}",
            case.fixture
        );
        assert!(
            analyze["message"]
                .as_str()
                .is_some_and(|message| message.contains(case.expected_message)),
            "{}: {analyze:#}",
            case.fixture
        );
        assert_eq!(
            analyze["loc"]["line"], case.expected_line,
            "{}: {analyze:#}",
            case.fixture
        );
        assert_eq!(
            analyze["loc"]["column"], case.expected_column,
            "{}: {analyze:#}",
            case.fixture
        );

        let (check, check_status) = run(&["check", case.fixture]);
        assert_eq!(
            check_status, analyze_status,
            "check/analyze exit code diverged for {}: check={check:#} analyze={analyze:#}",
            case.fixture
        );
        assert_eq!(
            check["kind"], analyze["kind"],
            "check/analyze diagnostic kind diverged for {}: check={check:#} analyze={analyze:#}",
            case.fixture
        );
        assert_eq!(
            check["loc"], analyze["loc"],
            "check/analyze location diverged for {}: check={check:#} analyze={analyze:#}",
            case.fixture
        );
    }
}

/// Positive control: a well-formed domain spec must keep succeeding through
/// `analyze` so the new guard rejects only the unlowerable constructs, not
/// domain specs in general.
#[test]
fn domain_analyze_still_accepts_a_well_formed_domain_spec() {
    let (analyze, status) = run(&[
        "domain",
        "analyze",
        "examples/domain/order_fulfillment_saga.fsl",
    ]);
    assert_eq!(status, 0, "{analyze:#}");
    assert_eq!(analyze["result"], "analyzed");
}

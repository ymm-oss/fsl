// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for the "Approval records" overlay section (issue
//! #333): `fsl_tools::document_render`'s `AppliedApproval`/`AppliedApprovals`
//! rendering. The CLI's own `--approval`/`--trust-key` contract (loading,
//! verification, drift detection) is exercised end-to-end in
//! `rust/fslc/tests/document_approval_cli.rs`.

use std::path::{Path, PathBuf};

use fsl_tools::{AppliedApproval, AppliedApprovals, Locale};

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_path(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn render_cancel_system(locale: Locale, approvals: Option<&AppliedApprovals<'_>>) -> String {
    let source = read("../../examples/pm/cancel_system.fsl");
    let root = manifest_path("../../examples/pm");
    let claims = fsl_tools::project_requirement_claims_from_source(
        &source,
        Some("examples/pm/cancel_system.fsl"),
        &root,
    )
    .expect("project cancel_system.fsl");
    let resolver = fsl_core::FsResolver::new(&root);
    let kernel = fsl_core::parse_kernel_source(&source, &resolver).expect("parse");
    let model = fsl_core::build_model(kernel.clone()).expect("build model");
    let trace = fsl_core::requirements_trace_contract(&source).expect("trace contract");
    fsl_tools::render_requirements_document(
        &claims,
        &kernel,
        &model,
        trace.as_ref(),
        locale,
        None,
        None,
        approvals,
    )
    .expect("render paired RCIR")
    .markdown
}

fn approval(record_path: &str, approver: &str, approved_at: &str) -> AppliedApproval {
    AppliedApproval {
        record_path: record_path.to_owned(),
        approver: approver.to_owned(),
        approved_at: approved_at.to_owned(),
        requirements: vec!["REQ-1".to_owned()],
        artifact_digest: "sha256:0000000000000000000000000000000000000000000000000000000000000"
            .to_owned(),
        signature_key_id: None,
    }
}

#[test]
fn no_approval_renders_byte_identically_to_no_approval_argument() {
    let markdown = render_cancel_system(Locale::Ja, None);
    assert!(!markdown.contains("approval_digest"));
    assert!(!markdown.contains("## 承認記録"));
}

#[test]
fn approvals_section_includes_the_intent_fidelity_disclaimer() {
    let records = vec![approval("a.json", "alice", "2026-07-17T00:00:00Z")];
    let applied = AppliedApprovals {
        records: &records,
        digest: "sha256:test",
    };
    let ja = render_cancel_system(Locale::Ja, Some(&applied));
    assert!(ja.contains("approval_digest: sha256:test"));
    assert!(ja.contains("## 承認記録"));
    assert!(ja.contains("原意への忠実性を証明するものではない"));

    let en = render_cancel_system(Locale::En, Some(&applied));
    assert!(en.contains("## Approval records"));
    assert!(en.contains("It does not prove fidelity to original intent."));
    assert!(en.contains("Reviewed artifact digest"));
    assert!(!en.contains("Target digest"));
}

#[test]
fn approvals_section_lists_records_sorted_by_approved_at_then_path() {
    let records = vec![
        approval("z.json", "bob", "2026-07-17T09:00:00Z"),
        approval("a.json", "alice", "2026-07-01T00:00:00Z"),
        approval("b.json", "carol", "2026-07-17T09:00:00Z"),
    ];
    let applied = AppliedApprovals {
        records: &records,
        digest: "sha256:test",
    };
    let markdown = render_cancel_system(Locale::Ja, Some(&applied));
    let start = markdown.find("## 承認記録").expect("section exists");
    let section = &markdown[start..];
    let alice_pos = section.find("alice").expect("alice row");
    let bob_pos = section.find("bob").expect("bob row");
    let carol_pos = section.find("carol").expect("carol row");
    // alice (a.json) has the earliest approved_at; bob (z.json) and carol
    // (b.json) share the later timestamp, so the record path breaks the tie:
    // b.json < z.json, so carol renders before bob.
    assert!(
        alice_pos < carol_pos && carol_pos < bob_pos,
        "rows must be sorted by (approved_at, record_path)"
    );
}

#[test]
fn signed_and_unsigned_records_render_distinct_signature_lines() {
    let unsigned = approval("a.json", "alice", "2026-07-17T00:00:00Z");
    let mut signed = approval("b.json", "bob", "2026-07-17T00:00:00Z");
    signed.signature_key_id =
        Some("sha256:1111111111111111111111111111111111111111111111111111111111111".to_owned());
    let records = vec![unsigned, signed];
    let applied = AppliedApprovals {
        records: &records,
        digest: "sha256:test",
    };
    let markdown = render_cancel_system(Locale::Ja, Some(&applied));
    assert!(markdown.contains("署名: なし"));
    assert!(markdown.contains("署名: あり"));
}

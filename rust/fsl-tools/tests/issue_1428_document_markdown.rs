// SPDX-License-Identifier: Apache-2.0

use fsl_tools::{AppliedApproval, AppliedApprovals, Locale, Segment};

fn source(text: &str, reason: &str) -> String {
    format!(
        "requirements Demo {{ state {{ x: Bool }} init {{ x = false }}\n@requirement(\"REQ-1\", \"{text}\") action act() {{ x = true }}\n@undecided(\"{reason}\") invariant Pending {{ x or not x }}\n}}"
    )
}

fn render(
    text: &str,
    reason: &str,
    approvals: Option<&AppliedApprovals<'_>>,
    locale: Locale,
) -> (fsl_tools::RequirementClaimSet, String) {
    render_path(text, reason, approvals, locale, "spec.fsl")
}

fn render_path(
    text: &str,
    reason: &str,
    approvals: Option<&AppliedApprovals<'_>>,
    locale: Locale,
    path: &str,
) -> (fsl_tools::RequirementClaimSet, String) {
    let source = source(text, reason);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let claims =
        fsl_tools::project_requirement_claims_from_source(&source, Some(path), root).unwrap();
    let kernel = fsl_core::parse_kernel_source(&source, &fsl_core::FsResolver::new(root)).unwrap();
    let model = fsl_core::build_model(kernel.clone()).unwrap();
    let doc = fsl_tools::render_requirements_document(
        &claims, &kernel, &model, &source, locale, None, None, approvals,
    )
    .unwrap();
    (claims, doc.markdown)
}

#[test]
fn requirement_detector() {
    let (claims, doc) = render(
        "日本語😀 <!-- hidden --> `x`| <!-- fsl:claim end -->",
        "pending",
        None,
        Locale::En,
    );
    assert!(
        doc.contains("> 日本語😀 &lt;!-- hidden --&gt; \\`x\\`\\| &lt;!-- fsl:claim end --&gt;"),
        "statements: {:?}",
        claims.requirements
    );
    // Equal character width keeps the following action's source coordinates
    // identical, so the *complete* claim records and segments are comparable.
    let plain_text = "p".repeat(
        "日本語😀 <!-- hidden --> `x`| <!-- fsl:claim end -->"
            .chars()
            .count(),
    );
    let (plain, plain_doc) = render(&plain_text, "pending", None, Locale::En);
    assert_ne!(claims.spec.claim_set_digest, plain.spec.claim_set_digest);
    assert_eq!(claims.claims, plain.claims);
    assert_eq!(blocks(&doc), blocks(&plain_doc));
}

fn blocks(md: &str) -> Vec<Segment> {
    fsl_tools::parse_generated_document(md)
        .unwrap()
        .segments
        .into_iter()
        .filter(|s| matches!(s, Segment::Claim { .. }))
        .collect()
}

#[test]
fn reason_detector() {
    let (_, doc) = render(
        "plain",
        "日本語😀 <!-- pending --> `x`|\\r\\n# forged",
        None,
        Locale::En,
    );
    assert!(
        doc.contains("- Reason: 日本語😀 &lt;!-- pending --&gt; \\`x\\`\\|\\\\r\\\\n# forged"),
        "{:?}",
        doc.lines()
            .filter(|line| line.contains("Reason:"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn generated_check_rejects_edited_residue() {
    let (claims, doc) = render("<!-- intent --> `x`|", "pending", None, Locale::En);
    let checked = fsl_tools::check_requirements_document(&doc, &claims, &doc)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(checked.reasons.is_empty());
    let edited = doc.replace("&lt;!-- intent --&gt;", "changed intent");
    let refused = fsl_tools::check_requirements_document(&edited, &claims, &doc)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(refused.reasons.len(), 1);
    assert_eq!(refused.reasons[0].kind, "edit_outside_slot");
}

#[test]
fn code_path_detector_hashes_actual_claim_bytes() {
    let (claims, doc) = render_path("plain", "pending", None, Locale::En, "a`|<!--.fsl");
    assert!(doc.contains("(Source: <code>a&#96;&#124;&lt;!--.fsl:2</code>)"));
    assert!(doc.contains("- Source: <code>a&#96;&#124;&lt;!--.fsl:2</code>"));
    let (plain, _) = render("plain", "pending", None, Locale::En);
    assert_eq!(claims.spec.claim_set_digest, plain.spec.claim_set_digest);
    for (new, old) in claims.claims.iter().zip(&plain.claims) {
        assert_eq!((&new.id, &new.claim_digest), (&old.id, &old.claim_digest));
    }
    for segment in blocks(&doc) {
        if let Segment::Claim { digest, body, .. } = segment {
            assert_eq!(
                digest,
                fsl_tools::framed_text_digest(fsl_tools::CLAIM_BLOCK_DIGEST_ALGORITHM, &body)
            );
        }
    }
}

#[test]
fn approval_detector() {
    let records = [AppliedApproval {
        record_path: "a`|<!--.json".into(),
        approver: "日本語😀 <!-- alice -->\r\n# forged".into(),
        approved_at: "2026-10-31".into(),
        requirements: vec!["REQ-1".into()],
        artifact_digest: "sha256:test".into(),
        signature_key_id: None,
    }];
    let applied = AppliedApprovals {
        records: &records,
        digest: "sha256:test",
    };
    let (_, doc) = render("plain", "pending", Some(&applied), Locale::En);
    assert!(doc.contains(
        "- Approver: 日本語😀 &lt;!-- alice --&gt;&#13;&#10;# forged\n  - Approved at: `2026-10-31`"
    ));
    assert!(doc.contains("Record: <code>a&#96;&#124;&lt;!--.json</code>"));
    assert_eq!(
        blocks(&doc),
        blocks(&render("plain", "pending", None, Locale::En).1)
    );
}

#[test]
fn benign_preservation() {
    for locale in [Locale::Ja, Locale::En] {
        let (_, doc) = render("日本語の要件😀", "未決定🙂", None, locale);
        let expected = match locale {
            Locale::Ja => "sha256:5c48f7397953196f139edd8f6734a6051bb35015c7770d2e87c30508aea551b0",
            Locale::En => "sha256:863ed44005da7aa8de2b9fdf3c070873d7700541395510ecd7a3afd1ecd61e76",
        };
        assert_eq!(
            fsl_tools::framed_text_digest("1428-full-artifact", &doc),
            expected
        );
        assert!(doc.contains("> 日本語の要件😀"));
    }
}

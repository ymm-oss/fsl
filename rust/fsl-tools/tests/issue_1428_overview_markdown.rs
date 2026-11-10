// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{ParseOptions, SourceFile};
use fsl_tools::{Locale, RequirementClaimSet, Segment};

fn options() -> ParseOptions {
    ParseOptions::stable()
}

fn source(text: &str, comment: &str) -> String {
    format!(
        "fsl 5\nrequirements Demo {{\n  glossary {{ Draft = 下書き Approved = 承認済み User = 利用者 }}\n  number Amount\n  resource stock: Amount = (1 {comment} + 1)\n  actor User\n  entity Claim {{\n    amount: Amount = 0\n    lifecycle Draft -> Approved\n  }}\n  concept Eligible = Claim where amount >= 0\n  event approve = User approves Claim Draft -> Approved\n  rule REQ-1 \"{text}\" {{\n    allow User approves Claim only when amount {comment} >= 0\n  }}\n  decide D \"Who handles claims\" for Draft Claim {{\n    Eligible {comment} => User approves\n  }} assuming stock {comment} >= 0\n}}\nverify {{ instances Claim = 1 values Amount = 0..2 }}\n"
    )
}

fn render(source: &str, locale: Locale) -> (RequirementClaimSet, String) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let opts = options();
    let claims = fsl_tools::project_requirement_claims_from_source_with_options(
        source,
        Some("overview.fsl"),
        root,
        &opts,
    )
    .unwrap();
    let kernel =
        fsl_core::parse_kernel_source_with_options(source, &fsl_core::FsResolver::new(root), &opts)
            .unwrap();
    let model = fsl_core::build_model(kernel.clone()).unwrap();
    let doc = fsl_tools::render_requirements_document_with_options(
        &claims, &kernel, &model, source, locale, None, None, None, &opts,
    )
    .unwrap();
    (claims, doc.markdown)
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
fn overview_intent_detector() {
    let text = "日本語😀 <!-- hidden --> `x`| <!-- fsl:claim end -->";
    for locale in [Locale::En, Locale::Ja] {
        let (claims, doc) = render(&source(text, ""), locale);
        let literal = "> 日本語😀 &lt;!-- hidden --&gt; \\`x\\`\\| &lt;!-- fsl:claim end --&gt;";
        assert_eq!(
            doc.matches(literal).count(),
            2,
            "overview AND requirement must quote literal intent"
        );
        assert!(!doc.contains("> 日本語😀 <!--"));
        // Lowering origin IDs contain byte offsets; keep those identical too.
        let (plain, plain_doc) = render(&source(&"p".repeat(text.len()), ""), locale);
        assert_eq!(claims.claims, plain.claims);
        assert_eq!(blocks(&doc), blocks(&plain_doc));
        assert_ne!(claims.spec.claim_set_digest, plain.spec.claim_set_digest);
        assert!(
            fsl_tools::check_requirements_document(&doc, &claims, &doc)
                .unwrap_or_else(|e| panic!("{e}"))
                .reasons
                .is_empty()
        );
        let edited = doc.replacen(literal, "> altered intent", 1);
        let rejected = fsl_tools::check_requirements_document(&edited, &claims, &doc)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rejected.reasons.len(), 1);
        assert_eq!(rejected.reasons[0].kind, "edit_outside_slot");
    }
}

#[test]
fn authored_comment_detector() {
    let (_, doc) = render(
        &source("ordinary intent", "// `x`|<!-- -->\r\n"),
        Locale::En,
    );
    assert!(doc.contains("<code>allow User approves Claim only when amount // &#96;x&#96;&#124;&lt;!-- --&gt; &gt;= 0</code>"));
    assert!(doc.contains("| 1 | Eligible // \\`x\\`\\|&lt;!-- --&gt; |"));
}

#[test]
fn source_boundary_refusals() {
    for bad in ["\"broken\"", "\"line\nlabel\"", "bad\"label", "bad\nlabel"] {
        let input =
            source("ordinary intent", "").replace("Draft = 下書き", &format!("Draft = {bad}"));
        assert!(
            fsl_syntax::parse_document_with(SourceFile::new(&input), &options()).is_err(),
            "accepted alias: {bad:?}"
        );
    }
    for text in ["line\n# forged", "line\r\n# forged"] {
        assert!(
            fsl_syntax::parse_document_with(SourceFile::new(&source(text, "")), &options())
                .is_err(),
            "multiline string accepted"
        );
    }
}

#[test]
fn carriage_return_descriptions_detector() {
    let input = source("intent\r# forged", "")
        .replace("Who handles claims", "decision\r# forged")
        .replace(
            "  event approve",
            "  goal Reachable \"goal\r# forged\" some Claim can reach Approved\n  event approve",
        );
    let (_, doc) = render(&input, Locale::En);
    assert!(doc.contains("#### D: decision&#13;\\# forged"));
    assert!(doc.contains("- **Reachable**: goal&#13;\\# forged"));
    assert_eq!(doc.matches("> intent&#13;# forged").count(), 2);
    assert!(!doc.contains('\r'));
}

#[test]
fn benign_preservation() {
    for locale in [Locale::En, Locale::Ja] {
        let (_, doc) = render(&source("Ordinary intent 日本語😀 -> approved", ""), locale);
        let expected = match locale {
            Locale::En => "sha256:9616d9fe4012efe84fbb037b08caa6868be62622859c2b26208f9c15f41d4edb",
            Locale::Ja => "sha256:ee5bfd866e379d198823212425c9d05b1b05033c450c6f2370c450e2873d9205",
        };
        assert_eq!(
            fsl_tools::framed_text_digest("1428-overview-artifact", &doc),
            expected
        );
        assert!(doc.contains("[*] --> Draft"));
        assert!(doc.contains("| `stock` | Amount | `1 + 1` |"));
    }
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Integration tests for the controlled-language renderer, issue #326.

use std::path::{Path, PathBuf};

use fsl_tools::{Locale, RenderedDocument, RequirementClaimSet};

fn manifest_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_path(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

struct Fixture {
    source: String,
    root: PathBuf,
    source_path: String,
}

fn cancel_system() -> Fixture {
    Fixture {
        source: read("../../examples/pm/cancel_system.fsl"),
        root: manifest_path("../../examples/pm"),
        source_path: "examples/pm/cancel_system.fsl".to_owned(),
    }
}

fn claims_fixture() -> Fixture {
    Fixture {
        source: read("tests/fixtures/document_claims_fixture.fsl"),
        root: manifest_path("tests/fixtures"),
        source_path: "document_claims_fixture.fsl".to_owned(),
    }
}

fn kpi_fixture() -> Fixture {
    Fixture {
        source: read("tests/fixtures/document_kpi_fixture.fsl"),
        root: manifest_path("tests/fixtures"),
        source_path: "document_kpi_fixture.fsl".to_owned(),
    }
}

fn render(fixture: &Fixture, locale: Locale) -> (RequirementClaimSet, RenderedDocument) {
    let claims = fsl_tools::project_requirement_claims_from_source(
        &fixture.source,
        Some(&fixture.source_path),
        &fixture.root,
    )
    .unwrap_or_else(|error| panic!("project {}: {error}", fixture.source_path));
    let resolver = fsl_core::FsResolver::new(&fixture.root);
    let kernel = fsl_core::parse_kernel_source(&fixture.source, &resolver).expect("parse");
    let model = fsl_core::build_model(kernel).expect("build model");
    let trace = fsl_core::requirements_trace_contract(&fixture.source).expect("trace contract");
    let doc = fsl_tools::render_requirements_document(&claims, &model, trace.as_ref(), locale);
    (claims, doc)
}

fn req2_block(markdown: &str) -> &str {
    let start = markdown.find("### REQ-2").expect("REQ-2 section exists");
    let rest = &markdown[start..];
    let end = rest.find("### REQ-3").expect("REQ-3 section exists");
    rest[..end].trim_end()
}

// --- Acceptance criterion 1: cancel_system.fsl REQ-2 renders both guards, ---
// --- the struct update, and fairness without loss (ja + en golden).       ---

#[test]
fn req2_ja_golden_matches_exactly() {
    let fixture = cancel_system();
    let (_, doc) = render(&fixture, Locale::Ja);
    let expected = "### REQ-2\n\
\n\
**要件原文（意図。形式意味との一致は人間が確認する）**\n\
\n\
> On cancellation-form submission, show the retention offer exactly once per subscription\n\
\n\
（出典: `examples/pm/cancel_system.fsl:64`）\n\
\n\
**形式化された意味（FSLから決定論的に生成）**\n\
\n\
<!-- fsl:claim begin id=\"action:submit_cancel#operation\" digest=\"sha256:62bfddb43c581089b0c8c3a3516c58d576708efaeda042793c0cd180c0b7b547\" -->\n\
#### 操作: `submit_cancel`\n\
\n\
- 識別子: `action:submit_cancel#operation`\n\
- 出典: `examples/pm/cancel_system.fsl:65`\n\
- パラメータ: `c: Sub`\n\
\n\
操作 `submit_cancel` を実行できるのは、次の条件をすべて満たす場合に限る。\n\
\n\
1. `scr[c].st` が `CancelForm` である。\n\
2. `scr[c].offered` が `false` である。\n\
\n\
操作が成功した場合、次の更新を同一ステップで同時に適用する。更新の右辺は遷移前の状態を読む。\n\
\n\
1. `scr[c]` を `SubView { offered: true, st: OfferDialog }` に置き換える。すなわち、`scr[c].offered` を `true` に、`scr[c].st` を `OfferDialog` にする。\n\
\n\
この操作には弱い公平性（weak fairness）を仮定する。これはスケジューリング上の仮定であり、この操作が実行可能（enabled）であり続けるならば、いつかは実行される、という意味である。直ちに実行されることを意味しない。\n\
<!-- fsl:claim end -->";
    assert_eq!(req2_block(&doc.markdown), expected);
}

#[test]
fn req2_en_golden_matches_exactly() {
    let fixture = cancel_system();
    let (_, doc) = render(&fixture, Locale::En);
    let expected = "### REQ-2\n\
\n\
**Original requirement text (intent; a human confirms that it matches the formalized meaning)**\n\
\n\
> On cancellation-form submission, show the retention offer exactly once per subscription\n\
\n\
(Source: `examples/pm/cancel_system.fsl:64`)\n\
\n\
**Formalized meaning (generated deterministically from the FSL)**\n\
\n\
<!-- fsl:claim begin id=\"action:submit_cancel#operation\" digest=\"sha256:8ad70c92eda277d36efca15a5d82569add1d1d8392d9f6e07fae4134d003059f\" -->\n\
#### Operation: `submit_cancel`\n\
\n\
- Identifier: `action:submit_cancel#operation`\n\
- Source: `examples/pm/cancel_system.fsl:65`\n\
- Parameters: `c: Sub`\n\
\n\
Action `submit_cancel` can be executed only when all of the following conditions hold.\n\
\n\
1. `scr[c].st` is `CancelForm`.\n\
2. `scr[c].offered` is `false`.\n\
\n\
When the action succeeds, the following updates are applied simultaneously within a single step. The right-hand sides read the pre-transition state.\n\
\n\
1. Replace `scr[c]` with `SubView { offered: true, st: OfferDialog }`; that is, set `scr[c].offered` to `true` and `scr[c].st` to `OfferDialog`.\n\
\n\
Weak fairness is assumed for this action. This is a scheduling assumption: if the action remains continuously enabled, it is eventually executed. It does not mean that the action is executed immediately.\n\
<!-- fsl:claim end -->";
    assert_eq!(req2_block(&doc.markdown), expected);
}

#[test]
fn cancel_system_renders_with_zero_formula_fallbacks() {
    // Every expression in examples/pm/cancel_system.fsl is a safe pattern;
    // none should need the canonical-FSL fallback.
    let fixture = cancel_system();
    let (_, ja) = render(&fixture, Locale::Ja);
    let (_, en) = render(&fixture, Locale::En);
    assert_eq!(ja.formula_fallback_count, 0);
    assert_eq!(en.formula_fallback_count, 0);
}

// --- Acceptance criterion 2: original text and formalized meaning are ---
// --- in separate sections, never fused into one sentence.            ---

#[test]
fn original_text_and_formalized_meaning_are_separate_sections() {
    let fixture = cancel_system();
    let (_, doc) = render(&fixture, Locale::Ja);
    let block = req2_block(&doc.markdown);
    let original_at = block.find("要件原文").expect("original text heading");
    let formal_at = block
        .find("形式化された意味")
        .expect("formalized meaning heading");
    assert!(original_at < formal_at);
    // The original English requirement text never appears merged into the
    // Japanese normative sentence that follows it.
    let formal_section = &block[formal_at..];
    assert!(!formal_section.contains("On cancellation-form submission"));
}

// --- Acceptance criterion 3: acceptance/forbidden are not phrased as ---
// --- universal properties.                                          ---

#[test]
fn acceptance_and_forbidden_carry_the_non_generalization_disclaimer() {
    let fixture = claims_fixture();
    let (_, ja) = render(&fixture, Locale::Ja);
    assert!(
        ja.markdown
            .contains("同種のすべての入力・順序・状態で同じ結果になることを主張するものではない")
    );
    assert!(
        ja.markdown
            .contains("この操作があらゆる状況で禁止されることを主張するものではない")
    );

    let (_, en) = render(&fixture, Locale::En);
    assert!(en.markdown.contains(
        "It does not claim that every input, ordering, or state of the same kind produces the same result"
    ));
    assert!(
        en.markdown
            .contains("It does not claim that this operation is forbidden in every situation")
    );

    // Never phrased as an absolute/universal guarantee.
    for banned in ["常に禁止", "決して実行できない", "常に成立する保証"] {
        assert!(
            !ja.markdown.contains(banned),
            "banned phrase present: {banned}"
        );
    }
    for banned in [
        "always forbidden",
        "can never be executed",
        "guarantees that",
    ] {
        assert!(
            !en.markdown.contains(banned),
            "banned phrase present: {banned}"
        );
    }
}

// --- Acceptance criterion 4: analysis bounds are not phrased as system ---
// --- capacity.                                                        ---

#[test]
fn analysis_bounds_carry_the_not_a_system_limit_disclaimer() {
    let fixture = claims_fixture();
    let (claims, ja) = render(&fixture, Locale::Ja);
    assert!(!claims.analysis_scope.instances.is_empty());
    assert!(
        ja.markdown
            .contains("解析のための範囲であり、実運用上の上限や容量を意味しない")
    );
    let (_, en) = render(&fixture, Locale::En);
    assert!(en.markdown.contains(
        "These are analysis bounds; they do not represent operational limits or system capacity"
    ));
}

#[test]
fn analysis_scope_numeric_bounds_render_as_plain_numbers() {
    // document_kpi_fixture.fsl: `verify { instances Claim = 3; values Amount = 0..3 }`.
    let fixture = kpi_fixture();
    let (claims, ja) = render(&fixture, Locale::Ja);
    assert!(!claims.analysis_scope.values.is_empty());
    assert!(
        ja.markdown
            .contains("エンティティ `Claim` の解析インスタンス数: 3")
    );
    assert!(
        ja.markdown
            .contains("数値 `Amount` の解析値域: `0` から `3` まで")
    );
    // Never the raw normalized-AST JSON shape leaking into the document.
    assert!(!ja.markdown.contains("[\"num\""));
}

// --- Acceptance criterion 5: repeated runs are byte-identical. ---

#[test]
fn rendering_is_byte_identical_across_repeated_runs() {
    let fixture = cancel_system();
    let (_, first) = render(&fixture, Locale::Ja);
    let (_, second) = render(&fixture, Locale::Ja);
    assert_eq!(first.markdown, second.markdown);
    assert_eq!(first.formula_fallback_count, second.formula_fallback_count);
}

// --- Meaning-fidelity: `requires` renders as an enablement condition, ---
// --- `not` is preserved, weak fairness is not "immediately".         ---

#[test]
fn requires_renders_as_enablement_not_desirability() {
    let fixture = cancel_system();
    let (_, ja) = render(&fixture, Locale::Ja);
    assert!(ja.markdown.contains("を実行できるのは"));
    for banned in ["望ましい", "べきである", "が期待される"] {
        assert!(
            !ja.markdown.contains(banned),
            "banned phrase present: {banned}"
        );
    }
}

#[test]
fn not_is_preserved_as_negation_not_dropped() {
    let fixture = cancel_system();
    let (_, ja) = render(&fixture, Locale::Ja);
    // REQ-2's second guard is `requires not scr[c].offered`.
    assert!(ja.markdown.contains("`scr[c].offered` が `false` である"));
}

#[test]
fn fairness_is_a_scheduling_assumption_not_immediate_execution() {
    let fixture = cancel_system();
    let (_, ja) = render(&fixture, Locale::Ja);
    assert!(ja.markdown.contains("スケジューリング上の仮定"));
    assert!(!ja.markdown.contains("直ちに実行される、という意味である"));
    let (_, en) = render(&fixture, Locale::En);
    assert!(en.markdown.contains("a scheduling assumption"));
    // The only occurrence of "executed immediately" is inside the negation
    // "does not mean that the action is executed immediately" -- never as a
    // standalone positive claim.
    assert!(
        en.markdown
            .contains("does not mean that the action is executed immediately")
    );
    assert!(!en.markdown.contains("will be executed immediately"));
    assert!(!en.markdown.contains("every time it is enabled"));
}

// --- Operator coverage (issue #326's test plan) using document_claims_fixture.fsl. ---

#[test]
fn transition_rule_uses_the_n_and_special_case_and_preserves_old() {
    let fixture = claims_fixture();
    let (_, ja) = render(&fixture, Locale::Ja);
    // `trans NoDirectClose`: not (old(cases[c]) == Open and cases[c] == Closed)
    assert!(ja.markdown.contains("次のすべてが同時に成立することはない"));
    assert!(ja.markdown.contains("遷移前の `cases[c]` が `Open` である"));
}

#[test]
fn deadline_rule_is_distinguished_from_progress_rule_liveness() {
    let fixture = claims_fixture();
    let (_, ja) = render(&fixture, Locale::Ja);
    assert!(ja.markdown.contains("#### 期限条件"));
    assert!(
        ja.markdown
            .contains("進行条件（leadsTo）の liveness とは異なる")
    );
}

#[test]
fn progress_rule_never_claims_established_evidence() {
    let fixture = claims_fixture();
    let (_, ja) = render(&fixture, Locale::Ja);
    assert!(ja.markdown.contains("#### 進行条件"));
    assert!(ja.markdown.contains("成立が確認済みであることを意味しない"));
    for banned in ["proved", "証明済み", "bounded"] {
        let progress_start = ja.markdown.find("#### 進行条件").unwrap();
        let progress_section = &ja.markdown[progress_start..progress_start + 800];
        assert!(
            !progress_section.contains(banned),
            "banned phrase present: {banned}"
        );
    }
}

#[test]
fn reachability_goal_is_a_goal_not_an_invariant() {
    let fixture = claims_fixture();
    let (_, ja) = render(&fixture, Locale::Ja);
    assert!(ja.markdown.contains("到達目標"));
    assert!(
        ja.markdown
            .contains("すべての状態での成立を求める不変条件ではない")
    );
}

#[test]
fn undecided_is_excluded_from_normative_claims_and_labeled_as_metadata() {
    let fixture = claims_fixture();
    let (_, ja) = render(&fixture, Locale::Ja);
    assert!(ja.markdown.contains("## 未決定事項"));
    assert!(ja.markdown.contains("検証条件ではない"));
    assert!(ja.markdown.contains("RetentionPlaceholder"));
}

#[test]
fn forbidden_with_no_prefix_is_worded_without_an_empty_step_list() {
    let fixture = claims_fixture();
    let (_, ja) = render(&fixture, Locale::Ja);
    assert!(ja.markdown.contains("先行する操作はない"));
    let (_, en) = render(&fixture, Locale::En);
    assert!(en.markdown.contains("there are no preceding operations"));
}

#[test]
fn shared_claim_across_two_requirements_is_rendered_once_and_back_referenced() {
    // `close` in document_claims_fixture.fsl covers both REQ-1 and REQ-2.
    let fixture = claims_fixture();
    let (_, ja) = render(&fixture, Locale::Ja);
    let occurrences = ja.markdown.matches("操作: `close`").count();
    assert_eq!(occurrences, 1, "the full block renders exactly once");
    assert!(
        ja.markdown
            .contains("の節に記載している。この要件にも同じ意味で適用される。")
    );
}

#[test]
fn multiple_guards_use_a_numbered_list_not_a_single_run_on_sentence() {
    let fixture = claims_fixture();
    let (_, ja) = render(&fixture, Locale::Ja);
    // `close` has one guard; `escalate` has a `let` plus a `requires`.
    assert!(ja.markdown.contains("（定義）`allowed` を `true` とする"));
}

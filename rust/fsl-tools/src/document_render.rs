// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The controlled-language renderer (issue #326): converts RCIR v1 claims
//! (issue #325) into deterministic Japanese/English Markdown. Normative text
//! comes from fixed per-claim-kind templates, never from an LLM. See
//! `docs/DESIGN-document-requirement-claim-ir.md` for the RCIR contract this
//! renders and the design rationale for reusing `fsl_core::source_expr_text`
//! as the canonical-expression fallback.
//!
//! Rendering reads the original checked `Expr`/`ActionDef`/... from the
//! `KernelModel` the RCIR claim set was projected from (looked up by the
//! claim's own `semantic_targets`), rather than re-deriving natural language
//! from RCIR's JSON AST, so the safe-pattern recognizer in
//! `document_render_expr` works directly against `fsl_syntax::Expr` and can
//! fall back to the exact same `source_expr_text` `explain --readable` uses.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use fsl_core::{
    ActionGuard, KernelModel, KernelSpec, LeadsToDef, ParamDef, PropertyDef, RequirementsTraceCase,
    RequirementsTraceContract, RequirementsTraceExpectation, TypeRef, action_target, display_name,
    requirements_trace_contract,
};

use crate::document::{Claim, ClaimKind, RequirementClaimSet};
use crate::document_glossary::AppliedGlossary;
use crate::document_project::project_renderer_contract;
use crate::document_render_expr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    Ja,
    En,
}

impl Locale {
    /// The short code recorded in a generated document's `lang` frontmatter
    /// key (issue #329) and accepted by `fslc document generate --lang`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ja => "ja",
            Self::En => "en",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ja" => Some(Self::Ja),
            "en" => Some(Self::En),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct RenderedDocument {
    pub markdown: String,
    pub formula_fallback_count: u64,
}

struct Ctx<'a> {
    model: &'a KernelModel,
    trace: Option<&'a RequirementsTraceContract>,
    locale: Locale,
    fallback_count: u64,
    rendered_claims: BTreeSet<String>,
    glossary: Option<&'a crate::document_glossary::Glossary>,
}

/// Compile RCIR claims into a controlled-language Markdown document.
///
/// `source` must be the exact source the RCIR claim set was projected from.
///
/// `glossary` (issue #330) is presentation-only: it can only change how an
/// `action:`/`state:`/`enum:` identifier *displays* (an action's own claim
/// heading, and a standalone "Glossary" reference section listing every
/// accepted label) — never modality, negation, or conditional structure,
/// since no other rendering decision ever consults it.
/// # Errors
///
/// Returns an error before rendering when the embedded Public Kernel differs
/// from the supplied checked model or a semantic target does not resolve once.
pub fn render_requirements_document(
    claims: &RequirementClaimSet,
    kernel: &KernelSpec,
    model: &KernelModel,
    source: &str,
    locale: Locale,
    glossary: Option<&AppliedGlossary<'_>>,
) -> Result<RenderedDocument, String> {
    let trace = requirements_trace_contract(source).map_err(|error| error.to_string())?;
    validate_render_inputs(claims, kernel, model, source, trace.as_ref())?;
    let mut ctx = Ctx {
        model,
        trace: trace.as_ref(),
        locale,
        fallback_count: 0,
        rendered_claims: BTreeSet::new(),
        glossary: glossary.map(|applied| applied.glossary),
    };

    let mut out = String::new();
    push_section(
        &mut out,
        &crate::document_markers::render_frontmatter(
            claims.spec.source.as_deref(),
            locale,
            &claims.spec.spec_digest,
            &claims.spec.claim_set_digest,
            glossary.map(|applied| applied.digest),
        ),
    );
    push_section(&mut out, &title(&claims.spec.name, locale));
    push_section(&mut out, &background_slot(locale));
    push_section(&mut out, &position_section(locale));
    push_section(&mut out, &semantics_section(locale));
    push_section(&mut out, &requirements_section(claims, &mut ctx));
    push_section(&mut out, &unattributed_section(claims, &mut ctx));
    push_section(&mut out, &undecided_section(claims, locale));
    push_section(&mut out, &analysis_scope_section(claims, locale));
    if let Some(section) = glossary_section(ctx.glossary, locale) {
        push_section(&mut out, &section);
    }
    push_section(
        &mut out,
        &generation_section(claims, ctx.fallback_count, locale),
    );
    if !out.is_empty() {
        out.push('\n');
    }

    Ok(RenderedDocument {
        markdown: out,
        formula_fallback_count: ctx.fallback_count,
    })
}

fn validate_render_inputs(
    claims: &RequirementClaimSet,
    kernel: &KernelSpec,
    model: &KernelModel,
    source: &str,
    trace: Option<&RequirementsTraceContract>,
) -> Result<(), String> {
    let projected =
        project_renderer_contract(kernel, model, source, claims.spec.source.as_deref(), trace)?;
    if projected != *claims {
        return Err("RCIR does not match the paired checked source and model".to_owned());
    }
    Ok(())
}

fn push_section(out: &mut String, section: &str) {
    if !out.is_empty() {
        out.push('\n');
        out.push('\n');
    }
    out.push_str(section.trim_end_matches('\n'));
}

fn heading(locale: Locale, level: &str, ja: &str, en: &str) -> String {
    format!(
        "{level} {}",
        match locale {
            Locale::Ja => ja,
            Locale::En => en,
        }
    )
}

fn title(spec_name: &str, locale: Locale) -> String {
    match locale {
        Locale::Ja => format!("# 要件仕様書: {spec_name}"),
        Locale::En => format!("# Requirements Specification: {spec_name}"),
    }
}

/// The one editable, non-normative slot v1 defines (issue #329). Its
/// contents are never inspected by `fslc document check` — only that the
/// `<!-- fsl:slot begin/end -->` markers around it are well-formed and
/// present exactly once.
fn background_slot(locale: Locale) -> String {
    let heading = heading(locale, "##", "背景", "Background");
    let placeholder = match locale {
        Locale::Ja => {
            "（この節は自由に編集できる。規範的な効力はない。規範文はこの節の外の生成ブロックにのみ存在する。）"
        }
        Locale::En => {
            "(This section can be edited freely. It has no normative force; normative text exists only in the generated blocks outside this section.)"
        }
    };
    crate::document_markers::wrap_slot("background", &format!("{heading}\n\n{placeholder}"))
}

fn position_section(locale: Locale) -> String {
    let head = heading(locale, "##", "本書の位置づけ", "Position of this document");
    let body = match locale {
        Locale::Ja => {
            "本書の「形式化された意味」は、検査済みの FSL 仕様から決定論的に生成した規範文である。同じ仕様からは、常にバイト単位で同一の文書が生成される。\n\n本書が保証するのは、FSL が検査した構造 — 実行条件、更新、事後条件、否定、公平性、期限、範囲 — を欠落なく決定論的に表示することである。本書は、日本語の文と FSL の式が意味的に同値であることを証明するものではない。また、FSL が元の業務意図を正しく捉えていることも保証しない。要件原文と形式化された意味との一致の確認は、人間のレビューに委ねられる。\n\n規範文は claim の種類ごとの固定テンプレートで生成しており、一義性を流暢さより優先している。"
        }
        Locale::En => {
            "The \"formalized meaning\" in this document is normative text generated deterministically from a checked FSL specification. The same specification always produces a byte-identical document.\n\nWhat this document guarantees is that it displays the structure the FSL checked \u{2014} enablement conditions, updates, postconditions, negation, fairness, deadlines, and bounds \u{2014} deterministically and without omission. It does not prove that the English sentences are semantically equivalent to the FSL expressions, and it does not guarantee that the FSL captures the original business intent. Confirming that the original requirement text matches the formalized meaning is left to human review.\n\nThe normative text is produced from fixed per-claim-kind templates; unambiguity is prioritized over fluency."
        }
    };
    format!("{head}\n\n{body}")
}

fn semantics_section(locale: Locale) -> String {
    let head = heading(
        locale,
        "##",
        "全体の意味規約",
        "Global semantic conventions",
    );
    let intro = match locale {
        Locale::Ja => "本仕様のすべての操作に、次の実行規約が適用される。",
        Locale::En => "All operations in this specification follow these execution conventions.",
    };
    let items = match locale {
        Locale::Ja => vec![
            "更新はステップ単位で同時にコミットされる（`updates: simultaneous`）。".to_owned(),
            "更新の右辺は遷移前の状態を読む（`reads: pre_state`）。".to_owned(),
            "事後条件・状態不変条件・遷移条件のいずれかに違反するステップはコミットされず、状態は遷移前のまま残る（`failed_step: rollback`）。".to_owned(),
            "公平性の仮定は弱い公平性である（`fairness: weak`）。公平性は `fair` と宣言された操作にのみ適用される。".to_owned(),
        ],
        Locale::En => vec![
            "Updates within a step are committed simultaneously (`updates: simultaneous`).".to_owned(),
            "The right-hand side of an update reads the pre-transition state (`reads: pre_state`).".to_owned(),
            "A step that violates a postcondition, a state invariant, or a transition rule is not committed; the state remains as it was before the transition (`failed_step: rollback`).".to_owned(),
            "The fairness assumption is weak fairness (`fairness: weak`) and applies only to operations declared `fair`.".to_owned(),
        ],
    };
    let list = items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{head}\n\n{intro}\n\n{list}")
}

fn requirements_section(claims: &RequirementClaimSet, ctx: &mut Ctx<'_>) -> String {
    let head = heading(ctx.locale, "##", "要件", "Requirements");
    if claims.requirements.is_empty() {
        let empty = match ctx.locale {
            Locale::Ja => "本仕様に要件 ID の宣言はない。",
            Locale::En => "This specification declares no requirement IDs.",
        };
        return format!("{head}\n\n{empty}");
    }
    let mut body = Vec::new();
    for requirement in &claims.requirements {
        body.push(requirement_block(requirement, claims, ctx));
    }
    format!("{head}\n\n{}", body.join("\n\n"))
}

fn requirement_block(
    requirement: &crate::document::Requirement,
    claims: &RequirementClaimSet,
    ctx: &mut Ctx<'_>,
) -> String {
    let head = format!("### {}", requirement.id);
    let original_head = heading(
        ctx.locale,
        "**",
        "要件原文（意図。形式意味との一致は人間が確認する）**",
        "Original requirement text (intent; a human confirms that it matches the formalized meaning)**",
    );
    let original_head = format!("**{}", original_head.trim_start_matches("** "));
    let statements = requirement
        .statements
        .iter()
        .map(|statement| {
            let text = statement.text.as_deref().unwrap_or("");
            let quote = if statement.text.is_some() {
                format!("> {text}")
            } else {
                match ctx.locale {
                    Locale::Ja => "> （原文は記載されていない）".to_owned(),
                    Locale::En => "> (No original text is recorded.)".to_owned(),
                }
            };
            match &statement.source {
                Some(source) => match ctx.locale {
                    Locale::Ja => format!(
                        "{quote}\n\n（出典: `{}:{}`）",
                        source.path.as_deref().unwrap_or(""),
                        source.line
                    ),
                    Locale::En => format!(
                        "{quote}\n\n(Source: `{}:{}`)",
                        source.path.as_deref().unwrap_or(""),
                        source.line
                    ),
                },
                None => quote,
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let formal_head = heading(
        ctx.locale,
        "**",
        "形式化された意味（FSLから決定論的に生成）**",
        "Formalized meaning (generated deterministically from the FSL)**",
    );
    let formal_head = format!("**{}", formal_head.trim_start_matches("** "));
    let claim_blocks = requirement
        .claim_ids
        .iter()
        .filter_map(|claim_id| claims.claims.iter().find(|claim| &claim.id == claim_id))
        .map(|claim| render_claim(claim, ctx))
        .collect::<Vec<_>>()
        .join("\n\n");

    format!("{head}\n\n{original_head}\n\n{statements}\n\n{formal_head}\n\n{claim_blocks}")
}

fn unattributed_section(claims: &RequirementClaimSet, ctx: &mut Ctx<'_>) -> String {
    let head = heading(
        ctx.locale,
        "##",
        "要件 ID に紐づかない形式要素",
        "Formal elements not linked to a requirement ID",
    );
    let unattributed = claims
        .claims
        .iter()
        .filter(|claim| claim.requirements.is_empty())
        .collect::<Vec<_>>();
    if unattributed.is_empty() {
        let empty = match ctx.locale {
            Locale::Ja => "該当なし。",
            Locale::En => "None.",
        };
        return format!("{head}\n\n{empty}");
    }
    let intro = match ctx.locale {
        Locale::Ja => {
            "次の形式要素は要件 ID に紐づけられていないが、本仕様の一部として検査される。"
        }
        Locale::En => {
            "The following formal elements are not linked to any requirement ID, but are checked as part of this specification."
        }
    };
    let blocks = unattributed
        .into_iter()
        .map(|claim| render_claim(claim, ctx))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{head}\n\n{intro}\n\n{blocks}")
}

fn undecided_section(claims: &RequirementClaimSet, locale: Locale) -> String {
    let head = heading(locale, "##", "未決定事項", "Undecided items");
    let intro = match locale {
        Locale::Ja => {
            "次の事項は未決定であり、検証条件ではない。本仕様の検証結果は、これらの事項について何も保証しない。"
        }
        Locale::En => {
            "The following items are undecided and are not verification conditions. The verification results for this specification guarantee nothing about them."
        }
    };
    if claims.undecided.is_empty() {
        let empty = match locale {
            Locale::Ja => "未決定として宣言された事項はない。",
            Locale::En => "No items are declared undecided.",
        };
        return format!("{head}\n\n{empty}");
    }
    let blocks = claims
        .undecided
        .iter()
        .map(|item| {
            let reason = if item.reason.is_empty() {
                match locale {
                    Locale::Ja => "（理由は記載されていない）".to_owned(),
                    Locale::En => "(No reason is recorded.)".to_owned(),
                }
            } else {
                item.reason.clone()
            };
            let requirement_ids = if item.requirement_ids.is_empty() {
                match locale {
                    Locale::Ja => "（関連付けられた要件 ID はない）".to_owned(),
                    Locale::En => "(No requirement IDs are linked.)".to_owned(),
                }
            } else {
                item.requirement_ids.join(", ")
            };
            let (decl_label, reason_label, req_label, source_label) = match locale {
                Locale::Ja => ("宣言", "理由", "関連する要件", "出典"),
                Locale::En => ("Declaration", "Reason", "Related requirements", "Source"),
            };
            let source_line = item.source.as_ref().map(|source| {
                format!(
                    "\n- {source_label}: `{}:{}`",
                    source.path.as_deref().unwrap_or(""),
                    source.line
                )
            });
            format!(
                "### `{}`\n\n- {decl_label}: `{}`\n- {reason_label}: {reason}\n- {req_label}: {requirement_ids}{}",
                item.target,
                item.declaration,
                source_line.unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{head}\n\n{intro}\n\n{blocks}")
}

/// `verify { values N = lo..hi }` bounds are almost always plain integer
/// literals (normalized AST shape `["num", N]`); show the bare number rather
/// than the raw AST JSON. Anything else (e.g. a const reference) falls back
/// to a compact JSON dump rather than fabricating a number.
fn ast_bound_text(value: &serde_json::Value) -> String {
    match value.as_array().map(Vec::as_slice) {
        Some([tag, number]) if tag.as_str() == Some("num") => number.to_string(),
        _ => value.to_string(),
    }
}

fn analysis_scope_section(claims: &RequirementClaimSet, locale: Locale) -> String {
    let head = heading(locale, "##", "解析スコープ", "Analysis scope");
    let intro = match locale {
        Locale::Ja => {
            "検証は次の範囲で行われる。これは解析のための範囲であり、実運用上の上限や容量を意味しない。"
        }
        Locale::En => {
            "Verification is performed within the following bounds. These are analysis bounds; they do not represent operational limits or system capacity."
        }
    };
    let has_instances = !claims.analysis_scope.instances.is_empty();
    let has_values = !claims.analysis_scope.values.is_empty();
    if !has_instances && !has_values {
        let empty = match locale {
            Locale::Ja => "本仕様に解析スコープの宣言（instances / values）はない。",
            Locale::En => {
                "This specification declares no analysis-scope bounds (instances / values)."
            }
        };
        return format!("{head}\n\n{empty}");
    }
    let mut items = Vec::new();
    for instance in &claims.analysis_scope.instances {
        let entity = instance["entity"].as_str().unwrap_or_default();
        let count = instance["count"].as_i64().unwrap_or_default();
        items.push(match locale {
            Locale::Ja => format!("エンティティ `{entity}` の解析インスタンス数: {count}"),
            Locale::En => format!("Analysis instance count of entity `{entity}`: {count}"),
        });
    }
    for value in &claims.analysis_scope.values {
        let number = value["number"].as_str().unwrap_or_default();
        let lo = ast_bound_text(&value["lo"]);
        let hi = ast_bound_text(&value["hi"]);
        items.push(match locale {
            Locale::Ja => format!("数値 `{number}` の解析値域: `{lo}` から `{hi}` まで"),
            Locale::En => format!("Analysis range of number `{number}`: `{lo}` to `{hi}`"),
        });
    }
    let list = items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{head}\n\n{intro}\n\n{list}")
}

/// A reference section listing every accepted glossary label (issue #330),
/// sorted by target. This is the only place a `state:`/`enum:` label is
/// ever shown — v1 does not substitute a label inside rendered expression
/// text (see `docs/DESIGN-document-glossary.md`); an `action:` label is
/// additionally shown at the action's own claim heading
/// (`render_operation`/`metadata_header`). Returns `None` when no glossary
/// was applied, so a glossary-less document renders byte-identically to
/// before this issue.
fn glossary_section(
    glossary: Option<&crate::document_glossary::Glossary>,
    locale: Locale,
) -> Option<String> {
    let glossary = glossary?;
    let head = heading(locale, "##", "用語集", "Glossary");
    let intro = match locale {
        Locale::Ja => {
            "本文中の識別子表示は次の用語集ラベルに基づく。ラベルは表示のみを変更し、検証条件を変更しない。"
        }
        Locale::En => {
            "Identifier display throughout this document follows the glossary labels below. \
             A label changes only display, never a verification condition."
        }
    };
    let rows = glossary
        .labels
        .iter()
        .map(|(target, label)| {
            format!(
                "- `{target}`: {}",
                crate::document_glossary::markdown_label(label)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!("{head}\n\n{intro}\n\n{rows}"))
}

fn generation_section(claims: &RequirementClaimSet, fallback_count: u64, locale: Locale) -> String {
    let head = heading(locale, "##", "生成情報", "Generation info");
    let source_line = match &claims.spec.source {
        Some(source) => format!("`{source}`"),
        None => match locale {
            Locale::Ja => "（ソースパスなし）".to_owned(),
            Locale::En => "(no source path)".to_owned(),
        },
    };
    let mut lines = Vec::new();
    lines.push(match locale {
        Locale::Ja => format!(
            "生成元仕様: {source_line}（`{}`、dialect: `{}`）",
            claims.spec.name, claims.spec.dialect
        ),
        Locale::En => format!(
            "Source specification: {source_line} (`{}`, dialect: `{}`)",
            claims.spec.name, claims.spec.dialect
        ),
    });
    lines.push(format!("spec digest: `{}`", claims.spec.spec_digest));
    lines.push(format!(
        "claim set digest: `{}`",
        claims.spec.claim_set_digest
    ));
    lines.push(match locale {
        Locale::Ja => format!(
            "形式要素の分類: rendered {} 件 / unattributed {} 件 / unsupported {} 件",
            claims.coverage.counts.rendered,
            claims.coverage.counts.unattributed,
            claims.coverage.counts.unsupported
        ),
        Locale::En => format!(
            "Classification of formal elements: {} rendered / {} unattributed / {} unsupported",
            claims.coverage.counts.rendered,
            claims.coverage.counts.unattributed,
            claims.coverage.counts.unsupported
        ),
    });
    lines.push(match locale {
        Locale::Ja => format!("自然言語への言い換えを行わなかった式: {fallback_count} 箇所"),
        Locale::En => format!("Expressions left unparaphrased: {fallback_count}"),
    });
    if !matches!(
        claims.provenance.completeness,
        crate::document::Completeness::Complete
    ) {
        lines.push(match locale {
            Locale::Ja => format!(
                "由来情報は不完全である（completeness: `{:?}`）。一部の要素について、FSL ソース上の出所を特定できていない。",
                claims.provenance.completeness
            ),
            Locale::En => format!(
                "Provenance information is incomplete (completeness: `{:?}`). For some elements, the source location in the FSL could not be determined.",
                claims.provenance.completeness
            ),
        });
    }
    if fallback_count > 0 {
        lines.push(match locale {
            Locale::Ja => format!(
                "上記 {fallback_count} 箇所では、誤解を招く言い換えを避けるため、自然言語文の代わりに FSL の canonical 形式をそのまま示した。これは本レンダラーの仕様どおりの動作であり、情報の欠落や生成の失敗ではない。"
            ),
            Locale::En => format!(
                "In the {fallback_count} places counted above, the canonical FSL form is shown as-is instead of a natural-language paraphrase, to avoid a misleading rendering. This is the specified behavior of this renderer; it is not missing information or a generation failure."
            ),
        });
    }
    if !claims.coverage.unsupported.is_empty() {
        let intro = match locale {
            Locale::Ja => {
                "次の形式要素は RCIR v1 が対応していないため、本書には規範文として現れない。省略は明示され、黙って落とされることはない。"
            }
            Locale::En => {
                "The following formal elements are not supported by RCIR v1 and therefore do not appear as normative text in this document. The omission is explicit; nothing is dropped silently."
            }
        };
        lines.push(intro.to_owned());
        for entry in &claims.coverage.unsupported {
            lines.push(format!("- `{}`: {}", entry.target, entry.reason));
        }
    }
    let list = lines
        .iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>();
    // The unsupported intro/list items above are not `- ` prefixed uniformly;
    // rebuild with consistent bullet formatting.
    let _ = list;
    let mut out = String::from(&head);
    out.push_str("\n\n");
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if line.starts_with('-') {
            out.push_str(line);
        } else {
            let _ = write!(out, "- {line}");
        }
    }
    out
}

// --- Claim rendering ---------------------------------------------------------

fn render_claim(claim: &Claim, ctx: &mut Ctx<'_>) -> String {
    if ctx.rendered_claims.contains(&claim.id) {
        return back_reference(claim, ctx.locale);
    }
    ctx.rendered_claims.insert(claim.id.clone());

    let body = match claim.kind {
        ClaimKind::Operation => render_operation(claim, ctx),
        ClaimKind::StateRule => render_state_rule(claim, ctx),
        ClaimKind::TransitionRule => render_transition_rule(claim, ctx),
        ClaimKind::ProgressRule => render_progress_rule(claim, ctx),
        ClaimKind::ReachabilityGoal => render_reachability_goal(claim, ctx),
        ClaimKind::AcceptanceTrace => render_trace(claim, ctx, true),
        ClaimKind::ForbiddenTrace => render_trace(claim, ctx, false),
        ClaimKind::DeadlineRule => render_deadline_rule(claim, ctx),
        ClaimKind::TerminalRule => render_terminal_rule(claim, ctx),
    };
    crate::document_markers::wrap_claim_block(&claim.id, &body)
}

fn kind_label(kind: ClaimKind, locale: Locale) -> &'static str {
    match (kind, locale) {
        (ClaimKind::Operation, Locale::Ja) => "操作",
        (ClaimKind::Operation, Locale::En) => "Operation",
        (ClaimKind::StateRule, Locale::Ja) => "状態不変条件",
        (ClaimKind::StateRule, Locale::En) => "State invariant",
        (ClaimKind::TransitionRule, Locale::Ja) => "遷移条件",
        (ClaimKind::TransitionRule, Locale::En) => "Transition rule",
        (ClaimKind::ProgressRule, Locale::Ja) => "進行条件",
        (ClaimKind::ProgressRule, Locale::En) => "Progress rule",
        (ClaimKind::ReachabilityGoal, Locale::Ja) => "到達目標",
        (ClaimKind::ReachabilityGoal, Locale::En) => "Reachability goal",
        (ClaimKind::AcceptanceTrace, Locale::Ja) => "受け入れ基準",
        (ClaimKind::AcceptanceTrace, Locale::En) => "Acceptance criterion",
        (ClaimKind::ForbiddenTrace, Locale::Ja) => "禁止手順",
        (ClaimKind::ForbiddenTrace, Locale::En) => "Forbidden sequence",
        (ClaimKind::DeadlineRule, Locale::Ja) => "期限条件",
        (ClaimKind::DeadlineRule, Locale::En) => "Deadline rule",
        (ClaimKind::TerminalRule, Locale::Ja) => "終端条件",
        (ClaimKind::TerminalRule, Locale::En) => "Terminal condition",
    }
}

fn back_reference(claim: &Claim, locale: Locale) -> String {
    let first_req = claim.requirements.first().cloned().unwrap_or_default();
    let label = kind_label(claim.kind, locale);
    match locale {
        Locale::Ja => format!(
            "この{label}の内容は、`{first_req}` の節に記載している。この要件にも同じ意味で適用される。"
        ),
        Locale::En => format!(
            "The content of this {} is given in the section for `{first_req}`. It applies to this requirement with the same meaning.",
            label.to_lowercase()
        ),
    }
}

/// `glossary_label`, when present, is an accepted glossary label for this
/// claim's own subject (issue #330) — shown alongside, never instead of, the
/// canonical identifier, so the heading stays a valid cross-reference back
/// to the FSL source no matter what a sidecar file says.
fn metadata_header(
    claim: &Claim,
    display: &str,
    glossary_label: Option<&str>,
    locale: Locale,
) -> String {
    let kind = kind_label(claim.kind, locale);
    let heading = match glossary_label {
        Some(label) => {
            let label = crate::document_glossary::markdown_label(label);
            match locale {
                Locale::Ja => format!("#### {kind}: {label}（`{display}`）"),
                Locale::En => format!("#### {kind}: {label} (`{display}`)"),
            }
        }
        None => format!("#### {kind}: `{display}`"),
    };
    let mut lines = vec![heading, String::new()];
    let (id_label, source_label) = match locale {
        Locale::Ja => ("識別子", "出典"),
        Locale::En => ("Identifier", "Source"),
    };
    lines.push(format!("- {id_label}: `{}`", claim.id));
    if let Some(source) = &claim.source {
        lines.push(format!(
            "- {source_label}: `{}:{}`",
            source.path.as_deref().unwrap_or(""),
            source.line
        ));
    }
    if !matches!(
        claim.provenance.assurance,
        crate::document::ProvenanceAssurance::SourceBacked
    ) {
        lines.push(provenance_note(claim, locale));
    }
    lines.join("\n")
}

fn provenance_note(claim: &Claim, locale: Locale) -> String {
    use crate::document::ProvenanceAssurance::{GeneratedFromSource, GeneratedOnly, Unknown};
    let assurance = claim.provenance.assurance;
    match (assurance, locale) {
        (GeneratedFromSource | GeneratedOnly, Locale::Ja) => format!(
            "- 由来: FSL ソースに直接対応する記述がなく、lowering により生成された要素である（assurance: `{}`）。",
            assurance.as_str()
        ),
        (GeneratedFromSource | GeneratedOnly, Locale::En) => format!(
            "- Provenance: this element does not correspond directly to FSL source text; it was produced by lowering (assurance: `{}`).",
            assurance.as_str()
        ),
        (Unknown, Locale::Ja) => {
            "- 由来: この要素の出所を特定できなかった（assurance: `unknown`）。".to_owned()
        }
        (Unknown, Locale::En) => {
            "- Provenance: the origin of this element could not be determined (assurance: `unknown`).".to_owned()
        }
        (_, _) => String::new(),
    }
}

fn condition_block(expr: &fsl_core::KernelExpr, ctx: &mut Ctx<'_>) -> String {
    let rendered = document_render_expr::render_condition(expr, ctx.model, ctx.locale);
    if rendered.used_fallback {
        ctx.fallback_count += 1;
    }
    rendered.text
}

fn param_text(param: &ParamDef) -> String {
    match param {
        ParamDef::Typed { name, ty } => format!("{name}: {}", type_ref_text(ty)),
        ParamDef::Range { name, lo, hi } => format!("{name}: {lo}..{hi}"),
    }
}

fn type_ref_text(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Int => "Int".to_owned(),
        TypeRef::Bool => "Bool".to_owned(),
        TypeRef::Named(name) => display_name(name),
        TypeRef::Range(lo, hi) => format!("{lo}..{hi}"),
        TypeRef::Map(key, value) => {
            format!("Map<{}, {}>", type_ref_text(key), type_ref_text(value))
        }
        TypeRef::Relation(left, right) => {
            format!(
                "Relation<{}, {}>",
                type_ref_text(left),
                type_ref_text(right)
            )
        }
        TypeRef::Set(element) => format!("Set<{}>", type_ref_text(element)),
        TypeRef::Seq(element, bound) => format!("Seq<{}, {bound}>", type_ref_text(element)),
        TypeRef::Option(element) => format!("Option<{}>", type_ref_text(element)),
    }
}

#[allow(clippy::too_many_lines)]
fn render_operation(claim: &Claim, ctx: &mut Ctx<'_>) -> String {
    let name = claim.subject["action"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let label = ctx
        .glossary
        .and_then(|glossary| glossary.labels.get(&action_target(&name)))
        .map(String::as_str);
    let Some(action) = ctx.model.actions.iter().find(|action| action.name == name) else {
        return metadata_header(claim, &display_name(&name), label, ctx.locale);
    };
    let mut out = metadata_header(claim, &display_name(&name), label, ctx.locale);

    let params_label = match ctx.locale {
        Locale::Ja => "パラメータ",
        Locale::En => "Parameters",
    };
    let params_text = if action.params.is_empty() {
        match ctx.locale {
            Locale::Ja => "なし".to_owned(),
            Locale::En => "none".to_owned(),
        }
    } else {
        action
            .params
            .iter()
            .map(|param| format!("`{}`", param_text(param)))
            .collect::<Vec<_>>()
            .join(match ctx.locale {
                Locale::Ja => "、",
                Locale::En => ", ",
            })
    };
    let _ = write!(out, "\n- {params_label}: {params_text}");

    let requires_count = action
        .guards
        .iter()
        .filter(|guard| matches!(guard, ActionGuard::Requires(_)))
        .count();
    let intro = match (requires_count, ctx.locale) {
        (0, Locale::Ja) => format!(
            "操作 `{}` は常に実行できる（enablement 条件は宣言されていない）。",
            display_name(&name)
        ),
        (0, Locale::En) => format!(
            "Action `{}` is always enabled (no enablement conditions are declared).",
            display_name(&name)
        ),
        (1, Locale::Ja) => format!(
            "操作 `{}` を実行できるのは、次の条件を満たす場合に限る。",
            display_name(&name)
        ),
        (1, Locale::En) => format!(
            "Action `{}` can be executed only when the following condition holds.",
            display_name(&name)
        ),
        (_, Locale::Ja) => format!(
            "操作 `{}` を実行できるのは、次の条件をすべて満たす場合に限る。",
            display_name(&name)
        ),
        (_, Locale::En) => format!(
            "Action `{}` can be executed only when all of the following conditions hold.",
            display_name(&name)
        ),
    };
    let _ = write!(out, "\n\n{intro}");
    if requires_count == 0 && !action.guards.is_empty() {
        out.push_str(&match ctx.locale {
            Locale::Ja => "\n\n次の定義を用いる。".to_owned(),
            Locale::En => "\n\nThe following definitions are used.".to_owned(),
        });
    }
    if !action.guards.is_empty() {
        let items = action
            .guards
            .iter()
            .map(|guard| match guard {
                ActionGuard::Requires(expr) => {
                    if let Some(text) =
                        document_render_expr::render_inline(expr, ctx.model, ctx.locale)
                    {
                        text
                    } else {
                        ctx.fallback_count += 1;
                        document_render_expr::fallback_list_item(expr, ctx.model, ctx.locale, 0)
                    }
                }
                ActionGuard::Let(name, expr) => {
                    let expr_text = fsl_core::source_expr_text(ctx.model, expr);
                    match ctx.locale {
                        Locale::Ja => format!("（定義）`{name}` を `{expr_text}` とする"),
                        Locale::En => format!("(Definition) Let `{name}` be `{expr_text}`"),
                    }
                }
            })
            .enumerate()
            .map(|(index, text)| format!("{}. {text}{}", index + 1, guard_stop(ctx.locale)))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = write!(out, "\n\n{items}");
    }

    let effects_intro = if action.statements.is_empty() {
        match ctx.locale {
            Locale::Ja => "この操作は状態を変更しない。".to_owned(),
            Locale::En => "This action does not modify the state.".to_owned(),
        }
    } else {
        match ctx.locale {
            Locale::Ja => "操作が成功した場合、次の更新を同一ステップで同時に適用する。更新の右辺は遷移前の状態を読む。".to_owned(),
            Locale::En => "When the action succeeds, the following updates are applied simultaneously within a single step. The right-hand sides read the pre-transition state.".to_owned(),
        }
    };
    let _ = write!(out, "\n\n{effects_intro}");
    if !action.statements.is_empty() {
        let items = action
            .statements
            .iter()
            .enumerate()
            .map(|(index, statement)| format!("{}. {}", index + 1, statement_text(statement, ctx)))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = write!(out, "\n\n{items}");
    }

    if !action.ensures.is_empty() {
        let intro = match ctx.locale {
            Locale::Ja => "操作の完了時には、次の事後条件が成立しなければならない。成立しない場合、このステップの更新は一切コミットされず、状態は遷移前のまま残る（ロールバック）。".to_owned(),
            Locale::En => "When the action completes, the following postconditions must hold. If any of them does not hold, none of this step's updates are committed and the state remains as it was before the transition (rollback).".to_owned(),
        };
        let items = action
            .ensures
            .iter()
            .enumerate()
            .map(|(index, expr)| {
                let text = document_render_expr::render_inline(expr, ctx.model, ctx.locale)
                    .unwrap_or_else(|| {
                        ctx.fallback_count += 1;
                        format!("`{}`", fsl_core::source_expr_text(ctx.model, expr))
                    });
                format!("{}. {text}{}", index + 1, guard_stop(ctx.locale))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = write!(out, "\n\n{intro}\n\n{items}");
    }

    let fairness = if action.fair {
        match ctx.locale {
            Locale::Ja => {
                "この操作には弱い公平性（weak fairness）を仮定する。これはスケジューリング上の仮定であり、この操作が実行可能（enabled）であり続けるならば、いつかは実行される、という意味である。直ちに実行されることを意味しない。"
            }
            Locale::En => {
                "Weak fairness is assumed for this action. This is a scheduling assumption: if the action remains continuously enabled, it is eventually executed. It does not mean that the action is executed immediately."
            }
        }
    } else {
        match ctx.locale {
            Locale::Ja => {
                "この操作に公平性の仮定はない。実行可能（enabled）であっても、実行されることは保証されない。"
            }
            Locale::En => {
                "No fairness is assumed for this action. Even when it is enabled, it is not guaranteed to be executed."
            }
        }
    };
    let _ = write!(out, "\n\n{fairness}");
    out
}

fn guard_stop(locale: Locale) -> &'static str {
    match locale {
        Locale::Ja => "。",
        Locale::En => ".",
    }
}

fn statement_text(statement: &fsl_core::KernelStatement, ctx: &mut Ctx<'_>) -> String {
    use fsl_core::KernelStatement as Statement;
    match statement {
        Statement::Assign { target, value, .. } => {
            let lvalue_text = lvalue_text(target, ctx.model);
            render_assign(&lvalue_text, value, ctx)
        }
        Statement::If {
            condition,
            then_statements,
            ..
        } => {
            let condition_text =
                document_render_expr::render_inline(condition, ctx.model, ctx.locale);
            let nested = nested_statement_text(then_statements, ctx);
            if let Some(condition_text) = condition_text {
                let intro = match ctx.locale {
                    Locale::Ja => format!("「{condition_text}」の場合に限り、次を適用する。"),
                    Locale::En => format!("Only if \"{condition_text}\", apply the following."),
                };
                format!("{intro}\n\n{nested}")
            } else {
                let condition =
                    document_render_expr::render_condition(condition, ctx.model, ctx.locale);
                ctx.fallback_count += u64::from(condition.used_fallback);
                let intro = match ctx.locale {
                    Locale::Ja => "次の条件が成立する場合に限り、次を適用する。",
                    Locale::En => "Only if the following condition holds, apply the following.",
                };
                format!("{intro}\n\n{}\n\n{nested}", condition.text)
            }
        }
        Statement::ForAll {
            binder, statements, ..
        } => {
            let binder_text = fsl_core::source_binder_text(ctx.model, binder);
            let intro = match ctx.locale {
                Locale::Ja => format!("すべての `{binder_text}` について、次を適用する。"),
                Locale::En => format!("For every `{binder_text}`, apply the following."),
            };
            let nested = nested_statement_text(statements, ctx);
            format!("{intro}\n\n{nested}")
        }
    }
}

fn nested_statement_text(statements: &[fsl_core::KernelStatement], ctx: &mut Ctx<'_>) -> String {
    statements
        .iter()
        .enumerate()
        .map(|(index, statement)| format!("   {}. {}", index + 1, statement_text(statement, ctx)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_assign(lvalue_text: &str, value: &fsl_core::KernelExpr, ctx: &mut Ctx<'_>) -> String {
    use fsl_core::KernelExpr as Expr;
    if let Expr::Struct { fields, .. } = value {
        let mut sorted = fields.iter().collect::<Vec<_>>();
        sorted.sort_by_key(|(name, _)| name.as_str());
        let struct_text = fsl_core::source_expr_text(ctx.model, value);
        let count = sorted.len();
        let field_updates = sorted
            .iter()
            .enumerate()
            .map(|(index, (field, field_value))| {
                let field_value_text = fsl_core::source_expr_text(ctx.model, field_value);
                match ctx.locale {
                    Locale::Ja => {
                        let suffix = if index + 1 == count {
                            "にする"
                        } else {
                            "に"
                        };
                        format!("`{lvalue_text}.{field}` を `{field_value_text}` {suffix}")
                    }
                    Locale::En => format!("`{lvalue_text}.{field}` to `{field_value_text}`"),
                }
            })
            .collect::<Vec<_>>();
        return match ctx.locale {
            Locale::Ja => format!(
                "`{lvalue_text}` を `{struct_text}` に置き換える。すなわち、{}。",
                field_updates.join("、")
            ),
            Locale::En => format!(
                "Replace `{lvalue_text}` with `{struct_text}`; that is, set {}.",
                join_en_set(&field_updates)
            ),
        };
    }
    let value_text = fsl_core::source_expr_text(ctx.model, value);
    match ctx.locale {
        Locale::Ja => format!("`{lvalue_text}` を `{value_text}` にする。"),
        Locale::En => format!("Set `{lvalue_text}` to `{value_text}`."),
    }
}

fn join_en_set(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [a, b] => format!("{a} and {b}"),
        _ => {
            let (last, head) = items.split_last().expect("non-empty");
            format!("{} and {last}", head.join(", "))
        }
    }
}

fn lvalue_text(lvalue: &fsl_core::KernelLValue, model: &KernelModel) -> String {
    use fsl_core::KernelLValue as LValue;
    match lvalue {
        LValue::Var(name) => name.clone(),
        LValue::Index(name, index) => {
            format!("{name}[{}]", fsl_core::source_expr_text(model, index))
        }
        LValue::Field(base, field) => format!("{}.{field}", lvalue_text(base, model)),
    }
}

fn render_state_rule(claim: &Claim, ctx: &mut Ctx<'_>) -> String {
    let name = claim.subject["property"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let Some(property) = find_property(&ctx.model.invariants, &name) else {
        return metadata_header(claim, &display_name(&name), None, ctx.locale);
    };
    let head = metadata_header(claim, &display_name(&name), None, ctx.locale);
    let lead = match ctx.locale {
        Locale::Ja => {
            "初期化後、および成功した各操作のコミット後に、次の条件が成立しなければならない。"
        }
        Locale::En => {
            "After initialization, and after each successful operation commits, the following condition must hold."
        }
    };
    let block = condition_block(&property.expr, ctx);
    let tail = match ctx.locale {
        Locale::Ja => {
            "この条件を満たさない候補遷移はコミットされない。条件が自動的に修復・回復されることを意味しない。"
        }
        Locale::En => {
            "A candidate transition that does not satisfy this condition is not committed. This does not mean that the condition is automatically repaired or restored."
        }
    };
    format!("{head}\n\n{lead}\n\n{block}\n\n{tail}")
}

fn render_transition_rule(claim: &Claim, ctx: &mut Ctx<'_>) -> String {
    let name = claim.subject["property"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let Some(property) = find_property(&ctx.model.transitions, &name) else {
        return metadata_header(claim, &display_name(&name), None, ctx.locale);
    };
    let head = metadata_header(claim, &display_name(&name), None, ctx.locale);
    let lead = match ctx.locale {
        Locale::Ja => {
            "成功する各遷移について、遷移前の状態と遷移後の状態は次の関係を満たさなければならない。以下で「遷移前の `x`」は遷移前の値を指し、それ以外の読み取りは遷移後の値を指す。"
        }
        Locale::En => {
            "For each successful transition, the pre-transition state and the post-transition state must satisfy the following relation. Below, \"the pre-transition value of `x`\" refers to the value before the transition; every other read refers to the value after the transition."
        }
    };
    let block = condition_block(&property.expr, ctx);
    let tail = match ctx.locale {
        Locale::Ja => "この関係を満たさない候補遷移はコミットされない。",
        Locale::En => {
            "A candidate transition that does not satisfy this relation is not committed."
        }
    };
    format!("{head}\n\n{lead}\n\n{block}\n\n{tail}")
}

fn render_reachability_goal(claim: &Claim, ctx: &mut Ctx<'_>) -> String {
    let name = claim.subject["property"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let Some(property) = find_property(&ctx.model.reachables, &name) else {
        return metadata_header(claim, &display_name(&name), None, ctx.locale);
    };
    let head = metadata_header(claim, &display_name(&name), None, ctx.locale);
    let lead = match ctx.locale {
        Locale::Ja => "次の状態に到達する実行例が存在しなければならない（到達目標）。",
        Locale::En => {
            "There must exist an execution that reaches a state satisfying the following condition (reachability goal)."
        }
    };
    let block = condition_block(&property.expr, ctx);
    let tail = match ctx.locale {
        Locale::Ja => {
            "これは「少なくとも一つの実行が存在する」ことを求める到達目標であり、すべての状態での成立を求める不変条件ではない。\n\n- 検証状態: 本書は検証結果を含まない。到達が確認済みであることを意味しない。"
        }
        Locale::En => {
            "This is a reachability goal \u{2014} it demands that at least one such execution exists. It is not an invariant that must hold in every state.\n\n- Verification status: this document contains no verification results. It does not mean that reachability has been confirmed."
        }
    };
    format!("{head}\n\n{lead}\n\n{block}\n\n{tail}")
}

fn render_deadline_rule(claim: &Claim, ctx: &mut Ctx<'_>) -> String {
    let name = claim.subject["property"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let Some(property) = find_property(&ctx.model.invariants, &name) else {
        return metadata_header(claim, &display_name(&name), None, ctx.locale);
    };
    let head = metadata_header(claim, &display_name(&name), None, ctx.locale);
    let lead = match ctx.locale {
        Locale::Ja => {
            "次の条件は期限条件（deadline）である。初期化後、および成功した各操作のコミット後に成立しなければならない。"
        }
        Locale::En => {
            "The following condition is a deadline rule. It must hold after initialization and after each successful operation commits."
        }
    };
    let block = condition_block(&property.expr, ctx);
    let tail = match ctx.locale {
        Locale::Ja => {
            "対象の状態が期限を超えて継続する実行は、この条件に違反する。これは不変条件として検査される安全条件であり、進行条件（leadsTo）の liveness とは異なる。この条件を満たさない候補遷移はコミットされない。"
        }
        Locale::En => {
            "An execution in which the target state persists beyond the deadline violates this condition. This is a safety condition checked as an invariant; it is distinct from the liveness of a progress rule (leadsTo). A candidate transition that does not satisfy this condition is not committed."
        }
    };
    format!("{head}\n\n{lead}\n\n{block}\n\n{tail}")
}

fn render_terminal_rule(claim: &Claim, ctx: &mut Ctx<'_>) -> String {
    let Some(expr) = ctx.model.terminal.clone() else {
        return metadata_header(claim, "", None, ctx.locale);
    };
    let head = format!(
        "#### {}",
        match ctx.locale {
            Locale::Ja => "終端条件",
            Locale::En => "Terminal condition",
        }
    );
    let mut lines = vec![head, String::new()];
    let source_label = match ctx.locale {
        Locale::Ja => "識別子",
        Locale::En => "Identifier",
    };
    lines.push(format!("- {source_label}: `{}`", claim.id));
    let head = lines.join("\n");
    let lead = match ctx.locale {
        Locale::Ja => "次の条件を満たす状態は、意図された終端状態（terminal）である。",
        Locale::En => "A state satisfying the following condition is an intended terminal state.",
    };
    let block = condition_block(&expr, ctx);
    let tail = match ctx.locale {
        Locale::Ja => {
            "終端状態では、それ以上操作を進めないことが意図されている。これは到達を要求する条件ではない。デッドロック検査において「意図された停止」を「意図しない停止」から区別するための宣言である。"
        }
        Locale::En => {
            "In a terminal state, no further operations are intended to proceed. This is not a reachability demand. It is a declaration that lets deadlock checking distinguish an intended stop from an unintended one."
        }
    };
    format!("{head}\n\n{lead}\n\n{block}\n\n{tail}")
}

fn render_progress_rule(claim: &Claim, ctx: &mut Ctx<'_>) -> String {
    let name = claim.subject["property"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let Some(property) = ctx.model.leadstos.iter().find(|p| p.name == name).cloned() else {
        return metadata_header(claim, &display_name(&name), None, ctx.locale);
    };
    render_leadsto(claim, &property, ctx)
}

#[allow(clippy::too_many_lines)]
fn render_leadsto(claim: &Claim, property: &LeadsToDef, ctx: &mut Ctx<'_>) -> String {
    let head = metadata_header(claim, &display_name(&property.name), None, ctx.locale);
    let lead = match ctx.locale {
        Locale::Ja => "次の進行条件（liveness）が、FSL 上の要求として宣言されている。",
        Locale::En => "The following progress rule (liveness) is declared as a demand in the FSL.",
    };
    let mut lines = Vec::new();
    if !property.binders.is_empty() {
        let binders = property
            .binders
            .iter()
            .map(|binder| format!("`{}`", fsl_core::source_binder_text(ctx.model, binder)))
            .collect::<Vec<_>>()
            .join(match ctx.locale {
                Locale::Ja => "、",
                Locale::En => ", ",
            });
        lines.push(match ctx.locale {
            Locale::Ja => format!("- 対象: すべての {binders} について。"),
            Locale::En => format!("- Scope: for every {binders}."),
        });
    }
    let before_inline =
        document_render_expr::render_inline(&property.before, ctx.model, ctx.locale);
    if let Some(text) = &before_inline {
        lines.push(match ctx.locale {
            Locale::Ja => format!("- 起点: 「{text}」が成立したとき。"),
            Locale::En => format!("- Trigger: when \"{text}\" holds."),
        });
    } else {
        ctx.fallback_count += 1;
        lines.push(match ctx.locale {
            Locale::Ja => "- 起点: 次の条件が成立したとき。".to_owned(),
            Locale::En => "- Trigger: when the following condition holds.".to_owned(),
        });
        lines.push(String::new());
        lines.push(format!(
            "  ```fsl\n  {}\n  ```",
            fsl_core::source_expr_text(ctx.model, &property.before)
        ));
    }
    let after_inline = document_render_expr::render_inline(&property.after, ctx.model, ctx.locale);
    let within_text = match (property.within, &after_inline) {
        (Some(n), Some(text)) => match ctx.locale {
            Locale::Ja => format!(
                "- 帰結: 起点の成立から {n} ステップ以内に、「{text}」が成立しなければならない。"
            ),
            Locale::En => {
                format!("- Consequence: within {n} steps of the trigger, \"{text}\" must hold.")
            }
        },
        (None, Some(text)) => match ctx.locale {
            Locale::Ja => format!(
                "- 帰結: それ以降のいつかの時点で、「{text}」が成立しなければならない。期限（within）は指定されていない。"
            ),
            Locale::En => format!(
                "- Consequence: at some later point, \"{text}\" must hold. No deadline (within) is specified."
            ),
        },
        (Some(n), None) => {
            ctx.fallback_count += 1;
            match ctx.locale {
                Locale::Ja => format!(
                    "- 帰結: 起点の成立から {n} ステップ以内に、次の条件が成立しなければならない。\n\n  ```fsl\n  {}\n  ```",
                    fsl_core::source_expr_text(ctx.model, &property.after)
                ),
                Locale::En => format!(
                    "- Consequence: within {n} steps of the trigger, the following condition must hold.\n\n  ```fsl\n  {}\n  ```",
                    fsl_core::source_expr_text(ctx.model, &property.after)
                ),
            }
        }
        (None, None) => {
            ctx.fallback_count += 1;
            match ctx.locale {
                Locale::Ja => format!(
                    "- 帰結: それ以降のいつかの時点で、次の条件が成立しなければならない。期限（within）は指定されていない。\n\n  ```fsl\n  {}\n  ```",
                    fsl_core::source_expr_text(ctx.model, &property.after)
                ),
                Locale::En => format!(
                    "- Consequence: at some later point, the following condition must hold. No deadline (within) is specified.\n\n  ```fsl\n  {}\n  ```",
                    fsl_core::source_expr_text(ctx.model, &property.after)
                ),
            }
        }
    };
    lines.push(within_text);
    lines.push(match ctx.locale {
        Locale::Ja => "- 前提: この進行条件の成立は、各操作に宣言された公平性の仮定（弱い公平性）に依存し得る。公平性の宣言は各操作の記述を参照。".to_owned(),
        Locale::En => "- Premise: whether this progress rule holds may depend on the fairness assumptions (weak fairness) declared on individual actions; see each action's description.".to_owned(),
    });
    if let Some(decreases) = &property.decreases {
        let decreases_text = fsl_core::source_expr_text(ctx.model, decreases);
        lines.push(match ctx.locale {
            Locale::Ja => format!("- 進行の根拠: `decreases {decreases_text}`（ランク付き証明のためのメタデータであり、公平性の仮定を追加しない）。"),
            Locale::En => format!("- Rank hint: `decreases {decreases_text}` (metadata for ranked proofs; it adds no fairness assumption)."),
        });
    }
    lines.push(match ctx.locale {
        Locale::Ja => "- 検証状態: 本書は検証結果を含まない。この条件は FSL が要求として宣言しているものであり、成立が確認済みであることを意味しない。".to_owned(),
        Locale::En => "- Verification status: this document contains no verification results. This rule is what the FSL declares as a demand; it does not mean the rule has been established.".to_owned(),
    });
    format!("{head}\n\n{lead}\n\n{}", lines.join("\n"))
}

fn find_property<'a>(properties: &'a [PropertyDef], name: &str) -> Option<&'a PropertyDef> {
    properties.iter().find(|property| property.name == name)
}

fn render_trace(claim: &Claim, ctx: &mut Ctx<'_>, is_acceptance: bool) -> String {
    let case_id = claim.subject["trace_case"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let Some(case) = find_case(ctx.trace, &case_id, is_acceptance) else {
        return metadata_header(claim, &case_id, None, ctx.locale);
    };
    let case = case.clone();
    if is_acceptance {
        render_acceptance(claim, &case, ctx)
    } else {
        render_forbidden(claim, &case, ctx)
    }
}

fn find_case<'a>(
    trace: Option<&'a RequirementsTraceContract>,
    id: &str,
    is_acceptance: bool,
) -> Option<&'a RequirementsTraceCase> {
    let trace = trace?;
    let cases = if is_acceptance {
        &trace.acceptance
    } else {
        &trace.forbidden
    };
    cases.iter().find(|case| case.id == id)
}

fn step_text(model: &KernelModel, name: &str, args: &[fsl_core::KernelExpr]) -> String {
    let args_text = args
        .iter()
        .map(|arg| fsl_core::source_expr_text(model, arg))
        .collect::<Vec<_>>()
        .join(", ");
    format!("`{}({args_text})`", display_name(name))
}

fn render_acceptance(claim: &Claim, case: &RequirementsTraceCase, ctx: &mut Ctx<'_>) -> String {
    let head = metadata_header(claim, &case.id, None, ctx.locale);
    let title_label = match ctx.locale {
        Locale::Ja => "表題",
        Locale::En => "Title",
    };
    let intro = match ctx.locale {
        Locale::Ja => "この受け入れ基準は、一つの具体的な実行例である。",
        Locale::En => "This acceptance criterion is a single concrete execution example.",
    };
    let given = match ctx.locale {
        Locale::Ja => "- 前提（Given）: 初期化直後の状態から開始する。",
        Locale::En => "- Given: start from the state immediately after initialization.",
    };
    let when_label = match ctx.locale {
        Locale::Ja => {
            "- 操作（When）: 次の操作をこの順に実行する。いずれも拒否されずに成功しなければならない。"
        }
        Locale::En => {
            "- When: execute the following operations in this order. Each of them must succeed without being rejected."
        }
    };
    let steps = case
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            format!(
                "  {}. {}",
                index + 1,
                step_text(ctx.model, &step.name, &step.args)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let then_line = expectation_text(case, ctx);
    let tail = match ctx.locale {
        Locale::Ja => {
            "この基準が示すのは、上記の一連の操作が成功し、期待が成立することのみである。同種のすべての入力・順序・状態で同じ結果になることを主張するものではない。"
        }
        Locale::En => {
            "This criterion shows only that the sequence of operations above succeeds and that the expectation holds. It does not claim that every input, ordering, or state of the same kind produces the same result."
        }
    };
    format!(
        "{head}\n- {title_label}: {}\n\n{intro}\n\n{given}\n{when_label}\n{steps}\n{then_line}\n\n{tail}",
        case.text
    )
}

fn render_forbidden(claim: &Claim, case: &RequirementsTraceCase, ctx: &mut Ctx<'_>) -> String {
    let head = metadata_header(claim, &case.id, None, ctx.locale);
    let title_label = match ctx.locale {
        Locale::Ja => "表題",
        Locale::En => "Title",
    };
    let intro = match ctx.locale {
        Locale::Ja => "この禁止手順は、一つの具体的な実行例である。",
        Locale::En => "This forbidden sequence is a single concrete execution example.",
    };
    let given = match ctx.locale {
        Locale::Ja => "- 前提（Given）: 初期化直後の状態から開始する。",
        Locale::En => "- Given: start from the state immediately after initialization.",
    };
    let (prefix, last) = case.steps.split_at(case.steps.len().saturating_sub(1));
    let last_step = last.first();
    let when_line = if prefix.is_empty() {
        match ctx.locale {
            Locale::Ja => "- 先行手順（When）: 先行する操作はない。初期化直後の状態で、次の操作を試みる。".to_owned(),
            Locale::En => "- When (prefix): there are no preceding operations. The following operation is attempted in the state immediately after initialization.".to_owned(),
        }
    } else {
        let steps = prefix
            .iter()
            .enumerate()
            .map(|(index, step)| {
                format!(
                    "  {}. {}",
                    index + 1,
                    step_text(ctx.model, &step.name, &step.args)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        match ctx.locale {
            Locale::Ja => format!(
                "- 先行手順（When）: 次の操作をこの順に実行する。いずれも成功しなければならない。\n{steps}"
            ),
            Locale::En => format!(
                "- When (prefix): execute the following operations in this order. Each of them must succeed.\n{steps}"
            ),
        }
    };
    let last_text = last_step
        .map(|step| step_text(ctx.model, &step.name, &step.args))
        .unwrap_or_default();
    let then_line = match ctx.locale {
        Locale::Ja => format!(
            "- 期待（Then）: 続けて実行しようとする最後の操作 {last_text} は、拒否されなければならない（この時点では実行できてはならない）。"
        ),
        Locale::En => format!(
            "- Then: the final operation {last_text}, attempted next, must be rejected (it must not be executable at this point)."
        ),
    };
    let tail = match ctx.locale {
        Locale::Ja => {
            "この基準が示すのは、上記の手順の直後に最後の操作が拒否されることのみである。この操作があらゆる状況で禁止されることを主張するものではない。"
        }
        Locale::En => {
            "This criterion shows only that the final operation is rejected immediately after the sequence above. It does not claim that this operation is forbidden in every situation."
        }
    };
    format!(
        "{head}\n- {title_label}: {}\n\n{intro}\n\n{given}\n{when_line}\n{then_line}\n\n{tail}",
        case.text
    )
}

fn expectation_text(case: &RequirementsTraceCase, ctx: &mut Ctx<'_>) -> String {
    match &case.expectation {
        None => match ctx.locale {
            Locale::Ja => "- 期待（Then）: 追加の期待条件はない。すべての操作が拒否されずに成功すること自体が確認内容である。".to_owned(),
            Locale::En => "- Then: there is no additional expectation. What is checked is that every operation succeeds without being rejected.".to_owned(),
        },
        Some(RequirementsTraceExpectation::Stage { entity, instance, stage }) => match ctx.locale {
            Locale::Ja => format!(
                "- 期待（Then）: 最後の操作のあと、`{entity}` の個体 `{instance}` が段階 `{stage}` にある。"
            ),
            Locale::En => format!(
                "- Then: after the final operation, instance `{instance}` of `{entity}` is in stage `{stage}`."
            ),
        },
        Some(RequirementsTraceExpectation::Expr(expr)) => {
            if let Some(text) = document_render_expr::render_inline(expr, ctx.model, ctx.locale) {
                match ctx.locale {
                    Locale::Ja => format!("- 期待（Then）: 最後の操作のあと、{text}。"),
                    Locale::En => format!("- Then: after the final operation, {text}."),
                }
            } else {
                ctx.fallback_count += 1;
                let canonical = fsl_core::source_expr_text(ctx.model, expr);
                match ctx.locale {
                    Locale::Ja => format!(
                        "- 期待（Then）: 最後の操作のあと、次が成立する。\n\n  ```fsl\n  {canonical}\n  ```"
                    ),
                    Locale::En => format!(
                        "- Then: after the final operation, the following holds.\n\n  ```fsl\n  {canonical}\n  ```"
                    ),
                }
            }
        }
    }
}

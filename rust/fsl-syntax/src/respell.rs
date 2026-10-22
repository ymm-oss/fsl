// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The formatter's rewrites of a next-edition requirements document:
//! keywords in one language (`fmt --lang ja|en`), names in one spelling
//! (`fmt --names alias|canonical`), and the space a particle written onto
//! its word needs (`particle_needs_space`).
//!
//! Every rewrite is a respelling of what the parser read, located by the
//! spans it recorded ([`crate::SourceSpellings`]): no word changes meaning.
//!
//! - **Keywords** are replaced through the keyword table, in the context the
//!   parser read them in. `and` / `or` of an expression are translated in
//!   the next edition's members; in a published item kept in the document
//!   (`requirement`, `acceptance`, …) they are written in English.
//! - **Names** are replaced through the glossary at exactly the positions
//!   alias resolution reads (after the glossary, outside `implements`): an
//!   alias by its canonical name, or a canonical name by its one alias.
//! - **Particles** follow the words. A phrase (an event's, a rule
//!   statement's, a decision table row's outcome, a scenario step) and a
//!   stage check (`x is S`) are written with particles — が, を, `で` for
//!   `with`, `は` for `is` — when the keywords are Japanese or one of their
//!   words is (an alias, a scenario's instance label); otherwise in English
//!   order with `with` and `is`. A phrase already in that form keeps its
//!   word order; one that changes form is written in the canonical order of
//!   its kind (English: `Actor verb Entity with inputs`; with particles:
//!   `Actor が Entity を verb inputs で` for an event, `Actor が instance を
//!   inputs で verb` for a step).
//!
//! The language of the keywords, when only `--names` is given, is the
//! language the document's `requirements` keyword is written in.

use std::collections::{BTreeMap, BTreeSet};

use crate::keywords::{Kw, Position};
use crate::{
    Edition, FormatError, Glossary, ParseError, ParseOptions, PhraseKind, PhraseSpelling,
    RequirementsItem, SourceEdit, SourceFile, SourceSpellings, Span, SurfaceDocument, Token,
    TokenKind, UnsafeFormatKind, apply_source_edits, parse_document_with,
};

/// `--lang`: the language keywords are written in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeywordLanguage {
    English,
    Japanese,
}

/// `--names`: how names with a glossary alias are written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NameForm {
    /// The glossary's alias of every name that has one.
    Alias,
    /// The canonical name of every alias.
    Canonical,
}

/// The rewrites one `fmt` run asks for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Respelling {
    pub language: Option<KeywordLanguage>,
    pub names: Option<NameForm>,
}

/// Code of the refusal to respell a document that is not a next-edition
/// requirements document: it has no keyword table and no glossary.
pub const RESPELL_NOT_APPLICABLE: &str = "respell_not_applicable";

/// `source` with every particle written onto its word (`従業員が`) spaced
/// off (`従業員 が`), in a next-edition requirements document read under
/// `options`; any other document as it is.
#[must_use]
pub fn space_particles(source: &str, options: &ParseOptions) -> String {
    if !matches!(crate::edition_header(source, options), Ok(Some(header)) if header.edition == Edition::Fsl5)
    {
        return source.to_owned();
    }
    let Ok(tokens) = crate::lex_with(source, options) else {
        return source.to_owned();
    };
    let Some(from) = requirements_body_start(&tokens) else {
        return source.to_owned();
    };
    let glossary = crate::document_glossary(source, options);
    let from = glossary.as_ref().map_or(from, |glossary| {
        tokens
            .iter()
            .position(|token| token.span.start.offset >= glossary.span.end.offset)
            .unwrap_or(tokens.len())
    });
    let edits = crate::parser::particles_without_space(&tokens, from, glossary.as_ref())
        .into_iter()
        .map(|found| {
            let offset = tokens[found.token].span.start.offset + found.stem_len;
            let at = crate::source_span(source, offset, offset);
            SourceEdit {
                span: at,
                replacement: " ".to_owned(),
            }
        })
        .collect::<Vec<_>>();
    apply_source_edits(source, edits).unwrap_or_else(|_| source.to_owned())
}

/// The index of the first token of a requirements body (`requirements Name
/// {` or `要件 Name {`), when the document is one.
fn requirements_body_start(tokens: &[Token]) -> Option<usize> {
    let open = tokens
        .iter()
        .position(|token| matches!(&token.kind, TokenKind::Symbol(symbol) if symbol == "{"))?;
    let keyword = open.checked_sub(2)?;
    matches!(&tokens[keyword].kind, TokenKind::Ident(word)
        if crate::keywords::spells(word, Kw::Requirements))
    .then_some(open + 1)
}

/// `source` respelled as `respelling` asks, before layout.
///
/// # Errors
///
/// The parse error of `source`; [`RESPELL_NOT_APPLICABLE`] when it is not a
/// next-edition requirements document; an unsafe-format error when a phrase
/// that changes form has a comment inside it.
pub fn respell_source(
    source: &str,
    options: &ParseOptions,
    respelling: Respelling,
) -> Result<String, FormatError> {
    if respelling == Respelling::default() {
        return Ok(source.to_owned());
    }
    let parsed =
        parse_document_with(SourceFile::new(source), options).map_err(FormatError::Parse)?;
    let SurfaceDocument::Requirements(requirements) = &parsed.surface else {
        return Err(not_applicable(source));
    };
    let Some(next) = &requirements.next else {
        return Err(not_applicable(source));
    };
    let glossary = requirements.items.iter().find_map(|item| match item {
        RequirementsItem::Glossary(glossary) => Some(glossary),
        _ => None,
    });
    let tokens = crate::lex_with(source, options).map_err(FormatError::Lex)?;
    let mut plan = Plan::new(source, &tokens, &next.spellings);
    if let (Some(form), Some(glossary)) = (respelling.names, glossary) {
        plan.names(form, glossary);
    }
    if let Some(language) = respelling.language {
        plan.keywords(language);
    }
    let japanese = respelling
        .language
        .unwrap_or_else(|| plan.document_language())
        == KeywordLanguage::Japanese;
    plan.checks(japanese);
    plan.phrases(japanese, respelling.language)?;
    apply_source_edits(source, plan.edits.into_values().collect())
}

pub(crate) fn not_applicable(source: &str) -> FormatError {
    FormatError::Parse(ParseError::coded(
        RESPELL_NOT_APPLICABLE,
        "--lang and --names rewrite a requirements document headed `fsl 5`; this document has no \
         keyword table or glossary to rewrite by",
        crate::source_span(source, 0, 0),
    ))
}

/// The edits of one respelling, keyed by their start offset.
struct Plan<'a> {
    source: &'a str,
    tokens: &'a [Token],
    spellings: &'a SourceSpellings,
    /// One edit per start offset; a phrase's replaces the edits inside it.
    edits: BTreeMap<usize, SourceEdit>,
    /// The start offsets of the keywords: never names, never a phrase's
    /// words.
    keyword_offsets: BTreeSet<usize>,
}

impl<'a> Plan<'a> {
    fn new(source: &'a str, tokens: &'a [Token], spellings: &'a SourceSpellings) -> Self {
        Self {
            source,
            tokens,
            spellings,
            edits: BTreeMap::new(),
            keyword_offsets: spellings
                .keywords
                .iter()
                .map(|keyword| keyword.span.start.offset)
                .chain(spellings.checks.iter().map(|check| check.is.start.offset))
                .collect(),
        }
    }

    fn replace(&mut self, span: Span, replacement: impl Into<String>) {
        let replacement = replacement.into();
        if self.source[span.start.offset..span.end.offset] == replacement {
            self.edits.remove(&span.start.offset);
        } else {
            self.edits
                .insert(span.start.offset, SourceEdit { span, replacement });
        }
    }

    /// The text of `span` with the edits planned inside it applied.
    fn text(&self, span: Span) -> String {
        let mut text = String::new();
        let mut offset = span.start.offset;
        for edit in self
            .edits
            .range(span.start.offset..span.end.offset)
            .map(|(_, edit)| edit)
            .filter(|edit| edit.span.end.offset <= span.end.offset)
        {
            text.push_str(&self.source[offset..edit.span.start.offset]);
            text.push_str(&edit.replacement);
            offset = edit.span.end.offset;
        }
        text.push_str(&self.source[offset..span.end.offset]);
        text
    }

    /// Whether a word inside `span`, as respelled, is not ASCII: a Japanese
    /// name or label. Keywords and particles are not words here.
    fn has_japanese_word(&self, span: Span) -> bool {
        self.tokens.iter().any(|token| {
            token.span.start.offset >= span.start.offset
                && token.span.end.offset <= span.end.offset
                && !self.keyword_offsets.contains(&token.span.start.offset)
                && matches!(&token.kind, TokenKind::Ident(word)
                    if crate::keywords::particle(word).is_none())
                && !self.text(token.span).is_ascii()
        })
    }

    /// The language the document's `requirements` keyword is written in.
    fn document_language(&self) -> KeywordLanguage {
        self.spellings
            .keywords
            .iter()
            .find(|keyword| keyword.kw == Kw::Requirements)
            .filter(|keyword| {
                self.source[keyword.span.start.offset..keyword.span.end.offset]
                    == *Kw::Requirements.japanese()
            })
            .map_or(KeywordLanguage::English, |_| KeywordLanguage::Japanese)
    }

    /// Respell every name alias resolution reads: after the glossary, outside
    /// `implements` bodies.
    fn names(&mut self, form: NameForm, glossary: &Glossary) {
        let rename = match form {
            NameForm::Canonical => glossary
                .entries
                .iter()
                .map(|entry| (entry.alias.clone(), entry.canonical.name.clone()))
                .collect::<BTreeMap<_, _>>(),
            NameForm::Alias => aliases_by_name(glossary),
        };
        let mut index = self
            .tokens
            .iter()
            .position(|token| token.span.start.offset >= glossary.span.end.offset)
            .unwrap_or(self.tokens.len());
        while index < self.tokens.len() {
            if let Some(end) = crate::glossary::implements_body_end(self.tokens, index) {
                index = end;
                continue;
            }
            let token = &self.tokens[index];
            if let TokenKind::Ident(word) = &token.kind
                && !self.keyword_offsets.contains(&token.span.start.offset)
                && let Some(renamed) = rename.get(word)
            {
                self.replace(token.span, renamed.clone());
            }
            index += 1;
        }
    }

    /// Respell every keyword the table reads in `language`. `with` and `is`
    /// are the phrases' and the checks' ([`Self::phrases`], [`Self::checks`]).
    fn keywords(&mut self, language: KeywordLanguage) {
        for keyword in &self.spellings.keywords {
            let entry = keyword.kw.entry();
            if entry.position == Position::After || matches!(keyword.kw, Kw::ThenIs | Kw::AlwaysIs)
            {
                continue;
            }
            self.replace(keyword.span, spelling(keyword.kw, language));
        }
        for expression in &self.spellings.expressions {
            let target = if expression.member {
                language
            } else {
                KeywordLanguage::English
            };
            for token in self.tokens.iter().filter(|token| {
                token.span.start.offset >= expression.span.start.offset
                    && token.span.end.offset <= expression.span.end.offset
            }) {
                if let TokenKind::Ident(word) = &token.kind
                    && let Some(kw) = [Kw::And, Kw::Or]
                        .into_iter()
                        .find(|kw| crate::keywords::spells(word, *kw))
                {
                    self.replace(token.span, spelling(kw, target));
                }
            }
        }
    }

    /// Write each stage check's `is` as は when its words or the keywords
    /// are Japanese.
    fn checks(&mut self, japanese: bool) {
        for check in &self.spellings.checks {
            let particle = japanese || self.has_japanese_word(check.span);
            let is = Kw::ThenIs.entry();
            self.replace(check.is, if particle { is.japanese } else { is.english });
        }
    }

    /// Rewrite each phrase whose form (with or without particles) changes.
    fn phrases(
        &mut self,
        japanese: bool,
        language: Option<KeywordLanguage>,
    ) -> Result<(), FormatError> {
        for phrase in &self.spellings.phrases {
            let particles = japanese || self.has_japanese_word(phrase.span);
            if particles == phrase.particles {
                continue;
            }
            let interior = &self.source[phrase.span.start.offset..phrase.span.end.offset];
            if interior.contains("//") {
                return Err(FormatError::Unsafe {
                    kind: UnsafeFormatKind::PhraseInteriorComment,
                    message: "cannot reorder a phrase with a comment inside it".to_owned(),
                    span: phrase.span,
                });
            }
            let written = self.phrase_text(phrase, particles, language);
            let inside = self
                .edits
                .range(phrase.span.start.offset..phrase.span.end.offset)
                .map(|(offset, _)| *offset)
                .collect::<Vec<_>>();
            for offset in inside {
                self.edits.remove(&offset);
            }
            self.edits.insert(
                phrase.span.start.offset,
                SourceEdit {
                    span: phrase.span,
                    replacement: written,
                },
            );
        }
        Ok(())
    }

    /// `phrase` written in the canonical order of its kind, with or without
    /// particles.
    fn phrase_text(
        &self,
        phrase: &PhraseSpelling,
        particles: bool,
        language: Option<KeywordLanguage>,
    ) -> String {
        let actor = self.text(phrase.actor);
        let verbs = phrase
            .verbs
            .iter()
            .map(|verb| self.text(*verb))
            .collect::<Vec<_>>();
        let object = phrase.object.map(|object| self.text(object));
        let inputs = phrase
            .inputs
            .iter()
            .map(|input| self.text(*input))
            .collect::<Vec<_>>()
            .join(", ");
        let mut words = vec![actor];
        if particles {
            words.push("が".to_owned());
            if let Some(object) = object {
                words.extend([object, "を".to_owned()]);
            }
            let inputs = (!inputs.is_empty()).then(|| format!("{inputs} で"));
            match phrase.kind {
                PhraseKind::Step => {
                    words.extend(inputs);
                    words.extend(verbs);
                }
                PhraseKind::DecisionRow => words.push(self.row_verbs(phrase, &verbs, language)),
                PhraseKind::Event | PhraseKind::Rule => {
                    words.extend(verbs);
                    words.extend(inputs);
                }
            }
        } else {
            if phrase.kind == PhraseKind::DecisionRow {
                words.push(self.row_verbs(phrase, &verbs, language));
            } else {
                words.extend(verbs);
                words.extend(object);
            }
            if !inputs.is_empty() {
                words.push(format!("{} {inputs}", Kw::EventWith.english()));
            }
        }
        words.join(" ")
    }

    /// A row's verbs joined by its `or`, as respelled.
    fn row_verbs(
        &self,
        phrase: &PhraseSpelling,
        verbs: &[String],
        language: Option<KeywordLanguage>,
    ) -> String {
        let or = self
            .spellings
            .keywords
            .iter()
            .find(|keyword| {
                keyword.kw == Kw::RowOr
                    && keyword.span.start.offset >= phrase.span.start.offset
                    && keyword.span.end.offset <= phrase.span.end.offset
            })
            .map_or_else(
                || spelling(Kw::RowOr, language.unwrap_or(KeywordLanguage::English)).to_owned(),
                |keyword| self.text(keyword.span),
            );
        verbs.join(&format!(" {or} "))
    }
}

/// The spelling of `kw` in `language`.
fn spelling(kw: Kw, language: KeywordLanguage) -> &'static str {
    match language {
        KeywordLanguage::English => kw.english(),
        KeywordLanguage::Japanese => kw.japanese(),
    }
}

/// Each canonical name with exactly one alias, by name: an unqualified entry
/// first, else the one field entry of that name (as
/// `fsl_core::naming::Glossary::alias` displays it).
fn aliases_by_name(glossary: &Glossary) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for entry in &glossary.entries {
        if entry.canonical.owner.is_none() {
            aliases.insert(entry.canonical.name.clone(), entry.alias.clone());
        }
    }
    for entry in &glossary.entries {
        if entry.canonical.owner.is_some()
            && !glossary.entries.iter().any(|other| {
                other.canonical.owner.is_none() && other.canonical.name == entry.canonical.name
            })
            && aliases
                .insert(entry.canonical.name.clone(), entry.alias.clone())
                .is_some()
        {
            ambiguous.insert(entry.canonical.name.clone());
        }
    }
    for name in ambiguous {
        aliases.remove(&name);
    }
    aliases
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{UnstableFeature, UnstableFeatures};

    fn gated() -> ParseOptions {
        ParseOptions::with_unstable(UnstableFeatures::NONE.with(UnstableFeature::Fsl5))
    }

    const REJECTION: &str = include_str!("../../fslc/tests/fixtures/fsl5/rejection/fsl5_en.fsl");

    fn japanese() -> Respelling {
        Respelling {
            language: Some(KeywordLanguage::Japanese),
            names: None,
        }
    }

    /// `and` / `or` are translated in the next edition's members and stay
    /// English in a published item kept in the document (range B).
    #[test]
    fn expression_keywords_are_translated_only_in_members() {
        let source = REJECTION.replace(
            "request_stage[r] == ActionExecuted }",
            "request_stage[r] == ActionExecuted and true }",
        );
        let respelled = respell_source(&source, &gated(), japanese()).expect("respell");
        assert!(
            respelled.contains("対象 ExplicitlyApproved かつ OperatorHeld"),
            "{respelled}"
        );
        assert!(
            respelled.contains("request_stage[r] == ActionExecuted and true }"),
            "{respelled}"
        );
        assert!(respelled.contains("  requirement GOAL"), "{respelled}");
    }

    /// A phrase that changes form is rebuilt from its words; a comment
    /// inside it has no place to go, so the rewrite is refused.
    #[test]
    fn a_phrase_with_a_comment_inside_is_not_reordered() {
        let source = REJECTION.replace(
            "= Agent executes Request        ToolApproved",
            "= Agent // who\n    executes Request ToolApproved",
        );
        assert!(crate::parse_document_with(SourceFile::new(&source), &gated()).is_ok());
        let error = respell_source(&source, &gated(), japanese()).expect_err("refused");
        assert!(
            matches!(
                error,
                FormatError::Unsafe {
                    kind: UnsafeFormatKind::PhraseInteriorComment,
                    ..
                }
            ),
            "{error}"
        );
    }

    #[test]
    fn only_next_edition_requirements_are_respelled() {
        let error = respell_source("spec S {}", &gated(), japanese()).expect_err("refused");
        assert!(
            matches!(&error, FormatError::Parse(error) if error.code() == RESPELL_NOT_APPLICABLE),
            "{error}"
        );
        assert_eq!(
            respell_source("spec S {}", &gated(), Respelling::default()).expect("no rewrite"),
            "spec S {}"
        );
    }

    /// The space a particle needs is inserted only in a next-edition
    /// document, and only after a word that is a name.
    #[test]
    fn particles_are_spaced_off_names_only_in_the_next_edition() {
        let source =
            "fsl 5\nrequirements R {\n  actor 従業員\n  // 従業員が\n  const 従業員が = 1\n}\n";
        assert_eq!(space_particles(source, &ParseOptions::stable()), source);
        assert_eq!(
            space_particles(source, &gated()),
            source.replace("const 従業員が", "const 従業員 が")
        );
        let unrelated = "fsl 5\nrequirements R {\n  const 金額が = 1\n}\n";
        assert_eq!(space_particles(unrelated, &gated()), unrelated);
    }
}

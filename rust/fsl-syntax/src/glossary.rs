// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The `glossary` block of a next-edition requirements document, and the
//! first step of name resolution: every alias the block declares is replaced
//! by its canonical name before any declaration is parsed.
//!
//! ```text
//! requirements ExpenseRequirements {
//!   glossary {
//!     Claim        = 申請
//!     Claim.amount = 申請金額
//!   }
//!   ...
//! }
//! ```
//!
//! The left side is the canonical (implementation-facing) name, qualified by
//! its entity for a field; the right side is one identifier, the alias. The
//! block is accepted only behind the `fsl5` gate in a document headed
//! `fsl 5`, and only as the first item of `requirements`. Aliases are resolved
//! on the token stream, so every position that names something reads the
//! canonical name and the frontends below see no alias. Inside
//! `implements { ... }` the mapping is written in canonical names, and an
//! alias there is an error.
//!
//! Collisions within the block are reported here; the rules that need the
//! lowered declarations (`alias_shadows_name` against a declared name) and
//! the publication of the table (`fsl_core::naming::Glossary`) belong to
//! name resolution in fsl-core.

use std::collections::{BTreeMap, BTreeSet};

use crate::keywords::Kw;
use crate::{ParseError, Span, Token, TokenKind};

/// One alias names two canonical names.
pub const ALIAS_COLLISION: &str = "alias_collision";
/// An alias is spelled like a canonical name.
pub const ALIAS_SHADOWS_NAME: &str = "alias_shadows_name";
/// One canonical name has two aliases.
pub const DUPLICATE_ALIAS: &str = "duplicate_alias";
/// An alias is spelled like a keyword (English or Japanese, any context).
pub const ALIAS_SHADOWS_KEYWORD: &str = "alias_shadows_keyword";
/// An alias written where only canonical names are read (`implements`).
pub const ALIAS_IN_IMPLEMENTS: &str = "alias_in_implements";
/// A `glossary` block that is not the first item of `requirements`, or a
/// second one.
pub const MISPLACED_GLOSSARY: &str = "misplaced_glossary";
/// Lint (off by default): one name written both as its canonical name and as
/// its alias in the same document.
pub const MIXED_NAMES: &str = "mixed_names";

/// Every error code of glossary resolution, including
/// [`crate::CANONICAL_NAME_NOT_ASCII`], which it emits.
pub const GLOSSARY_DIAGNOSTICS: &[&str] = &[
    ALIAS_COLLISION,
    ALIAS_SHADOWS_NAME,
    DUPLICATE_ALIAS,
    ALIAS_SHADOWS_KEYWORD,
    ALIAS_IN_IMPLEMENTS,
    MISPLACED_GLOSSARY,
    crate::CANONICAL_NAME_NOT_ASCII,
];

/// The English keywords an alias may not be spelled as: the union over every
/// context of the words the shared grammar reads as keywords and the English
/// words of the next edition's requirements. Their Japanese counterparts and
/// the particles come from the keyword table ([`crate::keywords`]);
/// [`is_keyword_spelling`] reads both. A single union, not one per context,
/// so that an alias is never usable in one place and a keyword in another.
/// `keyword_spellings_cover_the_parser` keeps this list in step with the
/// parser and the table.
pub const KEYWORD_SPELLINGS: &[&str] = &[
    "Bool",
    "Int",
    "Map",
    "Option",
    "Seq",
    "Set",
    "abs",
    "abstraction",
    "acceptance",
    "action",
    "actor",
    "after",
    "age",
    "all",
    "allow",
    "always",
    "and",
    "applies_to",
    "as",
    "assume",
    "assuming",
    "authority",
    "auto",
    "be",
    "because",
    "before",
    "belongs",
    "branches",
    "business",
    "by",
    "can",
    "checked_by",
    "compose",
    "concept",
    "const",
    "control",
    "conversion",
    "count",
    "covers",
    "deadline",
    "decide",
    "decreases",
    "def",
    "delegates",
    "else",
    "ensures",
    "enters",
    "entity",
    "enum",
    "environment",
    "event",
    "eventually",
    "every",
    "exists",
    "expect",
    "fair",
    "false",
    "fault",
    "for",
    "forall",
    "forbidden",
    "from",
    "glossary",
    "goal",
    "governance",
    "has",
    "have",
    "helpful",
    "if",
    "impl",
    "implements",
    "in",
    "init",
    "initial",
    "input",
    "instances",
    "internal",
    "invariant",
    "is",
    "kpi",
    "leadsTo",
    "let",
    "lifecycle",
    "map",
    "maps",
    "max",
    "measure",
    "min",
    "must",
    "never",
    "none",
    "not",
    "number",
    "of",
    "old",
    "only",
    "or",
    "otherwise",
    "out",
    "owner",
    "owns",
    "passed",
    "policy",
    "preservation",
    "preserve",
    "process",
    "progress",
    "reach",
    "reachable",
    "reaching",
    "refinement",
    "rejected",
    "relation",
    "require",
    "requirement",
    "requirements",
    "requires",
    "resource",
    "respond",
    "responds",
    "rule",
    "satisfied_by",
    "satisfies",
    "scenario",
    "scope",
    "set",
    "severity",
    "some",
    "spec",
    "stages",
    "state",
    "struct",
    "stutter",
    "sum",
    "symmetric",
    "terminal",
    "then",
    "through",
    "time",
    "to",
    "trans",
    "transition",
    "true",
    "type",
    "unless",
    "until",
    "urgent",
    "use",
    "values",
    "verify",
    "via",
    "when",
    "where",
    "while",
    "with",
    "within",
];

/// Whether `word` is one of [`KEYWORD_SPELLINGS`], a Japanese keyword of the
/// keyword table (any context, reserved rows included) or a particle.
#[must_use]
pub fn is_keyword_spelling(word: &str) -> bool {
    KEYWORD_SPELLINGS.contains(&word) || crate::keywords::is_japanese_spelling(word)
}

/// The canonical side of a glossary entry: a name, or `Entity.field`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct GlossaryName {
    /// The entity qualifying a field name; `None` for an unqualified name.
    pub owner: Option<String>,
    /// The name an alias resolves to (the field name for a qualified entry).
    pub name: String,
}

impl GlossaryName {
    /// The name as written on the left side (`Claim` or `Claim.amount`).
    #[must_use]
    pub fn written(&self) -> String {
        match &self.owner {
            Some(owner) => format!("{owner}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// One `Canonical = alias` line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlossaryEntry {
    pub canonical: GlossaryName,
    pub alias: String,
    pub canonical_span: Span,
    pub alias_span: Span,
}

/// Where the source spelled a glossary name, recorded while aliases are
/// resolved: an alias replaced by its canonical name, or a canonical name
/// that has an alias written as itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameUse {
    /// The canonical name read at this position.
    pub canonical: String,
    /// `true` when the source wrote the alias, `false` for the canonical name.
    pub via_alias: bool,
    pub span: Span,
}

/// A parsed `glossary` block and the name uses its resolution recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Glossary {
    pub entries: Vec<GlossaryEntry>,
    /// Every resolved alias and every canonical spelling of an aliased name
    /// outside `implements`, in source order.
    pub uses: Vec<NameUse>,
    pub span: Span,
}

impl Glossary {
    /// The alias of the canonical name written `written` (`Claim`,
    /// `Claim.amount`).
    #[must_use]
    pub fn alias_of(&self, written: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|entry| entry.canonical.written() == written)
            .map(|entry| entry.alias.as_str())
    }
}

/// Parse `glossary { ... }` at `tokens[*cursor]` and check the block on its
/// own: every left side ASCII, one alias per canonical name, one canonical
/// name per alias, and no alias spelled like a keyword or like a canonical
/// name of the block.
pub(crate) fn parse_glossary(tokens: &[Token], cursor: &mut usize) -> Result<Glossary, ParseError> {
    let start = tokens[*cursor].span;
    *cursor += 1;
    expect_symbol(tokens, cursor, "{")?;
    let mut entries = Vec::new();
    loop {
        let token = &tokens[*cursor];
        match &token.kind {
            TokenKind::Symbol(symbol) if symbol == "}" => break,
            TokenKind::Ident(_) => entries.push(entry(tokens, cursor)?),
            _ => {
                return Err(ParseError::new(
                    "expected a glossary entry 'Canonical = alias' or '}'",
                    token.span,
                ));
            }
        }
    }
    let end = tokens[*cursor].span;
    *cursor += 1;
    let glossary = Glossary {
        entries,
        uses: Vec::new(),
        span: Span {
            start: start.start,
            end: end.end,
        },
    };
    check_entries(&glossary)?;
    Ok(glossary)
}

fn entry(tokens: &[Token], cursor: &mut usize) -> Result<GlossaryEntry, ParseError> {
    let (first, first_span) = expect_ident(tokens, cursor)?;
    let (canonical, canonical_span) = if is_symbol(&tokens[*cursor], ".") {
        *cursor += 1;
        let (field, field_span) = expect_ident(tokens, cursor)?;
        (
            GlossaryName {
                owner: Some(first),
                name: field,
            },
            Span {
                start: first_span.start,
                end: field_span.end,
            },
        )
    } else {
        (
            GlossaryName {
                owner: None,
                name: first,
            },
            first_span,
        )
    };
    expect_symbol(tokens, cursor, "=")?;
    let (alias, alias_span) = expect_ident(tokens, cursor)?;
    Ok(GlossaryEntry {
        canonical,
        alias,
        canonical_span,
        alias_span,
    })
}

fn check_entries(glossary: &Glossary) -> Result<(), ParseError> {
    let canonical_names = glossary
        .entries
        .iter()
        .flat_map(|entry| entry.canonical.owner.iter().chain([&entry.canonical.name]))
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut by_canonical = BTreeMap::<String, &GlossaryEntry>::new();
    let mut by_alias = BTreeMap::<&str, &GlossaryEntry>::new();
    for entry in &glossary.entries {
        let written = entry.canonical.written();
        if !written.is_ascii() {
            return Err(ParseError::coded(
                crate::CANONICAL_NAME_NOT_ASCII,
                format!(
                    "canonical name '{written}' is not ASCII; the left side of a glossary entry is the name implementations see, and the right side is the alias"
                ),
                entry.canonical_span,
            ));
        }
        if let Some(first) = by_canonical.get(&written) {
            return Err(ParseError::coded(
                DUPLICATE_ALIAS,
                format!(
                    "'{written}' has two aliases, '{}' and '{}'; a canonical name has one alias, so that it displays one way",
                    first.alias, entry.alias
                ),
                entry.canonical_span,
            ));
        }
        if let Some(first) = by_alias.get(entry.alias.as_str()) {
            return Err(ParseError::coded(
                ALIAS_COLLISION,
                format!(
                    "alias '{}' names both '{}' and '{written}'; an alias names one canonical name",
                    entry.alias,
                    first.canonical.written()
                ),
                entry.alias_span,
            ));
        }
        if is_keyword_spelling(&entry.alias) {
            return Err(ParseError::coded(
                ALIAS_SHADOWS_KEYWORD,
                format!(
                    "alias '{}' of '{written}' is spelled like a keyword; choose a longer alias",
                    entry.alias
                ),
                entry.alias_span,
            ));
        }
        if canonical_names.contains(entry.alias.as_str()) {
            return Err(ParseError::coded(
                ALIAS_SHADOWS_NAME,
                format!(
                    "alias '{}' of '{written}' is spelled like the canonical name '{}'",
                    entry.alias, entry.alias
                ),
                entry.alias_span,
            ));
        }
        by_canonical.insert(written, entry);
        by_alias.insert(&entry.alias, entry);
    }
    Ok(())
}

/// Replace every alias in `tokens[from..]` by its canonical name, recording
/// each use in `glossary.uses`. The body of `implements NAME from "path" {
/// ... }` is read in canonical names: an alias there is
/// [`ALIAS_IN_IMPLEMENTS`], and nothing in it is rewritten.
pub(crate) fn resolve_alias_tokens(
    tokens: &mut [Token],
    from: usize,
    glossary: &mut Glossary,
) -> Result<(), ParseError> {
    let aliases = glossary
        .entries
        .iter()
        .map(|entry| (entry.alias.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let canonical_targets = glossary
        .entries
        .iter()
        .map(|entry| entry.canonical.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut uses = Vec::new();
    let mut index = from;
    while index < tokens.len() {
        if let Some(body_end) = implements_body_end(tokens, index) {
            for token in &tokens[index..body_end] {
                if let TokenKind::Ident(word) = &token.kind
                    && let Some(entry) = aliases.get(word)
                {
                    return Err(ParseError::coded(
                        ALIAS_IN_IMPLEMENTS,
                        format!(
                            "alias '{word}' inside implements; the mapping to another layer is written in canonical names: write '{}'",
                            entry.canonical.name
                        ),
                        token.span,
                    ));
                }
            }
            index = body_end;
            continue;
        }
        let token = &mut tokens[index];
        if let TokenKind::Ident(word) = &mut token.kind {
            if let Some(entry) = aliases.get(word.as_str()) {
                uses.push(NameUse {
                    canonical: entry.canonical.name.clone(),
                    via_alias: true,
                    span: token.span,
                });
                word.clone_from(&entry.canonical.name);
            } else if canonical_targets.contains(word.as_str()) {
                uses.push(NameUse {
                    canonical: word.clone(),
                    via_alias: false,
                    span: token.span,
                });
            }
        }
        index += 1;
    }
    glossary.uses = uses;
    Ok(())
}

/// After resolution, a name that is still not ASCII has no alias: it would
/// reach implementations as written. A `scenario` or `forbidden` body is not
/// scanned: it names instances with its own labels (`申請1`), which become
/// instance numbers and never reach the kernel, and every other word there is
/// an actor, a verb, a stage or an input that the trace resolves against the
/// declarations, which are scanned.
pub(crate) fn reject_unaliased_non_ascii(tokens: &[Token]) -> Result<(), ParseError> {
    let mut index = 0;
    while index < tokens.len() {
        if let Some(body_end) =
            implements_body_end(tokens, index).or_else(|| trace_body_end(tokens, index))
        {
            index = body_end;
            continue;
        }
        let token = &tokens[index];
        if let TokenKind::Ident(word) = &token.kind
            && !word.is_ascii()
            && !is_keyword_spelling(word)
        {
            return Err(ParseError::coded(
                crate::CANONICAL_NAME_NOT_ASCII,
                format!(
                    "name '{word}' is not ASCII and has no glossary entry; declare an ASCII canonical name and add '<Canonical> = {word}' to the glossary"
                ),
                token.span,
            ));
        }
        index += 1;
    }
    Ok(())
}

/// Outside requirements no document has a glossary, so every name it writes
/// is canonical: a name that is not ASCII is [`crate::CANONICAL_NAME_NOT_ASCII`]
/// there. `keyword` is the document's declaration keyword, for the message.
/// Keywords are English in these documents, so every non-ASCII word is a
/// name.
pub(crate) fn reject_non_ascii_names(tokens: &[Token], keyword: &str) -> Result<(), ParseError> {
    match tokens.iter().find_map(|token| match &token.kind {
        TokenKind::Ident(word) if !word.is_ascii() => Some((word, token.span)),
        _ => None,
    }) {
        Some((word, span)) => Err(ParseError::coded(
            crate::CANONICAL_NAME_NOT_ASCII,
            format!(
                "name '{word}' is not ASCII; a {keyword} document declares canonical names, which are ASCII: write an ASCII name (a Japanese name is an alias, in the glossary of a requirements document)"
            ),
            span,
        )),
        None => Ok(()),
    }
}

/// When `tokens[index]` starts `implements NAME from "path" {`, the index
/// just past the block's closing brace.
pub(crate) fn implements_body_end(tokens: &[Token], index: usize) -> Option<usize> {
    let head = tokens.get(index..index + 5)?;
    let opens = matches!(&head[0].kind, TokenKind::Ident(word) if word == "implements")
        && matches!(head[1].kind, TokenKind::Ident(_))
        && matches!(&head[2].kind, TokenKind::Ident(word) if word == "from")
        && matches!(head[3].kind, TokenKind::String(_))
        && is_symbol(&head[4], "{");
    opens.then(|| block_end(tokens, index + 4))
}

/// When `tokens[index]` starts a `scenario` or `forbidden` block, the index
/// just past its closing brace.
fn trace_body_end(tokens: &[Token], index: usize) -> Option<usize> {
    if !matches!(&tokens[index].kind, TokenKind::Ident(word)
        if crate::keywords::spells(word, Kw::Scenario) || crate::keywords::spells(word, Kw::Forbidden))
    {
        return None;
    }
    let open = index
        + tokens[index..]
            .iter()
            .position(|token| is_symbol(token, "{"))?;
    Some(block_end(tokens, open))
}

/// The index just past the brace closing the block opened at `open`.
fn block_end(tokens: &[Token], open: usize) -> usize {
    let mut depth = 0usize;
    for (offset, token) in tokens[open..].iter().enumerate() {
        if is_symbol(token, "{") {
            depth += 1;
        } else if is_symbol(token, "}") {
            depth -= 1;
            if depth == 0 {
                return open + offset + 1;
            }
        } else if matches!(token.kind, TokenKind::Eof) {
            return open + offset;
        }
    }
    tokens.len()
}

fn is_symbol(token: &Token, symbol: &str) -> bool {
    matches!(&token.kind, TokenKind::Symbol(value) if value == symbol)
}

fn expect_symbol(tokens: &[Token], cursor: &mut usize, symbol: &str) -> Result<(), ParseError> {
    let token = &tokens[*cursor];
    if is_symbol(token, symbol) {
        *cursor += 1;
        Ok(())
    } else {
        Err(ParseError::new(format!("expected '{symbol}'"), token.span))
    }
}

fn expect_ident(tokens: &[Token], cursor: &mut usize) -> Result<(String, Span), ParseError> {
    let token = &tokens[*cursor];
    if let TokenKind::Ident(word) = &token.kind {
        *cursor += 1;
        Ok((word.clone(), token.span))
    } else {
        Err(ParseError::new("expected identifier", token.span))
    }
}

/// The glossary of `source` read under `options`: `None` unless the gate is
/// on, the document is headed `fsl 5`, and it is a requirements document
/// whose first item is a well-formed `glossary` block.
#[must_use]
pub fn document_glossary(source: &str, options: &crate::ParseOptions) -> Option<Glossary> {
    let header = crate::edition_header(source, options).ok()??;
    if header.edition != crate::Edition::Fsl5 {
        return None;
    }
    let tokens = crate::lex_with(source, options).ok()?;
    let mut cursor = 0;
    crate::annotation_parse::leading_annotations(&tokens, &mut cursor).ok()?;
    let opens = tokens.get(cursor..cursor + 4).is_some_and(|head| {
        matches!(&head[0].kind, TokenKind::Ident(word) if crate::keywords::spells(word, Kw::Requirements))
            && matches!(head[1].kind, TokenKind::Ident(_))
            && is_symbol(&head[2], "{")
            && matches!(&head[3].kind, TokenKind::Ident(word) if crate::keywords::spells(word, Kw::Glossary))
    });
    if !opens {
        return None;
    }
    cursor += 3;
    parse_glossary(&tokens, &mut cursor).ok()
}

/// The `(canonical, alias)` pair when the source wrote an alias at
/// `line`:`column` (1-based, as diagnostics report it), either in the body or
/// on the right side of the glossary. Diagnostics use it to show the reader's
/// word beside the canonical name; `None` without a glossary.
#[must_use]
pub fn alias_at(
    source: &str,
    options: &crate::ParseOptions,
    line: u32,
    column: u32,
) -> Option<(String, String)> {
    let glossary = document_glossary(source, options)?;
    let tokens = crate::lex_with(source, options).ok()?;
    let token = tokens
        .iter()
        .find(|token| token.span.start.line == line && token.span.start.column == column)?;
    let TokenKind::Ident(word) = &token.kind else {
        return None;
    };
    glossary
        .entries
        .iter()
        .find(|entry| &entry.alias == word)
        .map(|entry| (entry.canonical.name.clone(), entry.alias.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ParseOptions, RequirementsItem, SurfaceDocument, UnstableFeature, UnstableFeatures,
    };

    fn gated() -> ParseOptions {
        ParseOptions::with_unstable(UnstableFeatures::NONE.with(UnstableFeature::Fsl5))
    }

    const DOCUMENT: &str = "requirements R {\n  glossary {\n    Claim = Shinsei\n    Claim.amount = Kingaku\n  }\n  entity Shinsei\n  state { total: Map<Shinsei, Int> }\n  implements A from \"a.fsl\" { action go(c) -> go(c) }\n}\n";

    /// Every word the shared grammar compares an identifier with is a keyword
    /// spelling, so an alias can never take a keyword's place.
    #[test]
    fn keyword_spellings_cover_the_parser() {
        let sources = [
            include_str!("parser.rs"),
            include_str!("parser/next_requirements.rs"),
            include_str!("parser/next_requirements/rules.rs"),
            include_str!("parser/next_requirements/decide.rs"),
            include_str!("syntax_expr.rs"),
        ];
        let mut missing = BTreeSet::new();
        for source in sources {
            for call in ["peek_ident(\"", "eat_ident(\"", "expect_ident_value(\""] {
                for (index, _) in source.match_indices(call) {
                    let word = source[index + call.len()..]
                        .split('"')
                        .next()
                        .expect("closing quote");
                    if !is_keyword_spelling(word) {
                        missing.insert(word.to_owned());
                    }
                }
            }
        }
        // Every word the parser reads through the keyword table.
        for keyword in crate::keywords::KEYWORDS {
            for word in keyword.english.split(' ') {
                if !KEYWORD_SPELLINGS.contains(&word) {
                    missing.insert(word.to_owned());
                }
            }
        }
        assert!(missing.is_empty(), "not in KEYWORD_SPELLINGS: {missing:?}");
        assert!(
            KEYWORD_SPELLINGS.iter().all(|word| word.is_ascii()),
            "the Japanese spellings come from the keyword table"
        );
        let mut unique = BTreeSet::new();
        assert!(
            KEYWORD_SPELLINGS.iter().all(|word| unique.insert(*word)),
            "a keyword spelling is listed twice"
        );
    }

    /// `alias_shadows_keyword` reads the keyword table: every Japanese
    /// spelling of every context, reserved rows included, and every particle
    /// is a keyword spelling, and no other non-ASCII word is.
    #[test]
    fn japanese_keyword_spellings_are_the_keyword_tables() {
        for keyword in crate::keywords::KEYWORDS {
            assert!(is_keyword_spelling(keyword.japanese), "{:?}", keyword.kw);
        }
        for (_, particle) in crate::keywords::PARTICLES {
            assert!(is_keyword_spelling(particle), "{particle}");
        }
        assert!(!is_keyword_spelling("申請"));
    }

    /// The block is the next edition's: elsewhere `glossary` is the published
    /// language's unknown requirements declaration.
    #[test]
    fn glossary_is_read_only_behind_the_gate_in_an_fsl5_document() {
        for (source, options) in [
            (DOCUMENT.to_owned(), ParseOptions::stable()),
            (DOCUMENT.to_owned(), gated()),
            (format!("fsl 4\n{DOCUMENT}"), gated()),
        ] {
            let error = crate::parse_surface_document_with(&source, &options).unwrap_err();
            assert_eq!(error.code(), "FSL-PARSE", "{source}");
            assert_eq!(error.message, "expected requirements declaration");
        }
        let parsed =
            crate::parse_surface_document_with(&format!("fsl 5\n{DOCUMENT}"), &gated()).unwrap();
        let SurfaceDocument::Requirements(requirements) = parsed else {
            panic!("requirements document");
        };
        let RequirementsItem::Glossary(glossary) = &requirements.items[0] else {
            panic!("the glossary is the first item");
        };
        assert_eq!(glossary.alias_of("Claim.amount"), Some("Kingaku"));
        assert!(
            matches!(
                &requirements.items[1],
                RequirementsItem::Common(crate::SpecItem::Entity(name, _)) if name == "Claim"
            ),
            "{:?}",
            requirements.items[1]
        );
        // The alias at `entity Shinsei` and at `Map<Shinsei, ..>` resolved;
        // the canonical names inside implements are not recorded.
        assert_eq!(
            glossary
                .uses
                .iter()
                .map(|name| (name.canonical.as_str(), name.via_alias))
                .collect::<Vec<_>>(),
            vec![("Claim", true), ("Claim", true)]
        );
    }

    #[test]
    fn alias_at_names_the_written_alias() {
        let source = format!("fsl 5\n{DOCUMENT}");
        // `entity Shinsei` is line 7 (after the header), column 10.
        assert_eq!(
            alias_at(&source, &gated(), 7, 10),
            Some(("Claim".to_owned(), "Shinsei".to_owned()))
        );
        assert_eq!(alias_at(&source, &gated(), 7, 3), None, "the keyword");
        assert_eq!(alias_at(&source, &ParseOptions::stable(), 7, 10), None);
    }
}

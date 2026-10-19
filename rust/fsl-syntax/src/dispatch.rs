// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::BTreeSet;
use std::sync::LazyLock;

use crate::annotation_parse::leading_annotations;
use crate::lexer::lex_declaration_prefix;
use crate::{Annotations, ParseError, ParseOptions, SurfaceDocument, Token, TokenKind, lex_with};

/// Original source passed beside the shared token stream to a dialect frontend.
#[derive(Clone, Copy, Debug)]
pub struct SourceFile<'a> {
    source: &'a str,
}

impl<'a> SourceFile<'a> {
    #[must_use]
    pub const fn new(source: &'a str) -> Self {
        Self { source }
    }

    #[must_use]
    pub const fn source(self) -> &'a str {
        self.source
    }
}

/// One parsed surface document plus annotations attached before its declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedDocument {
    pub keyword: &'static str,
    pub annotations: Annotations,
    pub surface: SurfaceDocument,
}

type Frontend =
    for<'a> fn(SourceFile<'a>, Vec<Token>, usize) -> Result<SurfaceDocument, ParseError>;

#[derive(Clone, Copy)]
struct FrontendRegistration {
    keyword: &'static str,
    parse: Frontend,
}

macro_rules! frontends {
    ($($keyword:literal => $parse:path),+ $(,)?) => {
        pub const DIALECT_KEYWORDS: &[&str] = &[$($keyword),+];
        const FRONTENDS: &[FrontendRegistration] = &[
            $(FrontendRegistration { keyword: $keyword, parse: $parse }),+
        ];
    };
}

frontends! {
    "spec" => parse_shared,
    "refinement" => parse_shared,
    "compose" => parse_shared,
    "business" => parse_shared,
    "governance" => parse_shared,
    "requirements" => parse_shared,
    "domain" => parse_domain,
    "dbsystem" => parse_db,
    "ai_component" => parse_ai,
    "agent" => parse_agent,
}

static VALID_REGISTRY: LazyLock<()> = LazyLock::new(|| {
    validate_registry(FRONTENDS).expect("duplicate FSL dialect registry keyword");
});

/// Validate that every registered top-level keyword is unique.
///
/// # Errors
///
/// Returns the duplicated keyword when registry construction is invalid.
pub fn validate_frontend_registry() -> Result<(), String> {
    validate_registry(FRONTENDS)
}

fn validate_registry(registry: &[FrontendRegistration]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for registration in registry {
        if !seen.insert(registration.keyword) {
            return Err(format!(
                "duplicate FSL dialect registry keyword '{}'",
                registration.keyword
            ));
        }
    }
    Ok(())
}

/// Return the declaration keyword selected by the shared lexer and annotation scanner.
///
/// # Errors
///
/// Returns a coded parse error for empty, annotation-only, or unknown documents.
pub fn dialect_keyword(source: &str) -> Result<&'static str, ParseError> {
    dialect_keyword_with(source, &ParseOptions::stable())
}

/// [`dialect_keyword`] under explicit [`ParseOptions`].
///
/// # Errors
///
/// Returns a coded parse error for empty, annotation-only, or unknown documents.
pub fn dialect_keyword_with(
    source: &str,
    options: &ParseOptions,
) -> Result<&'static str, ParseError> {
    let tokens = lex_with(source, options).map_err(ParseError::from)?;
    let mut cursor = 0;
    leading_annotations(&tokens, &mut cursor)?;
    select_frontend(&tokens, cursor).map(|registration| registration.keyword)
}

/// Return the first significant declaration identifier, including identifiers
/// owned by feature-level parsers rather than the dialect registry.
///
/// # Errors
///
/// Returns a coded parse error when the document has no declaration identifier.
pub fn declaration_keyword(source: &str) -> Result<String, ParseError> {
    let tokens = lex_declaration_prefix(source).map_err(ParseError::from)?;
    let mut cursor = 0;
    leading_annotations(&tokens, &mut cursor)?;
    declaration_identifier(&tokens, cursor).map(str::to_owned)
}

/// Lex once, select a registered frontend, and parse using that same token stream.
///
/// This is the published language: it is [`parse_document_with`] under
/// [`ParseOptions::stable`].
///
/// # Errors
///
/// Returns lexical, annotation, dispatch, or frontend syntax errors.
pub fn parse_document(source: SourceFile<'_>) -> Result<ParsedDocument, ParseError> {
    parse_document_with(source, &ParseOptions::stable())
}

/// [`parse_document`] under explicit [`ParseOptions`]. Every registered
/// frontend is reached only through this function.
///
/// No unstable feature changes parsing yet, so the options do not affect the
/// result; later stages attach syntax to them.
///
/// # Errors
///
/// Returns lexical, annotation, dispatch, or frontend syntax errors.
pub fn parse_document_with(
    source: SourceFile<'_>,
    options: &ParseOptions,
) -> Result<ParsedDocument, ParseError> {
    LazyLock::force(&VALID_REGISTRY);
    let tokens = lex_with(source.source(), options).map_err(ParseError::from)?;
    let mut cursor = 0;
    let annotations = leading_annotations(&tokens, &mut cursor)?;
    let registration = select_frontend(&tokens, cursor)?;
    let surface = (registration.parse)(source, tokens, cursor)?;
    Ok(ParsedDocument {
        keyword: registration.keyword,
        annotations,
        surface,
    })
}

fn select_frontend(
    tokens: &[Token],
    cursor: usize,
) -> Result<&'static FrontendRegistration, ParseError> {
    let token = &tokens[cursor];
    let keyword = declaration_identifier(tokens, cursor)?;
    FRONTENDS
        .iter()
        .find(|registration| registration.keyword == keyword)
        .ok_or_else(|| unknown_dialect(token))
}

fn declaration_identifier(tokens: &[Token], cursor: usize) -> Result<&str, ParseError> {
    let token = &tokens[cursor];
    match &token.kind {
        TokenKind::Eof => Err(ParseError::coded(
            if cursor == 0 {
                "FSL-DIALECT-EMPTY"
            } else {
                "FSL-DIALECT-ANNOTATION-TARGET"
            },
            if cursor == 0 {
                format!(
                    "empty or comment-only FSL document; supported dialect keywords: {}",
                    DIALECT_KEYWORDS.join(", ")
                )
            } else {
                "top-level annotation must be followed by a declaration".to_owned()
            },
            token.span,
        )),
        TokenKind::Ident(keyword) => Ok(keyword),
        _ => Err(unknown_dialect(token)),
    }
}

fn unknown_dialect(token: &Token) -> ParseError {
    let found = match &token.kind {
        TokenKind::Ident(value) | TokenKind::String(value) | TokenKind::Symbol(value) => {
            value.clone()
        }
        TokenKind::Int(value) => value.to_string(),
        TokenKind::Eof => "end of file".to_owned(),
    };
    ParseError::coded(
        "FSL-DIALECT-UNKNOWN",
        format!(
            "unsupported top-level declaration '{found}'; supported dialect keywords: {}",
            DIALECT_KEYWORDS.join(", ")
        ),
        token.span,
    )
}

fn parse_shared(
    source: SourceFile<'_>,
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<SurfaceDocument, ParseError> {
    crate::parser::parse_shared_tokens(source.source(), tokens, cursor)
}

fn parse_db(
    _source: SourceFile<'_>,
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<SurfaceDocument, ParseError> {
    crate::db::parse_db_system_tokens(tokens, cursor).map(SurfaceDocument::Db)
}

fn parse_domain(
    _source: SourceFile<'_>,
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<SurfaceDocument, ParseError> {
    crate::domain::parse_domain_tokens(tokens, cursor).map(SurfaceDocument::Domain)
}

fn parse_ai(
    source: SourceFile<'_>,
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<SurfaceDocument, ParseError> {
    crate::ai::parse_ai_component_tokens(source.source(), tokens, cursor)
        .map(SurfaceDocument::AiComponent)
}

fn parse_agent(
    source: SourceFile<'_>,
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<SurfaceDocument, ParseError> {
    crate::ai::parse_agent_tokens(source.source(), tokens, cursor).map(SurfaceDocument::Agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_rejects_duplicate_keywords() {
        let duplicate = [
            FrontendRegistration {
                keyword: "spec",
                parse: parse_shared,
            },
            FrontendRegistration {
                keyword: "spec",
                parse: parse_agent,
            },
        ];
        assert_eq!(
            validate_registry(&duplicate).unwrap_err(),
            "duplicate FSL dialect registry keyword 'spec'"
        );
    }

    #[test]
    fn every_registered_dialect_uses_the_significant_keyword_rule() {
        validate_frontend_registry().unwrap();
        for keyword in DIALECT_KEYWORDS {
            let source = format!(
                "\u{feff}// leading comment\n@acme.route(spec, requirements)\n{keyword} Name {{}}"
            );
            assert_eq!(dialect_keyword(&source).unwrap(), *keyword);
        }
    }

    #[test]
    fn annotation_keyword_arguments_do_not_drive_dispatch() {
        let parsed = parse_document(SourceFile::new(
            "\u{feff}@acme.route(spec, requirements)\n// target follows\ndomain Orders {}",
        ))
        .unwrap();
        assert_eq!(parsed.keyword, "domain");
        assert_eq!(parsed.annotations.source_order().len(), 1);
        assert_eq!(
            declaration_keyword("@acme.route(spec)\ndataset Eval {}"),
            Ok("dataset".to_owned())
        );
        assert_eq!(
            declaration_keyword("ai_action Draft { arbitrary & opaque ? text }"),
            Ok("ai_action".to_owned())
        );
    }

    #[test]
    fn specialized_frontends_parse_after_leading_trivia() {
        let db = parse_document(SourceFile::new(
            "// comment\ndbsystem Demo { database app { schema 0 table users { column id: Int present not_null; } } }",
        ))
        .unwrap();
        let ai = parse_document(SourceFile::new(
            "\u{feff}// comment\nai_component Demo { tool Search { schema SearchV1; } authority { may_execute Search; } }",
        ))
        .unwrap();
        assert!(matches!(db.surface, SurfaceDocument::Db(_)));
        assert!(matches!(ai.surface, SurfaceDocument::AiComponent(_)));
    }

    #[test]
    fn empty_and_unknown_documents_have_stable_diagnostics() {
        let empty = parse_document(SourceFile::new(" // only a comment\n")).unwrap_err();
        assert_eq!(empty.code(), "FSL-DIALECT-EMPTY");
        assert!(empty.message.contains("spec, refinement, compose"));
        let unknown = parse_document(SourceFile::new("mystery X {}")).unwrap_err();
        assert_eq!(unknown.code(), "FSL-DIALECT-UNKNOWN");
        assert_eq!(unknown.span.start.column, 1);
        assert!(unknown.message.contains("spec, refinement, compose"));
    }

    /// decisions/049 D2: a document that declares a syntax edition in its
    /// first line is never read as the published language. Under stable
    /// options every document entry rejects the header with the same
    /// unknown-declaration error a user sees today, and the gate adds no
    /// syntax yet, so enabling it does not change any of these results.
    #[test]
    fn edition_header_fails_closed_under_stable_options() {
        let gated = ParseOptions::with_unstable(
            crate::UnstableFeatures::NONE.with(crate::UnstableFeature::Fsl5),
        );
        for source in [
            "fsl 5\nrequirements R {}\n",
            "fsl 4\nspec S { state { x: Int } }\n",
            "\u{feff}// header after trivia\nfsl 5\nrequirements R {}\n",
        ] {
            let error = parse_document(SourceFile::new(source)).unwrap_err();
            assert_eq!(error.code(), "FSL-DIALECT-UNKNOWN", "{source:?}");
            assert!(error.message.contains("'fsl'"), "{}", error.message);
            assert_eq!(
                crate::parse_surface_document(source).unwrap_err(),
                error,
                "{source:?}"
            );
            assert_eq!(dialect_keyword(source).unwrap_err(), error, "{source:?}");

            assert_eq!(
                parse_document_with(SourceFile::new(source), &gated).unwrap_err(),
                error,
                "{source:?}"
            );
            assert_eq!(
                dialect_keyword_with(source, &gated).unwrap_err(),
                error,
                "{source:?}"
            );
            assert_eq!(
                crate::lex_with(source, &gated).unwrap(),
                crate::lex(source).unwrap()
            );
        }
    }

    #[test]
    fn agent_frontend_rejects_extra_tokens() {
        for source in ["agent A junk {}", "agent A {} spec B {}"] {
            let error = parse_document(SourceFile::new(source)).unwrap_err();
            assert_eq!(error.code(), "FSL-PARSE");
        }
    }
}

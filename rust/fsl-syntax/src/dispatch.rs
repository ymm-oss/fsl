// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::collections::BTreeSet;
use std::sync::LazyLock;

use crate::lexer::lex_declaration_prefix;
use crate::{
    Annotation, AnnotationValue, Annotations, ParseError, Span, SurfaceAgent, SurfaceDocument,
    SymbolPath, SyntaxIdent, Token, TokenKind, lex,
};

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
    let tokens = lex(source).map_err(ParseError::from)?;
    let (_, cursor) = leading_annotations(&tokens)?;
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
    let (_, cursor) = leading_annotations(&tokens)?;
    declaration_identifier(&tokens, cursor).map(str::to_owned)
}

/// Lex once, select a registered frontend, and parse using that same token stream.
///
/// # Errors
///
/// Returns lexical, annotation, dispatch, or frontend syntax errors.
pub fn parse_document(source: SourceFile<'_>) -> Result<ParsedDocument, ParseError> {
    LazyLock::force(&VALID_REGISTRY);
    let tokens = lex(source.source()).map_err(ParseError::from)?;
    let (annotations, cursor) = leading_annotations(&tokens)?;
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
    _source: SourceFile<'_>,
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<SurfaceDocument, ParseError> {
    crate::parser::parse_shared_tokens(tokens, cursor)
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
    _source: SourceFile<'_>,
    tokens: Vec<Token>,
    cursor: usize,
) -> Result<SurfaceDocument, ParseError> {
    let mut tokens = tokens.into_iter().skip(cursor);
    let start = tokens.next().expect("dispatch selected agent token").span;
    let name = match tokens.next() {
        Some(Token {
            kind: TokenKind::Ident(name),
            ..
        }) => name,
        Some(token) => {
            return Err(ParseError::coded(
                "FSL-PARSE",
                "expected agent name",
                token.span,
            ));
        }
        None => unreachable!("lexer always appends EOF"),
    };
    let open = tokens.next().expect("lexer appends EOF");
    if !matches!(&open.kind, TokenKind::Symbol(symbol) if symbol == "{") {
        return Err(ParseError::new("expected '{' after agent name", open.span));
    }
    let mut depth = 1_usize;
    let end = loop {
        let token = tokens.next().expect("lexer appends EOF");
        match &token.kind {
            TokenKind::Symbol(symbol) if symbol == "{" => {
                depth += 1;
            }
            TokenKind::Symbol(symbol) if symbol == "}" => {
                depth -= 1;
                if depth == 0 {
                    break token.span;
                }
            }
            TokenKind::Eof => {
                return Err(ParseError::new(
                    "agent declaration must contain balanced braces",
                    token.span,
                ));
            }
            _ => {}
        }
    };
    let trailing = tokens.next().expect("lexer appends EOF");
    if !matches!(trailing.kind, TokenKind::Eof) {
        return Err(ParseError::new(
            "unexpected token after agent",
            trailing.span,
        ));
    }
    Ok(SurfaceDocument::Agent(SurfaceAgent {
        name,
        span: Span {
            start: start.start,
            end: end.end,
        },
    }))
}

fn leading_annotations(tokens: &[Token]) -> Result<(Annotations, usize), ParseError> {
    let mut cursor = 0;
    let mut annotations = Vec::new();
    while symbol(tokens, cursor, "@") {
        annotations.push(annotation(tokens, &mut cursor)?);
    }
    let annotations = Annotations::new(annotations);
    annotations
        .validate()
        .map_err(|error| ParseError::coded("FSL-ANNOTATION-INVALID", error.message, error.span))?;
    Ok((annotations, cursor))
}

fn annotation(tokens: &[Token], cursor: &mut usize) -> Result<Annotation, ParseError> {
    let start = tokens[*cursor].span;
    *cursor += 1;
    let (path, _) = symbol_path(tokens, cursor)?;
    expect_symbol(tokens, cursor, "(")?;
    let mut arguments = Vec::new();
    if !symbol(tokens, *cursor, ")") {
        loop {
            arguments.push(annotation_value(tokens, cursor)?);
            if symbol(tokens, *cursor, ")") {
                break;
            }
            expect_symbol(tokens, cursor, ",")?;
        }
    }
    let end = tokens[*cursor].span;
    *cursor += 1;
    let span = Span {
        start: start.start,
        end: end.end,
    };
    match path.segments() {
        [name] if name == "requirement" => match arguments.as_slice() {
            [AnnotationValue::String(id)] => Ok(Annotation::Requirement {
                id: id.clone(),
                text: None,
                span,
            }),
            [AnnotationValue::String(id), AnnotationValue::String(text)] => {
                Ok(Annotation::Requirement {
                    id: id.clone(),
                    text: Some(text.clone()),
                    span,
                })
            }
            _ => Err(annotation_shape(
                "requirement",
                "one or two string arguments",
                span,
            )),
        },
        [name] if name == "undecided" => match arguments.as_slice() {
            [AnnotationValue::String(reason)] => Ok(Annotation::Undecided {
                reason: reason.clone(),
                span,
            }),
            _ => Err(annotation_shape("undecided", "one string argument", span)),
        },
        [name] if name == "kind" => match arguments.as_slice() {
            [AnnotationValue::String(id)] => Ok(Annotation::Kind {
                id: id.clone(),
                text: None,
                span,
            }),
            [AnnotationValue::String(id), AnnotationValue::String(text)] => Ok(Annotation::Kind {
                id: id.clone(),
                text: Some(text.clone()),
                span,
            }),
            _ => Err(annotation_shape(
                "kind",
                "one or two string arguments",
                span,
            )),
        },
        _ => Ok(Annotation::Custom {
            namespace: path,
            arguments,
            span,
        }),
    }
}

fn annotation_shape(name: &str, expected: &str, span: Span) -> ParseError {
    ParseError::coded(
        "FSL-ANNOTATION-ARGUMENTS",
        format!("@{name} expects {expected}"),
        span,
    )
}

fn annotation_value(tokens: &[Token], cursor: &mut usize) -> Result<AnnotationValue, ParseError> {
    let token = &tokens[*cursor];
    match &token.kind {
        TokenKind::String(value) => {
            *cursor += 1;
            Ok(AnnotationValue::String(value.clone()))
        }
        TokenKind::Int(value) => {
            *cursor += 1;
            Ok(AnnotationValue::Integer(*value))
        }
        TokenKind::Ident(value) if value == "true" || value == "false" => {
            *cursor += 1;
            Ok(AnnotationValue::Boolean(value == "true"))
        }
        TokenKind::Ident(_) => {
            symbol_path(tokens, cursor).map(|(path, _)| AnnotationValue::Symbol(path))
        }
        _ => Err(ParseError::coded(
            "FSL-ANNOTATION-ARGUMENTS",
            "annotation argument must be a string, integer, Boolean, or symbol path",
            token.span,
        )),
    }
}

fn symbol_path(tokens: &[Token], cursor: &mut usize) -> Result<(SymbolPath, Span), ParseError> {
    let start = tokens[*cursor].span;
    let mut segments = Vec::new();
    loop {
        match &tokens[*cursor].kind {
            TokenKind::Ident(segment) => {
                segments.push(SyntaxIdent {
                    text: segment.clone(),
                    span: tokens[*cursor].span,
                });
                *cursor += 1;
            }
            _ => {
                return Err(ParseError::coded(
                    "FSL-ANNOTATION-PATH",
                    "expected annotation symbol path segment",
                    tokens[*cursor].span,
                ));
            }
        }
        if !symbol(tokens, *cursor, ".") {
            break;
        }
        *cursor += 1;
    }
    let end = tokens[*cursor - 1].span;
    let span = Span {
        start: start.start,
        end: end.end,
    };
    SymbolPath::from_idents(segments, span)
        .map(|path| (path, span))
        .map_err(|error| ParseError::coded("FSL-ANNOTATION-PATH", error.message, error.span))
}

fn expect_symbol(tokens: &[Token], cursor: &mut usize, expected: &str) -> Result<(), ParseError> {
    if symbol(tokens, *cursor, expected) {
        *cursor += 1;
        Ok(())
    } else {
        Err(ParseError::coded(
            "FSL-ANNOTATION-SYNTAX",
            format!("expected '{expected}' in annotation"),
            tokens[*cursor].span,
        ))
    }
}

fn symbol(tokens: &[Token], cursor: usize, expected: &str) -> bool {
    matches!(tokens.get(cursor).map(|token| &token.kind), Some(TokenKind::Symbol(value)) if value == expected)
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

    #[test]
    fn agent_frontend_rejects_extra_tokens() {
        for source in ["agent A junk {}", "agent A {} spec B {}"] {
            let error = parse_document(SourceFile::new(source)).unwrap_err();
            assert_eq!(error.code(), "FSL-PARSE");
        }
    }
}

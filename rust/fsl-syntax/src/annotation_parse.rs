// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Shared `@name(args...)` token-level parsing, used both for the one
//! top-level annotation group a document's dialect keyword carries
//! (`dispatch.rs`) and for annotation groups attached to declarations nested
//! inside a dialect body (`parser.rs`).

use crate::{
    Annotation, AnnotationValue, Annotations, ParseError, Span, SymbolPath, SyntaxIdent, Token,
    TokenKind,
};

/// Parse zero or more leading `@name(args...)` annotations starting at
/// `*cursor`, advancing it past them, and validate the resulting group.
///
/// # Errors
///
/// Returns [`ParseError`] for malformed annotation syntax or a validation
/// failure (empty requirement ID, reserved `undecided` ID, conflicting
/// requirement text, etc.).
pub(crate) fn leading_annotations(
    tokens: &[Token],
    cursor: &mut usize,
) -> Result<Annotations, ParseError> {
    let mut annotations = Vec::new();
    while symbol(tokens, *cursor, "@") {
        annotations.push(annotation(tokens, cursor)?);
    }
    let annotations = Annotations::new(annotations);
    annotations
        .validate()
        .map_err(|error| ParseError::coded("FSL-ANNOTATION-INVALID", error.message, error.span))?;
    Ok(annotations)
}

pub(crate) fn annotation(tokens: &[Token], cursor: &mut usize) -> Result<Annotation, ParseError> {
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

pub(crate) fn annotation_value(
    tokens: &[Token],
    cursor: &mut usize,
) -> Result<AnnotationValue, ParseError> {
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

pub(crate) fn symbol_path(
    tokens: &[Token],
    cursor: &mut usize,
) -> Result<(SymbolPath, Span), ParseError> {
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

pub(crate) fn expect_symbol(
    tokens: &[Token],
    cursor: &mut usize,
    expected: &str,
) -> Result<(), ParseError> {
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

pub(crate) fn symbol(tokens: &[Token], cursor: usize, expected: &str) -> bool {
    matches!(tokens.get(cursor).map(|token| &token.kind), Some(TokenKind::Symbol(value)) if value == expected)
}

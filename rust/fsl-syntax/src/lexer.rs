// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::fmt;

use crate::identifier::{self, IdentifierRule};
use crate::{SourcePos, Span};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Int(i64),
    String(String),
    Symbol(String),
    Eof,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexError {
    pub message: String,
    pub span: Span,
    code: &'static str,
}

impl LexError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self::coded("FSL-PARSE", message.into(), span)
    }

    const fn coded(code: &'static str, message: String, span: Span) -> Self {
        Self {
            message,
            span,
            code,
        }
    }

    /// The diagnostic code: `FSL-PARSE`, or one of
    /// [`crate::IDENTIFIER_DIAGNOSTICS`] under [`IdentifierRule::Unicode`].
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

/// A finding the lexer reports beside a document it still reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LexWarning {
    /// One of the warning codes of [`crate::IDENTIFIER_DIAGNOSTICS`].
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    /// The text that reads the same without the warning.
    pub replacement: String,
}

impl fmt::Display for LexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}:{}",
            self.message, self.span.start.line, self.span.start.column
        )
    }
}

impl std::error::Error for LexError {}

/// Tokenize FSL source while retaining byte offsets and one-based locations.
///
/// # Errors
///
/// Returns [`LexError`] for an unsupported character, an unterminated string,
/// or an integer literal outside the accepted `i64` representation.
pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    lex_with(source, &crate::ParseOptions::stable())
}

/// [`lex`] under explicit [`crate::ParseOptions`], with the identifier rule
/// [`IdentifierRule::for_source`] selects. The tokens of an edition header
/// ([`crate::edition_header`]) are not part of the stream.
///
/// # Errors
///
/// Returns [`LexError`] for characters or literals the lexer rejects.
pub fn lex_with(source: &str, options: &crate::ParseOptions) -> Result<Vec<Token>, LexError> {
    let mut tokens = lex_with_header(source, *options)?;
    if matches!(crate::edition_header(source, options), Ok(Some(_))) {
        debug_assert!(
            matches!(&tokens[0].kind, TokenKind::Ident(name) if name == "fsl")
                && matches!(tokens[1].kind, TokenKind::Int(_))
        );
        tokens.drain(..2);
    }
    Ok(tokens)
}

/// [`lex_with`] keeping the edition header's tokens, for the lossless tree.
pub(crate) fn lex_with_header(
    source: &str,
    options: crate::ParseOptions,
) -> Result<Vec<Token>, LexError> {
    Lexer::new(source, IdentifierRule::for_source(source, &options)).lex_all()
}

/// The warnings lexing `source` under `options` reports, up to the first
/// lexical error. Always empty under [`IdentifierRule::Ascii`].
#[must_use]
pub fn lexical_warnings(source: &str, options: &crate::ParseOptions) -> Vec<LexWarning> {
    let rule = IdentifierRule::for_source(source, options);
    if rule == IdentifierRule::Ascii {
        return Vec::new();
    }
    let mut lexer = Lexer::new(source, rule);
    while lexer
        .next_token()
        .is_ok_and(|token| !matches!(token.kind, TokenKind::Eof))
    {}
    lexer.warnings
}

pub(crate) fn is_single_identifier(name: &str, rule: IdentifierRule) -> bool {
    let mut lexer = Lexer::new(name, rule);
    let Ok(token) = lexer.next_token() else {
        return false;
    };
    matches!(&token.kind, TokenKind::Ident(value) if value == name)
        && token.span.start.offset == 0
        && token.span.end.offset == name.len()
        && lexer
            .next_token()
            .is_ok_and(|token| matches!(token.kind, TokenKind::Eof))
        && lexer.warnings.is_empty()
}

pub(crate) fn lex_declaration_prefix(source: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer::new(source, IdentifierRule::Ascii);
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next_token()?;
        let annotation = matches!(&token.kind, TokenKind::Symbol(symbol) if symbol == "@");
        let eof = matches!(token.kind, TokenKind::Eof);
        tokens.push(token);
        if eof || !annotation {
            return Ok(tokens);
        }
        loop {
            let token = lexer.next_token()?;
            let annotation_end = matches!(&token.kind, TokenKind::Symbol(symbol) if symbol == ")");
            let eof = matches!(token.kind, TokenKind::Eof);
            tokens.push(token);
            if annotation_end || eof {
                break;
            }
        }
    }
}

struct Lexer<'a> {
    source: &'a str,
    offset: usize,
    line: u32,
    column: u32,
    rule: IdentifierRule,
    warnings: Vec<LexWarning>,
}

impl<'a> Lexer<'a> {
    const fn new(source: &'a str, rule: IdentifierRule) -> Self {
        Self {
            source,
            offset: 0,
            line: 1,
            column: 1,
            rule,
            warnings: Vec::new(),
        }
    }

    fn lex_all(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let eof = matches!(token.kind, TokenKind::Eof);
            tokens.push(token);
            if eof {
                return Ok(tokens);
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_trivia();
        let start = self.pos();
        let Some(ch) = self.peek() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                span: Span { start, end: start },
            });
        };
        let kind = if self.rule.starts(ch) {
            self.lex_ident(start)?
        } else if ch.is_ascii_digit() {
            self.lex_int(start)?
        } else if ch == '"' {
            self.lex_string(start)?
        } else {
            self.lex_symbol(start)?
        };
        Ok(Token {
            kind,
            span: Span {
                start,
                end: self.pos(),
            },
        })
    }

    fn skip_trivia(&mut self) {
        loop {
            if self.offset == 0 && self.peek() == Some('\u{feff}') {
                self.bump();
            }
            while self.peek().is_some_and(char::is_whitespace) {
                let start = self.pos();
                let ch = self.bump();
                if self.rule == IdentifierRule::Unicode && ch == Some('\u{3000}') {
                    self.warnings.push(LexWarning {
                        code: identifier::FULLWIDTH_SPACE,
                        message: "full-width space U+3000 is read as a space; write ' '".to_owned(),
                        span: Span {
                            start,
                            end: self.pos(),
                        },
                        replacement: " ".to_owned(),
                    });
                }
            }
            if self.remaining().starts_with("//") {
                while self.peek().is_some_and(|ch| ch != '\n') {
                    self.bump();
                }
            } else {
                break;
            }
        }
    }

    fn lex_ident(&mut self, start: SourcePos) -> Result<TokenKind, LexError> {
        let rule = self.rule;
        while self.peek().is_some_and(|ch| rule.continues(ch)) {
            self.bump();
        }
        let raw = &self.source[start.offset..self.offset];
        if rule == IdentifierRule::Ascii {
            return Ok(TokenKind::Ident(raw.to_owned()));
        }
        let span = Span {
            start,
            end: self.pos(),
        };
        let name = identifier::unicode_identifier(raw)
            .map_err(|(code, message)| LexError::coded(code, message, span))?;
        if name != raw {
            self.warnings.push(LexWarning {
                code: identifier::NON_NFC_IDENTIFIER,
                message: format!(
                    "identifier is not in Unicode normalization form C; it is read as '{name}'"
                ),
                span,
                replacement: name.clone(),
            });
        }
        Ok(TokenKind::Ident(name))
    }

    fn lex_int(&mut self, start: SourcePos) -> Result<TokenKind, LexError> {
        let start_offset = self.offset;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.bump();
        }
        let text = &self.source[start_offset..self.offset];
        text.parse::<i64>().map(TokenKind::Int).map_err(|_| {
            LexError::new(
                "integer literal is outside the i64 range",
                Span {
                    start,
                    end: self.pos(),
                },
            )
        })
    }

    fn lex_string(&mut self, start: SourcePos) -> Result<TokenKind, LexError> {
        self.bump();
        let content_start = self.offset;
        while let Some(ch) = self.peek() {
            if ch == '"' {
                let text = self.source[content_start..self.offset].to_owned();
                self.bump();
                return Ok(TokenKind::String(text));
            }
            if ch == '\n' {
                break;
            }
            self.bump();
        }
        Err(LexError::new(
            "unterminated string literal",
            Span {
                start,
                end: self.pos(),
            },
        ))
    }

    fn lex_symbol(&mut self, start: SourcePos) -> Result<TokenKind, LexError> {
        const DOUBLE: [&str; 9] = ["=>", "==", "!=", "<=", ">=", "~>", "..", "||", "->"];
        if let Some(symbol) = DOUBLE
            .iter()
            .find(|symbol| self.remaining().starts_with(**symbol))
        {
            self.bump();
            self.bump();
            return Ok(TokenKind::Symbol((*symbol).to_owned()));
        }
        let ch = self.bump().expect("peeked character");
        if "{}()[],:;.+-*/%<>=.|@".contains(ch) {
            return Ok(TokenKind::Symbol(ch.to_string()));
        }
        let span = Span {
            start,
            end: self.pos(),
        };
        match identifier::unexpected_character(ch) {
            Some((code, message)) if self.rule == IdentifierRule::Unicode => {
                Err(LexError::coded(code, message, span))
            }
            _ => Err(LexError::new(format!("unexpected character {ch:?}"), span)),
        }
    }

    fn remaining(&self) -> &'a str {
        &self.source[self.offset..]
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    fn pos(&self) -> SourcePos {
        SourcePos {
            offset: self.offset,
            line: self.line,
            column: self.column,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_are_trivia_and_locations_are_one_based() {
        let tokens = lex("// hi\nfoo <= 12").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Ident("foo".to_owned()));
        assert_eq!(tokens[0].span.start.line, 2);
        assert_eq!(tokens[0].span.start.column, 1);
        assert_eq!(tokens[1].kind, TokenKind::Symbol("<=".to_owned()));
    }

    #[test]
    fn leading_bom_is_trivia() {
        let tokens = lex("\u{feff}  spec Example {}").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Ident("spec".to_owned()));
        assert_eq!(tokens[0].span.start.column, 4);
    }

    #[test]
    fn rejects_i64_overflow_instead_of_wrapping() {
        let error = lex("9223372036854775808").unwrap_err();
        assert!(error.message.contains("i64"));
    }
}

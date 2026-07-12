// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

use std::fmt;

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
    Lexer::new(source).lex_all()
}

struct Lexer<'a> {
    source: &'a str,
    offset: usize,
    line: u32,
    column: u32,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    fn lex_all(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia();
            let start = self.pos();
            let Some(ch) = self.peek() else {
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    span: Span { start, end: start },
                });
                return Ok(tokens);
            };
            let kind = if ch.is_ascii_alphabetic() || ch == '_' {
                self.lex_ident()
            } else if ch.is_ascii_digit() {
                self.lex_int(start)?
            } else if ch == '"' {
                self.lex_string(start)?
            } else {
                self.lex_symbol(start)?
            };
            tokens.push(Token {
                kind,
                span: Span {
                    start,
                    end: self.pos(),
                },
            });
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.bump();
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

    fn lex_ident(&mut self) -> TokenKind {
        let start = self.offset;
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            self.bump();
        }
        TokenKind::Ident(self.source[start..self.offset].to_owned())
    }

    fn lex_int(&mut self, start: SourcePos) -> Result<TokenKind, LexError> {
        let start_offset = self.offset;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.bump();
        }
        let text = &self.source[start_offset..self.offset];
        text.parse::<i64>()
            .map(TokenKind::Int)
            .map_err(|_| LexError {
                message: "integer literal is outside the i64 range".to_owned(),
                span: Span {
                    start,
                    end: self.pos(),
                },
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
        Err(LexError {
            message: "unterminated string literal".to_owned(),
            span: Span {
                start,
                end: self.pos(),
            },
        })
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
        if "{}()[],:;.+-*/%<>=.|".contains(ch) {
            Ok(TokenKind::Symbol(ch.to_string()))
        } else {
            Err(LexError {
                message: format!("unexpected character {ch:?}"),
                span: Span {
                    start,
                    end: self.pos(),
                },
            })
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
    fn rejects_i64_overflow_instead_of_wrapping() {
        let error = lex("9223372036854775808").unwrap_err();
        assert!(error.message.contains("i64"));
    }
}

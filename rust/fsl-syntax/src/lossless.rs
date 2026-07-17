// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Lossless source nodes and the shared, trivia-preserving formatter.

use std::fmt;

use crate::{LexError, ParseError, SourceFile, Span, TokenKind, lex, parse_document};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LosslessKind {
    Token(TokenKind),
    Whitespace,
    LineComment,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LosslessNode {
    pub kind: LosslessKind,
    pub text: String,
    pub span: Span,
}

impl LosslessNode {
    #[must_use]
    pub fn ident(&self) -> Option<&str> {
        match &self.kind {
            LosslessKind::Token(TokenKind::Ident(value)) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn symbol(&self) -> Option<&str> {
        match &self.kind {
            LosslessKind::Token(TokenKind::Symbol(value)) => Some(value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LosslessDocument {
    source: String,
    nodes: Vec<LosslessNode>,
    error: Option<LexError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceEdit {
    pub span: Span,
    pub replacement: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalRewriteKind {
    DomainEnum,
    LogicalOperator,
    Quantifier,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalRewrite {
    pub kind: CanonicalRewriteKind,
    pub span: Span,
    pub canonical_replacement: String,
    pub edits: Vec<SourceEdit>,
}

impl LosslessDocument {
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn nodes(&self) -> &[LosslessNode] {
        &self.nodes
    }

    #[must_use]
    pub const fn error(&self) -> Option<&LexError> {
        self.error.as_ref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatEdition {
    Current,
    Next,
}

impl FormatEdition {
    /// Parse the public CLI spelling.
    ///
    /// # Errors
    ///
    /// Returns an error for editions that have no formatting policy.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "current" => Ok(Self::Current),
            "next" => Ok(Self::Next),
            _ => Err("--edition must be current or next".to_owned()),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Next => "next",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FormatError {
    Lex(LexError),
    Parse(ParseError),
    Unsafe { message: String, span: Span },
}

impl FormatError {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Lex(error) => error.span,
            Self::Parse(error) => error.span,
            Self::Unsafe { span, .. } => *span,
        }
    }
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lex(error) => error.fmt(formatter),
            Self::Parse(error) => error.fmt(formatter),
            Self::Unsafe { message, span } => write!(
                formatter,
                "{message} at {}:{}",
                span.start.line, span.start.column
            ),
        }
    }
}

impl std::error::Error for FormatError {}

/// Build a lossless tree from the shared token stream and its source gaps.
/// Lexical failures are retained as error nodes so callers can refuse rewrites
/// without losing the original bytes.
#[must_use]
pub fn lossless_document(source: &str) -> LosslessDocument {
    let tokens = match lex(source) {
        Ok(tokens) => tokens,
        Err(error) => {
            return LosslessDocument {
                source: source.to_owned(),
                nodes: vec![LosslessNode {
                    kind: LosslessKind::Error,
                    text: source.to_owned(),
                    span: Span {
                        start: source_position(source, 0),
                        end: source_position(source, source.len()),
                    },
                }],
                error: Some(error),
            };
        }
    };
    let mut nodes = Vec::new();
    let mut offset = 0;
    for token in tokens {
        push_trivia(source, offset, token.span.start.offset, &mut nodes);
        if matches!(token.kind, TokenKind::Eof) {
            offset = token.span.end.offset;
            continue;
        }
        nodes.push(LosslessNode {
            kind: LosslessKind::Token(token.kind),
            text: source[token.span.start.offset..token.span.end.offset].to_owned(),
            span: token.span,
        });
        offset = token.span.end.offset;
    }
    push_trivia(source, offset, source.len(), &mut nodes);
    LosslessDocument {
        source: source.to_owned(),
        nodes,
        error: None,
    }
}

fn push_trivia(source: &str, start: usize, end: usize, nodes: &mut Vec<LosslessNode>) {
    let mut offset = start;
    while offset < end {
        let comment = source[offset..end].starts_with("//");
        let node_start = offset;
        if comment {
            while offset < end && !source[offset..end].starts_with('\n') {
                offset += source[offset..]
                    .chars()
                    .next()
                    .expect("in bounds")
                    .len_utf8();
            }
        } else {
            while offset < end && !source[offset..end].starts_with("//") {
                offset += source[offset..]
                    .chars()
                    .next()
                    .expect("in bounds")
                    .len_utf8();
            }
        }
        let start_pos = source_position(source, node_start);
        let end_pos = source_position(source, offset);
        nodes.push(LosslessNode {
            kind: if comment {
                LosslessKind::LineComment
            } else {
                LosslessKind::Whitespace
            },
            text: source[node_start..offset].to_owned(),
            span: Span {
                start: start_pos,
                end: end_pos,
            },
        });
    }
}

#[must_use]
/// Convert a valid UTF-8 byte boundary into a one-based source position.
///
/// # Panics
///
/// Panics when `offset` is not a source boundary or the line/column count
/// exceeds `u32`.
pub fn source_position(source: &str, offset: usize) -> crate::SourcePos {
    let prefix = &source[..offset];
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count())
        .expect("FSL source line count exceeds u32")
        + 1;
    let column = u32::try_from(
        prefix
            .rsplit_once('\n')
            .map_or(prefix, |(_, tail)| tail)
            .chars()
            .count(),
    )
    .expect("FSL source column exceeds u32")
        + 1;
    crate::SourcePos {
        offset,
        line,
        column,
    }
}

#[must_use]
/// Build a source span from two valid UTF-8 byte boundaries.
///
/// # Panics
///
/// Panics under the same invalid-boundary or oversized-source conditions as
/// [`source_position`].
pub fn source_span(source: &str, start: usize, end: usize) -> Span {
    Span {
        start: source_position(source, start),
        end: source_position(source, end),
    }
}

/// Format one complete registered FSL document without mutating the source.
///
/// # Errors
///
/// Returns the original lexical or parse error. Sources containing a legacy
/// domain enum with an interior comment are refused until that structural
/// rewrite can preserve the comment attachment unambiguously.
pub fn format_source(source: &str, _edition: FormatEdition) -> Result<String, FormatError> {
    let rewrites = canonical_rewrites(source)?;
    let rewritten = apply_source_edits(
        source,
        rewrites
            .into_iter()
            .flat_map(|rewrite| rewrite.edits)
            .collect(),
    )?;
    let tree = lossless_document(&rewritten);
    let mut formatter = Formatter::new();
    formatter.write(tree.nodes());
    let output = formatter.finish();
    parse_document(SourceFile::new(&output)).map_err(FormatError::Parse)?;
    Ok(output)
}

/// Plan the syntax-only canonical rewrites shared by formatting and migration.
///
/// # Errors
///
/// Returns the original parse failure or an unsafe-rewrite error when source
/// trivia cannot be attached unambiguously.
pub fn canonical_rewrites(source: &str) -> Result<Vec<CanonicalRewrite>, FormatError> {
    let tree = lossless_document(source);
    if let Some(error) = tree.error() {
        return Err(FormatError::Lex(error.clone()));
    }
    let parsed = parse_document(SourceFile::new(source)).map_err(FormatError::Parse)?;
    refuse_opaque_agent(&tree, parsed.keyword)?;
    let mut rewrites = plan_domain_enum(&tree, parsed.keyword)?;
    rewrites.extend(plan_domain_logical(&tree, parsed.keyword));
    rewrites.extend(plan_quantifiers(&tree)?);
    Ok(rewrites)
}

fn plan_domain_logical(tree: &LosslessDocument, dialect: &str) -> Vec<CanonicalRewrite> {
    if dialect != "domain" {
        return Vec::new();
    }
    let tokens = tree
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, LosslessKind::Token(_)))
        .collect::<Vec<_>>();
    let mut edits = Vec::new();
    for (index, node) in tokens.iter().enumerate() {
        let Some(symbol) = node.symbol() else {
            continue;
        };
        if symbol == "||" {
            edits.push(CanonicalRewrite {
                kind: CanonicalRewriteKind::LogicalOperator,
                span: node.span,
                canonical_replacement: "or".to_owned(),
                edits: vec![SourceEdit {
                    span: node.span,
                    replacement: "or".to_owned(),
                }],
            });
        } else if symbol == "->" {
            let await_branch = index >= 2
                && tokens[index - 2].ident() == Some("on")
                && tokens[index - 1].ident().is_some()
                && tokens
                    .get(index + 1)
                    .and_then(|node| node.ident())
                    .is_some();
            if !await_branch {
                edits.push(CanonicalRewrite {
                    kind: CanonicalRewriteKind::LogicalOperator,
                    span: node.span,
                    canonical_replacement: "=>".to_owned(),
                    edits: vec![SourceEdit {
                        span: node.span,
                        replacement: "=>".to_owned(),
                    }],
                });
            }
        }
    }
    edits
}

fn refuse_opaque_agent(tree: &LosslessDocument, dialect: &str) -> Result<(), FormatError> {
    if dialect != "agent" {
        return Ok(());
    }
    let tokens = tree
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, LosslessKind::Token(_)))
        .collect::<Vec<_>>();
    let open = tokens.iter().position(|node| node.symbol() == Some("{"));
    let close = tokens.iter().rposition(|node| node.symbol() == Some("}"));
    if let (Some(open), Some(close)) = (open, close)
        && close > open + 1
    {
        return Err(FormatError::Unsafe {
            message: "cannot format an opaque agent body without a native semantic grammar"
                .to_owned(),
            span: Span {
                start: tokens[open + 1].span.start,
                end: tokens[close - 1].span.end,
            },
        });
    }
    Ok(())
}

fn plan_quantifiers(tree: &LosslessDocument) -> Result<Vec<CanonicalRewrite>, FormatError> {
    let tokens = tree
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, LosslessKind::Token(_)))
        .collect::<Vec<_>>();
    let mut edits = Vec::new();
    for (start, token) in tokens.iter().enumerate() {
        if !matches!(token.ident(), Some("forall" | "exists")) {
            continue;
        }
        let typed = tokens.get(start + 2).and_then(|node| node.symbol()) == Some(":");
        let mut colons = Vec::new();
        let mut brace = None;
        let mut index = start + 1;
        while let Some(node) = tokens.get(index) {
            if node.span.start.line > token.span.start.line {
                break;
            }
            match node.symbol() {
                Some(":") => colons.push(index),
                Some("{") => {
                    brace = Some(index);
                    break;
                }
                Some(";" | "}") => break,
                _ => {}
            }
            index += 1;
        }
        let separator = colons.get(usize::from(typed)).copied();
        if let Some(separator) = separator {
            if brace == Some(separator + 1) {
                edits.push(CanonicalRewrite {
                    kind: CanonicalRewriteKind::Quantifier,
                    span: token.span,
                    canonical_replacement: "braced quantifier".to_owned(),
                    edits: vec![SourceEdit {
                        span: tokens[separator].span,
                        replacement: String::new(),
                    }],
                });
                continue;
            }
            let body_start = separator + 1;
            let Some(body_end) = quantifier_body_end(&tokens, body_start) else {
                return Err(FormatError::Unsafe {
                    message: "cannot determine the end of an unbraced quantifier".to_owned(),
                    span: token.span,
                });
            };
            edits.push(CanonicalRewrite {
                kind: CanonicalRewriteKind::Quantifier,
                span: token.span,
                canonical_replacement: "braced quantifier".to_owned(),
                edits: vec![
                    SourceEdit {
                        span: tokens[separator].span,
                        replacement: " {".to_owned(),
                    },
                    SourceEdit {
                        span: Span {
                            start: tokens[body_end].span.end,
                            end: tokens[body_end].span.end,
                        },
                        replacement: " }".to_owned(),
                    },
                ],
            });
        } else if brace.is_none() {
            return Err(FormatError::Unsafe {
                message: "cannot canonicalize a quantifier without braces or a separator colon"
                    .to_owned(),
                span: token.span,
            });
        }
    }
    Ok(edits)
}

fn quantifier_body_end(tokens: &[&LosslessNode], start: usize) -> Option<usize> {
    let first = tokens.get(start)?;
    let line = first.span.start.line;
    let mut depth = 0_i32;
    let mut end = None;
    for (index, node) in tokens.iter().enumerate().skip(start) {
        if node.span.start.line > line && depth == 0 {
            break;
        }
        match node.symbol() {
            Some("(" | "[" | "{") => depth += 1,
            Some("}" | ";") if depth == 0 => break,
            Some(")" | "]" | "}") => depth -= 1,
            _ => {}
        }
        end = Some(index);
    }
    end
}

/// Apply non-overlapping byte edits against one source snapshot.
///
/// # Errors
///
/// Returns an unsafe-format error when edits overlap or address invalid byte
/// boundaries.
pub fn apply_source_edits(source: &str, mut edits: Vec<SourceEdit>) -> Result<String, FormatError> {
    if let Some(edit) = edits.iter().find(|edit| {
        edit.span.start.offset > edit.span.end.offset
            || edit.span.end.offset > source.len()
            || !source.is_char_boundary(edit.span.start.offset)
            || !source.is_char_boundary(edit.span.end.offset)
    }) {
        return Err(FormatError::Unsafe {
            message: "formatter edit is outside a UTF-8 source boundary".to_owned(),
            span: edit.span,
        });
    }
    edits.sort_by_key(|edit| (edit.span.start.offset, edit.span.end.offset));
    if edits.windows(2).any(|pair| {
        pair[0].span.end.offset > pair[1].span.start.offset
            && pair[0].span.start.offset != pair[0].span.end.offset
    }) {
        return Err(FormatError::Unsafe {
            message: "overlapping formatter edits are not safe".to_owned(),
            span: Span {
                start: edits[0].span.start,
                end: edits[1].span.end,
            },
        });
    }
    let mut output = source.to_owned();
    for edit in edits.into_iter().rev() {
        output.replace_range(
            edit.span.start.offset..edit.span.end.offset,
            &edit.replacement,
        );
    }
    Ok(output)
}

fn plan_domain_enum(
    tree: &LosslessDocument,
    dialect: &str,
) -> Result<Vec<CanonicalRewrite>, FormatError> {
    if dialect != "domain" {
        return Ok(Vec::new());
    }
    let tokens = tree
        .nodes()
        .iter()
        .filter(|node| matches!(node.kind, LosslessKind::Token(_)))
        .collect::<Vec<_>>();
    let mut replacements = Vec::new();
    let mut cursor = 0;
    while cursor + 4 < tokens.len() {
        if tokens[cursor].ident() != Some("type")
            || tokens[cursor + 1].ident().is_none()
            || tokens[cursor + 2].symbol() != Some("=")
        {
            cursor += 1;
            continue;
        }
        let start = cursor;
        let mut index = cursor + 3;
        let mut members = Vec::new();
        while let Some(member) = tokens.get(index).and_then(|node| node.ident()) {
            members.push(member);
            index += 1;
            if tokens.get(index).and_then(|node| node.symbol()) == Some("|") {
                index += 1;
                continue;
            }
            break;
        }
        if members.len() < 2 {
            cursor += 1;
            continue;
        }
        let semicolon = tokens.get(index).and_then(|node| node.symbol()) == Some(";");
        let end = if semicolon { index } else { index - 1 };
        let span = Span {
            start: tokens[start].span.start,
            end: tokens[end].span.end,
        };
        let original = &tree.source()[span.start.offset..span.end.offset];
        if original.contains("//") {
            return Err(FormatError::Unsafe {
                message: "cannot canonicalize a legacy domain enum with interior comments"
                    .to_owned(),
                span,
            });
        }
        let replacement = format!(
            "enum {} {{ {} }}",
            tokens[start + 1].ident().expect("checked name"),
            members.join(", ")
        );
        replacements.push(CanonicalRewrite {
            kind: CanonicalRewriteKind::DomainEnum,
            span,
            canonical_replacement: replacement.clone(),
            edits: vec![SourceEdit { span, replacement }],
        });
        cursor = index + usize::from(semicolon);
    }
    Ok(replacements)
}

struct Formatter {
    output: String,
    indent: usize,
    line_start: bool,
    pending_lines: usize,
    pending_space: bool,
    previous: Option<TokenKind>,
    type_argument_depth: usize,
    source_line_break: bool,
}

impl Formatter {
    fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
            line_start: true,
            pending_lines: 0,
            pending_space: false,
            previous: None,
            type_argument_depth: 0,
            source_line_break: false,
        }
    }

    fn write(&mut self, nodes: &[LosslessNode]) {
        for node in nodes {
            match &node.kind {
                LosslessKind::Whitespace => self.whitespace(&node.text),
                LosslessKind::LineComment => self.comment(&node.text),
                LosslessKind::Token(kind) => self.token(kind, &node.text),
                LosslessKind::Error => unreachable!("lexical errors are refused"),
            }
        }
    }

    fn whitespace(&mut self, value: &str) {
        let lines = value.bytes().filter(|byte| *byte == b'\n').count();
        if lines > 0 {
            self.pending_lines = self.pending_lines.max(lines.min(2));
            self.pending_space = false;
            self.source_line_break = true;
        } else if !value.is_empty() {
            self.pending_space = true;
        }
    }

    fn comment(&mut self, value: &str) {
        let inline = !self.source_line_break && !self.line_start;
        let lines_after_attachment = if inline { self.pending_lines } else { 0 };
        if inline {
            self.pending_lines = 0;
            self.pending_space = true;
        } else {
            self.flush_pending();
        }
        if !self.line_start && !self.output.ends_with(' ') {
            self.output.push(' ');
        }
        self.write_indent();
        self.output.push_str(value.trim_end());
        self.newline();
        self.pending_lines = lines_after_attachment;
        self.source_line_break = true;
    }

    fn token(&mut self, kind: &TokenKind, original: &str) {
        let closes = matches!(kind, TokenKind::Symbol(symbol) if symbol == "}");
        if closes {
            self.indent = self.indent.saturating_sub(1);
            if !self.line_start {
                self.pending_lines = 1;
            }
        }
        self.flush_pending();
        let at_line_start = self.line_start;
        self.write_indent();
        if !at_line_start
            && needs_space(
                self.previous.as_ref(),
                kind,
                self.pending_space,
                self.type_argument_depth,
            )
        {
            self.output.push(' ');
        }
        self.output.push_str(original);
        self.line_start = false;
        self.pending_space = false;
        match kind {
            TokenKind::Symbol(symbol) if symbol == "{" => {
                self.indent += 1;
                self.pending_lines = 1;
            }
            TokenKind::Symbol(symbol) if symbol == "}" => self.pending_lines = 2,
            TokenKind::Symbol(symbol) if symbol == ";" => self.pending_lines = 1,
            _ => {}
        }
        if is_type_argument_open(self.previous.as_ref(), kind) {
            self.type_argument_depth += 1;
        } else if matches!(kind, TokenKind::Symbol(symbol) if symbol == ">")
            && self.type_argument_depth > 0
        {
            self.type_argument_depth -= 1;
        }
        self.previous = Some(kind.clone());
        self.source_line_break = false;
    }

    fn flush_pending(&mut self) {
        if self.pending_lines > 0 {
            if !self.line_start {
                self.newline();
            }
            if self.pending_lines > 1 && !self.output.ends_with("\n\n") {
                self.output.push('\n');
            }
            self.pending_lines = 0;
        }
    }

    fn write_indent(&mut self) {
        if self.line_start {
            self.output.push_str(&"  ".repeat(self.indent));
            self.line_start = false;
        }
    }

    fn newline(&mut self) {
        while self.output.ends_with(' ') {
            self.output.pop();
        }
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.line_start = true;
    }

    fn finish(mut self) -> String {
        while self
            .output
            .ends_with(|character: char| character.is_whitespace())
        {
            self.output.pop();
        }
        self.output.push('\n');
        self.output
    }
}

fn needs_space(
    previous: Option<&TokenKind>,
    current: &TokenKind,
    had_space: bool,
    type_argument_depth: usize,
) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    let previous_symbol = match previous {
        TokenKind::Symbol(value) => Some(value.as_str()),
        _ => None,
    };
    let current_symbol = match current {
        TokenKind::Symbol(value) => Some(value.as_str()),
        _ => None,
    };
    if is_type_argument_open(Some(previous), current)
        || type_argument_depth > 0 && current_symbol == Some(">")
        || type_argument_depth > 0 && previous_symbol == Some("<")
    {
        return false;
    }
    if !had_space
        && matches!(
            (previous, current),
            (TokenKind::Int(_), TokenKind::Ident(_))
        )
    {
        return false;
    }
    if matches!(current_symbol, Some("," | ";" | ")" | "]" | "." | ".."))
        || matches!(previous_symbol, Some("(" | "[" | "." | ".." | "@"))
    {
        return false;
    }
    if current_symbol == Some("[")
        && matches!(previous, TokenKind::Ident(value) if !list_prefix(value))
    {
        return false;
    }
    if current_symbol == Some("(") {
        return matches!(previous, TokenKind::Ident(value) if matches!(value.as_str(), "if" | "while"));
    }
    if matches!(current_symbol, Some(":")) {
        return false;
    }
    if matches!(previous_symbol, Some("," | ":")) {
        return true;
    }
    if current_symbol == Some("{") || previous_symbol == Some("}") {
        return true;
    }
    had_space
        || !matches!(previous_symbol, Some("(" | "[")) && !matches!(current_symbol, Some(")" | "]"))
}

fn is_type_argument_open(previous: Option<&TokenKind>, current: &TokenKind) -> bool {
    matches!(
        (previous, current),
        (
            Some(TokenKind::Ident(name)),
            TokenKind::Symbol(symbol)
        ) if symbol == "<" && matches!(name.as_str(), "Map" | "Set" | "Seq" | "Option")
    )
}

fn list_prefix(value: &str) -> bool {
    matches!(
        value,
        "authority"
            | "context"
            | "fields"
            | "forbidden"
            | "in"
            | "may_execute"
            | "may_suggest"
            | "one_of"
            | "requires_human_approval"
            | "tools"
            | "trusted"
            | "untrusted"
            | "visibility"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_nodes_reconstruct_comments_blank_lines_and_token_spelling() {
        let source = "// lead\n\ndomain D { invariant P { a || b -> not c } }\n";
        let tree = lossless_document(source);
        assert!(tree.error().is_none());
        assert_eq!(
            tree.nodes()
                .iter()
                .map(|node| node.text.as_str())
                .collect::<String>(),
            source
        );
        assert!(
            tree.nodes()
                .iter()
                .any(|node| matches!(node.kind, LosslessKind::LineComment))
        );
    }

    #[test]
    fn malformed_source_is_retained_and_formatting_refuses_it() {
        let source = "spec Broken { invariant P { x ` y } }";
        let tree = lossless_document(source);
        assert_eq!(tree.source(), source);
        assert!(matches!(tree.nodes()[0].kind, LosslessKind::Error));
        assert_eq!(tree.nodes()[0].text, source);
        assert!(matches!(
            format_source(source, FormatEdition::Current),
            Err(FormatError::Lex(_))
        ));
    }

    #[test]
    fn formatter_is_idempotent_and_preserves_comments() {
        let source =
            include_str!("../../fslc/tests/fixtures/domain_characterization/expressions_valid.fsl");
        let once = format_source(source, FormatEdition::Next).expect("format");
        let twice = format_source(&once, FormatEdition::Next).expect("format twice");
        assert_eq!(once, twice);
        assert!(once.contains("// SPDX-License-Identifier: Apache-2.0"));
        assert!(once.contains("enum OrderStatus"));
        assert!(once.contains(" or "));
        assert!(once.contains(" => "));
    }

    #[test]
    fn commented_legacy_enum_is_refused_without_losing_source() {
        let source = "domain D { type Status = Draft // keep\n | Done; }";
        let error = format_source(source, FormatEdition::Next).expect_err("unsafe rewrite");
        assert!(matches!(error, FormatError::Unsafe { .. }));
        assert_eq!(lossless_document(source).source(), source);
    }

    #[test]
    fn legacy_quantifier_colons_become_idempotent_braces() {
        let source = include_str!("../../../specs/cart_buggy.fsl");
        let once = format_source(source, FormatEdition::Next).expect("format quantifiers");
        let twice = format_source(&once, FormatEdition::Next).expect("format twice");
        assert_eq!(once, twice);
        assert!(!once.contains("MAXI:"));
        assert!(!once.contains("MAXU:"));
        assert!(once.contains("forall i in 0..MAXI {"));
    }

    #[test]
    fn nonempty_opaque_agent_body_is_refused() {
        let source = "agent Planner { opaque_call() }";
        let error = format_source(source, FormatEdition::Current).expect_err("opaque body");
        assert!(matches!(error, FormatError::Unsafe { .. }));
    }

    #[test]
    fn equivalent_layouts_converge_to_one_canonical_form() {
        let compact = "spec S { state { stock: Map<Int, Bool> } init { stock[0] = false } invariant P { stock[0] == false } }";
        let spaced = "spec S {\n  state { stock: Map < Int, Bool > }\n\n  init { stock [0] = false } invariant P { stock [0] == false }\n}\n";
        let compact = format_source(compact, FormatEdition::Current).expect("compact format");
        let spaced = format_source(spaced, FormatEdition::Current).expect("spaced format");
        assert_eq!(compact, spaced);
        assert!(compact.contains("\n  state {"));
        assert!(compact.contains("Map<Int, Bool>"));
        assert!(compact.contains("stock[0]"));
    }

    #[test]
    fn detached_comment_blank_line_is_preserved() {
        let source = "spec S {\n\n  // detached\n  state { x: Bool }\n  init { x = false }\n  invariant P { x == false }\n}\n";
        let formatted = format_source(source, FormatEdition::Current).expect("format");
        assert!(formatted.contains("spec S {\n\n  // detached\n"));
    }

    #[test]
    fn trailing_comment_stays_attached_to_its_closing_brace() {
        let source = "spec S { struct Pair { left: Bool, right: Bool } // attached\n state { x: Bool } init { x = false } invariant P { x == false } }";
        let formatted = format_source(source, FormatEdition::Current).expect("format");
        assert!(formatted.contains("} // attached\n\n  state"));
    }

    #[test]
    fn domain_identifier_named_on_does_not_hide_legacy_implication() {
        let source = "domain D { implementation_profile functional_ddd type Id = 0..1 aggregate A { id Id state { on: Bool = false; } invariant P { on -> not on } } }";
        let formatted = format_source(source, FormatEdition::Next).expect("format");
        assert!(formatted.contains("on => not on"));
    }
}

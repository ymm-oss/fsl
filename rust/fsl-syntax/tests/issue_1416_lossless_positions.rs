// SPDX-License-Identifier: Apache-2.0

use fsl_syntax::{
    LosslessKind, LosslessNode, SourcePos, Span, TokenKind, lossless_document, source_position,
    source_span,
};

// Independent oracle: UTF-8 bytes, scalar columns (CR/tab/BOM included), LF resets.
fn positions(source: &str) -> Vec<SourcePos> {
    let mut position = SourcePos {
        offset: 0,
        line: 1,
        column: 1,
    };
    let mut result = vec![position];
    for ch in source.chars() {
        position.offset += ch.len_utf8();
        if ch == '\n' {
            position.line += 1;
            position.column = 1;
        } else {
            position.column += 1;
        }
        result.push(position);
    }
    result
}

fn span_at(positions: &[SourcePos], start: usize, end: usize) -> Span {
    let at = |offset| *positions.iter().find(|pos| pos.offset == offset).unwrap();
    Span {
        start: at(start),
        end: at(end),
    }
}

fn ident(text: &str) -> LosslessKind {
    LosslessKind::Token(TokenKind::Ident(text.into()))
}

fn symbol(text: &str) -> LosslessKind {
    LosslessKind::Token(TokenKind::Symbol(text.into()))
}

fn assert_document(source: &str, pieces: Vec<(LosslessKind, &str)>) {
    let document = lossless_document(source);
    assert_eq!(document.source(), source);
    assert_eq!(document.error(), None);
    let expected_positions = positions(source);
    let mut offset = 0;
    let expected: Vec<_> = pieces
        .into_iter()
        .map(|(kind, text)| {
            let start = offset;
            offset += text.len();
            assert_eq!(&source[start..offset], text);
            LosslessNode {
                kind,
                text: text.into(),
                span: span_at(&expected_positions, start, offset),
            }
        })
        .collect();
    assert_eq!(offset, source.len(), "fixture covers every byte");
    assert_eq!(document.nodes(), expected);
    let reconstructed: String = document.nodes().iter().map(|n| n.text.as_str()).collect();
    assert_eq!(reconstructed, source);
}

#[test]
fn preservation_ascii_tokens_and_all_trivia_endpoints() {
    use LosslessKind::{LineComment as C, Whitespace as W};
    let source = "// head\n\nspec S {\n\tx: Int // tail\r\n}\n";
    assert_document(
        source,
        vec![
            (C, "// head"),
            (W, "\n\n"),
            (ident("spec"), "spec"),
            (W, " "),
            (ident("S"), "S"),
            (W, " "),
            (symbol("{"), "{"),
            (W, "\n\t"),
            (ident("x"), "x"),
            (symbol(":"), ":"),
            (W, " "),
            (ident("Int"), "Int"),
            (W, " "),
            (C, "// tail\r"),
            (W, "\n"),
            (symbol("}"), "}"),
            (W, "\n"),
        ],
    );
}

#[test]
fn preservation_unicode_bom_whitespace_comments_and_string_spelling() {
    use LosslessKind::{LineComment as C, Whitespace as W};
    let source = "\u{feff}\u{2003}// 日本😀 e\u{301}\r\n\tmodel \"日本😀//literal\"; //終";
    assert_document(
        source,
        vec![
            (W, "\u{feff}\u{2003}"),
            (C, "// 日本😀 e\u{301}\r"),
            (W, "\n\t"),
            (ident("model"), "model"),
            (W, " "),
            (
                LosslessKind::Token(TokenKind::String("日本😀//literal".into())),
                "\"日本😀//literal\"",
            ),
            (symbol(";"), ";"),
            (W, " "),
            (C, "//終"),
        ],
    );
}

#[test]
fn preservation_empty_and_comment_only_documents() {
    use LosslessKind::{LineComment as C, Whitespace as W};
    assert_document("", vec![]);
    assert_document(" \r\n\t\u{3000}", vec![(W, " \r\n\t\u{3000}")]);
    assert_document(
        "//😀\n\n// EOF",
        vec![(C, "//😀"), (W, "\n\n"), (C, "// EOF")],
    );
}

#[test]
fn preservation_positions_at_every_utf8_boundary_and_span() {
    for source in ["", "a\t\r\nb\rc\n", "\u{feff}日😀e\u{301}\n\u{3000}終\n"] {
        let expected = positions(source);
        for &position in &expected {
            assert_eq!(source_position(source, position.offset), position);
        }
        for pair in expected.windows(2) {
            assert_eq!(
                source_span(source, pair[0].offset, pair[1].offset),
                span_at(&expected, pair[0].offset, pair[1].offset)
            );
        }
        assert_eq!(
            source_span(source, 0, source.len()),
            span_at(&expected, 0, source.len())
        );
    }
}

#[test]
fn preservation_many_trivia_gaps_without_timing_assertions() {
    // Exercises push_trivia's prefix-rescan workload; no flaky wall-time gate.
    for count in [16, 128, 1024] {
        let source = "x // 日本😀\r\n\t".repeat(count);
        let pieces = (0..count)
            .flat_map(|_| {
                [
                    (ident("x"), "x"),
                    (LosslessKind::Whitespace, " "),
                    (LosslessKind::LineComment, "// 日本😀\r"),
                    (LosslessKind::Whitespace, "\n\t"),
                ]
            })
            .collect();
        assert_document(&source, pieces);
    }
}

#[test]
fn preservation_lexical_failures_are_one_complete_error_node() {
    for (source, offending, message) in [
        ("// 日本😀\r\n\t§", "§", "unexpected character '§'"),
        (
            "x \"日本😀\nnext",
            "\"日本😀",
            "unterminated string literal",
        ),
        (
            "// prefix\n9223372036854775808",
            "9223372036854775808",
            "integer literal is outside the i64 range",
        ),
    ] {
        let document = lossless_document(source);
        let expected = positions(source);
        let start = source.find(offending).unwrap();
        let error = document.error().expect("lexical rejection");
        assert_eq!(error.code(), "FSL-PARSE");
        assert_eq!(error.message, message);
        assert_eq!(
            error.span,
            span_at(&expected, start, start + offending.len())
        );
        assert_eq!(document.source(), source);
        assert_eq!(
            document.nodes(),
            [LosslessNode {
                kind: LosslessKind::Error,
                text: source.into(),
                span: span_at(&expected, 0, source.len()),
            }]
        );
    }
}

#[test]
fn preservation_syntactically_incomplete_is_not_a_lexical_error() {
    assert_document(
        "spec Broken {",
        vec![
            (ident("spec"), "spec"),
            (LosslessKind::Whitespace, " "),
            (ident("Broken"), "Broken"),
            (LosslessKind::Whitespace, " "),
            (symbol("{"), "{"),
        ],
    );
}

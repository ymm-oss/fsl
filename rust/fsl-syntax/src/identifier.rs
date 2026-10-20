// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Which identifiers the lexer reads, and the diagnostics of the wider rule.
//!
//! The published language reads `[A-Za-z_][A-Za-z0-9_]*`. Behind the `fsl5`
//! unstable feature, a document whose first significant line is the `fsl 5`
//! edition header reads UAX #31 identifiers (`XID_Start` `XID_Continue*`,
//! plus `_`) instead, read in Unicode normalization form C. Characters that
//! look like ASCII or like a word separator are rejected there with their own
//! codes rather than read as a different name.

use unicode_normalization::UnicodeNormalization;
use unicode_security::mixed_script::AugmentedScriptSet;

use crate::ParseOptions;

/// An identifier spelled with full-width letters or digits (`Ｃｌａｉｍ`).
pub const FULLWIDTH_ASCII_IDENTIFIER: &str = "fullwidth_ascii_identifier";
/// A full-width punctuation or operator character (`－`, `＝`, `（`).
pub const FULLWIDTH_SYMBOL: &str = "fullwidth_symbol";
/// The katakana middle dot `・` (or its half-width form) inside a name.
pub const MIDDLE_DOT_IDENTIFIER: &str = "middle_dot_identifier";
/// An identifier mixing scripts beyond UTS #39 "highly restrictive"
/// (`Сlaim`: Cyrillic `С` with Latin).
pub const MIXED_SCRIPT_IDENTIFIER: &str = "mixed_script_identifier";
/// A requirement identifier (`REQ-1`) with a non-ASCII character.
pub const NON_ASCII_REQUIREMENT_ID: &str = "non_ascii_requirement_id";
/// Warning: an identifier not written in NFC; it is read as its NFC form.
pub const NON_NFC_IDENTIFIER: &str = "non_nfc_identifier";
/// Warning: the ideographic space U+3000, read as a space.
pub const FULLWIDTH_SPACE: &str = "fullwidth_space";
/// A canonical (implementation-facing) name that is not ASCII. Reserved:
/// emitted by name resolution of glossary aliases, not by the lexer.
pub const CANONICAL_NAME_NOT_ASCII: &str = "canonical_name_not_ascii";

/// How severe an identifier diagnostic is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentifierSeverity {
    /// The document is rejected.
    Error,
    /// The document is read; the finding is reported beside it.
    Warning,
}

/// Every diagnostic the wider identifier rule emits, with its severity.
pub const IDENTIFIER_DIAGNOSTICS: &[(&str, IdentifierSeverity)] = &[
    (FULLWIDTH_ASCII_IDENTIFIER, IdentifierSeverity::Error),
    (FULLWIDTH_SYMBOL, IdentifierSeverity::Error),
    (MIDDLE_DOT_IDENTIFIER, IdentifierSeverity::Error),
    (MIXED_SCRIPT_IDENTIFIER, IdentifierSeverity::Error),
    (NON_ASCII_REQUIREMENT_ID, IdentifierSeverity::Error),
    (NON_NFC_IDENTIFIER, IdentifierSeverity::Warning),
    (FULLWIDTH_SPACE, IdentifierSeverity::Warning),
];

/// Codes reserved for the same family but emitted outside the lexer.
pub const RESERVED_IDENTIFIER_CODES: &[&str] = &[CANONICAL_NAME_NOT_ASCII];

/// The identifier rule a document is lexed with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentifierRule {
    /// `[A-Za-z_][A-Za-z0-9_]*`: the published language.
    Ascii,
    /// UAX #31 identifiers in NFC, with the look-alike checks.
    Unicode,
}

impl IdentifierRule {
    /// The rule for `source` under `options`: [`Self::Unicode`] only when the
    /// `fsl5` feature is enabled and the document opens with `fsl 5`.
    #[must_use]
    pub fn for_source(source: &str, options: &ParseOptions) -> Self {
        match crate::edition_header(source, options) {
            Ok(Some(header)) if header.edition == crate::Edition::Fsl5 => Self::Unicode,
            _ => Self::Ascii,
        }
    }

    /// Whether `name` is exactly one identifier under this rule, already in
    /// the spelling the lexer reads it as (so no warning is attached).
    #[must_use]
    pub fn accepts(self, name: &str) -> bool {
        crate::lexer::is_single_identifier(name, self)
    }

    pub(crate) fn starts(self, ch: char) -> bool {
        match self {
            Self::Ascii => ch.is_ascii_alphabetic() || ch == '_',
            Self::Unicode => ch == '_' || (unicode_ident::is_xid_start(ch) && !is_middle_dot(ch)),
        }
    }

    pub(crate) fn continues(self, ch: char) -> bool {
        match self {
            Self::Ascii => ch.is_ascii_alphanumeric() || ch == '_',
            Self::Unicode => unicode_ident::is_xid_continue(ch) && !is_middle_dot(ch),
        }
    }
}

/// `・` U+30FB and `･` U+FF65. Unicode 15.1 made both `XID_Continue`; a name
/// never contains them, because to a reader they separate two words.
pub(crate) const fn is_middle_dot(ch: char) -> bool {
    matches!(ch, '\u{30fb}' | '\u{ff65}')
}

/// The ASCII character a full-width form (U+FF01..=U+FF5E) stands for.
pub(crate) fn halfwidth(ch: char) -> Option<char> {
    ('\u{ff01}'..='\u{ff5e}')
        .contains(&ch)
        .then(|| char::from_u32(u32::from(ch) - 0xfee0))
        .flatten()
}

/// A rejected character or identifier: `(code, message)`.
pub(crate) type Rejection = (&'static str, String);

/// The diagnostic for a character no token starts with, when it is one of the
/// look-alikes this rule names.
pub(crate) fn unexpected_character(ch: char) -> Option<Rejection> {
    if is_middle_dot(ch) {
        return Some((
            MIDDLE_DOT_IDENTIFIER,
            format!(
                "'{ch}' cannot be part of an identifier; join the words with '_' or write them as one word"
            ),
        ));
    }
    let ascii = halfwidth(ch)?;
    let code = if ascii.is_ascii_alphanumeric() {
        FULLWIDTH_ASCII_IDENTIFIER
    } else {
        FULLWIDTH_SYMBOL
    };
    Some((
        code,
        format!("full-width character '{ch}'; write '{ascii}'"),
    ))
}

/// Check one identifier read under [`IdentifierRule::Unicode`] and return the
/// name it is read as (its NFC form).
pub(crate) fn unicode_identifier(raw: &str) -> Result<String, Rejection> {
    if raw
        .chars()
        .any(|ch| halfwidth(ch).is_some_and(|ascii| ascii.is_ascii_alphanumeric()))
    {
        let ascii = raw
            .chars()
            .map(|ch| halfwidth(ch).unwrap_or(ch))
            .collect::<String>();
        return Err((
            FULLWIDTH_ASCII_IDENTIFIER,
            format!("identifier '{raw}' uses full-width letters or digits; write '{ascii}'"),
        ));
    }
    let name = raw.nfc().collect::<String>();
    if !highly_restrictive(&name) {
        return Err((
            MIXED_SCRIPT_IDENTIFIER,
            format!(
                "identifier '{name}' mixes scripts; write it in one script (Latin may be combined only with Han, Hiragana, Katakana, Hangul or Bopomofo)"
            ),
        ));
    }
    Ok(name)
}

/// UTS #39 restriction level "highly restrictive" or stricter, on scripts
/// alone: one script (with Japanese, Korean and Han-with-Bopomofo counted as
/// one), or Latin plus one of those three. Identifier status is not
/// consulted, so uncommon but valid ideographs (CJK Extension B) stay usable.
fn highly_restrictive(name: &str) -> bool {
    let latin = AugmentedScriptSet::for_char('a').base;
    let mut all = AugmentedScriptSet::default();
    let mut without_latin = AugmentedScriptSet::default();
    for ch in name.chars() {
        let set = AugmentedScriptSet::for_char(ch);
        all.intersect_with(set);
        if set.base.intersection(latin).is_empty() {
            without_latin.intersect_with(set);
        }
    }
    !all.is_empty() || without_latin.jpan || without_latin.hanb || without_latin.kore
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{UnstableFeature, UnstableFeatures};

    fn gated() -> ParseOptions {
        ParseOptions::with_unstable(UnstableFeatures::NONE.with(UnstableFeature::Fsl5))
    }

    #[test]
    fn unicode_rule_needs_the_gate_and_the_fsl5_header() {
        for (source, gate, expected) in [
            ("fsl 5\nspec S {}", true, IdentifierRule::Unicode),
            (
                "\u{feff}// 経費の仕様\n\n  fsl\t5 \r\nspec S {}",
                true,
                IdentifierRule::Unicode,
            ),
            ("fsl 5", true, IdentifierRule::Unicode),
            ("fsl 5\nspec S {}", false, IdentifierRule::Ascii),
            ("fsl 4\nspec S {}", true, IdentifierRule::Ascii),
            ("spec S {}", true, IdentifierRule::Ascii),
            ("fsl 55\nspec S {}", true, IdentifierRule::Ascii),
            ("fsl5\nspec S {}", true, IdentifierRule::Ascii),
            ("fsl 5 spec S {}", true, IdentifierRule::Ascii),
            ("spec S {}\nfsl 5\n", true, IdentifierRule::Ascii),
            ("// fsl 5\nspec S {}", true, IdentifierRule::Ascii),
        ] {
            let options = if gate {
                gated()
            } else {
                ParseOptions::stable()
            };
            assert_eq!(
                IdentifierRule::for_source(source, &options),
                expected,
                "{source:?} gate={gate}"
            );
        }
    }

    #[test]
    fn middle_dots_are_xid_continue_but_never_continue_a_name() {
        assert!(unicode_ident::is_xid_continue('・'));
        assert!(!IdentifierRule::Unicode.continues('・'));
        assert!(!IdentifierRule::Unicode.continues('･'));
        assert!(IdentifierRule::Unicode.continues('ー'));
    }

    #[test]
    fn script_mixing_follows_highly_restrictive() {
        for allowed in [
            "Claim",
            "申請",
            "申請データ",
            "Claim申請",
            "𠮷野家",
            "заявка",
            "x_1",
        ] {
            assert!(highly_restrictive(allowed), "{allowed}");
        }
        for rejected in ["Сlaim", "αlpha", "申請Сlaim"] {
            assert!(!highly_restrictive(rejected), "{rejected}");
        }
    }

    /// The surface of a `fsl 5` document parsed behind the gate.
    fn fsl5_surface(body: &str) -> Result<crate::SurfaceDocument, crate::ParseError> {
        crate::parse_surface_document_with(&format!("fsl 5\n{body}"), &gated())
    }

    fn fsl5_warnings(body: &str) -> Vec<&'static str> {
        crate::lexical_warnings(&format!("fsl 5\n{body}"), &gated())
            .into_iter()
            .map(|warning| warning.code)
            .collect()
    }

    /// fsl 5 plan S2 acceptance 1: outside a gated `fsl 5` document a
    /// Japanese identifier is the published unexpected character, error for
    /// error.
    #[test]
    fn published_identifiers_hold_without_the_gate_or_the_header() {
        let body = "spec S { state { 経費: Int } }";
        for (source, options) in [
            (body.to_owned(), gated()),
            (format!("fsl 4\n{body}"), gated()),
            (format!("fsl 5\n{body}"), ParseOptions::stable()),
        ] {
            let published = crate::lex(&source).unwrap_err();
            assert_eq!(crate::lex_with(&source, &options), Err(published.clone()));
            assert_eq!(published.code(), "FSL-PARSE");
            assert_eq!(published.message, "unexpected character '経'");
            assert!(crate::lexical_warnings(&source, &options).is_empty());
        }
        assert!(fsl5_surface(body).is_ok());
    }

    /// Acceptance 2: every code of the catalog is produced by its look-alike
    /// (design C.4), and the corrected spelling is accepted.
    #[test]
    fn every_identifier_diagnostic_is_produced_by_its_look_alike() {
        let cases = [
            (
                FULLWIDTH_ASCII_IDENTIFIER,
                "spec S { state { Ｃｌａｉｍ: Int } }",
                "spec S { state { Claim: Int } }",
            ),
            (
                FULLWIDTH_SYMBOL,
                "spec S { state { x: Int } init { x = 2 － 1 } }",
                "spec S { state { x: Int } init { x = 2 - 1 } }",
            ),
            (
                MIDDLE_DOT_IDENTIFIER,
                "spec S { state { 申請・承認: Int } }",
                "spec S { state { 申請_承認: Int } }",
            ),
            (
                MIXED_SCRIPT_IDENTIFIER,
                "spec S { state { Сlaim: Int } }",
                "spec S { state { Claim申請: Int } }",
            ),
            (
                NON_ASCII_REQUIREMENT_ID,
                "requirements R { requirement 要件-1 \"text\" {} }",
                "requirements R { requirement REQ-1 \"text\" {} }",
            ),
            (
                NON_NFC_IDENTIFIER,
                "spec S { state { \u{304b}\u{3099}: Int } }",
                "spec S { state { \u{304c}: Int } }",
            ),
            (
                FULLWIDTH_SPACE,
                "spec S {\u{3000}state { x: Int } }",
                "spec S { state { x: Int } }",
            ),
        ];
        assert_eq!(
            cases.iter().map(|case| case.0).collect::<Vec<_>>(),
            IDENTIFIER_DIAGNOSTICS
                .iter()
                .map(|(code, _)| *code)
                .collect::<Vec<_>>()
        );
        for ((code, rejected, accepted), (_, severity)) in cases.iter().zip(IDENTIFIER_DIAGNOSTICS)
        {
            match severity {
                IdentifierSeverity::Error => {
                    let error = fsl5_surface(rejected).unwrap_err();
                    assert_eq!(error.code(), *code, "{rejected}: {error}");
                    assert!(fsl5_warnings(rejected).is_empty(), "{rejected}");
                    assert_eq!(
                        crate::parse_surface_document(rejected).unwrap_err().code(),
                        "FSL-PARSE",
                        "{rejected}: published language"
                    );
                }
                IdentifierSeverity::Warning => {
                    assert!(fsl5_surface(rejected).is_ok(), "{rejected}");
                    assert_eq!(fsl5_warnings(rejected), vec![*code], "{rejected}");
                }
            }
            assert!(fsl5_surface(accepted).is_ok(), "{accepted}");
            assert!(fsl5_warnings(accepted).is_empty(), "{accepted}");
        }
        assert!(RESERVED_IDENTIFIER_CODES.iter().all(|code| {
            IDENTIFIER_DIAGNOSTICS
                .iter()
                .all(|(listed, _)| listed != code)
        }));
    }

    #[test]
    fn rejections_point_at_the_offending_text() {
        let error = fsl5_surface("spec S { state { 申請・承認: Int } }").unwrap_err();
        assert_eq!((error.span.start.line, error.span.start.column), (2, 20));
        assert_eq!(
            error.span.end.offset - error.span.start.offset,
            '・'.len_utf8()
        );
        let error = fsl5_surface("requirements R { requirement 要件-1 \"text\" {} }").unwrap_err();
        assert_eq!(
            (error.span.start.column, error.span.end.column),
            (30, 34),
            "the whole identifier"
        );
        let warning = &crate::lexical_warnings(
            "fsl 5\nspec S { state { \u{304b}\u{3099}: Int } }",
            &gated(),
        )[0];
        assert_eq!(warning.replacement, "\u{304c}");
        assert_eq!(
            (warning.span.start.column, warning.span.end.column),
            (18, 20)
        );
    }

    /// Acceptance 3: an identifier written in NFD is the same name as its NFC
    /// spelling, so a declaration in one form and a use in the other agree.
    #[test]
    fn nfd_and_nfc_spellings_are_one_name() {
        let nfc = "\u{304c}\u{304f}";
        let nfd = nfc.nfd().collect::<String>();
        assert_ne!(nfc, nfd);
        let mixed = fsl5_surface(&format!(
            "spec S {{ state {{ {nfc}: Int }} init {{ {nfd} = 0 }} }}"
        ))
        .unwrap();
        let composed = fsl5_surface(&format!(
            "spec S {{ state {{ {nfc}: Int }} init {{ {nfc} = 0 }} }}"
        ))
        .unwrap();
        assert_eq!(mixed.kernel_ast_v1(), composed.kernel_ast_v1());
        assert_eq!(
            fsl5_warnings(&format!("spec S {{ state {{ {nfd}: Int }} }}")),
            vec![NON_NFC_IDENTIFIER]
        );
    }

    #[test]
    fn unicode_rule_reads_identifiers_beyond_the_bmp_and_long_vowel_marks() {
        for name in ["𠮷野家", "申請データ", "Claim申請", "_内部", "x1"] {
            assert!(IdentifierRule::Unicode.accepts(name), "{name}");
            assert_eq!(
                IdentifierRule::Ascii.accepts(name),
                name.is_ascii(),
                "{name}"
            );
        }
        for name in [
            "",
            "1x",
            "a b",
            "申請・承認",
            "Ｃｌａｉｍ",
            "\u{304b}\u{3099}",
            " x",
        ] {
            assert!(!IdentifierRule::Unicode.accepts(name), "{name:?}");
        }
    }

    #[test]
    fn full_width_forms_name_their_ascii_counterpart() {
        assert_eq!(halfwidth('Ｃ'), Some('C'));
        assert_eq!(halfwidth('－'), Some('-'));
        assert_eq!(halfwidth('～'), Some('~'));
        assert_eq!(halfwidth('\u{ff5f}'), None);
        assert_eq!(halfwidth('\u{3000}'), None);
    }
}

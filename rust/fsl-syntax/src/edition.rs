// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Syntax editions: the edition header a document opens with, and the one
//! edition type every command and the formatter share.
//!
//! A document whose first significant line (after a BOM, whitespace and `//`
//! comments, as the lexer skips them) is `fsl`, blanks, an integer is headed
//! by that edition. The header is read before lexing, because the edition
//! picks the lexer's identifier rule, and its two tokens never reach a
//! frontend. A document without a header is read as `fsl 4`.

use crate::{ParseError, ParseOptions, Span};

/// `--edition` contradicts the document's edition header.
pub const EDITION_CONFLICT: &str = "edition_conflict";
/// An edition header naming an edition this release does not have.
pub const UNSUPPORTED_EDITION: &str = "unsupported_edition";
/// An edition header on a causal or AI document, which has no editions.
pub const EDITION_HEADER_NOT_ACCEPTED: &str = "edition_header_not_accepted";
/// Lint information: a document checked for the next edition has no header.
pub const MISSING_EDITION_HEADER: &str = "missing_edition_header";

/// Migration codes exposed by consumers in the current edition.
/// Implicit-initial-value diagnostics have a separate initialization contract.
pub const CURRENT_EDITION_MIGRATION_CODES: [&str; 4] = [
    "deprecated_domain_enum_union",
    "legacy_string_metadata",
    "legacy_quantifier_colon",
    "inline_action_maps",
];

/// Declarations that take no edition header: the causal and AI dialects
/// (decisions/049 D8). Their frontends read the published language only.
pub const EDITIONLESS_DECLARATIONS: &[&str] = &[
    "causal",
    "ai_component",
    "agent",
    "ai_action",
    "ai_contract",
    "ai_migration",
    "authority",
    "dataset",
    "evaluator",
    "failure_mode",
    "observed_property",
    "retriever",
    "statistical_property",
    "trust_boundary",
];

/// A syntax edition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Edition {
    /// The published language; a document without a header is read in it.
    #[default]
    Fsl4,
    /// The next edition (`--edition next`).
    Fsl5,
}

impl Edition {
    /// Every edition, oldest first.
    pub const ALL: &'static [Self] = &[Self::Fsl4, Self::Fsl5];

    /// The number written in the header (`fsl 4`, `fsl 5`).
    #[must_use]
    pub const fn number(self) -> u32 {
        match self {
            Self::Fsl4 => 4,
            Self::Fsl5 => 5,
        }
    }

    /// The edition a header number names.
    #[must_use]
    pub fn from_number(number: u64) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|edition| u64::from(edition.number()) == number)
    }

    /// The published `--edition` spelling: `current` or `next`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fsl4 => "current",
            Self::Fsl5 => "next",
        }
    }

    /// Parse an `--edition` value. `current` requests no edition (the
    /// document's header decides, and no header is [`Self::Fsl4`]); `next` is
    /// [`Self::Fsl5`]. The edition numbers `4` and `5` are accepted too.
    ///
    /// # Errors
    ///
    /// Returns the usage message for any other value.
    pub fn parse_option(value: &str, options: &ParseOptions) -> Result<Option<Self>, String> {
        EditionRequest::parse(value, options).map(|request| request.map(|request| request.edition))
    }
}

/// An explicit `--edition` request: the edition it names and the spelling
/// that named it, so an [`EDITION_CONFLICT`] echoes what was typed (`next`,
/// `5` or `4`) rather than a number the user did not write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EditionRequest {
    pub edition: Edition,
    /// `next`, `4` or `5`.
    pub spelling: &'static str,
}

impl EditionRequest {
    /// A request spelled with the edition's number (`4`, `5`).
    #[must_use]
    pub const fn numbered(edition: Edition) -> Self {
        Self {
            edition,
            spelling: match edition {
                Edition::Fsl4 => "4",
                Edition::Fsl5 => "5",
            },
        }
    }

    /// Parse an `--edition` value, keeping its spelling. `current` is `None`,
    /// as in [`Edition::parse_option`].
    ///
    /// # Errors
    ///
    /// Returns the usage message for any other value.
    pub fn parse(value: &str, _options: &ParseOptions) -> Result<Option<Self>, String> {
        match value {
            "current" => Ok(None),
            "next" => Ok(Some(Self {
                edition: Edition::Fsl5,
                spelling: "next",
            })),
            "5" => Ok(Some(Self::numbered(Edition::Fsl5))),
            "4" => Ok(Some(Self::numbered(Edition::Fsl4))),
            _ => Err("--edition must be current, next, 4, or 5".to_owned()),
        }
    }
}

/// The edition header of one document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditionHeader {
    pub edition: Edition,
    /// From `fsl` to the end of the number.
    pub span: Span,
}

/// Read the edition header of `source` under `options`, before lexing.
///
/// `Ok(None)` when `fsl5` is not enabled or the first significant line is not
/// a header. A header line is `fsl`, spaces or tabs, decimal digits, then only
/// spaces, tabs or `\r` to the end of the line; `fsl` followed by anything else
/// is an ordinary identifier, so `const fsl = 1` keeps its meaning.
///
/// # Errors
///
/// Returns [`UNSUPPORTED_EDITION`] for a header naming another number.
pub fn edition_header(
    source: &str,
    _options: &ParseOptions,
) -> Result<Option<EditionHeader>, ParseError> {
    let Some((start, digits)) = header_line(source) else {
        return Ok(None);
    };
    let end = digits.end;
    let span = crate::source_span(source, start, end);
    let number = &source[digits];
    match number.parse::<u64>().ok().and_then(Edition::from_number) {
        Some(edition) => Ok(Some(EditionHeader { edition, span })),
        None => Err(ParseError::coded(
            UNSUPPORTED_EDITION,
            format!(
                "unsupported syntax edition 'fsl {number}'; supported editions: {}",
                Edition::ALL
                    .iter()
                    .map(|edition| edition.number().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            span,
        )),
    }
}

/// The edition `source` is read in: its header's, else `requested` (an
/// explicit `--edition`), else `default`.
///
/// # Errors
///
/// Returns [`UNSUPPORTED_EDITION`] for an unknown header and
/// [`EDITION_CONFLICT`], at the header, when the header and `requested` name
/// different editions. The message quotes the request as it was spelled.
pub fn document_edition(
    source: &str,
    requested: Option<EditionRequest>,
    default: Edition,
    options: &ParseOptions,
) -> Result<Edition, ParseError> {
    match (edition_header(source, options)?, requested) {
        (Some(header), Some(requested)) if header.edition != requested.edition => {
            Err(ParseError::coded(
                EDITION_CONFLICT,
                format!(
                    "the document declares 'fsl {}' but --edition {} was requested",
                    header.edition.number(),
                    requested.spelling
                ),
                header.span,
            ))
        }
        (Some(EditionHeader { edition, .. }), _) => Ok(edition),
        (None, Some(requested)) => Ok(requested.edition),
        (None, None) => Ok(default),
    }
}

/// The byte offset of the first significant character: after a leading BOM,
/// whitespace and `//` comments, as the lexer skips them. Where an edition
/// header starts, and where one is inserted.
#[must_use]
pub fn first_significant_offset(source: &str) -> usize {
    let mut offset = if source.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    loop {
        let rest = &source[offset..];
        let trimmed = rest.trim_start_matches(char::is_whitespace);
        offset += rest.len() - trimmed.len();
        if trimmed.starts_with("//") {
            offset += trimmed.find('\n').unwrap_or(trimmed.len());
        } else {
            return offset;
        }
    }
}

/// `(offset of "fsl", byte range of the digits)` of a header line.
fn header_line(source: &str) -> Option<(usize, std::ops::Range<usize>)> {
    let start = first_significant_offset(source);
    let line = source[start..].split('\n').next().unwrap_or_default();
    let tail = line.strip_prefix("fsl")?;
    let number = tail.trim_start_matches([' ', '\t']);
    if number.len() == tail.len() {
        return None;
    }
    let digits = number.len()
        - number
            .trim_start_matches(|ch: char| ch.is_ascii_digit())
            .len();
    if digits == 0 || !number[digits..].trim_matches([' ', '\t', '\r']).is_empty() {
        return None;
    }
    let digits_start = start + line.len() - number.len();
    Some((start, digits_start..digits_start + digits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_read_by_the_published_language() {
        let source = "fsl 5\nrequirements R {}\n";
        let header = edition_header(source, &ParseOptions::stable())
            .unwrap()
            .unwrap();
        assert_eq!(header.edition, Edition::Fsl5);
        assert_eq!((header.span.start.line, header.span.start.column), (1, 1));
        assert_eq!((header.span.end.line, header.span.end.column), (1, 6));
    }

    #[test]
    fn header_follows_the_lexers_trivia() {
        for (source, edition) in [
            ("fsl 4\nspec S {}", Edition::Fsl4),
            ("\u{feff}// note\n\n  fsl\t5  \r\nspec S {}", Edition::Fsl5),
            ("// 日本語のコメント\nfsl 5\nspec S {}", Edition::Fsl5),
            ("fsl 5", Edition::Fsl5),
        ] {
            assert_eq!(
                edition_header(source, &ParseOptions::stable())
                    .unwrap()
                    .map(|h| h.edition),
                Some(edition),
                "{source:?}"
            );
        }
        let header = edition_header("// 日本語\n  fsl 5\n", &ParseOptions::stable())
            .unwrap()
            .unwrap();
        assert_eq!((header.span.start.line, header.span.start.column), (2, 3));
    }

    #[test]
    fn other_lines_are_not_headers() {
        for source in [
            "spec S { const fsl = 1 }",
            "fsl S {}",
            "fsl5\nspec S {}",
            "fsl 5 spec S {}",
            "fsl 5x\nspec S {}",
            "spec S {}\nfsl 5\n",
            "fsl\n5\nspec S {}",
        ] {
            assert_eq!(
                edition_header(source, &ParseOptions::stable()),
                Ok(None),
                "{source:?}"
            );
        }
    }

    #[test]
    fn unknown_edition_numbers_are_errors() {
        for (source, number) in [
            ("fsl 6\nspec S {}", "6"),
            ("fsl 3\n", "3"),
            ("fsl 99999999999999999999999\n", "99999999999999999999999"),
        ] {
            let error = edition_header(source, &ParseOptions::stable()).unwrap_err();
            assert_eq!(error.code(), UNSUPPORTED_EDITION);
            assert!(
                error.message.contains(&format!("'fsl {number}'")),
                "{}",
                error.message
            );
        }
    }

    #[test]
    fn edition_option_and_resolution() {
        use Edition::{Fsl4, Fsl5};
        let stable = ParseOptions::stable();
        assert_eq!(Edition::parse_option("current", &stable), Ok(None));
        assert_eq!(
            Edition::parse_option("next", &stable),
            Ok(Some(Edition::Fsl5))
        );
        assert_eq!(Edition::parse_option("4", &stable), Ok(Some(Edition::Fsl4)));
        assert_eq!(Edition::parse_option("5", &stable), Ok(Some(Edition::Fsl5)));
        for value in ["6", "fsl5", "FSL5", ""] {
            assert_eq!(
                Edition::parse_option(value, &stable),
                Err("--edition must be current, next, 4, or 5".to_owned())
            );
        }

        let resolve = |source: &str, requested: Option<Edition>, default| {
            document_edition(
                source,
                requested.map(EditionRequest::numbered),
                default,
                &ParseOptions::stable(),
            )
            .map_err(|error| error.code().to_owned())
        };
        let plain = "spec S {}";
        let four = "fsl 4\nspec S {}";
        let five = "fsl 5\nspec S {}";
        assert_eq!(resolve(plain, None, Fsl4), Ok(Fsl4));
        assert_eq!(resolve(plain, None, Fsl5), Ok(Fsl5));
        assert_eq!(resolve(plain, Some(Fsl5), Fsl4), Ok(Fsl5));
        assert_eq!(resolve(five, None, Fsl4), Ok(Fsl5));
        assert_eq!(resolve(four, None, Fsl5), Ok(Fsl4));
        assert_eq!(resolve(four, Some(Fsl4), Fsl4), Ok(Fsl4));
        assert_eq!(
            resolve(five, Some(Fsl4), Fsl4),
            Err(EDITION_CONFLICT.to_owned())
        );
        assert_eq!(
            resolve(four, Some(Fsl5), Fsl4),
            Err(EDITION_CONFLICT.to_owned())
        );
        assert_eq!(
            resolve("fsl 6\nspec S {}", Some(Fsl5), Fsl4),
            Err(UNSUPPORTED_EDITION.to_owned())
        );
        // #1757: the conflict echoes the request as it was spelled; `next`
        // and `5` name one edition but are not one spelling.
        for (source, value, expected) in [
            (
                five,
                "4",
                "the document declares 'fsl 5' but --edition 4 was requested",
            ),
            (
                four,
                "5",
                "the document declares 'fsl 4' but --edition 5 was requested",
            ),
            (
                four,
                "next",
                "the document declares 'fsl 4' but --edition next was requested",
            ),
        ] {
            let requested = EditionRequest::parse(value, &stable).expect("valid --edition");
            let conflict =
                document_edition(source, requested, Fsl4, &ParseOptions::stable()).unwrap_err();
            assert_eq!(conflict.message, expected, "--edition {value}");
            assert_eq!(
                (conflict.span.start.line, conflict.span.start.column),
                (1, 1)
            );
        }
        // `current` requests nothing, so it never conflicts with a header.
        assert_eq!(EditionRequest::parse("current", &stable), Ok(None));
        assert_eq!(Fsl4.as_str(), "current");
        assert_eq!(Fsl5.as_str(), "next");
    }
}

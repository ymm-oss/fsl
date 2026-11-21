// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Syntax editions: the edition header a document opens with, and the one
//! edition type every command and the formatter share.
//!
//! A document whose first significant line (after a BOM, whitespace and `//`
//! comments, as the lexer skips them) is `fsl`, blanks, a major number and
//! optionally `.` and a minor number is headed by that edition
//! (`docs/design/DESIGN-migration.md`, "Edition minors"). The header is read
//! before lexing, because the edition picks the lexer's identifier rule, and
//! its tokens never reach a frontend. A document without a header is read as
//! `fsl 4`. A minor other than `0` is read only behind the unstable gate
//! ([`crate::UnstableFeature::Fsl5Minor`]).

use std::ops::Range;

use crate::{ParseError, ParseOptions, Span, UnstableFeature};

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

/// The development minor of the `fsl 5` family: `fsl 5.DEV_MINOR` is read
/// only behind [`UnstableFeature::Fsl5Minor`] (#1818 D1). The release that
/// publishes the minor renumbers this one constant.
pub const DEV_MINOR: u32 = 1;

/// A syntax edition: the `(major, minor)` a header names. `fsl 5` and
/// `fsl 5.0` are one edition.
///
/// Code that asks whether a document is in the next edition's family asks
/// [`Edition::family_fsl5`]; code that asks for one minor's addition asks
/// [`Edition::has`]. Comparing with [`Edition::Fsl5`] matches only `5.0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edition {
    major: u32,
    minor: u32,
}

impl Default for Edition {
    /// The published language; a document without a header is read in it.
    fn default() -> Self {
        Self::Fsl4
    }
}

impl Edition {
    /// The published language; a document without a header is read in it.
    /// (Spelled as the enum variant it was before minors, #1818 D1.)
    #[allow(non_upper_case_globals)]
    pub const Fsl4: Self = Self { major: 4, minor: 0 };
    /// The next edition (`--edition next`): `fsl 5`, `fsl 5.0`.
    #[allow(non_upper_case_globals)]
    pub const Fsl5: Self = Self { major: 5, minor: 0 };
    /// The development minor `fsl 5.DEV_MINOR` ([`DEV_MINOR`]), read only
    /// behind [`UnstableFeature::Fsl5Minor`].
    #[allow(non_upper_case_globals)]
    pub const Fsl5Dev: Self = Self {
        major: 5,
        minor: DEV_MINOR,
    };

    /// Every published edition, oldest first. [`Self::Fsl5Dev`] is not one.
    pub const ALL: &'static [Self] = &[Self::Fsl4, Self::Fsl5];

    /// The major number written in the header (`fsl 4`, `fsl 5`, and `5`
    /// for `fsl 5.1`).
    #[must_use]
    pub const fn number(self) -> u32 {
        self.major
    }

    /// The minor number: `0` for `fsl 5` and `fsl 5.0`.
    #[must_use]
    pub const fn minor(self) -> u32 {
        self.minor
    }

    /// Whether this edition is `fsl 5` or one of its minors: the edition
    /// family a document's next-edition reading (identifiers, keywords,
    /// glossary, layout, requirements checks) depends on.
    #[must_use]
    pub const fn family_fsl5(self) -> bool {
        self.major == 5
    }

    /// Whether `feature` is in this edition: same major, and a minor at or
    /// after the one that added it.
    #[must_use]
    pub const fn has(self, feature: Feature) -> bool {
        let since = feature.since();
        self.major == since.major && self.minor >= since.minor
    }

    /// Whether reading this edition needs [`UnstableFeature::Fsl5Minor`].
    #[must_use]
    pub const fn is_unstable(self) -> bool {
        self.major == 5 && self.minor != 0
    }

    /// The published edition a header major names (minor `0`).
    #[must_use]
    pub fn from_number(number: u64) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|edition| u64::from(edition.number()) == number)
    }

    /// The edition a header `major[.minor]` names under `options`: a
    /// published edition (`.0` names it too), or [`Self::Fsl5Dev`] when the
    /// gate is on. `None` for anything else.
    fn from_header(major: &str, minor: Option<&str>, options: &ParseOptions) -> Option<Self> {
        let edition = Self::from_number(major.parse().ok()?)?;
        match minor {
            None | Some("0") => Some(edition),
            Some(minor)
                if edition == Self::Fsl5
                    && minor.parse() == Ok(DEV_MINOR)
                    && options.unstable().contains(UnstableFeature::Fsl5Minor) =>
            {
                Some(Self::Fsl5Dev)
            }
            Some(_) => None,
        }
    }

    /// The published `--edition` spelling: `current`, or `next` for the
    /// `fsl 5` family.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        if self.family_fsl5() {
            "next"
        } else {
            "current"
        }
    }

    /// Parse an `--edition` value. `current` requests no edition (the
    /// document's header decides, and no header is [`Self::Fsl4`]); `next` is
    /// [`Self::Fsl5`]. The edition numbers `4` and `5` are accepted too, and
    /// `5.DEV_MINOR` behind the gate.
    ///
    /// # Errors
    ///
    /// Returns the usage message for any other value.
    pub fn parse_option(value: &str, options: &ParseOptions) -> Result<Option<Self>, String> {
        EditionRequest::parse(value, options).map(|request| request.map(|request| request.edition))
    }
}

impl std::fmt::Display for Edition {
    /// The number a header writes: `4`, `5`, `5.1`. `5.0` is written `5`.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.minor == 0 {
            write!(formatter, "{}", self.major)
        } else {
            write!(formatter, "{}.{}", self.major, self.minor)
        }
    }
}

/// A language addition that arrives with a minor of an edition family; ask
/// [`Edition::has`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feature {
    /// Stands in for the first addition of the development minor until one
    /// is named; only tests ask for it.
    #[doc(hidden)]
    DevPlaceholder,
}

impl Feature {
    /// The edition that added this feature.
    #[must_use]
    pub const fn since(self) -> Edition {
        match self {
            Self::DevPlaceholder => Edition::Fsl5Dev,
        }
    }
}

/// An explicit `--edition` request: the edition it names and whether it was
/// spelled `next`, so an [`EDITION_CONFLICT`] echoes what was typed (`next`,
/// `5`, `4` or `5.N`) rather than a number the user did not write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EditionRequest {
    pub edition: Edition,
    /// Spelled `next` rather than as a number.
    spelled_next: bool,
}

impl EditionRequest {
    /// A request spelled with the edition's number (`4`, `5`, `5.N`).
    #[must_use]
    pub const fn numbered(edition: Edition) -> Self {
        Self {
            edition,
            spelled_next: false,
        }
    }

    /// The request as it was spelled: `next`, `4`, `5` or `5.N`.
    #[must_use]
    pub fn spelling(&self) -> String {
        if self.spelled_next {
            "next".to_owned()
        } else {
            self.edition.to_string()
        }
    }

    /// Parse an `--edition` value, keeping its spelling. `current` is `None`,
    /// as in [`Edition::parse_option`]. `5.DEV_MINOR` is accepted only when
    /// `options` enable [`UnstableFeature::Fsl5Minor`]; the usage message
    /// does not name it.
    ///
    /// # Errors
    ///
    /// Returns the usage message for any other value.
    pub fn parse(value: &str, options: &ParseOptions) -> Result<Option<Self>, String> {
        match value {
            "current" => Ok(None),
            "next" => Ok(Some(Self {
                edition: Edition::Fsl5,
                spelled_next: true,
            })),
            "5" => Ok(Some(Self::numbered(Edition::Fsl5))),
            "4" => Ok(Some(Self::numbered(Edition::Fsl4))),
            value
                if options.unstable().contains(UnstableFeature::Fsl5Minor)
                    && value == Edition::Fsl5Dev.to_string() =>
            {
                Ok(Some(Self::numbered(Edition::Fsl5Dev)))
            }
            _ => Err("--edition must be current, next, 4, or 5".to_owned()),
        }
    }
}

/// The edition header of one document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditionHeader {
    pub edition: Edition,
    /// From `fsl` to the end of the number (its minor included).
    pub span: Span,
}

/// Read the edition header of `source` under `options`, before lexing.
///
/// `Ok(None)` when the first significant line is not a header. A header line
/// is `fsl`, spaces or tabs, a decimal major, optionally `.` and a decimal
/// minor (`0`, or digits without a leading zero), then only spaces, tabs or
/// `\r` to the end of the line. `fsl` followed by anything other than blanks
/// and a digit is an ordinary identifier, so `const fsl = 1` keeps its
/// meaning, and so is a major run directly followed by another character
/// (`fsl 5x`).
///
/// # Errors
///
/// Returns [`UNSUPPORTED_EDITION`] for a header naming another edition, for
/// a minor other than `0` unless it is [`DEV_MINOR`] of `fsl 5` and `options`
/// enable [`UnstableFeature::Fsl5Minor`], and for a header-shaped line that
/// is not a header: a major followed by `.` or a blank and then something
/// other than the grammar above (`fsl 5.`, `fsl 5.01`, `fsl 5.x`,
/// `fsl 5 next`).
pub fn edition_header(
    source: &str,
    options: &ParseOptions,
) -> Result<Option<EditionHeader>, ParseError> {
    let Some(line) = header_line(source) else {
        return Ok(None);
    };
    let span = crate::source_span(source, line.start, line.end);
    let edition = line.number.as_ref().and_then(|(major, minor)| {
        Edition::from_header(
            &source[major.clone()],
            minor.clone().map(|minor| &source[minor]),
            options,
        )
    });
    match edition {
        Some(edition) => Ok(Some(EditionHeader { edition, span })),
        None => Err(ParseError::coded(
            UNSUPPORTED_EDITION,
            format!(
                "unsupported syntax edition 'fsl {}'; supported editions: {}",
                &source[line.text_start..line.end],
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
/// different editions (major and minor compared). The message quotes the
/// request as it was spelled.
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
                    header.edition,
                    requested.spelling()
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

/// A header-shaped first significant line.
struct HeaderLine {
    /// The offset of `fsl`.
    start: usize,
    /// The offset of the major's first digit.
    text_start: usize,
    /// The end of the number, or of the line's text before trailing blanks
    /// when it is not well formed.
    end: usize,
    /// The byte ranges of the major and the minor when the line is well
    /// formed.
    number: Option<(Range<usize>, Option<Range<usize>>)>,
}

const BLANKS: [char; 3] = [' ', '\t', '\r'];

/// The number of ASCII digits `text` starts with.
fn digit_run(text: &str) -> usize {
    text.len()
        - text
            .trim_start_matches(|ch: char| ch.is_ascii_digit())
            .len()
}

/// The header-shaped first significant line of `source`, if any.
fn header_line(source: &str) -> Option<HeaderLine> {
    let start = first_significant_offset(source);
    let line = source[start..].split('\n').next().unwrap_or_default();
    let tail = line.strip_prefix("fsl")?;
    let number = tail.trim_start_matches([' ', '\t']);
    if number.len() == tail.len() {
        return None;
    }
    let major_len = digit_run(number);
    let rest = &number[major_len..];
    if major_len == 0 || !(rest.is_empty() || rest.starts_with(BLANKS) || rest.starts_with('.')) {
        return None;
    }
    let text_start = start + line.len() - number.len();
    let major = text_start..text_start + major_len;
    let well_formed = if rest.trim_matches(BLANKS).is_empty() {
        Some((major.clone(), None))
    } else {
        rest.strip_prefix('.').and_then(|after| {
            let minor_len = digit_run(after);
            let minor = &after[..minor_len];
            (minor_len > 0
                && (minor == "0" || !minor.starts_with('0'))
                && after[minor_len..].trim_matches(BLANKS).is_empty())
            .then(|| {
                let minor_start = major.end + 1;
                (major.clone(), Some(minor_start..minor_start + minor_len))
            })
        })
    };
    let end = match &well_formed {
        Some((_, Some(minor))) => minor.end,
        Some((major, None)) => major.end,
        None => text_start + number.trim_end_matches(BLANKS).len(),
    };
    Some(HeaderLine {
        start,
        text_start,
        end,
        number: well_formed,
    })
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
        #[allow(non_upper_case_globals)]
        const Fsl4: Edition = Edition::Fsl4;
        #[allow(non_upper_case_globals)]
        const Fsl5: Edition = Edition::Fsl5;
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

    fn gated() -> ParseOptions {
        ParseOptions::with_unstable(crate::UnstableFeatures::NONE.with(UnstableFeature::Fsl5Minor))
    }

    fn dev() -> String {
        format!("5.{DEV_MINOR}")
    }

    /// #1818 D1: `fsl 5.0` is `fsl 5` with or without the gate, and its span
    /// covers the minor; `fsl 4.0` is `fsl 4` the same way.
    #[test]
    fn a_zero_minor_names_the_published_edition() {
        for options in [ParseOptions::stable(), gated()] {
            for (source, edition, end) in [
                ("fsl 5.0\nspec S {}", Edition::Fsl5, 8),
                ("fsl 4.0\nspec S {}", Edition::Fsl4, 8),
                ("// c\n fsl\t5.0 \r\nspec S {}", Edition::Fsl5, 9),
            ] {
                let header = edition_header(source, &options).unwrap().unwrap();
                assert_eq!(header.edition, edition, "{source:?}");
                assert_eq!(header.span.end.column, end, "{source:?}");
                assert_eq!(
                    document_edition(source, None, Edition::Fsl4, &options),
                    document_edition(&source.replacen(".0", "", 1), None, Edition::Fsl4, &options),
                    "{source:?}"
                );
            }
        }
    }

    /// The development minor reads only behind the gate; without it the
    /// header is an unsupported edition listing the published ones.
    #[test]
    fn the_development_minor_needs_the_gate() {
        let source = format!("fsl {}\nspec S {{}}", dev());
        let error = edition_header(&source, &ParseOptions::stable()).unwrap_err();
        assert_eq!(error.code(), UNSUPPORTED_EDITION);
        assert_eq!(
            error.message,
            format!(
                "unsupported syntax edition 'fsl {}'; supported editions: 4, 5",
                dev()
            )
        );
        assert_eq!((error.span.start.column, error.span.end.column), (1, 8));

        let header = edition_header(&source, &gated()).unwrap().unwrap();
        assert_eq!(header.edition, Edition::Fsl5Dev);
        assert_eq!((header.span.start.column, header.span.end.column), (1, 8));
        let spaced = format!("\u{feff}// n\n\tfsl  {} \t\r\nspec S {{}}", dev());
        assert_eq!(
            edition_header(&spaced, &gated())
                .unwrap()
                .map(|h| h.edition),
            Some(Edition::Fsl5Dev)
        );
        assert!(edition_header(&spaced, &ParseOptions::stable()).is_err());

        // Other minors, and minors of other majors, are not editions even
        // with the gate on.
        for number in [
            format!("5.{}", DEV_MINOR + 1),
            format!("4.{DEV_MINOR}"),
            format!("6.{DEV_MINOR}"),
            "6.0".to_owned(),
            "5.99999999999999999999".to_owned(),
        ] {
            for options in [ParseOptions::stable(), gated()] {
                let error = edition_header(&format!("fsl {number}\n"), &options).unwrap_err();
                assert_eq!(error.code(), UNSUPPORTED_EDITION, "{number}");
                assert!(
                    error.message.contains(&format!("'fsl {number}'")),
                    "{}",
                    error.message
                );
            }
        }
    }

    /// A header-shaped line that is not a header is an unsupported edition,
    /// not a header-less document; the message quotes the line.
    #[test]
    fn malformed_header_shaped_lines_are_unsupported_editions() {
        for (source, quoted) in [
            ("fsl 5.\nspec S {}", "fsl 5."),
            ("fsl 5.01\nspec S {}", "fsl 5.01"),
            ("fsl 5.00\nspec S {}", "fsl 5.00"),
            ("fsl 5.x\nspec S {}", "fsl 5.x"),
            ("fsl 5.1x\nspec S {}", "fsl 5.1x"),
            ("fsl 5..1\nspec S {}", "fsl 5..1"),
            ("fsl 5.1.0\nspec S {}", "fsl 5.1.0"),
            ("fsl 5 next\nspec S {}", "fsl 5 next"),
            ("fsl 5.0 next  \r\nspec S {}", "fsl 5.0 next"),
            ("fsl 5 spec S {}", "fsl 5 spec S {}"),
            ("fsl\t5\t.1\n", "fsl 5\t.1"),
        ] {
            for options in [ParseOptions::stable(), gated()] {
                let error = edition_header(source, &options).unwrap_err();
                assert_eq!(error.code(), UNSUPPORTED_EDITION, "{source:?}");
                assert!(
                    error.message.contains(&format!("'{quoted}'")),
                    "{source:?}: {}",
                    error.message
                );
                assert_eq!(error.span.start.column, 1, "{source:?}");
            }
        }
    }

    #[test]
    fn family_and_feature_predicates() {
        assert!(!Edition::Fsl4.family_fsl5());
        assert!(Edition::Fsl5.family_fsl5());
        assert!(Edition::Fsl5Dev.family_fsl5());
        assert!(Edition::Fsl5Dev.has(Feature::DevPlaceholder));
        assert!(!Edition::Fsl5.has(Feature::DevPlaceholder));
        assert!(!Edition::Fsl4.has(Feature::DevPlaceholder));
        assert_eq!(Feature::DevPlaceholder.since(), Edition::Fsl5Dev);
        assert!(Edition::Fsl5Dev.is_unstable());
        assert!(!Edition::Fsl5.is_unstable() && !Edition::Fsl4.is_unstable());
        assert!(!Edition::ALL.contains(&Edition::Fsl5Dev));
        assert_eq!(Edition::default(), Edition::Fsl4);
        assert_eq!(
            (Edition::Fsl5Dev.number(), Edition::Fsl5Dev.minor()),
            (5, DEV_MINOR)
        );
        assert_eq!(Edition::Fsl5Dev.as_str(), "next");
        assert_eq!(Edition::Fsl5Dev.to_string(), dev());
        assert_eq!(Edition::Fsl5.to_string(), "5");
        assert!(Edition::Fsl4 < Edition::Fsl5 && Edition::Fsl5 < Edition::Fsl5Dev);
    }

    /// `--edition 5.N` is accepted only behind the gate, and a conflict
    /// compares major and minor.
    #[test]
    fn the_development_minor_as_an_edition_request() {
        let stable = ParseOptions::stable();
        let usage = Err("--edition must be current, next, 4, or 5".to_owned());
        assert_eq!(EditionRequest::parse(&dev(), &stable), usage);
        assert_eq!(
            EditionRequest::parse(&dev(), &gated()),
            Ok(Some(EditionRequest::numbered(Edition::Fsl5Dev)))
        );
        for value in [
            "5.0".to_owned(),
            format!("5.{}", DEV_MINOR + 1),
            format!("{} ", dev()),
            "fsl5-minor".to_owned(),
        ] {
            assert_eq!(EditionRequest::parse(&value, &gated()), usage, "{value}");
        }
        // `next` stays 5.0 behind the gate.
        assert_eq!(
            Edition::parse_option("next", &gated()),
            Ok(Some(Edition::Fsl5))
        );

        let options = gated();
        let minor = format!("fsl {}\nspec S {{}}", dev());
        let five = "fsl 5\nspec S {}";
        let resolve = |source: &str, value: &str| {
            let requested = EditionRequest::parse(value, &options).expect("valid --edition");
            document_edition(source, requested, Edition::Fsl4, &options)
        };
        assert_eq!(resolve(&minor, &dev()), Ok(Edition::Fsl5Dev));
        assert_eq!(resolve(&minor, "current"), Ok(Edition::Fsl5Dev));
        assert_eq!(resolve("spec S {}", &dev()), Ok(Edition::Fsl5Dev));
        assert_eq!(resolve("fsl 5.0\nspec S {}", "5"), Ok(Edition::Fsl5));
        for (source, value, declared) in [
            (minor.as_str(), "5", dev()),
            (minor.as_str(), "next", dev()),
            (minor.as_str(), "4", dev()),
            (five, dev().as_str(), "5".to_owned()),
            ("fsl 5.0\nspec S {}", dev().as_str(), "5".to_owned()),
            ("fsl 4\nspec S {}", dev().as_str(), "4".to_owned()),
        ] {
            let error = resolve(source, value).unwrap_err();
            assert_eq!(error.code(), EDITION_CONFLICT, "{source:?} {value}");
            assert_eq!(
                error.message,
                format!(
                    "the document declares 'fsl {declared}' but --edition {value} was requested"
                )
            );
        }
    }
}

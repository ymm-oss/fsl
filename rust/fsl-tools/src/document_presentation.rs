// SPDX-License-Identifier: Apache-2.0

//! Shared presentation values for requirements-document generation and checking.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    Ja,
    En,
}

impl Locale {
    /// The short code recorded in a generated document's `lang` frontmatter
    /// key (issue #329) and accepted by `fslc document generate --lang`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ja => "ja",
            Self::En => "en",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ja" => Some(Self::Ja),
            "en" => Some(Self::En),
            _ => None,
        }
    }
}

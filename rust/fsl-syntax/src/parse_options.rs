// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Parse options every document parse entry receives.
//!
//! The options carry nothing today: the `fsl 5` edition is published, so an
//! edition header is always read (`docs/design/parse-entry-inventory.md`).
//! The value is kept so the parse entries keep one explicit options argument.

/// Options every document parse entry receives.
///
/// The type has no public field; [`ParseOptions::stable`] (or `Default`)
/// is the only way to obtain one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ParseOptions {
    _private: (),
}

impl ParseOptions {
    /// The published language: every edition, read from the document's
    /// header (a document without one is `fsl 4`).
    #[must_use]
    pub const fn stable() -> Self {
        Self { _private: () }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_are_the_published_language() {
        assert_eq!(ParseOptions::default(), ParseOptions::stable());
    }
}

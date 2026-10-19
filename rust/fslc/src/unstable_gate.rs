// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The one place that turns the process environment into
//! [`fsl_syntax::ParseOptions`].
//!
//! Only process entry points call [`from_environment`]: the native CLI's
//! `main` and the language server's start, each exactly once. The browser
//! Worker has no environment and reads its request instead. Everything below
//! those entry points receives the options as a value.
//!
//! The gate is not advertised. An unknown feature name enables nothing and
//! prints nothing, so a process with the variable unset, empty, or holding
//! only unknown names behaves byte-for-byte like the published language.

use fsl_syntax::{ParseOptions, UnstableFeatures};

/// The environment variable holding the comma-separated feature list.
pub const ENVIRONMENT_VARIABLE: &str = "FSLC_UNSTABLE";

/// Options for a feature list taken from the environment (`None` = unset).
#[must_use]
pub fn from_value(value: Option<&str>) -> ParseOptions {
    let (features, _unknown) = UnstableFeatures::parse_list(value.unwrap_or_default());
    ParseOptions::with_unstable(features)
}

/// Read [`ENVIRONMENT_VARIABLE`] once. A value that is not valid Unicode is
/// treated as unset.
#[must_use]
pub fn from_environment() -> ParseOptions {
    from_value(std::env::var(ENVIRONMENT_VARIABLE).ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsl_syntax::UnstableFeature;

    #[test]
    fn unset_empty_and_unknown_values_are_the_stable_language() {
        for value in [None, Some(""), Some(" , "), Some("later"), Some("FSL5")] {
            assert_eq!(from_value(value), ParseOptions::stable(), "{value:?}");
        }
    }

    #[test]
    fn listed_feature_is_enabled() {
        for value in ["fsl5", "later,fsl5", " fsl5 "] {
            assert!(
                from_value(Some(value))
                    .unstable()
                    .contains(UnstableFeature::Fsl5),
                "{value:?}"
            );
        }
    }
}

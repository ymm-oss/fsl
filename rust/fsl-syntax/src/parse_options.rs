// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Parse options every document parse entry receives, and the unstable
//! feature gate.
//!
//! The published language needs no option: `fsl 4` and `fsl 5` headers are
//! always read (`docs/design/parse-entry-inventory.md`). The gate carries what
//! is developed on `main` before it is published: today only the development
//! minor of the `fsl 5` family ([`crate::DEV_MINOR`], #1818 D1).
//!
//! The gate is configured by the binary entry points only: the native CLI
//! and the language server read `FSLC_UNSTABLE` once at start
//! (`fslc_rust::unstable_gate`); the browser Worker reads its request's
//! `unstable` field. This library never reads the environment: callers pass a
//! [`ParseOptions`] explicitly, so tests choose the gate without touching
//! process state. With no feature enabled the parser behaves exactly as the
//! published language.

/// One unstable language feature that can be enabled behind the gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnstableFeature {
    /// The development minor of the `fsl 5` family: a `fsl 5.N` header
    /// ([`crate::DEV_MINOR`]) and `--edition 5.N`.
    Fsl5Minor,
}

impl UnstableFeature {
    /// Every feature, in bit order.
    pub const ALL: &'static [Self] = &[Self::Fsl5Minor];

    /// The name accepted in the feature list.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fsl5Minor => "fsl5-minor",
        }
    }

    const fn bit(self) -> u32 {
        match self {
            Self::Fsl5Minor => 1,
        }
    }

    /// Look a feature up by its exact name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|feature| feature.name() == name)
    }
}

/// A set of enabled unstable features.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct UnstableFeatures(u32);

impl UnstableFeatures {
    /// No feature enabled.
    pub const NONE: Self = Self(0);

    /// Whether `feature` is enabled.
    #[must_use]
    pub const fn contains(self, feature: UnstableFeature) -> bool {
        self.0 & feature.bit() != 0
    }

    /// This set with `feature` enabled.
    #[must_use]
    pub const fn with(self, feature: UnstableFeature) -> Self {
        Self(self.0 | feature.bit())
    }

    /// Whether no feature is enabled.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The enabled features' names, in bit order.
    #[must_use]
    pub fn names(self) -> Vec<&'static str> {
        UnstableFeature::ALL
            .iter()
            .filter(|feature| self.contains(**feature))
            .map(|feature| feature.name())
            .collect()
    }

    /// Parse a comma-separated feature list such as `"fsl5-minor"`.
    ///
    /// Surrounding whitespace and empty items are ignored. Unknown names are
    /// returned beside the set instead of failing, so a caller decides whether
    /// to report them; an unknown name never enables anything.
    #[must_use]
    pub fn parse_list(list: &str) -> (Self, Vec<String>) {
        Self::from_names(list.split(','))
    }

    /// Collect features from individual names (for example the items of a
    /// JSON array). Unknown names are returned beside the set.
    #[must_use]
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> (Self, Vec<String>) {
        let mut features = Self::NONE;
        let mut unknown = Vec::new();
        for name in names
            .into_iter()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            match UnstableFeature::from_name(name) {
                Some(feature) => features = features.with(feature),
                None => unknown.push(name.to_owned()),
            }
        }
        (features, unknown)
    }
}

/// Options every document parse entry receives.
///
/// The field is private: [`ParseOptions::stable`] (or `Default`) and
/// [`ParseOptions::with_unstable`] are the only ways to obtain one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ParseOptions {
    unstable: UnstableFeatures,
}

impl ParseOptions {
    /// The published language: every published edition, read from the
    /// document's header (a document without one is `fsl 4`), and no
    /// unstable feature.
    #[must_use]
    pub const fn stable() -> Self {
        Self {
            unstable: UnstableFeatures::NONE,
        }
    }

    /// Options with the given unstable features enabled.
    #[must_use]
    pub const fn with_unstable(unstable: UnstableFeatures) -> Self {
        Self { unstable }
    }

    /// The enabled unstable features.
    #[must_use]
    pub const fn unstable(&self) -> UnstableFeatures {
        self.unstable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_are_the_published_language() {
        assert_eq!(ParseOptions::default(), ParseOptions::stable());
        assert!(ParseOptions::stable().unstable().is_empty());
    }

    #[test]
    fn feature_list_enables_known_names_and_reports_unknown_ones() {
        let (features, unknown) = UnstableFeatures::parse_list(" fsl5-minor ,, later ");
        assert!(features.contains(UnstableFeature::Fsl5Minor));
        assert_eq!(features.names(), vec!["fsl5-minor"]);
        assert_eq!(unknown, vec!["later".to_owned()]);

        // The feature #1434 published is no longer a gate name.
        for list in ["FSL5-MINOR", "fsl5", "fsl5_minor"] {
            let (features, unknown) = UnstableFeatures::parse_list(list);
            assert!(features.is_empty(), "names are exact: {list}");
            assert_eq!(unknown, vec![list.to_owned()]);
        }

        let (features, unknown) = UnstableFeatures::parse_list("");
        assert!(features.is_empty());
        assert!(unknown.is_empty());
        assert!(features.names().is_empty());
    }

    #[test]
    fn gated_options_carry_the_feature() {
        let gated =
            ParseOptions::with_unstable(UnstableFeatures::NONE.with(UnstableFeature::Fsl5Minor));
        assert!(gated.unstable().contains(UnstableFeature::Fsl5Minor));
        assert_ne!(gated, ParseOptions::stable());
    }
}

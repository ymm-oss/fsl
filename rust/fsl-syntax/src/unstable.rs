// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Parse options and the unstable-feature gate.
//!
//! The gate is configured by the three binary entry points only (the native
//! CLI and the language server read one environment variable at start; the
//! browser Worker reads one request field). This library never reads the
//! environment: callers pass a [`ParseOptions`] explicitly, so tests can
//! choose the gate without touching process state.
//!
//! With no feature enabled the parser behaves exactly as before the gate
//! existed. Enabling `fsl5` so far only widens the identifier rule of a
//! document headed by its edition ([`crate::IdentifierRule`]). The inventory of
//! every parse entry and how it receives these options lives in
//! `docs/design/parse-entry-inventory.md`.

/// One unstable language feature that can be enabled behind the gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnstableFeature {
    /// The next syntax edition, developed on `main` behind the gate.
    Fsl5,
}

impl UnstableFeature {
    /// Every feature, in bit order.
    pub const ALL: &'static [Self] = &[Self::Fsl5];

    /// The name accepted in the feature list.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fsl5 => "fsl5",
        }
    }

    const fn bit(self) -> u32 {
        match self {
            Self::Fsl5 => 1,
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

    /// Parse a comma-separated feature list such as `"fsl5"`.
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
/// Fields are private so that the only ways to obtain options are the named
/// constructors below; the parse-entry inventory test treats a stable
/// construction outside the inventory as a bypass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ParseOptions {
    unstable: UnstableFeatures,
}

impl ParseOptions {
    /// The published language: no unstable feature.
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
    fn feature_list_enables_known_names_and_reports_unknown_ones() {
        let (features, unknown) = UnstableFeatures::parse_list(" fsl5 ,, later ");
        assert!(features.contains(UnstableFeature::Fsl5));
        assert_eq!(unknown, vec!["later".to_owned()]);

        let (features, unknown) = UnstableFeatures::parse_list("FSL5");
        assert!(features.is_empty(), "names are exact, not case-folded");
        assert_eq!(unknown, vec!["FSL5".to_owned()]);

        let (features, unknown) = UnstableFeatures::parse_list("");
        assert!(features.is_empty());
        assert!(unknown.is_empty());
    }

    #[test]
    fn stable_options_enable_nothing() {
        assert!(ParseOptions::stable().unstable().is_empty());
        assert_eq!(ParseOptions::default(), ParseOptions::stable());
        let gated = ParseOptions::with_unstable(UnstableFeatures::NONE.with(UnstableFeature::Fsl5));
        assert!(gated.unstable().contains(UnstableFeature::Fsl5));
    }
}

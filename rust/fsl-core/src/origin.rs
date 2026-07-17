// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use fsl_syntax::{MetaTag, Span};

/// Stable identity for one source node across one-to-many lowering.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct OriginId(pub String);

/// The source-side location represented by an origin chain.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OriginSite {
    pub source_file: Option<String>,
    pub span: Option<Span>,
    pub dialect: String,
    pub declaration_path: Vec<String>,
}

/// One semantic rewrite performed while lowering a source node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoweringStep {
    pub kind: String,
    pub detail: Option<String>,
}

/// Internal provenance carrier. This is deliberately not serialized by the
/// public Kernel v1 exporter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OriginChain {
    pub id: OriginId,
    pub dialect: String,
    pub primary: Option<OriginSite>,
    pub secondary: Vec<OriginSite>,
    pub lowering_steps: Vec<LoweringStep>,
    pub generated: bool,
}

impl OriginChain {
    #[must_use]
    pub fn generated_only(id: impl Into<String>, dialect: impl Into<String>) -> Self {
        Self {
            id: OriginId(id.into()),
            dialect: dialect.into(),
            primary: None,
            secondary: Vec::new(),
            lowering_steps: vec![LoweringStep {
                kind: "generated".to_owned(),
                detail: None,
            }],
            generated: true,
        }
    }

    pub(crate) fn set_source_file(&mut self, source_file: &str) {
        if let Some(primary) = &mut self.primary {
            primary.source_file = Some(source_file.to_owned());
        }
        for secondary in &mut self.secondary {
            secondary.source_file = Some(source_file.to_owned());
        }
    }
}

/// Internal target-to-origin graph. A target can have several origins and the
/// same origin identity can be bound to several targets.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OriginRegistry {
    bindings: BTreeMap<String, Vec<OriginChain>>,
}

impl OriginRegistry {
    pub fn bind(&mut self, target: impl Into<String>, origin: OriginChain) {
        let origins = self.bindings.entry(target.into()).or_default();
        if !origins.contains(&origin) {
            origins.push(origin);
        }
    }

    #[must_use]
    pub fn origins_for(&self, target: &str) -> &[OriginChain] {
        self.bindings.get(target).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn primary_for(&self, target: &str) -> Option<&OriginChain> {
        self.origins_for(target).first()
    }

    #[must_use]
    pub fn diagnostic_origin(&self, target: &str) -> Option<OriginChain> {
        let (primary, rest) = self.origins_for(target).split_first()?;
        let mut combined = primary.clone();
        combined.secondary.extend(
            rest.iter()
                .filter_map(|origin| origin.primary.clone())
                .chain(rest.iter().flat_map(|origin| origin.secondary.clone())),
        );
        Some(combined)
    }

    pub(crate) fn set_source_file(&mut self, source_file: &str) {
        for origins in self.bindings.values_mut() {
            for origin in origins {
                origin.set_source_file(source_file);
            }
        }
    }

    pub fn targets(&self) -> impl Iterator<Item = (&str, &[OriginChain])> {
        self.bindings
            .iter()
            .map(|(target, origins)| (target.as_str(), origins.as_slice()))
    }
}

/// Requirement relations are intentionally separate from declaration/source
/// provenance. `MetaTag.id` must never become an origin identity.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TraceabilityRegistry {
    relations: BTreeMap<String, Vec<MetaTag>>,
}

impl TraceabilityRegistry {
    pub(crate) fn bind(&mut self, target: impl Into<String>, requirement: MetaTag) {
        let requirements = self.relations.entry(target.into()).or_default();
        if !requirements
            .iter()
            .any(|existing| existing.id == requirement.id && existing.text == requirement.text)
        {
            requirements.push(requirement);
            requirements
                .sort_by(|left, right| (&left.id, &left.text).cmp(&(&right.id, &right.text)));
        }
    }

    #[must_use]
    pub fn requirement_for(&self, target: &str) -> Option<&MetaTag> {
        self.requirements_for(target).first()
    }

    #[must_use]
    pub fn requirements_for(&self, target: &str) -> &[MetaTag] {
        self.relations.get(target).map_or(&[], Vec::as_slice)
    }
}

#[must_use]
pub fn state_target(name: &str) -> String {
    format!("state:{name}")
}

#[must_use]
pub fn type_target(name: &str) -> String {
    format!("type:{name}")
}

pub const SPEC_TARGET: &str = "spec";
pub const INIT_TARGET: &str = "init";

#[must_use]
pub fn init_statement_target(index: usize) -> String {
    format!("init:{index}")
}

#[must_use]
pub fn action_target(name: &str) -> String {
    format!("action:{name}")
}

#[must_use]
pub fn action_guard_target(name: &str, index: usize) -> String {
    format!("action:{name}:guard:{index}")
}

#[must_use]
pub fn action_statement_target(name: &str, index: usize) -> String {
    format!("action:{name}:statement:{index}")
}

#[must_use]
pub fn property_target(kind: &str, name: &str) -> String {
    format!("property:{kind}:{name}")
}

pub const TERMINAL_TARGET: &str = "terminal";

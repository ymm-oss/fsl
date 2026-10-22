// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! The one owner of internal Kernel names and their public spellings.
//!
//! Three kinds of name meet at the output boundary:
//!
//! - declarations a compose document builds from two parts, internally
//!   `alias__name` ([`ComposeName`]);
//! - names an engine synthesises for a violation or an implicit property
//!   ([`SyntheticName`]);
//! - every other Kernel identifier, published as written except for the
//!   legacy string rule (the first `__` reads as `.`, and the database
//!   dialect's separator sentinel reads as `__`).
//!
//! [`NameMap`], owned by each [`crate::KernelModel`], is the only producer of
//! a [`PublicName`] and the only reader that maps a public spelling back to an
//! internal name. Model-free contexts (a Public Kernel or projection JSON
//! document, a bare state map, expression text) use
//! [`unmapped_public_spelling`], whose callers are enumerated by the
//! repository's naming drift test.

use std::collections::BTreeMap;
use std::fmt;

/// The database dialect's stand-in for a public `__` inside a generated
/// name, restored when the name is published.
pub(crate) const DB_SEPARATOR: &str = "QqDbSepqQ";

/// The legacy internal-to-public string rule.
fn display_name(name: &str) -> String {
    name.replacen("__", ".", 1).replace(DB_SEPARATOR, "__")
}

/// Public spelling of a Kernel identifier read where no [`NameMap`] is in
/// reach.
///
/// This is the legacy string rule, so it misspells an internal name the rule
/// cannot invert (an alias containing `__`, or a non-compose name containing
/// `__`). A caller that has a model publishes through [`NameMap`] instead.
#[must_use]
pub fn unmapped_public_spelling(name: &str) -> String {
    display_name(name)
}

/// Structural identity of one compose component declaration: the `use` alias
/// and the name declared by the component spec.
///
/// The internal Kernel name `alias__name` is not injective (alias `a__b` with
/// name `c` and alias `a` with name `b__c` share it), so public spellings are
/// built from these two parts rather than parsed back out of the internal name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComposeName {
    pub alias: String,
    pub name: String,
}

impl ComposeName {
    pub(crate) fn new(alias: &str, name: &str) -> Self {
        Self {
            alias: alias.to_owned(),
            name: name.to_owned(),
        }
    }

    /// The canonical public spelling, `alias.name`.
    #[must_use]
    pub fn display(&self) -> String {
        format!("{}.{}", self.alias, self.name)
    }

    /// The internal spelling `alias__name`, which v1 replay traces written
    /// before #1234 use for component actions.
    #[must_use]
    pub fn legacy(&self) -> String {
        compose_internal(&self.alias, &self.name)
    }
}

/// The internal Kernel name of a compose component declaration.
pub(crate) fn compose_internal(alias: &str, name: &str) -> String {
    format!("{alias}__{name}")
}

/// The stage state of a requirements `process`: `claim_stage` for `Claim`.
pub(crate) fn process_stage_state(process: &str) -> String {
    format!("{}_stage", process.to_lowercase())
}

/// The stage enum of a requirements `process`: `ClaimStage` for `Claim`.
pub(crate) fn process_stage_enum(process: &str) -> String {
    format!("{process}Stage")
}

/// The state of one data field of a requirements `process`: `claim_amount`.
pub(crate) fn process_field_state(process: &str, field: &str) -> String {
    format!("{}_{}", process.to_lowercase(), field)
}

/// The binder a next-edition entity's events and initial values quantify
/// over: the entity's first letter in lower case (`c` for `Claim`, `r` for
/// `Request`). A `process` quantifies over `c`.
pub(crate) fn entity_binder(entity: &str) -> String {
    entity
        .chars()
        .next()
        .map_or_else(|| "c".to_owned(), |first| first.to_lowercase().collect())
}

/// The invariant a next-edition decision table's `assuming Q` adds:
/// `_decide_REQ_2_3_assuming` for table `REQ-2-3`.
pub(crate) fn decide_assumption_invariant(table: &str) -> String {
    let safe = table
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("_decide_{safe}_assuming")
}

/// Every declaration name a next-edition entity lowers to: its stage enum,
/// its stage state, and one state per field.
pub(crate) fn entity_generated_names<'a>(
    entity: &str,
    fields: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut names = vec![process_stage_enum(entity), process_stage_state(entity)];
    names.extend(
        fields
            .into_iter()
            .map(|field| process_field_state(entity, field)),
    );
    names
}

/// The property an unnamed next-edition rule statement lowers to: the
/// requirement id with every character that cannot appear in a name spelled
/// `_`, then the statement kind, then the statement's ordinal among the
/// requirement's unnamed statements of that kind when it is not the first. `REQ_5_always` is the
/// first `always` of rule `REQ-5`, `REQ_5_always_2` its second.
pub(crate) fn rule_property_name(requirement: &str, kind: &str, ordinal: usize) -> String {
    let mut name = requirement
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if name.starts_with(|first: char| first.is_ascii_digit()) {
        name.insert_str(0, "REQ_");
    }
    if ordinal > 1 {
        format!("{name}_{kind}_{ordinal}")
    } else {
        format!("{name}_{kind}")
    }
}

/// Why a name written for a rule property (`Name: always …`) cannot be a
/// Kernel property name, or `None` when it can: a literal reads as itself in
/// every expression, and `__` is the compose separator a public spelling
/// reads back as `.`.
pub(crate) fn invalid_rule_property_name(name: &str) -> Option<&'static str> {
    if crate::reserved::is_reserved(name) {
        Some("is a literal")
    } else if name.contains("__") {
        Some("contains `__`, which a published name reads as a separator")
    } else {
        None
    }
}

/// A name an engine builds for a violation or an implicit property.
///
/// [`Self::internal`] is the only spelling and [`Self::parse_internal`] the
/// only reader. The prefixes overlap (`_partial_property_x` is both the
/// partial-operation name of property `x` and of an action named
/// `property_x`); the reader resolves an overlap toward the property kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntheticName<'a> {
    /// A conformance step whose `requires` does not hold.
    RequiresFailed { action: &'a str },
    /// A partial operation while an action's guard or body evaluates.
    PartialAction { action: &'a str },
    /// A partial operation while a property evaluates.
    PartialProperty { property: &'a str },
    /// A partial operation while the `terminal` condition evaluates.
    PartialTerminal,
    /// The implicit type-bound invariant of a state variable.
    Bounds { state: &'a str },
    /// A finite Map key outside its domain in an action step.
    KeyDomainAction { action: &'a str },
    /// A finite Map key outside its domain in a property.
    KeyDomainProperty { property: &'a str },
}

const REQUIRES_FAILED: &str = "_requires_failed_";
const PARTIAL: &str = "_partial_";
const PARTIAL_PROPERTY: &str = "_partial_property_";
const PARTIAL_TERMINAL: &str = "_partial_property_terminal";
const BOUNDS: &str = "_bounds_";
const KEY_DOMAIN: &str = "_key_domain_";
const KEY_DOMAIN_PROPERTY: &str = "_key_domain_property_";

impl<'a> SyntheticName<'a> {
    /// The internal spelling every engine reports.
    #[must_use]
    pub fn internal(&self) -> String {
        match self {
            Self::RequiresFailed { action } => format!("{REQUIRES_FAILED}{action}"),
            Self::PartialAction { action } => format!("{PARTIAL}{action}"),
            Self::PartialProperty { property } => format!("{PARTIAL_PROPERTY}{property}"),
            Self::PartialTerminal => PARTIAL_TERMINAL.to_owned(),
            Self::Bounds { state } => format!("{BOUNDS}{state}"),
            Self::KeyDomainAction { action } => format!("{KEY_DOMAIN}{action}"),
            Self::KeyDomainProperty { property } => format!("{KEY_DOMAIN_PROPERTY}{property}"),
        }
    }

    /// Classify `name` by its synthetic prefix; `None` for a declared name.
    ///
    /// The remainder is returned as written, so this reads either an internal
    /// name or a public spelling of one.
    #[must_use]
    pub fn parse_internal(name: &'a str) -> Option<Self> {
        if let Some(action) = name.strip_prefix(REQUIRES_FAILED) {
            return Some(Self::RequiresFailed { action });
        }
        if let Some(state) = name.strip_prefix(BOUNDS) {
            return Some(Self::Bounds { state });
        }
        if name == PARTIAL_TERMINAL {
            return Some(Self::PartialTerminal);
        }
        if let Some(property) = name.strip_prefix(PARTIAL_PROPERTY) {
            return Some(Self::PartialProperty { property });
        }
        if let Some(action) = name.strip_prefix(PARTIAL) {
            return Some(Self::PartialAction { action });
        }
        if let Some(property) = name.strip_prefix(KEY_DOMAIN_PROPERTY) {
            return Some(Self::KeyDomainProperty { property });
        }
        name.strip_prefix(KEY_DOMAIN)
            .map(|action| Self::KeyDomainAction { action })
    }

    /// The action a name built in an action step names.
    #[must_use]
    pub fn action(&self) -> Option<&'a str> {
        match self {
            Self::RequiresFailed { action }
            | Self::PartialAction { action }
            | Self::KeyDomainAction { action } => Some(action),
            _ => None,
        }
    }

    /// The state variable of a type-bound name.
    #[must_use]
    pub fn bounds_state(&self) -> Option<&'a str> {
        match self {
            Self::Bounds { state } => Some(state),
            _ => None,
        }
    }

    /// The property a partial-operation name names, with the `terminal`
    /// condition read as the property `terminal`.
    #[must_use]
    pub fn partial_property(&self) -> Option<&'a str> {
        match self {
            Self::PartialProperty { property } => Some(property),
            Self::PartialTerminal => Some("terminal"),
            _ => None,
        }
    }
}

/// A name as published in JSON, a replay echo, a mutation target, or a report.
///
/// Only [`NameMap`] produces one; a string cannot be promoted to a public name
/// outside `naming`:
///
/// ```compile_fail
/// let _ = fsl_core::PublicName(String::from("acct__go"));
/// ```
///
/// ```
/// fn publish(name: &fsl_core::PublicName) -> &str {
///     name.as_str()
/// }
/// let names = fsl_core::NameMap::default();
/// assert_eq!(publish(&names.public_action_key("acct__go")), "acct__go");
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PublicName(String);

impl PublicName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for PublicName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for PublicName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for PublicName {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for PublicName {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl From<PublicName> for String {
    fn from(name: PublicName) -> Self {
        name.0
    }
}

impl From<PublicName> for serde_json::Value {
    fn from(name: PublicName) -> Self {
        Self::String(name.0)
    }
}

/// A public spelling that matches more than one internal name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AmbiguousName<'n> {
    pub candidates: Vec<&'n str>,
}

/// The aliases a next-edition requirements glossary gives canonical names.
///
/// Aliases are display spellings only: name resolution replaced every alias
/// by its canonical name before lowering, so no Kernel name, and no
/// [`PublicName`], is ever an alias. This table is how a delivery surface
/// that displays names for a reader (diagnostics, `explain`) finds the alias
/// of a canonical name, and the one place the pair is kept.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Glossary {
    /// `(canonical as written, alias)` in glossary order. A field's canonical
    /// side is written `Entity.field`.
    entries: Vec<(String, String)>,
}

impl Glossary {
    pub(crate) fn insert(&mut self, written: String, alias: String) {
        self.entries.push((written, alias));
    }

    /// Whether the document had no glossary.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `(canonical, alias)` pairs in glossary order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(canonical, alias)| (canonical.as_str(), alias.as_str()))
    }

    /// The alias of `name`: an unqualified canonical name, `Entity.field`, or
    /// the bare name of a field when exactly one entry gives that field name
    /// an alias.
    #[must_use]
    pub fn alias(&self, name: &str) -> Option<&str> {
        if let Some((_, alias)) = self.entries.iter().find(|(canonical, _)| canonical == name) {
            return Some(alias);
        }
        let mut fields = self.entries.iter().filter(|(canonical, _)| {
            canonical
                .split_once('.')
                .is_some_and(|(_, field)| field == name)
        });
        match (fields.next(), fields.next()) {
            (Some((_, alias)), None) => Some(alias),
            _ => None,
        }
    }

    /// The canonical name (the field name for a qualified entry) `alias`
    /// resolves to.
    #[must_use]
    pub fn canonical(&self, alias: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(_, candidate)| candidate == alias)
            .map(|(canonical, _)| {
                canonical
                    .split_once('.')
                    .map_or(canonical.as_str(), |(_, field)| field)
            })
    }

    /// How a reader sees `name` in a message: `alias（name）` when it has an
    /// alias, `name` otherwise.
    #[must_use]
    pub fn display(&self, name: &str) -> String {
        self.alias(name)
            .map_or_else(|| name.to_owned(), |alias| format!("{alias}（{name}）"))
    }
}

/// Internal name to structural identity for one model's compose declarations.
/// Empty for any document that is not a compose.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NameMap {
    actions: BTreeMap<String, ComposeName>,
    states: BTreeMap<String, ComposeName>,
    properties: BTreeMap<String, ComposeName>,
    /// Empty unless the document is a next-edition requirements document
    /// with a glossary.
    glossary: Glossary,
}

impl NameMap {
    /// The aliases of the document's glossary (empty without one).
    #[must_use]
    pub fn glossary(&self) -> &Glossary {
        &self.glossary
    }

    pub(crate) fn set_glossary(&mut self, glossary: Glossary) {
        self.glossary = glossary;
    }

    pub(crate) fn insert_action(&mut self, alias: &str, name: &str) {
        self.actions
            .insert(compose_internal(alias, name), ComposeName::new(alias, name));
    }

    pub(crate) fn insert_state(&mut self, alias: &str, name: &str) {
        self.states
            .insert(compose_internal(alias, name), ComposeName::new(alias, name));
    }

    pub(crate) fn insert_property(&mut self, alias: &str, name: &str) {
        self.properties
            .insert(compose_internal(alias, name), ComposeName::new(alias, name));
    }

    #[cfg(test)]
    pub(crate) fn insert_action_identity(&mut self, internal: &str, identity: ComposeName) {
        self.actions.insert(internal.to_owned(), identity);
    }

    /// Structural compose identity of an action, or `None` when the action is
    /// not a compose component action (a direct spec action or a sync action).
    #[must_use]
    pub fn compose_action(&self, name: &str) -> Option<&ComposeName> {
        self.actions.get(name)
    }

    /// Structural compose identity of a component state variable.
    #[must_use]
    pub fn compose_state(&self, name: &str) -> Option<&ComposeName> {
        self.states.get(name)
    }

    /// Structural compose identity of a component property.
    #[must_use]
    pub fn compose_property(&self, name: &str) -> Option<&ComposeName> {
        self.properties.get(name)
    }

    /// Public spelling of `name` when it is, or was built from, the compose
    /// component action `action`; `None` for every other name.
    ///
    /// This is the only place a component action's public spelling is
    /// produced; the other action renderings differ only in how they spell a
    /// name no component owns.
    ///
    /// `name` is either the action itself or a name built while `action`
    /// stepped: a kind prefix followed by the internal action name
    /// (`_requires_failed_<action>`, `_partial_<action>`). The prefix is kept
    /// and the action suffix is respelled from the structural table.
    #[must_use]
    pub fn component_public_name(&self, name: &str, action: &str) -> Option<PublicName> {
        let component = self.compose_action(action)?;
        let prefix = name.strip_suffix(action)?;
        (prefix.is_empty() || prefix.starts_with('_') && prefix.ends_with('_'))
            .then(|| PublicName(format!("{prefix}{}", component.display())))
    }

    /// An action in a human-facing field: `alias.action` for a compose
    /// component action, the string rule otherwise.
    #[must_use]
    pub fn public_action(&self, name: &str) -> PublicName {
        self.component_public_name(name, name)
            .unwrap_or_else(|| PublicName(display_name(name)))
    }

    /// An action in an identifier field (a replay echo, a graph node ID, a
    /// claim key, a mutation target): `alias.action` for a compose component
    /// action, the exact Kernel name otherwise.
    #[must_use]
    pub fn public_action_key(&self, name: &str) -> PublicName {
        self.component_public_name(name, name)
            .unwrap_or_else(|| PublicName(name.to_owned()))
    }

    /// An action spelled by the string rule even when a component owns it,
    /// for the outputs that have always done so; [`Self::public_action`]
    /// differs from it only for a compose component action.
    #[must_use]
    pub fn legacy_action_spelling(&self, name: &str) -> PublicName {
        PublicName(display_name(name))
    }

    /// A violation, CTI, or mutation-kill name produced while `action`
    /// stepped (see [`Self::component_public_name`]), or the string rule
    /// over the whole name.
    #[must_use]
    pub fn public_violation(&self, name: &str, action: Option<&str>) -> PublicName {
        action
            .and_then(|action| self.component_public_name(name, action))
            .unwrap_or_else(|| PublicName(display_name(name)))
    }

    /// A violation name produced while `action` stepped, in an identifier
    /// field: respelled like [`Self::public_violation`] when the action is a
    /// component action and the name was built from it, the exact Kernel
    /// name otherwise (so a component invariant keeps its internal
    /// `alias__name`).
    #[must_use]
    pub fn public_violation_key(&self, name: &str, action: &str) -> PublicName {
        self.component_public_name(name, action)
            .unwrap_or_else(|| PublicName(name.to_owned()))
    }

    /// A mutation target path rooted at `action` (`<action>` followed by the
    /// path inside it), with the action respelled as a component action; the
    /// exact target otherwise.
    #[must_use]
    pub fn public_action_target(&self, target: &str, action: Option<&str>) -> PublicName {
        action
            .and_then(|action| {
                let rest = target.strip_prefix(action)?;
                let action = self.component_public_name(action, action)?;
                Some(PublicName(format!("{action}{rest}")))
            })
            .unwrap_or_else(|| PublicName(target.to_owned()))
    }

    /// A state variable.
    #[must_use]
    pub fn public_state(&self, name: &str) -> PublicName {
        PublicName(display_name(name))
    }

    /// A declared property (invariant, transition, reachable, leadsTo).
    #[must_use]
    pub fn public_property(&self, name: &str) -> PublicName {
        PublicName(display_name(name))
    }

    /// A declared type (domain, enum, struct).
    #[must_use]
    pub fn public_type(&self, name: &str) -> PublicName {
        PublicName(display_name(name))
    }

    /// Any other Kernel identifier: a spec name, a constant, an enum member,
    /// a state root read from an update target.
    #[must_use]
    pub fn public_identifier(&self, name: &str) -> PublicName {
        PublicName(display_name(name))
    }

    /// A synthetic name rendered from its parts: an action-step name like
    /// [`Self::public_violation`] in that action's context, `_bounds_`
    /// followed by the state's public spelling, and the string rule over the
    /// whole name for the property kinds.
    #[must_use]
    pub fn public_synthetic(&self, name: &SyntheticName<'_>) -> PublicName {
        match name {
            SyntheticName::Bounds { state } => {
                PublicName(format!("{BOUNDS}{}", self.public_state(state)))
            }
            _ => self.public_violation(&name.internal(), name.action()),
        }
    }

    /// Public spelling of a name derived from a compose component
    /// declaration: an action, a state variable, a property, or the
    /// `_bounds_<state>` name built from a state variable. `None` for every
    /// other name.
    ///
    /// Actions use their structural `alias.action`; state and property names
    /// keep the string rule every other output uses.
    #[must_use]
    pub fn compose_public_name(&self, name: &str) -> Option<PublicName> {
        let declared =
            |name: &str| self.states.contains_key(name) || self.properties.contains_key(name);
        self.component_public_name(name, name)
            .or_else(|| declared(name).then(|| PublicName(display_name(name))))
            .or_else(|| {
                let state = SyntheticName::parse_internal(name)?.bounds_state()?;
                declared(state).then(|| self.public_synthetic(&SyntheticName::Bounds { state }))
            })
    }

    /// Resolve an action spelling against `candidates` (internal names).
    ///
    /// A compose component action matches its canonical `alias.action` or its
    /// pre-#1234 `alias__action` spelling, both looked up from the structural
    /// table; every other action matches only its exact Kernel name.
    ///
    /// # Errors
    ///
    /// Returns every matching candidate when more than one matches, so the
    /// caller fails closed instead of picking one.
    pub fn resolve_action<'n>(
        &self,
        spelling: &str,
        candidates: impl IntoIterator<Item = &'n str>,
    ) -> Result<Option<&'n str>, AmbiguousName<'n>> {
        unique(
            candidates
                .into_iter()
                .filter(|name| match self.compose_action(name) {
                    Some(component) => {
                        component.display() == spelling || component.legacy() == spelling
                    }
                    None => *name == spelling,
                }),
        )
    }

    /// Resolve a state spelling against `candidates` by its public spelling.
    ///
    /// # Errors
    ///
    /// Returns every matching candidate when more than one matches.
    pub fn resolve_state<'n>(
        &self,
        spelling: &str,
        candidates: impl IntoIterator<Item = &'n str>,
    ) -> Result<Option<&'n str>, AmbiguousName<'n>> {
        unique(
            candidates
                .into_iter()
                .filter(|name| self.public_state(name) == spelling),
        )
    }

    /// Resolve a property spelling against `candidates` by its public
    /// spelling.
    ///
    /// # Errors
    ///
    /// Returns every matching candidate when more than one matches.
    pub fn resolve_property<'n>(
        &self,
        spelling: &str,
        candidates: impl IntoIterator<Item = &'n str>,
    ) -> Result<Option<&'n str>, AmbiguousName<'n>> {
        unique(
            candidates
                .into_iter()
                .filter(|name| self.public_property(name) == spelling),
        )
    }

    /// Resolve a synthetic spelling against `candidates` (internal synthetic
    /// names) by [`Self::public_synthetic`].
    ///
    /// # Errors
    ///
    /// Returns every matching candidate when more than one matches.
    pub fn resolve_synthetic<'n>(
        &self,
        spelling: &str,
        candidates: impl IntoIterator<Item = &'n str>,
    ) -> Result<Option<&'n str>, AmbiguousName<'n>> {
        unique(candidates.into_iter().filter(|name| {
            SyntheticName::parse_internal(name)
                .is_some_and(|synthetic| self.public_synthetic(&synthetic) == spelling)
        }))
    }
}

fn unique<'n>(
    mut matches: impl Iterator<Item = &'n str>,
) -> Result<Option<&'n str>, AmbiguousName<'n>> {
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    let rest = matches.collect::<Vec<_>>();
    if rest.is_empty() {
        return Ok(Some(first));
    }
    let mut candidates = vec![first];
    candidates.extend(rest);
    Err(AmbiguousName { candidates })
}

#[cfg(test)]
mod tests {
    use super::{AmbiguousName, ComposeName, NameMap, SyntheticName};
    use crate::{CoreError, FileResolver, KernelModel, build_model, parse_kernel_source};

    struct Components;

    impl FileResolver for Components {
        fn read(&self, path: &str) -> Result<String, CoreError> {
            let name = path.trim_end_matches(".fsl");
            Ok(format!(
                "spec {name} {{ state {{ bal: 0..1 }} init {{ bal = 0 }} \
                 action go() {{ requires bal < 1  bal = 1 }} \
                 invariant Inv {{ bal >= 0 }} }}"
            ))
        }
    }

    fn compose(aliases: &[&str]) -> KernelModel {
        let uses = aliases
            .iter()
            .map(|alias| format!("use Acct as {alias} from \"Acct.fsl\""))
            .collect::<Vec<_>>()
            .join("  ");
        build_model(
            parse_kernel_source(&format!("compose Bank {{ {uses} }}"), &Components)
                .expect("lower compose"),
        )
        .expect("build compose model")
    }

    fn actions(model: &KernelModel) -> Vec<&str> {
        model
            .actions
            .iter()
            .map(|action| action.name.as_str())
            .collect()
    }

    fn states(model: &KernelModel) -> Vec<&str> {
        model.state.iter().map(|(name, _)| name.as_str()).collect()
    }

    fn properties(model: &KernelModel) -> Vec<&str> {
        model
            .invariants
            .iter()
            .map(|property| property.name.as_str())
            .collect()
    }

    #[test]
    fn compose_action_state_and_property_round_trip_through_the_structural_table() {
        let model = compose(&["acct"]);
        let names = model.names();
        assert_eq!(
            names
                .compose_state("acct__bal")
                .map(ComposeName::display)
                .as_deref(),
            Some("acct.bal")
        );
        assert_eq!(
            names
                .compose_property("acct__Inv")
                .map(ComposeName::display)
                .as_deref(),
            Some("acct.Inv")
        );

        let action = names.public_action("acct__go");
        assert_eq!(action, "acct.go");
        assert_eq!(
            names.resolve_action(action.as_str(), actions(&model)),
            Ok(Some("acct__go"))
        );
        assert_eq!(
            names.resolve_action("acct__go", actions(&model)),
            Ok(Some("acct__go"))
        );

        let state = names.public_state("acct__bal");
        assert_eq!(state, "acct.bal");
        assert_eq!(
            names.resolve_state(state.as_str(), states(&model)),
            Ok(Some("acct__bal"))
        );

        let property = names.public_property("acct__Inv");
        assert_eq!(property, "acct.Inv");
        assert_eq!(
            names.resolve_property(property.as_str(), properties(&model)),
            Ok(Some("acct__Inv"))
        );
    }

    #[test]
    fn every_synthetic_kind_round_trips_through_its_internal_and_public_spelling() {
        let model = compose(&["acct"]);
        let names = model.names();
        for (synthetic, public) in [
            (
                SyntheticName::RequiresFailed { action: "acct__go" },
                "_requires_failed_acct.go",
            ),
            (
                SyntheticName::PartialAction { action: "acct__go" },
                "_partial_acct.go",
            ),
            (
                SyntheticName::PartialProperty {
                    property: "acct__Inv",
                },
                "_partial_property_acct.Inv",
            ),
            (SyntheticName::PartialTerminal, "_partial_property_terminal"),
            (
                SyntheticName::Bounds { state: "acct__bal" },
                "_bounds_acct.bal",
            ),
            (
                SyntheticName::KeyDomainAction { action: "acct__go" },
                "_key_domain_acct.go",
            ),
            (
                SyntheticName::KeyDomainProperty {
                    property: "acct__Inv",
                },
                "_key_domain_property_acct.Inv",
            ),
        ] {
            let internal = synthetic.internal();
            assert_eq!(
                SyntheticName::parse_internal(&internal),
                Some(synthetic),
                "{internal}"
            );
            assert_eq!(names.public_synthetic(&synthetic), public, "{internal}");
            assert_eq!(
                names.resolve_synthetic(public, [internal.as_str()]),
                Ok(Some(internal.as_str())),
                "{internal}"
            );
        }
        assert_eq!(SyntheticName::parse_internal("acct__Inv"), None);
    }

    #[test]
    fn a_non_compose_double_underscore_name_round_trips_only_through_its_key() {
        let names = NameMap::default();
        // #1256: the display spelling of an action is not resolvable; the
        // identifier spelling is.
        assert_eq!(names.public_action("foo__bar"), "foo.bar");
        assert_eq!(names.resolve_action("foo.bar", ["foo__bar"]), Ok(None));
        assert_eq!(names.public_action_key("foo__bar"), "foo__bar");
        assert_eq!(
            names.resolve_action("foo__bar", ["foo__bar"]),
            Ok(Some("foo__bar"))
        );
        assert_eq!(names.public_state("foo__bar"), "foo.bar");
        assert_eq!(
            names.resolve_state("foo.bar", ["foo__bar"]),
            Ok(Some("foo__bar"))
        );
    }

    #[test]
    fn an_alias_containing_the_separator_keeps_todays_state_spelling() {
        let model = compose(&["a__b"]);
        let names = model.names();
        assert_eq!(
            names
                .compose_state("a__b__bal")
                .map(ComposeName::display)
                .as_deref(),
            Some("a__b.bal")
        );
        assert_eq!(names.public_action("a__b__go"), "a__b.go");
        assert_eq!(
            names.resolve_action("a__b.go", actions(&model)),
            Ok(Some("a__b__go"))
        );
        // Stage 1 keeps the string rule for state and property names, so the
        // structural `a__b.bal` is not today's spelling.
        assert_eq!(names.public_state("a__b__bal"), "a.b__bal");
        assert_eq!(
            names.resolve_state("a.b__bal", states(&model)),
            Ok(Some("a__b__bal"))
        );
        assert_eq!(names.resolve_state("a__b.bal", states(&model)), Ok(None));
        assert_eq!(names.public_property("a__b__Inv"), "a.b__Inv");
    }

    #[test]
    fn overlapping_synthetic_prefixes_resolve_toward_the_property_kind() {
        let action = SyntheticName::PartialAction {
            action: "property_x",
        };
        assert_eq!(
            SyntheticName::parse_internal(&action.internal()),
            Some(SyntheticName::PartialProperty { property: "x" })
        );
        let action = SyntheticName::KeyDomainAction {
            action: "property_x",
        };
        assert_eq!(
            SyntheticName::parse_internal(&action.internal()),
            Some(SyntheticName::KeyDomainProperty { property: "x" })
        );
        let property = SyntheticName::PartialProperty {
            property: "terminal",
        };
        assert_eq!(
            SyntheticName::parse_internal(&property.internal()),
            Some(SyntheticName::PartialTerminal)
        );
        assert_eq!(
            SyntheticName::PartialTerminal.partial_property(),
            Some("terminal")
        );
    }

    #[test]
    fn a_public_spelling_two_internal_names_share_fails_closed() {
        let names = NameMap::default();
        let candidates = ["x__y__z", "x__yQqDbSepqQz"];
        assert_eq!(
            names.public_state(candidates[0]),
            names.public_state(candidates[1])
        );
        assert_eq!(
            names.resolve_state("x.y__z", candidates),
            Err(AmbiguousName {
                candidates: candidates.to_vec()
            })
        );
    }

    #[test]
    fn todays_string_rule_edges_are_pinned() {
        let model = compose(&["acct"]);
        let names = model.names();
        // A property whose name ends in `_<component action>` is respelled
        // as if the action had built it.
        assert_eq!(
            names.public_violation("_partial_property_p_acct__go", Some("acct__go")),
            "_partial_property_p_acct.go"
        );
        // `_bounds_` rendered from its parts and the whole-name rule part
        // ways for a state that starts with `_`.
        let state = "_a__b";
        assert_eq!(
            names.public_synthetic(&SyntheticName::Bounds { state }),
            "_bounds__a.b"
        );
        assert_eq!(
            names.public_violation(&SyntheticName::Bounds { state }.internal(), None),
            "_bounds.a__b"
        );
        // A component invariant keeps its internal spelling in an identifier
        // field (#1255).
        assert_eq!(
            names.public_violation_key("acct__Inv", "acct__go"),
            "acct__Inv"
        );
    }
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Typed `CausalModel` construction and well-formedness validation for the
//! review-only causal profile (`docs/DESIGN-causal.md`, issue #321).
//!
//! The model is an authority surface separate from `KernelModel`: causal
//! claims are hypotheses about the real world and never lower into kernel
//! actions or properties. Validation here is well-formedness only — it never
//! judges real-world causal validity, and nothing in this module attaches
//! `proved`/`verified` to a claim.

use std::collections::{BTreeMap, BTreeSet};

use fsl_core::{FileResolver, build_model};
use fsl_syntax::{
    CausalClaimDecl, CausalInterval, CausalLag, CausalPersistence, CausalSource, MeasurementKind,
    ScopeRelationKind, ScopeSelection, Span, parse_causal,
};

pub const TIMEBASE_UNITS: &[&str] = &["tick", "hour", "day", "week"];
pub const SCOPE_DIMENSIONS: &[&str] = &["population", "environment", "segment"];

/// A fatal well-formedness error (analysis never starts).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalError {
    pub kind: &'static str,
    pub message: String,
    pub line: u32,
    pub column: u32,
}

impl CausalError {
    fn at(kind: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            kind,
            message: message.into(),
            line: span.start.line,
            column: span.start.column,
        }
    }
}

/// A non-fatal review warning emitted by `causal check`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalWarning {
    pub kind: &'static str,
    pub message: String,
    pub line: u32,
    pub column: u32,
    pub involved_nodes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum VariableRole {
    Intervention,
    Mediator,
    Outcome,
    Context,
}

impl VariableRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intervention => "intervention",
            Self::Mediator => "mediator",
            Self::Outcome => "outcome",
            Self::Context => "context",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Polarity {
    Positive,
    Negative,
    Unknown,
}

impl Polarity {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
            Self::Unknown => "unknown",
        }
    }

    /// Sign product with `unknown` absorption (`docs/DESIGN-causal.md` §6).
    #[must_use]
    pub fn product(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Positive, Self::Positive) | (Self::Negative, Self::Negative) => Self::Positive,
            _ => Self::Negative,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Interval {
    pub min: u64,
    pub max: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lag {
    Known(Interval),
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Persistence {
    Known(Interval),
    Unknown,
    Unbounded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingKind {
    Action,
    Kpi,
    State,
    Property,
}

impl BindingKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Kpi => "kpi",
            Self::State => "state",
            Self::Property => "property",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Binding {
    pub kind: BindingKind,
    pub alias: String,
    pub name: String,
}

impl Binding {
    #[must_use]
    pub fn node_id(&self) -> String {
        format!("{}:{}.{}", self.kind.as_str(), self.alias, self.name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Variable {
    pub id: String,
    pub role: VariableRole,
    pub binds_action: Option<Binding>,
    pub observes: Option<Binding>,
    pub proxy: Option<Binding>,
    pub latent: bool,
    pub cadence: Option<u64>,
    pub deadline: Option<u64>,
    pub window: Option<Interval>,
    pub covers: Vec<String>,
    pub scope: BTreeMap<String, String>,
    pub line: u32,
    pub column: u32,
}

impl Variable {
    /// A variable is observable when it carries a direct or proxy measurement.
    #[must_use]
    pub fn observable(&self) -> bool {
        self.observes.is_some() || self.proxy.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimStatus {
    Active,
    Retired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Claim {
    pub id: String,
    pub source: String,
    pub target: String,
    pub version: u64,
    pub status: ClaimStatus,
    pub superseded_by: Option<String>,
    pub polarity: Polarity,
    pub lag: Lag,
    pub persists: Persistence,
    pub basis: String,
    pub evidence: Vec<String>,
    pub covers: Vec<String>,
    pub scope: BTreeMap<String, String>,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Feedback {
    pub id: String,
    pub claims: Vec<String>,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Clock {
    pub name: String,
    pub kernel_alias: String,
    pub ticks: u64,
    pub units: u64,
}

/// Per-dimension scope vocabulary with closed relations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScopeVocabulary {
    /// dimension -> declared tokens.
    pub tokens: BTreeMap<String, BTreeSet<String>>,
    /// dimension -> strict `subset_of` edges (child -> parents, transitive closure).
    pub subset_closure: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
    /// dimension -> symmetric `overlaps` pairs.
    pub overlaps: BTreeMap<String, BTreeSet<(String, String)>>,
    /// dimension -> symmetric `disjoint_with` pairs (closed over subsets).
    pub disjoint: BTreeMap<String, BTreeSet<(String, String)>>,
}

/// Names exported by one imported kernel/business/requirements spec.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedSpec {
    pub spec_name: String,
    pub actions: BTreeSet<String>,
    pub states: BTreeSet<String>,
    pub properties: BTreeSet<String>,
    pub kpis: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CausalModel {
    pub name: String,
    pub timebase: String,
    pub horizon: u64,
    pub scope: ScopeVocabulary,
    pub default_scope: BTreeMap<String, String>,
    pub clocks: BTreeMap<String, Clock>,
    pub imports: BTreeMap<String, ImportedSpec>,
    pub variables: BTreeMap<String, Variable>,
    pub claims: BTreeMap<String, Claim>,
    pub feedbacks: BTreeMap<String, Feedback>,
    pub evidence_refs: BTreeMap<String, String>,
}

impl CausalModel {
    /// Claims participating in current analysis (active only).
    pub fn active_claims(&self) -> impl Iterator<Item = &Claim> {
        self.claims
            .values()
            .filter(|claim| claim.status == ClaimStatus::Active)
    }

    /// The effective scope of a claim: explicit, else the model default,
    /// else empty (`unspecified`).
    #[must_use]
    pub fn claim_scope<'model>(
        &'model self,
        claim: &'model Claim,
    ) -> &'model BTreeMap<String, String> {
        if claim.scope.is_empty() {
            &self.default_scope
        } else {
            &claim.scope
        }
    }
}

/// Parse and type one causal document, resolving imports via `resolver`.
///
/// # Errors
///
/// Returns the first fatal well-formedness [`CausalError`]; parse errors use
/// kind `parse`. Warnings never block model construction.
pub fn build_causal_model(
    source: &str,
    resolver: &dyn FileResolver,
) -> Result<(CausalModel, Vec<CausalWarning>), CausalError> {
    let surface = parse_causal(source).map_err(|error| CausalError {
        kind: "parse",
        message: error.message.clone(),
        line: error.span.start.line,
        column: error.span.start.column,
    })?;
    type_causal_model(&surface, resolver)
}

fn duplicate(kind: &str, id: &str, span: Span) -> CausalError {
    CausalError::at(
        "causal_duplicate_id",
        format!("duplicate {kind} id '{id}'"),
        span,
    )
}

#[allow(clippy::too_many_lines)]
fn type_causal_model(
    surface: &CausalSource,
    resolver: &dyn FileResolver,
) -> Result<(CausalModel, Vec<CausalWarning>), CausalError> {
    let timebase = validate_timebase(surface)?;
    let horizon = surface
        .horizon
        .ok_or_else(|| {
            CausalError::at(
                "causal_invalid_model",
                "causal model requires a horizon declaration",
                surface.name_span,
            )
        })?
        .0;
    let imports = resolve_imports(surface, resolver)?;
    let scope = build_scope_vocabulary(surface)?;
    let default_scope = resolve_scope_selections(&surface.default_scope, &scope)?;
    let clocks = resolve_clocks(surface, &imports, &timebase)?;
    let evidence_refs = resolve_evidence(surface)?;
    let variables = resolve_variables(surface, &imports, &scope, horizon)?;
    let claims = resolve_claims(surface, &variables, &evidence_refs, &scope, horizon)?;
    let feedbacks = resolve_feedbacks(surface, &claims)?;
    let model = CausalModel {
        name: surface.name.clone(),
        timebase,
        horizon,
        scope,
        default_scope,
        clocks,
        imports,
        variables,
        claims,
        feedbacks,
        evidence_refs,
    };
    validate_loops(&model, surface)?;
    let warnings = collect_warnings(&model);
    Ok((model, warnings))
}

fn validate_timebase(surface: &CausalSource) -> Result<String, CausalError> {
    let Some((unit, span)) = surface.timebase.clone() else {
        return Err(CausalError::at(
            "causal_invalid_model",
            "causal model requires a timebase declaration",
            surface.name_span,
        ));
    };
    if TIMEBASE_UNITS.contains(&unit.as_str()) {
        Ok(unit)
    } else {
        Err(CausalError::at(
            "causal_invalid_model",
            format!(
                "unsupported timebase '{unit}' (v0 accepts {})",
                TIMEBASE_UNITS.join(" | ")
            ),
            span,
        ))
    }
}

fn resolve_imports(
    surface: &CausalSource,
    resolver: &dyn FileResolver,
) -> Result<BTreeMap<String, ImportedSpec>, CausalError> {
    let mut imports = BTreeMap::new();
    for import in &surface.uses {
        if imports.contains_key(&import.alias) {
            return Err(duplicate("uses alias", &import.alias, import.span));
        }
        let source = resolver.read(&import.path).map_err(|error| {
            CausalError::at(
                "causal_unknown_reference",
                format!("cannot read '{}': {}", import.path, error.message),
                import.span,
            )
        })?;
        let kernel = fsl_core::parse_kernel_source_with_file(&source, resolver, &import.path)
            .map_err(|error| {
                CausalError::at(
                    "causal_unknown_reference",
                    format!(
                        "imported spec '{}' failed its own check: {} (at {}:{})",
                        import.path, error.message, error.line, error.column
                    ),
                    import.span,
                )
            })
            .and_then(|kernel| {
                build_model(kernel).map_err(|error| {
                    CausalError::at(
                        "causal_unknown_reference",
                        format!(
                            "imported spec '{}' failed its own check: {}",
                            import.path, error.message
                        ),
                        import.span,
                    )
                })
            })?;
        let mut spec = ImportedSpec {
            spec_name: kernel.name.clone(),
            ..ImportedSpec::default()
        };
        spec.actions
            .extend(kernel.actions.iter().map(|action| action.name.clone()));
        spec.states
            .extend(kernel.state.iter().map(|(name, _)| name.clone()));
        spec.properties.extend(
            kernel
                .invariants
                .iter()
                .chain(kernel.transitions.iter())
                .chain(kernel.reachables.iter())
                .map(|property| property.name.clone()),
        );
        spec.properties
            .extend(kernel.leadstos.iter().map(|leadsto| leadsto.name.clone()));
        spec.kpis.extend(
            kernel
                .projections
                .iter()
                .map(|projection| projection.name.clone()),
        );
        imports.insert(import.alias.clone(), spec);
    }
    Ok(imports)
}

fn build_scope_vocabulary(surface: &CausalSource) -> Result<ScopeVocabulary, CausalError> {
    let mut vocabulary = ScopeVocabulary::default();
    for block in &surface.scopes {
        if !SCOPE_DIMENSIONS.contains(&block.dimension.as_str()) {
            return Err(CausalError::at(
                "causal_scope_invalid",
                format!(
                    "unknown scope dimension '{}' (expected {})",
                    block.dimension,
                    SCOPE_DIMENSIONS.join(" | ")
                ),
                block.dimension_span,
            ));
        }
        let tokens = vocabulary
            .tokens
            .entry(block.dimension.clone())
            .or_default();
        for (token, span) in &block.tokens {
            if !tokens.insert(token.clone()) {
                return Err(duplicate("scope token", token, *span));
            }
        }
    }
    for block in &surface.scopes {
        let tokens = vocabulary
            .tokens
            .get(&block.dimension)
            .cloned()
            .unwrap_or_default();
        let subsets = vocabulary
            .subset_closure
            .entry(block.dimension.clone())
            .or_default();
        let overlaps = vocabulary
            .overlaps
            .entry(block.dimension.clone())
            .or_default();
        let disjoint = vocabulary
            .disjoint
            .entry(block.dimension.clone())
            .or_default();
        for relation in &block.relations {
            for token in [&relation.left, &relation.right] {
                if !tokens.contains(token) {
                    return Err(CausalError::at(
                        "causal_scope_invalid",
                        format!(
                            "scope relation references undeclared token '{token}' in dimension '{}'",
                            block.dimension
                        ),
                        relation.span,
                    ));
                }
            }
            match relation.kind {
                ScopeRelationKind::SubsetOf => {
                    subsets
                        .entry(relation.left.clone())
                        .or_default()
                        .insert(relation.right.clone());
                }
                ScopeRelationKind::Overlaps => {
                    overlaps.insert(ordered_pair(&relation.left, &relation.right));
                }
                ScopeRelationKind::DisjointWith => {
                    disjoint.insert(ordered_pair(&relation.left, &relation.right));
                }
            }
        }
    }
    close_scope_vocabulary(&mut vocabulary, surface)?;
    Ok(vocabulary)
}

fn ordered_pair(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_owned(), right.to_owned())
    } else {
        (right.to_owned(), left.to_owned())
    }
}

fn close_scope_vocabulary(
    vocabulary: &mut ScopeVocabulary,
    surface: &CausalSource,
) -> Result<(), CausalError> {
    let anchor = surface.name_span;
    for (dimension, subsets) in &mut vocabulary.subset_closure {
        // Transitive closure of subset_of.
        loop {
            let mut additions: Vec<(String, String)> = Vec::new();
            for (child, parents) in subsets.iter() {
                for parent in parents {
                    if let Some(grandparents) = subsets.get(parent) {
                        for grandparent in grandparents {
                            if !parents.contains(grandparent) {
                                additions.push((child.clone(), grandparent.clone()));
                            }
                        }
                    }
                }
            }
            if additions.is_empty() {
                break;
            }
            for (child, parent) in additions {
                subsets.entry(child).or_default().insert(parent);
            }
        }
        // Cyclic strict inclusion.
        for (child, parents) in subsets.iter() {
            if parents.contains(child) {
                return Err(CausalError::at(
                    "causal_scope_invalid",
                    format!(
                        "cyclic strict inclusion involving scope token '{child}' in dimension '{dimension}'"
                    ),
                    anchor,
                ));
            }
        }
        // A subset pair shares members, so disjointness is contradictory;
        // disjoint(a, b) also propagates to every subset of a versus b.
        let disjoint = vocabulary.disjoint.entry(dimension.clone()).or_default();
        let mut closed: BTreeSet<(String, String)> = disjoint.clone();
        let tokens: Vec<String> = vocabulary
            .tokens
            .get(dimension)
            .map(|tokens| tokens.iter().cloned().collect())
            .unwrap_or_default();
        for (left, right) in disjoint.iter() {
            for token in &tokens {
                let token_parents = subsets.get(token);
                let under = |ancestor: &str| {
                    token == ancestor
                        || token_parents.is_some_and(|parents| parents.contains(ancestor))
                };
                for other in &tokens {
                    let other_parents = subsets.get(other);
                    let other_under = |ancestor: &str| {
                        other == ancestor
                            || other_parents.is_some_and(|parents| parents.contains(ancestor))
                    };
                    if under(left) && other_under(right) {
                        closed.insert(ordered_pair(token, other));
                    }
                }
            }
        }
        *disjoint = closed;
        let overlaps = vocabulary.overlaps.entry(dimension.clone()).or_default();
        for (left, right) in disjoint.iter() {
            if left == right {
                return Err(CausalError::at(
                    "causal_scope_invalid",
                    format!(
                        "scope token '{left}' cannot be disjoint with itself in dimension '{dimension}'"
                    ),
                    anchor,
                ));
            }
            if overlaps.contains(&(left.clone(), right.clone())) {
                return Err(CausalError::at(
                    "causal_scope_invalid",
                    format!(
                        "contradictory scope relations: '{left}' and '{right}' are declared both overlapping and disjoint in dimension '{dimension}'"
                    ),
                    anchor,
                ));
            }
            let left_under_right = subsets
                .get(left)
                .is_some_and(|parents| parents.contains(right));
            let right_under_left = subsets
                .get(right)
                .is_some_and(|parents| parents.contains(left));
            if left_under_right || right_under_left {
                return Err(CausalError::at(
                    "causal_scope_invalid",
                    format!(
                        "contradictory scope relations: '{left}' and '{right}' are related by subset_of but declared disjoint in dimension '{dimension}'"
                    ),
                    anchor,
                ));
            }
        }
    }
    Ok(())
}

fn resolve_scope_selections(
    selections: &[ScopeSelection],
    vocabulary: &ScopeVocabulary,
) -> Result<BTreeMap<String, String>, CausalError> {
    let mut resolved = BTreeMap::new();
    for selection in selections {
        if !SCOPE_DIMENSIONS.contains(&selection.dimension.as_str()) {
            return Err(CausalError::at(
                "causal_scope_invalid",
                format!("unknown scope dimension '{}'", selection.dimension),
                selection.span,
            ));
        }
        let declared = vocabulary
            .tokens
            .get(&selection.dimension)
            .is_some_and(|tokens| tokens.contains(&selection.token));
        if !declared {
            return Err(CausalError::at(
                "causal_scope_invalid",
                format!(
                    "scope token '{}' is not declared in dimension '{}'",
                    selection.token, selection.dimension
                ),
                selection.span,
            ));
        }
        if resolved
            .insert(selection.dimension.clone(), selection.token.clone())
            .is_some()
        {
            return Err(CausalError::at(
                "causal_scope_invalid",
                format!("duplicate scope dimension '{}'", selection.dimension),
                selection.span,
            ));
        }
    }
    Ok(resolved)
}

fn resolve_clocks(
    surface: &CausalSource,
    imports: &BTreeMap<String, ImportedSpec>,
    timebase: &str,
) -> Result<BTreeMap<String, Clock>, CausalError> {
    let mut clocks = BTreeMap::new();
    for clock in &surface.clocks {
        if clocks.contains_key(&clock.name) {
            return Err(duplicate("clock", &clock.name, clock.name_span));
        }
        if !imports.contains_key(&clock.kernel_alias) {
            return Err(CausalError::at(
                "causal_unknown_reference",
                format!(
                    "clock '{}' references unknown kernel alias '{}'",
                    clock.name, clock.kernel_alias
                ),
                clock.kernel_alias_span,
            ));
        }
        if clock.ticks == 0 || clock.units == 0 {
            return Err(CausalError::at(
                "causal_invalid_interval",
                format!(
                    "clock '{}' requires positive integers on both sides of the tick mapping",
                    clock.name
                ),
                clock.ratio_span,
            ));
        }
        if clock.unit_name != timebase {
            return Err(CausalError::at(
                "causal_invalid_model",
                format!(
                    "clock '{}' maps ticks to '{}' but the model timebase is '{timebase}'",
                    clock.name, clock.unit_name
                ),
                clock.ratio_span,
            ));
        }
        clocks.insert(
            clock.name.clone(),
            Clock {
                name: clock.name.clone(),
                kernel_alias: clock.kernel_alias.clone(),
                ticks: clock.ticks,
                units: clock.units,
            },
        );
    }
    Ok(clocks)
}

fn resolve_evidence(surface: &CausalSource) -> Result<BTreeMap<String, String>, CausalError> {
    let mut evidence = BTreeMap::new();
    for declaration in &surface.evidence {
        if evidence
            .insert(declaration.id.clone(), declaration.path.clone())
            .is_some()
        {
            return Err(duplicate("evidence", &declaration.id, declaration.id_span));
        }
    }
    Ok(evidence)
}

fn resolve_binding(
    reference: &fsl_syntax::CausalRef,
    kind: BindingKind,
    imports: &BTreeMap<String, ImportedSpec>,
) -> Result<Binding, CausalError> {
    let Some(spec) = imports.get(&reference.alias) else {
        return Err(CausalError::at(
            "causal_unknown_reference",
            format!("unknown uses alias '{}'", reference.alias),
            reference.span,
        ));
    };
    let names = match kind {
        BindingKind::Action => &spec.actions,
        BindingKind::Kpi => &spec.kpis,
        BindingKind::State => &spec.states,
        BindingKind::Property => &spec.properties,
    };
    if !names.contains(&reference.name) {
        return Err(CausalError::at(
            "causal_unknown_reference",
            format!(
                "spec '{}' (alias '{}') has no {} named '{}'",
                spec.spec_name,
                reference.alias,
                kind.as_str(),
                reference.name
            ),
            reference.span,
        ));
    }
    Ok(Binding {
        kind,
        alias: reference.alias.clone(),
        name: reference.name.clone(),
    })
}

fn measurement_binding(
    measurement: &fsl_syntax::MeasurementRef,
    imports: &BTreeMap<String, ImportedSpec>,
) -> Result<Binding, CausalError> {
    let kind = match measurement.kind {
        MeasurementKind::Kpi => BindingKind::Kpi,
        MeasurementKind::State => BindingKind::State,
        MeasurementKind::Property => BindingKind::Property,
    };
    resolve_binding(&measurement.target, kind, imports)
}

fn check_interval_bounds(
    interval: CausalInterval,
    what: &str,
    horizon: u64,
) -> Result<Interval, CausalError> {
    if interval.min > interval.max {
        return Err(CausalError::at(
            "causal_invalid_interval",
            format!("{what} interval has min > max"),
            interval.span,
        ));
    }
    if interval.max > horizon {
        return Err(CausalError::at(
            "causal_invalid_interval",
            format!("{what} interval exceeds the model horizon ({horizon})"),
            interval.span,
        ));
    }
    Ok(Interval {
        min: interval.min,
        max: interval.max,
    })
}

#[allow(clippy::too_many_lines)]
fn resolve_variables(
    surface: &CausalSource,
    imports: &BTreeMap<String, ImportedSpec>,
    vocabulary: &ScopeVocabulary,
    horizon: u64,
) -> Result<BTreeMap<String, Variable>, CausalError> {
    let mut variables = BTreeMap::new();
    for declaration in &surface.variables {
        if variables.contains_key(&declaration.id) {
            return Err(duplicate("variable", &declaration.id, declaration.id_span));
        }
        let Some((role_name, role_span)) = declaration.role.clone() else {
            return Err(CausalError::at(
                "causal_invalid_model",
                format!("variable '{}' requires a role", declaration.id),
                declaration.id_span,
            ));
        };
        let role = match role_name.as_str() {
            "intervention" => VariableRole::Intervention,
            "mediator" => VariableRole::Mediator,
            "outcome" => VariableRole::Outcome,
            "context" => VariableRole::Context,
            other => {
                return Err(CausalError::at(
                    "causal_invalid_model",
                    format!(
                        "unknown role '{other}' (expected intervention | mediator | outcome | context)"
                    ),
                    role_span,
                ));
            }
        };
        let binds_action = declaration
            .binds_action
            .as_ref()
            .map(|reference| resolve_binding(reference, BindingKind::Action, imports))
            .transpose()?;
        if binds_action.is_some() && role != VariableRole::Intervention {
            return Err(CausalError::at(
                "causal_binding_kind_mismatch",
                format!(
                    "variable '{}' has role {} but binds an action; only interventions bind actions",
                    declaration.id,
                    role.as_str()
                ),
                declaration.id_span,
            ));
        }
        let observes = declaration
            .observes
            .as_ref()
            .map(|measurement| measurement_binding(measurement, imports))
            .transpose()?;
        let proxy = declaration
            .proxy
            .as_ref()
            .map(|measurement| measurement_binding(measurement, imports))
            .transpose()?;
        if declaration.latent.is_some() && observes.is_some() {
            return Err(CausalError::at(
                "causal_binding_kind_mismatch",
                format!(
                    "variable '{}' is latent but declares a direct observes measurement",
                    declaration.id
                ),
                declaration.id_span,
            ));
        }
        if proxy.is_some() && declaration.latent.is_none() {
            return Err(CausalError::at(
                "causal_binding_kind_mismatch",
                format!(
                    "variable '{}' declares a proxy but is not latent",
                    declaration.id
                ),
                declaration.id_span,
            ));
        }
        if let Some((cadence, span)) = declaration.cadence {
            if cadence == 0 || cadence > horizon {
                return Err(CausalError::at(
                    "causal_invalid_interval",
                    format!(
                        "cadence for variable '{}' must be positive and within the horizon",
                        declaration.id
                    ),
                    span,
                ));
            }
            if observes.is_none() && proxy.is_none() {
                return Err(CausalError::at(
                    "causal_binding_kind_mismatch",
                    format!(
                        "variable '{}' declares a cadence but no observes/proxy measurement",
                        declaration.id
                    ),
                    span,
                ));
            }
        }
        if let Some((deadline, span)) = declaration.deadline
            && deadline > horizon
        {
            return Err(CausalError::at(
                "causal_invalid_interval",
                format!(
                    "deadline for variable '{}' exceeds the model horizon",
                    declaration.id
                ),
                span,
            ));
        }
        let window = declaration
            .window
            .map(|interval| check_interval_bounds(interval, "window", horizon))
            .transpose()?;
        let scope = resolve_scope_selections(&declaration.scope, vocabulary)?;
        variables.insert(
            declaration.id.clone(),
            Variable {
                id: declaration.id.clone(),
                role,
                binds_action,
                observes,
                proxy,
                latent: declaration.latent.is_some(),
                cadence: declaration.cadence.map(|(value, _)| value),
                deadline: declaration.deadline.map(|(value, _)| value),
                window,
                covers: declaration
                    .covers
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect(),
                scope,
                line: declaration.id_span.start.line,
                column: declaration.id_span.start.column,
            },
        );
    }
    Ok(variables)
}

fn claim_required<T>(
    value: Option<T>,
    claim: &CausalClaimDecl,
    field: &str,
) -> Result<T, CausalError> {
    value.ok_or_else(|| {
        CausalError::at(
            "causal_invalid_model",
            format!("claim '{}' requires a {field} field", claim.id),
            claim.id_span,
        )
    })
}

#[allow(clippy::too_many_lines)]
fn resolve_claims(
    surface: &CausalSource,
    variables: &BTreeMap<String, Variable>,
    evidence_refs: &BTreeMap<String, String>,
    vocabulary: &ScopeVocabulary,
    horizon: u64,
) -> Result<BTreeMap<String, Claim>, CausalError> {
    let mut claims = BTreeMap::new();
    for declaration in &surface.claims {
        if claims.contains_key(&declaration.id) {
            return Err(duplicate("claim", &declaration.id, declaration.id_span));
        }
        for (endpoint, span) in [&declaration.source, &declaration.target] {
            if !variables.contains_key(endpoint) {
                return Err(CausalError::at(
                    "causal_unknown_reference",
                    format!(
                        "claim '{}' references unknown variable '{endpoint}'",
                        declaration.id
                    ),
                    *span,
                ));
            }
        }
        if declaration.source.0 == declaration.target.0 {
            return Err(CausalError::at(
                "causal_instantaneous_loop",
                format!(
                    "claim '{}' is a self-loop on variable '{}'",
                    declaration.id, declaration.source.0
                ),
                declaration.id_span,
            ));
        }
        let (version, version_span) = claim_required(declaration.version, declaration, "version")?;
        if version == 0 {
            return Err(CausalError::at(
                "causal_invalid_model",
                format!(
                    "claim '{}' version must be a positive integer",
                    declaration.id
                ),
                version_span,
            ));
        }
        let (status_name, status_span) =
            claim_required(declaration.status.clone(), declaration, "status")?;
        let status = match status_name.as_str() {
            "active" => ClaimStatus::Active,
            "retired" => ClaimStatus::Retired,
            other => {
                return Err(CausalError::at(
                    "causal_invalid_model",
                    format!("unknown claim status '{other}' (expected active | retired)"),
                    status_span,
                ));
            }
        };
        if declaration.superseded_by.is_some() && status != ClaimStatus::Retired {
            return Err(CausalError::at(
                "causal_invalid_model",
                format!(
                    "claim '{}' declares superseded_by but is not retired",
                    declaration.id
                ),
                declaration.id_span,
            ));
        }
        let (polarity_name, polarity_span) =
            claim_required(declaration.polarity.clone(), declaration, "polarity")?;
        let polarity = match polarity_name.as_str() {
            "positive" => Polarity::Positive,
            "negative" => Polarity::Negative,
            "unknown" => Polarity::Unknown,
            other => {
                return Err(CausalError::at(
                    "causal_invalid_model",
                    format!("unknown polarity '{other}' (expected positive | negative | unknown)"),
                    polarity_span,
                ));
            }
        };
        let lag = match claim_required(declaration.lag, declaration, "lag")? {
            CausalLag::Known(interval) => {
                Lag::Known(check_interval_bounds(interval, "lag", horizon)?)
            }
            CausalLag::Unknown(_) => Lag::Unknown,
        };
        let persists = match claim_required(declaration.persists, declaration, "persists")? {
            CausalPersistence::Known(interval) => {
                Persistence::Known(check_interval_bounds(interval, "persists", horizon)?)
            }
            CausalPersistence::Unknown(_) => Persistence::Unknown,
            CausalPersistence::Unbounded(_) => Persistence::Unbounded,
        };
        let (basis, basis_span) = claim_required(declaration.basis.clone(), declaration, "basis")?;
        if basis != "hypothesis" && basis != "assumption" {
            return Err(CausalError::at(
                "causal_invalid_model",
                format!(
                    "unknown basis '{basis}' (expected hypothesis | assumption; supported support is derived from evidence only)"
                ),
                basis_span,
            ));
        }
        for (evidence_id, span) in &declaration.evidence {
            if !evidence_refs.contains_key(evidence_id) {
                return Err(CausalError::at(
                    "causal_unknown_reference",
                    format!(
                        "claim '{}' references undeclared evidence '{evidence_id}'",
                        declaration.id
                    ),
                    *span,
                ));
            }
        }
        let scope = resolve_scope_selections(&declaration.scope, vocabulary)?;
        claims.insert(
            declaration.id.clone(),
            Claim {
                id: declaration.id.clone(),
                source: declaration.source.0.clone(),
                target: declaration.target.0.clone(),
                version,
                status,
                superseded_by: declaration.superseded_by.as_ref().map(|(id, _)| id.clone()),
                polarity,
                lag,
                persists,
                basis,
                evidence: declaration
                    .evidence
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect(),
                covers: declaration
                    .covers
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect(),
                scope,
                line: declaration.id_span.start.line,
                column: declaration.id_span.start.column,
            },
        );
    }
    // superseded_by must resolve after all claims exist.
    for declaration in &surface.claims {
        if let Some((successor, span)) = &declaration.superseded_by
            && !claims.contains_key(successor)
        {
            return Err(CausalError::at(
                "causal_unknown_reference",
                format!(
                    "claim '{}' names unknown successor '{successor}'",
                    declaration.id
                ),
                *span,
            ));
        }
    }
    Ok(claims)
}

fn resolve_feedbacks(
    surface: &CausalSource,
    claims: &BTreeMap<String, Claim>,
) -> Result<BTreeMap<String, Feedback>, CausalError> {
    let mut feedbacks = BTreeMap::new();
    for declaration in &surface.feedbacks {
        if feedbacks.contains_key(&declaration.id) {
            return Err(duplicate("feedback", &declaration.id, declaration.id_span));
        }
        let mut ordered = Vec::new();
        for (claim_id, span) in &declaration.claims {
            let Some(claim) = claims.get(claim_id) else {
                return Err(CausalError::at(
                    "causal_unknown_reference",
                    format!(
                        "feedback '{}' references unknown claim '{claim_id}'",
                        declaration.id
                    ),
                    *span,
                ));
            };
            ordered.push(claim);
        }
        // The ordered claims must chain target -> source and close a simple cycle.
        let cyclic = !ordered.is_empty()
            && ordered
                .windows(2)
                .all(|pair| pair[0].target == pair[1].source)
            && ordered[ordered.len() - 1].target == ordered[0].source;
        let mut visited_sources = BTreeSet::new();
        let simple = ordered
            .iter()
            .all(|claim| visited_sources.insert(claim.source.clone()));
        if !cyclic || !simple {
            return Err(CausalError::at(
                "causal_feedback_not_cyclic",
                format!(
                    "feedback '{}' claims do not form one simple directed cycle",
                    declaration.id
                ),
                declaration.id_span,
            ));
        }
        feedbacks.insert(
            declaration.id.clone(),
            Feedback {
                id: declaration.id.clone(),
                claims: declaration
                    .claims
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect(),
                line: declaration.id_span.start.line,
                column: declaration.id_span.start.column,
            },
        );
    }
    Ok(feedbacks)
}

/// Instantaneous loops (a cycle whose minimum lag sum is zero) are errors:
/// a zero-lag cycle exists iff the subgraph of active claims with `lag.min == 0`
/// (or unknown lag, which cannot prove a positive delay) contains a cycle.
fn validate_loops(model: &CausalModel, surface: &CausalSource) -> Result<(), CausalError> {
    let mut zero_edges: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for claim in model.active_claims() {
        let zero_min = match claim.lag {
            Lag::Known(interval) => interval.min == 0,
            Lag::Unknown => false,
        };
        if zero_min {
            zero_edges
                .entry(claim.source.as_str())
                .or_default()
                .push((claim.target.as_str(), claim.id.as_str()));
        }
    }
    if let Some(cycle) = find_cycle(&zero_edges) {
        let span = surface
            .claims
            .iter()
            .find(|declaration| declaration.id == cycle[0])
            .map_or(surface.name_span, |declaration| declaration.id_span);
        return Err(CausalError::at(
            "causal_instantaneous_loop",
            format!(
                "instantaneous loop: cycle [{}] has minimum lag sum 0",
                cycle.join(", ")
            ),
            span,
        ));
    }
    Ok(())
}

/// Find one cycle in the zero-lag edge subgraph; returns the claim ids.
/// Recursive three-color DFS — causal graphs are review artifacts with at
/// most hundreds of nodes, so recursion depth is not a concern.
fn find_cycle(edges: &BTreeMap<&str, Vec<(&str, &str)>>) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        Visiting,
        Done,
    }
    fn visit<'graph>(
        node: &'graph str,
        edges: &BTreeMap<&'graph str, Vec<(&'graph str, &'graph str)>>,
        colors: &mut BTreeMap<&'graph str, Color>,
        path: &mut Vec<(&'graph str, &'graph str)>,
    ) -> Option<Vec<String>> {
        colors.insert(node, Color::Visiting);
        for &(target, claim_id) in edges.get(node).map_or(&[][..], Vec::as_slice) {
            match colors.get(target) {
                Some(Color::Visiting) => {
                    let mut cycle: Vec<String> = path
                        .iter()
                        .skip_while(|(source, _)| *source != target)
                        .map(|(_, id)| (*id).to_owned())
                        .collect();
                    cycle.push(claim_id.to_owned());
                    return Some(cycle);
                }
                Some(Color::Done) => {}
                None => {
                    path.push((node, claim_id));
                    if let Some(cycle) = visit(target, edges, colors, path) {
                        return Some(cycle);
                    }
                    path.pop();
                }
            }
        }
        colors.insert(node, Color::Done);
        None
    }
    let mut colors = BTreeMap::new();
    for &start in edges.keys() {
        if !colors.contains_key(start) {
            let mut path = Vec::new();
            if let Some(cycle) = visit(start, edges, &mut colors, &mut path) {
                return Some(cycle);
            }
        }
    }
    None
}

fn collect_warnings(model: &CausalModel) -> Vec<CausalWarning> {
    let mut warnings = Vec::new();
    for variable in model.variables.values() {
        match variable.role {
            VariableRole::Outcome if !variable.observable() => warnings.push(CausalWarning {
                kind: "causal_outcome_unmeasured",
                message: format!(
                    "outcome '{}' has no observes or proxy measurement",
                    variable.id
                ),
                line: variable.line,
                column: variable.column,
                involved_nodes: vec![format!("variable:{}", variable.id)],
            }),
            VariableRole::Intervention if variable.binds_action.is_none() => {
                warnings.push(CausalWarning {
                    kind: "causal_intervention_unbound",
                    message: format!(
                        "intervention '{}' binds no action or external intervention id",
                        variable.id
                    ),
                    line: variable.line,
                    column: variable.column,
                    involved_nodes: vec![format!("variable:{}", variable.id)],
                });
            }
            _ => {}
        }
        if variable.latent
            && variable.proxy.is_none()
            && on_intervention_outcome_path(model, &variable.id)
        {
            warnings.push(CausalWarning {
                kind: "causal_latent_without_proxy",
                message: format!(
                    "latent variable '{}' sits on an intervention-to-outcome path without a proxy",
                    variable.id
                ),
                line: variable.line,
                column: variable.column,
                involved_nodes: vec![format!("variable:{}", variable.id)],
            });
        }
    }
    warnings.extend(unacknowledged_feedback_warnings(model));
    warnings
}

/// Adjacency over active claims.
#[must_use]
pub fn active_adjacency(model: &CausalModel) -> BTreeMap<&str, Vec<&Claim>> {
    let mut adjacency: BTreeMap<&str, Vec<&Claim>> = BTreeMap::new();
    for claim in model.active_claims() {
        adjacency
            .entry(claim.source.as_str())
            .or_default()
            .push(claim);
    }
    adjacency
}

#[must_use]
pub fn reachable_from(adjacency: &BTreeMap<&str, Vec<&Claim>>, start: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![start.to_owned()];
    while let Some(node) = stack.pop() {
        if !seen.insert(node.clone()) {
            continue;
        }
        if let Some(claims) = adjacency.get(node.as_str()) {
            for claim in claims {
                if !seen.contains(&claim.target) {
                    stack.push(claim.target.clone());
                }
            }
        }
    }
    seen
}

fn on_intervention_outcome_path(model: &CausalModel, variable: &str) -> bool {
    let adjacency = active_adjacency(model);
    let reaches_outcome = reachable_from(&adjacency, variable).iter().any(|node| {
        node != variable
            && model
                .variables
                .get(node)
                .is_some_and(|candidate| candidate.role == VariableRole::Outcome)
    });
    if !reaches_outcome {
        return false;
    }
    model
        .variables
        .values()
        .filter(|candidate| candidate.role == VariableRole::Intervention)
        .any(|intervention| reachable_from(&adjacency, &intervention.id).contains(variable))
}

/// Tarjan strongly connected components over active claims, deterministic
/// (variables visited in sorted order). Returns SCCs as sorted variable sets.
/// Recursive — causal graphs are small review artifacts.
#[must_use]
pub fn strongly_connected_components(model: &CausalModel) -> Vec<BTreeSet<String>> {
    struct Tarjan<'graph> {
        adjacency: BTreeMap<&'graph str, Vec<&'graph Claim>>,
        counter: usize,
        indices: BTreeMap<String, usize>,
        lowlinks: BTreeMap<String, usize>,
        on_stack: BTreeSet<String>,
        stack: Vec<String>,
        components: Vec<BTreeSet<String>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, node: &str) {
            self.indices.insert(node.to_owned(), self.counter);
            self.lowlinks.insert(node.to_owned(), self.counter);
            self.counter += 1;
            self.stack.push(node.to_owned());
            self.on_stack.insert(node.to_owned());
            let successors: Vec<String> = self
                .adjacency
                .get(node)
                .map(|claims| claims.iter().map(|claim| claim.target.clone()).collect())
                .unwrap_or_default();
            for target in successors {
                if !self.indices.contains_key(&target) {
                    self.visit(&target);
                    let target_low = self.lowlinks[&target];
                    let entry = self.lowlinks.get_mut(node).expect("visited");
                    *entry = (*entry).min(target_low);
                } else if self.on_stack.contains(&target) {
                    let target_index = self.indices[&target];
                    let entry = self.lowlinks.get_mut(node).expect("visited");
                    *entry = (*entry).min(target_index);
                }
            }
            if self.lowlinks[node] == self.indices[node] {
                let mut component = BTreeSet::new();
                while let Some(member) = self.stack.pop() {
                    self.on_stack.remove(&member);
                    let done = member == node;
                    component.insert(member);
                    if done {
                        break;
                    }
                }
                self.components.push(component);
            }
        }
    }
    let mut tarjan = Tarjan {
        adjacency: active_adjacency(model),
        counter: 0,
        indices: BTreeMap::new(),
        lowlinks: BTreeMap::new(),
        on_stack: BTreeSet::new(),
        stack: Vec::new(),
        components: Vec::new(),
    };
    for start in model.variables.keys() {
        if !tarjan.indices.contains_key(start) {
            tarjan.visit(start);
        }
    }
    tarjan.components
}

fn unacknowledged_feedback_warnings(model: &CausalModel) -> Vec<CausalWarning> {
    let mut warnings = Vec::new();
    let declared_edges: BTreeSet<&str> = model
        .feedbacks
        .values()
        .flat_map(|feedback| feedback.claims.iter().map(String::as_str))
        .collect();
    for component in strongly_connected_components(model) {
        if component.len() < 2 {
            continue;
        }
        let undeclared: Vec<&Claim> = model
            .active_claims()
            .filter(|claim| {
                component.contains(&claim.source)
                    && component.contains(&claim.target)
                    && !declared_edges.contains(claim.id.as_str())
            })
            .collect();
        if let Some(first) = undeclared.first() {
            let involved = component
                .iter()
                .map(|variable| format!("variable:{variable}"))
                .chain(undeclared.iter().map(|claim| format!("claim:{}", claim.id)))
                .collect();
            warnings.push(CausalWarning {
                kind: "causal_unacknowledged_feedback",
                message: format!(
                    "cycle through [{}] contains claims not covered by any feedback declaration",
                    component.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
                line: first.line,
                column: first.column,
                involved_nodes: involved,
            });
        }
    }
    warnings
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use fsl_core::CoreError;

    pub(crate) struct MapResolver {
        pub files: BTreeMap<String, String>,
    }

    impl FileResolver for MapResolver {
        fn read(&self, path: &str) -> Result<String, CoreError> {
            self.files.get(path).cloned().ok_or_else(|| CoreError {
                message: format!("no such file '{path}'"),
                line: 1,
                column: 1,
                origin: None,
            })
        }
    }

    pub(crate) const BIZ_SPEC: &str = "spec Biz {
  state { x: 0..3 }
  init { x = 0 }
  action enable() { requires x < 3  x = x + 1 }
  invariant Ok { x >= 0 }
}";

    pub(crate) fn resolver() -> MapResolver {
        MapResolver {
            files: BTreeMap::from([("biz.fsl".to_owned(), BIZ_SPEC.to_owned())]),
        }
    }

    pub(crate) const VALID_MODEL: &str = r#"
causal Retention {
  uses biz from "biz.fsl"
  timebase day
  horizon 730
  scope population {
    token all_users
    token new_users subset_of all_users
  }
  default_scope { population all_users }
  clock biz_clock { kernel biz 1 tick = 1 day }
  variable support {
    role intervention
    binds action biz.enable
  }
  variable habit {
    role mediator
    latent
    proxy state biz.x
    cadence 7
  }
  variable retention {
    role outcome
    observes state biz.x
    cadence 7
    deadline 180
  }
  claim C_SupportHabit support -> habit {
    version 1
    status active
    polarity positive
    lag 7..30
    persists 30..180
    basis hypothesis
    evidence E1
  }
  claim C_HabitRetention habit -> retention {
    version 1
    status active
    polarity positive
    lag 30..90
    persists 90..365
    basis hypothesis
  }
  evidence E1 from "evidence/e1.causal.json"
}
"#;

    pub(crate) fn build(source: &str) -> Result<(CausalModel, Vec<CausalWarning>), CausalError> {
        build_causal_model(source, &resolver())
    }

    #[test]
    fn builds_valid_model_without_warnings() {
        let (model, warnings) = build(VALID_MODEL).expect("valid model");
        assert_eq!(model.name, "Retention");
        assert_eq!(model.timebase, "day");
        assert_eq!(model.horizon, 730);
        assert_eq!(model.variables.len(), 3);
        assert_eq!(model.claims.len(), 2);
        assert_eq!(model.clocks["biz_clock"].ticks, 1);
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn unknown_action_reference_fails_closed() {
        let source = VALID_MODEL.replace("binds action biz.enable", "binds action biz.missing");
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_unknown_reference");
        assert!(error.message.contains("missing"));
        assert!(error.line > 1);
    }

    #[test]
    fn unknown_uses_alias_fails_closed() {
        let source = VALID_MODEL.replace("binds action biz.enable", "binds action nope.enable");
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_unknown_reference");
    }

    #[test]
    fn duplicate_claim_id_is_rejected() {
        let source = VALID_MODEL.replace("C_HabitRetention habit", "C_SupportHabit habit");
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_duplicate_id");
    }

    #[test]
    fn interval_beyond_horizon_is_rejected() {
        let source = VALID_MODEL.replace("lag 30..90", "lag 30..900");
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_invalid_interval");
    }

    #[test]
    fn binding_kind_mismatch_is_rejected() {
        let source =
            VALID_MODEL.replace("role outcome", "role outcome\n    binds action biz.enable");
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_binding_kind_mismatch");
    }

    #[test]
    fn unsupported_timebase_is_rejected() {
        let source = VALID_MODEL.replace("timebase day", "timebase fortnight");
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_invalid_model");
        assert!(error.message.contains("fortnight"));
    }

    #[test]
    fn clock_unit_must_match_timebase() {
        let source = VALID_MODEL.replace("timebase day", "timebase week");
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_invalid_model");
        assert!(error.message.contains("timebase"));
    }

    #[test]
    fn scope_contradiction_is_rejected() {
        let source = VALID_MODEL.replace(
            "token new_users subset_of all_users",
            "token new_users subset_of all_users\n    new_users disjoint_with all_users",
        );
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_scope_invalid");
        assert!(error.message.contains("subset_of but declared disjoint"));
    }

    #[test]
    fn undeclared_default_scope_token_is_rejected() {
        let source = VALID_MODEL.replace(
            "default_scope { population all_users }",
            "default_scope { population everyone }",
        );
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_scope_invalid");
    }

    const LOOP_TAIL: &str = r"
  claim C_Back retention -> support {
    version 1
    status active
    polarity positive
    lag LAGRANGE
    persists unknown
    basis assumption
  }
FEEDBACK}
";

    fn looped(lag: &str, feedback: &str) -> String {
        let tail = LOOP_TAIL
            .replace("LAGRANGE", lag)
            .replace("FEEDBACK", feedback);
        VALID_MODEL.trim_end().trim_end_matches('}').to_owned() + &tail
    }

    #[test]
    fn delayed_cycle_without_feedback_declaration_warns() {
        let (model, warnings) = build(&looped("30..60", "")).expect("valid");
        assert_eq!(model.claims.len(), 3);
        assert_eq!(
            warnings
                .iter()
                .map(|warning| warning.kind)
                .collect::<Vec<_>>(),
            vec!["causal_unacknowledged_feedback"]
        );
    }

    #[test]
    fn declared_feedback_silences_the_warning() {
        let feedback = "  feedback F_Loop { claims C_Back, C_SupportHabit, C_HabitRetention }\n";
        let (model, warnings) = build(&looped("30..60", feedback)).expect("valid");
        assert_eq!(model.feedbacks.len(), 1);
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn instantaneous_loop_is_rejected_only_when_min_lag_sum_is_zero() {
        // One zero-min edge is fine: the cycle's minimum lag sum is 0+7+30 > 0.
        let (_, warnings) = build(&looped("0..60", "")).expect("delayed cycle is valid");
        assert_eq!(warnings.len(), 1);
        // Every edge min 0 -> the cycle's minimum lag sum is 0 -> error.
        let source = looped("0..60", "")
            .replace("lag 7..30", "lag 0..30")
            .replace("lag 30..90", "lag 0..90");
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_instantaneous_loop");
        assert!(error.message.contains("minimum lag sum 0"));
    }

    #[test]
    fn non_cyclic_feedback_declaration_is_rejected() {
        let feedback = "  feedback F_Broken { claims C_SupportHabit, C_Back }\n";
        let error = build(&looped("30..60", feedback)).expect_err("must fail");
        assert_eq!(error.kind, "causal_feedback_not_cyclic");
    }

    #[test]
    fn retired_claims_leave_current_analysis_but_keep_identity() {
        let source = looped("30..60", "").replace(
            "claim C_Back retention -> support {\n    version 1\n    status active",
            "claim C_Back retention -> support {\n    version 2\n    status retired",
        );
        let (model, warnings) = build(&source).expect("valid");
        assert_eq!(model.claims["C_Back"].status, ClaimStatus::Retired);
        assert_eq!(model.claims["C_Back"].version, 2);
        // Retired back-edge means no active cycle: no feedback warning.
        assert_eq!(warnings, Vec::new());
        assert_eq!(model.active_claims().count(), 2);
    }

    #[test]
    fn intervention_without_action_and_outcome_without_measurement_warn() {
        let source = VALID_MODEL
            .replace("    binds action biz.enable\n", "")
            .replace("    observes state biz.x\n    cadence 7\n", "");
        let (_, warnings) = build(&source).expect("valid");
        let kinds: Vec<&str> = warnings.iter().map(|warning| warning.kind).collect();
        assert!(kinds.contains(&"causal_intervention_unbound"));
        assert!(kinds.contains(&"causal_outcome_unmeasured"));
    }

    #[test]
    fn zero_version_is_rejected() {
        let source = VALID_MODEL.replace(
            "version 1\n    status active\n    polarity positive\n    lag 7..30",
            "version 0\n    status active\n    polarity positive\n    lag 7..30",
        );
        let error = build(&source).expect_err("must fail");
        assert_eq!(error.kind, "causal_invalid_model");
        assert!(error.message.contains("positive integer"));
    }

    #[test]
    fn polarity_product_follows_sign_rules_with_unknown_absorption() {
        use Polarity::{Negative, Positive, Unknown};
        assert_eq!(Positive.product(Positive), Positive);
        assert_eq!(Negative.product(Negative), Positive);
        assert_eq!(Positive.product(Negative), Negative);
        assert_eq!(Negative.product(Positive), Negative);
        for polarity in [Positive, Negative, Unknown] {
            assert_eq!(polarity.product(Unknown), Unknown);
            assert_eq!(Unknown.product(polarity), Unknown);
        }
    }
}

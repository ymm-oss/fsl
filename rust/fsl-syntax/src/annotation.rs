// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Typed declaration annotations shared by every FSL frontend.

use std::collections::BTreeMap;
use std::fmt;

use crate::{Span, SyntaxIdent};

/// A namespaced symbol such as `acme.review.owner`.
#[derive(Clone, Debug)]
pub struct SymbolPath {
    segments: Vec<String>,
    segment_spans: Vec<Span>,
    span: Span,
}

impl SymbolPath {
    /// Construct a validated symbol path.
    ///
    /// # Errors
    ///
    /// Returns [`AnnotationError`] when the path is empty or contains an empty segment.
    pub fn new(
        segments: impl IntoIterator<Item = String>,
        span: Span,
    ) -> Result<Self, AnnotationError> {
        let segments = segments.into_iter().collect::<Vec<_>>();
        if segments.is_empty() || segments.iter().any(|segment| segment.trim().is_empty()) {
            return Err(AnnotationError::new(
                "custom annotation namespace must contain non-empty segments",
                span,
            ));
        }
        let segment_spans = vec![span; segments.len()];
        Ok(Self {
            segments,
            segment_spans,
            span,
        })
    }

    /// Construct a path while retaining the exact span of every segment.
    ///
    /// # Errors
    ///
    /// Returns [`AnnotationError`] when the path is empty or contains an empty segment.
    pub fn from_idents(segments: Vec<SyntaxIdent>, span: Span) -> Result<Self, AnnotationError> {
        if segments.is_empty()
            || segments
                .iter()
                .any(|segment| segment.text.trim().is_empty())
        {
            return Err(AnnotationError::new(
                "custom annotation namespace must contain non-empty segments",
                span,
            ));
        }
        Ok(Self {
            segment_spans: segments.iter().map(|segment| segment.span).collect(),
            segments: segments.into_iter().map(|segment| segment.text).collect(),
            span,
        })
    }

    #[must_use]
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    #[must_use]
    pub fn span(&self) -> Span {
        self.span
    }

    #[must_use]
    pub fn segment_spans(&self) -> &[Span] {
        &self.segment_spans
    }

    #[must_use]
    pub fn has_namespace(&self) -> bool {
        self.segments.len() > 1
    }

    /// Return the final path segment.
    ///
    /// # Panics
    ///
    /// This panics only if the private non-empty path invariant is violated.
    #[must_use]
    pub fn name(&self) -> &str {
        self.segments.last().expect("symbol paths are non-empty")
    }

    /// Adapt the loss-aware path to the frozen two-field Kernel name shape.
    #[must_use]
    pub fn legacy_parts(&self) -> (Option<String>, String) {
        let namespace =
            (self.segments.len() > 1).then(|| self.segments[..self.segments.len() - 1].join("."));
        (namespace, self.name().to_owned())
    }
}

impl PartialEq for SymbolPath {
    fn eq(&self, other: &Self) -> bool {
        self.segments == other.segments
    }
}

impl Eq for SymbolPath {}

impl PartialOrd for SymbolPath {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SymbolPath {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.segments.cmp(&other.segments)
    }
}

impl fmt::Display for SymbolPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.segments.join("."))
    }
}

/// One typed custom-annotation argument.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AnnotationValue {
    String(String),
    Integer(i64),
    Boolean(bool),
    Symbol(SymbolPath),
}

/// Typed metadata attached to a declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Annotation {
    Requirement {
        id: String,
        text: Option<String>,
        span: Span,
    },
    Undecided {
        reason: String,
        span: Span,
    },
    Kind {
        id: String,
        text: Option<String>,
        span: Span,
    },
    Custom {
        namespace: SymbolPath,
        arguments: Vec<AnnotationValue>,
        span: Span,
    },
}

impl Annotation {
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Requirement { span, .. }
            | Self::Undecided { span, .. }
            | Self::Kind { span, .. }
            | Self::Custom { span, .. } => *span,
        }
    }

    /// Adapt the legacy declaration string slot without changing its public projection.
    #[must_use]
    pub fn from_legacy(id: impl Into<String>, text: Option<String>, span: Span) -> Self {
        let id = id.into();
        if id.eq_ignore_ascii_case("undecided") {
            return Self::Undecided {
                reason: text.unwrap_or_default(),
                span,
            };
        }
        Self::Requirement { id, text, span }
    }

    /// Adapt the legacy spec-level badge, which is descriptive kind metadata.
    #[must_use]
    pub fn from_legacy_kind(id: impl Into<String>, text: Option<String>, span: Span) -> Self {
        Self::Kind {
            id: id.into(),
            text,
            span,
        }
    }
}

/// A normalized requirement relation derived from one or more annotations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequirementLink {
    pub id: String,
    pub text: Option<String>,
    pub span: Span,
}

/// Validation failure for typed annotation data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationError {
    pub message: String,
    pub span: Span,
}

impl AnnotationError {
    fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }
}

impl fmt::Display for AnnotationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AnnotationError {}

/// Source-ordered annotation carrier with order-independent semantic queries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Annotations(Vec<Annotation>);

impl Annotations {
    #[must_use]
    pub fn new(items: Vec<Annotation>) -> Self {
        Self(items)
    }

    pub fn push(&mut self, annotation: Annotation) {
        self.0.push(annotation);
    }

    pub fn extend(&mut self, annotations: impl IntoIterator<Item = Annotation>) {
        self.0.extend(annotations);
    }

    #[must_use]
    pub fn source_order(&self) -> &[Annotation] {
        &self.0
    }

    /// Validate built-in arity/type constraints and conflicting requirement text.
    ///
    /// Identical requirement relations are accepted and normalized away by
    /// [`Self::requirements`].
    ///
    /// # Errors
    ///
    /// Returns [`AnnotationError`] for empty required values or one requirement ID
    /// carrying conflicting text.
    pub fn validate(&self) -> Result<(), AnnotationError> {
        let mut requirements = BTreeMap::<String, (Option<String>, Span)>::new();
        for annotation in &self.0 {
            match annotation {
                Annotation::Requirement { id, text, span } => {
                    if id.trim().is_empty() {
                        return Err(AnnotationError::new(
                            "requirement annotation ID must not be empty",
                            *span,
                        ));
                    }
                    if id.eq_ignore_ascii_case("undecided") {
                        return Err(AnnotationError::new(
                            "'undecided' is reserved and cannot be used as a requirement ID",
                            *span,
                        ));
                    }
                    if let Some((existing, _)) = requirements.get(id)
                        && existing != text
                    {
                        return Err(AnnotationError::new(
                            format!("requirement annotation '{id}' has conflicting text"),
                            *span,
                        ));
                    }
                    requirements
                        .entry(id.clone())
                        .or_insert_with(|| (text.clone(), *span));
                }
                Annotation::Undecided { reason, span } => {
                    if reason.trim().is_empty() {
                        return Err(AnnotationError::new(
                            "undecided annotation reason must not be empty",
                            *span,
                        ));
                    }
                }
                Annotation::Kind { id, span, .. } => {
                    if id.trim().is_empty() {
                        return Err(AnnotationError::new(
                            "kind annotation ID must not be empty",
                            *span,
                        ));
                    }
                }
                Annotation::Custom {
                    namespace, span, ..
                } => {
                    if namespace
                        .segments()
                        .iter()
                        .any(|segment| segment.trim().is_empty())
                    {
                        return Err(AnnotationError::new(
                            "custom annotation namespace must contain non-empty segments",
                            *span,
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Return a deterministic, de-duplicated requirement relation set.
    ///
    /// # Errors
    ///
    /// Returns [`AnnotationError`] when validation fails.
    pub fn requirements(&self) -> Result<Vec<RequirementLink>, AnnotationError> {
        self.validate()?;
        let mut requirements = BTreeMap::<(String, Option<String>), Span>::new();
        for annotation in &self.0 {
            if let Annotation::Requirement { id, text, span } = annotation {
                requirements
                    .entry((id.clone(), text.clone()))
                    .or_insert(*span);
            }
        }
        Ok(requirements
            .into_iter()
            .map(|((id, text), span)| RequirementLink { id, text, span })
            .collect())
    }

    #[must_use]
    pub fn undecided(&self) -> Vec<(&str, Span)> {
        let mut values = self
            .0
            .iter()
            .filter_map(|annotation| match annotation {
                Annotation::Undecided { reason, span } => Some((reason.as_str(), *span)),
                _ => None,
            })
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.0.cmp(right.0));
        values.dedup();
        values
    }
}

/// Target-keyed sidecar used across dialect lowering without adding one field per dialect.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AnnotationRegistry {
    bindings: BTreeMap<String, Annotations>,
}

impl AnnotationRegistry {
    pub fn bind(&mut self, target: impl Into<String>, annotation: Annotation) {
        self.bindings
            .entry(target.into())
            .or_default()
            .push(annotation);
    }

    pub fn bind_legacy(
        &mut self,
        target: impl Into<String>,
        id: impl Into<String>,
        text: Option<String>,
        span: Span,
    ) {
        self.bind(target, Annotation::from_legacy(id, text, span));
    }

    pub fn bind_legacy_kind(
        &mut self,
        target: impl Into<String>,
        id: impl Into<String>,
        text: Option<String>,
        span: Span,
    ) {
        self.bind(target, Annotation::from_legacy_kind(id, text, span));
    }

    pub fn extend(&mut self, target: impl Into<String>, annotations: Annotations) {
        self.bindings
            .entry(target.into())
            .or_default()
            .extend(annotations.0);
    }

    #[must_use]
    pub fn annotations_for(&self, target: &str) -> &Annotations {
        static EMPTY: std::sync::LazyLock<Annotations> =
            std::sync::LazyLock::new(Annotations::default);
        self.bindings.get(target).unwrap_or(&EMPTY)
    }

    pub fn targets(&self) -> impl Iterator<Item = (&str, &Annotations)> {
        self.bindings
            .iter()
            .map(|(target, annotations)| (target.as_str(), annotations))
    }

    /// Validate every target independently.
    ///
    /// # Errors
    ///
    /// Returns the first deterministic validation failure.
    pub fn validate(&self) -> Result<(), AnnotationError> {
        self.bindings.values().try_for_each(Annotations::validate)
    }
}

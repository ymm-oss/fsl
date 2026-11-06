// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Declarations that may be written only once and that lowering reads by name
//! (#1686): a `verify` bound, a `kpi` and a `concept`. Lowering keeps one copy of
//! each, so a second copy would be dropped without a word. A `spec` with an `entity`
//! or a `number` refuses a repeated bound while it reads them, in the same words. A
//! `spec` with neither does not read its bounds, so nothing is dropped there.

use std::collections::BTreeSet;

use fsl_syntax::{BusinessItem, RequirementsItem, Span, SpecItem, SurfaceBusiness, VerifyItem};

use super::{SurfaceRequirements, core_error};
use crate::{CoreError, OriginChain, OriginId, OriginSite};

/// The origin id prefix of a refusal of a repeated declaration.
const REPEATED_DECLARATION: &str = "requirements:repeated-declaration:";

/// Whether `error` refuses a repeated declaration.
pub(crate) fn is_repeated_declaration(error: &CoreError) -> bool {
    error
        .origin
        .as_ref()
        .is_some_and(|origin| origin.id.0.starts_with(REPEATED_DECLARATION))
}

fn refusal(what: &str, name: &str, at: Span) -> CoreError {
    let mut error = core_error(format!("duplicate {what} '{name}'"), at);
    error.origin = Some(Box::new(OriginChain {
        id: OriginId(format!("{REPEATED_DECLARATION}{}", at.start.offset)),
        dialect: "requirements".to_owned(),
        primary: Some(OriginSite {
            source_file: None,
            span: Some(at),
            dialect: "requirements".to_owned(),
            declaration_path: Vec::new(),
        }),
        secondary: Vec::new(),
        lowering_steps: Vec::new(),
        generated: false,
    }));
    error
}

/// The kinds of declaration seen so far, by name.
#[derive(Default)]
struct Seen<'a>(BTreeSet<(&'static str, &'a str)>);

impl<'a> Seen<'a> {
    fn declare(&mut self, what: &'static str, name: &'a str, at: Span) -> Result<(), CoreError> {
        if self.0.insert((what, name)) {
            Ok(())
        } else {
            Err(refusal(what, name, at))
        }
    }

    fn bounds(&mut self, items: &'a [VerifyItem]) -> Result<(), CoreError> {
        for item in items {
            match item {
                VerifyItem::Instances(name, _, at) => {
                    self.declare("instances bound for", name, *at)?;
                }
                VerifyItem::Values(name, _, _, at) => {
                    self.declare("values bound for", name, *at)?;
                }
            }
        }
        Ok(())
    }

    fn kpi(&mut self, item: &'a BusinessItem) -> Result<(), CoreError> {
        match item {
            BusinessItem::Kpi { name, span, .. } => self.declare("kpi", name, *span),
            _ => Ok(()),
        }
    }
}

/// Refuse a repeated `concept` of a requirements document, then the first repeated
/// bound or `kpi` in source order. A repeated `concept` is reported even when a repeated
/// `kpi` is written above it.
pub(crate) fn reject_repeated_requirements_declarations(
    requirements: &SurfaceRequirements,
) -> Result<(), CoreError> {
    let mut seen = Seen::default();
    for concept in requirements.next.iter().flat_map(|next| &next.concepts) {
        seen.declare("concept", &concept.name, concept.span)?;
    }
    for item in &requirements.items {
        match item {
            RequirementsItem::Common(SpecItem::VerifyBounds { items, .. }) => seen.bounds(items)?,
            RequirementsItem::Kpi(item) => seen.kpi(item)?,
            _ => {}
        }
    }
    Ok(())
}

/// Refuse the first repeated bound or `kpi` of a business document.
pub(crate) fn reject_repeated_business_declarations(
    business: &SurfaceBusiness,
) -> Result<(), CoreError> {
    let mut seen = Seen::default();
    for item in &business.items {
        if let BusinessItem::VerifyBounds { items, .. } = item {
            seen.bounds(items)?;
        } else {
            seen.kpi(item)?;
        }
    }
    Ok(())
}

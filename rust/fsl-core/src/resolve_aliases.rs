// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Name resolution of a requirements document's glossary aliases.
//!
//! The parser has already replaced every alias by its canonical name (the
//! first step of name resolution, `fsl_syntax::Glossary`), so lowering sees
//! canonical names only. What is left needs the lowered declarations: an
//! alias spelled like a name the spec declares is `alias_shadows_name`, and
//! the table of aliases is published on the Kernel's [`crate::NameMap`] for the
//! surfaces that display names to a reader. No Kernel name is ever an alias.

use std::collections::BTreeSet;

use fsl_syntax::{
    ALIAS_SHADOWS_NAME, Param, RequirementsItem, SpecItem, SurfaceRequirements, SurfaceSpec,
};

use crate::{CoreError, KernelSpec, naming};

/// Lower the requirements dialect into the shared kernel.
///
/// # Errors
///
/// Returns [`CoreError`] for missing entity/number bounds, malformed process
/// declarations, or a glossary alias spelled like a declared name.
pub fn lower_requirements(requirements: SurfaceRequirements) -> Result<KernelSpec, CoreError> {
    let glossary = requirements.items.iter().find_map(|item| match item {
        RequirementsItem::Glossary(glossary) => Some(glossary.clone()),
        _ => None,
    });
    let mut kernel = crate::dialect::lower_resolved_requirements(requirements)?;
    if let Some(glossary) = glossary {
        reject_shadowed_names(&glossary, kernel.syntax())?;
        let mut table = naming::Glossary::default();
        for entry in glossary.entries {
            table.insert(entry.canonical.written(), entry.alias);
        }
        kernel.names.set_glossary(table);
    }
    Ok(kernel)
}

/// An alias that is also a declared name of the lowered spec would read as
/// that name in one place and as its canonical name in another. Resolution
/// replaced every alias the source wrote, so a declared name spelled like an
/// alias is one the source did not write as the alias: a generated name, or
/// one introduced by lowering.
fn reject_shadowed_names(
    glossary: &fsl_syntax::Glossary,
    spec: &SurfaceSpec,
) -> Result<(), CoreError> {
    let declared = declared_names(spec);
    for entry in &glossary.entries {
        if declared.contains(entry.alias.as_str()) {
            return Err(CoreError {
                message: format!(
                    "alias '{}' of '{}' is spelled like the declared name '{}'",
                    entry.alias,
                    entry.canonical.written(),
                    entry.alias
                ),
                line: entry.alias_span.start.line,
                column: entry.alias_span.start.column,
                origin: None,
                kind: crate::CoreErrorKind::Semantics,
                diagnostic_code: Some(ALIAS_SHADOWS_NAME),
            });
        }
    }
    Ok(())
}

/// Every name the lowered spec declares: types and their members or fields,
/// constants, definitions and their parameters, state variables, actions and
/// their parameters, and properties.
fn declared_names(spec: &SurfaceSpec) -> BTreeSet<&str> {
    let mut names = BTreeSet::from([spec.name.as_str()]);
    for item in &spec.items {
        match item {
            SpecItem::Const { name, .. }
            | SpecItem::Type { name, .. }
            | SpecItem::Entity(name, _)
            | SpecItem::Number(name, _)
            | SpecItem::Invariant { name, .. }
            | SpecItem::Trans { name, .. }
            | SpecItem::Reachable { name, .. }
            | SpecItem::Until { name, .. }
            | SpecItem::Unless { name, .. }
            | SpecItem::LeadsTo { name, .. } => {
                names.insert(name);
            }
            SpecItem::Def { name, params, .. } => {
                names.insert(name);
                names.extend(params.iter().map(|(param, _)| param.as_str()));
            }
            SpecItem::Enum { name, members, .. } => {
                names.insert(name);
                names.extend(members.iter().map(String::as_str));
            }
            SpecItem::Struct { name, fields, .. } => {
                names.insert(name);
                names.extend(fields.iter().map(|(field, _)| field.as_str()));
            }
            SpecItem::State(fields) => {
                names.extend(fields.iter().map(|field| field.name.as_str()));
            }
            SpecItem::Action { name, params, .. } => {
                names.insert(name);
                names.extend(params.iter().map(|param| match param {
                    Param::Typed(name, _) | Param::Range(name, _, _) => name.as_str(),
                }));
            }
            SpecItem::Init { .. } | SpecItem::Terminal { .. } | SpecItem::VerifyBounds { .. } => {}
        }
    }
    names
}

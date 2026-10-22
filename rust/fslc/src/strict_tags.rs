// SPDX-License-Identifier: Apache-2.0

//! `--strict-tags` warnings: requirement tags a spec declares but does not
//! reference, and the reverse.

use std::path::Path;

use fsl_core::KernelModel;
use serde_json::{Value, json};

#[allow(clippy::too_many_lines)]
pub(crate) fn strict_tag_warnings(
    model: &KernelModel,
    source_path: &Path,
    requirements: Option<&Path>,
) -> Result<Vec<Value>, String> {
    let source = std::fs::read_to_string(source_path).map_err(|error| error.to_string())?;
    strict_tag_warnings_from_source(model, &source, requirements)
}

/// Collect every requirement ID that some declaration or trace case references.
///
/// The two halves are not the same shape. The first enumerates the `KernelModel` collections
/// that carry annotations, so a newly added annotation-bearing collection has to be added here
/// as well. The second reads a trace case's whole annotation set rather than its `id`, because
/// lowering synthesises the case's own `Requirement{id,text}` into that set before extending it
/// with the surface annotations (`fsl_core::dialect::trace_case_annotations`) -- so one read
/// yields both the owned and the linked IDs, and neither can be forgotten separately.
pub(crate) fn referenced_requirement_ids(
    model: &KernelModel,
    source: &str,
) -> std::collections::BTreeSet<String> {
    let mut referenced = std::collections::BTreeSet::new();
    for annotations in model
        .actions
        .iter()
        .map(|item| &item.annotations)
        .chain(model.invariants.iter().map(|item| &item.annotations))
        .chain(model.transitions.iter().map(|item| &item.annotations))
        .chain(model.leadstos.iter().map(|item| &item.annotations))
        .chain(model.reachables.iter().map(|item| &item.annotations))
        .chain(std::iter::once(&model.init_annotations))
    {
        referenced.extend(
            annotations
                .requirements()
                .expect("checked model annotations are valid")
                .into_iter()
                .map(|requirement| requirement.id),
        );
    }
    // Ungated: `--strict-tags` runs only on an ok / verified / proved result,
    // which already passed the gate. Under `--instances` / `--values` that was
    // the scoped gate, which may skip a case the full gate would reject, so
    // the full gate is not re-run here.
    if let Ok(Some(contract)) =
        fsl_core::requirements_trace_contract_with_options(source, crate::process::parse_options())
    {
        for case in contract
            .unchecked_acceptance()
            .iter()
            .chain(contract.unchecked_forbidden())
        {
            referenced.extend(
                case.annotations
                    .requirements()
                    .expect("checked trace case annotations are valid")
                    .into_iter()
                    .map(|requirement| requirement.id),
            );
        }
    }
    referenced
}

pub(crate) fn strict_tag_warnings_from_source(
    model: &KernelModel,
    source: &str,
    requirements: Option<&Path>,
) -> Result<Vec<Value>, String> {
    let mut warnings = Vec::new();
    // The hint names the canonical link form from `docs/design/DESIGN-id-policy.md`.
    // It used to propose the `"REQ-1: original requirement"` string slot, which
    // that policy classifies as non-canonical migration input and `fslc lint`
    // reports as `legacy_string_metadata` — so the repair this diagnostic asked
    // for was the one the next gate rejects.
    let hint = "tag the declaration with a typed annotation such as @requirement(\"REQ-SCOPE-001\", \"original requirement\"); use a MODEL-/ASSUME-prefixed id for modeling intent, or process `covers` in the business/requirements dialects. The legacy \"REQ-1: text\" string slot is non-canonical (fslc lint reports it as legacy_string_metadata)";
    for (element, name, span, annotations) in model
        .actions
        .iter()
        .map(|item| ("action", item.name.as_str(), &item.span, &item.annotations))
        .chain(model.invariants.iter().map(|item| {
            (
                "invariant",
                item.name.as_str(),
                &item.span,
                &item.annotations,
            )
        }))
        .chain(
            model
                .transitions
                .iter()
                .map(|item| ("trans", item.name.as_str(), &item.span, &item.annotations)),
        )
        .chain(
            model
                .leadstos
                .iter()
                .map(|item| ("leadsTo", item.name.as_str(), &item.span, &item.annotations)),
        )
        .chain(model.reachables.iter().map(|item| {
            (
                "reachable",
                item.name.as_str(),
                &item.span,
                &item.annotations,
            )
        }))
    {
        if annotations.source_order().is_empty() && !name.starts_with('_') {
            warnings.push(json!({
                "kind": "untagged",
                "element": element,
                "name": if element == "action" {
                    model.action_display_name(name)
                } else {
                    model.names().public_identifier(name).into_string()
                },
                "loc": span.python_loc(),
                "hint": hint,
            }));
        }
    }

    let referenced = referenced_requirement_ids(model, source);
    // `Declared` = requirement-block IDs auto-collected from the requirements dialect
    // (docs/design/DESIGN-strict-tags.md section 2 calls this "essential" for catching an
    // empty block) union `--requirements` file IDs. Both halves run regardless of
    // whether `--requirements` was passed; only the file half is optional. The file
    // half keeps reporting in its original file-line order, so an established
    // `--requirements` file's warning order is undisturbed; IDs that reach
    // `Declared` only through auto-collection are reported afterward, in a fixed
    // (sorted) order.
    let declared_from_blocks =
        fsl_core::requirements_declared_ids_with_options(source, crate::process::parse_options())
            .unwrap_or_default();
    let mut declared_from_file: Vec<String> = Vec::new();
    if let Some(path) = requirements {
        let source = std::fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("file not found: {}", path.display())
            } else {
                error.to_string()
            }
        })?;
        declared_from_file.extend(
            source
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned),
        );
    }
    for requirement in &declared_from_file {
        if !referenced.contains(requirement.as_str()) {
            warnings.push(json!({
                "kind": "unreferenced_requirement",
                "element": "requirement",
                "name": requirement,
                "loc": Value::Null,
                "hint": "no declaration tag, acceptance, or forbidden block references this requirement ID",
            }));
        }
    }
    for requirement in &declared_from_blocks {
        if declared_from_file.iter().any(|line| line == requirement) {
            continue;
        }
        if !referenced.contains(requirement.as_str()) {
            warnings.push(json!({
                "kind": "unreferenced_requirement",
                "element": "requirement",
                "name": requirement,
                "loc": Value::Null,
                "hint": "no declaration tag, acceptance, or forbidden block references this requirement ID",
            }));
        }
    }
    Ok(warnings)
}

pub(crate) fn add_strict_tag_warnings(
    output: &mut Value,
    model: &KernelModel,
    source_path: &Path,
    strict_tags: bool,
    requirements: Option<&Path>,
) -> Result<(), String> {
    if !strict_tags
        || !matches!(
            output.get("result").and_then(Value::as_str),
            Some("ok" | "verified" | "proved")
        )
    {
        return Ok(());
    }
    let additions = strict_tag_warnings(model, source_path, requirements)?;
    let Some(envelope) = output.as_object_mut() else {
        return Ok(());
    };
    envelope
        .entry("warnings")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("warnings is an array")
        .extend(additions);
    Ok(())
}

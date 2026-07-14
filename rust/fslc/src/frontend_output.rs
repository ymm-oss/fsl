// SPDX-License-Identifier: Apache-2.0

//! Shared frontend diagnostics used by native and browser delivery surfaces.

use serde_json::{Map, Value, json};

/// Render a surface-parser diagnostic using the public check/verify envelope.
#[must_use]
pub fn render_surface_parse_error(
    mut output: Map<String, Value>,
    error: &fsl_syntax::ParseError,
) -> Value {
    output.insert("result".to_owned(), json!("error"));
    output.insert("kind".to_owned(), json!("parse"));
    output.insert("message".to_owned(), json!(error.to_string()));
    output.insert("diagnostic_code".to_owned(), json!(error.code()));
    output.insert("loc".to_owned(), error.span.python_loc());
    if matches!(
        error.code(),
        "FSL-DIALECT-EMPTY" | "FSL-DIALECT-ANNOTATION-TARGET" | "FSL-DIALECT-UNKNOWN"
    ) {
        output.insert(
            "supported_dialects".to_owned(),
            json!(fsl_syntax::DIALECT_KEYWORDS),
        );
    }
    Value::Object(output)
}

/// Render the legacy multi-declaration AI project check result when applicable.
#[must_use]
pub fn ai_project_check_output(
    source: &str,
    source_file: &str,
    mut output: Map<String, Value>,
) -> Option<Value> {
    if !is_ai_project(source) {
        return None;
    }
    let spec = std::path::Path::new(source_file)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("AiProject");
    output.insert("result".to_owned(), json!("ok"));
    output.insert("spec".to_owned(), json!(spec));
    output.insert("dialect".to_owned(), json!("fsl-ai-project.v0"));
    output.insert("warnings".to_owned(), json!([]));
    output.insert(
        "ai_analysis_result".to_owned(),
        json!("ai_project_analyzed"),
    );
    Some(Value::Object(output))
}

/// Return whether source uses the legacy multi-declaration AI project dialect.
#[must_use]
pub fn is_ai_project(source: &str) -> bool {
    const PROJECT_BLOCKS: &[&str] = &[
        "ai_action",
        "ai_component",
        "ai_contract",
        "ai_migration",
        "authority",
        "dataset",
        "evaluator",
        "failure_mode",
        "observed_property",
        "retriever",
        "statistical_property",
        "trust_boundary",
    ];
    let has_project_property = source.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("statistical_property ")
            || line.starts_with("ai_migration ")
            || line.starts_with("observed_property ")
    });
    has_project_property
        && fsl_syntax::declaration_keyword(source)
            .is_ok_and(|keyword| PROJECT_BLOCKS.contains(&keyword.as_str()))
}

/// Return deterministic warnings for omitted domain/requirements initial values.
#[must_use]
pub fn implicit_initial_value_warnings(source: &str, source_file: &str) -> Vec<Value> {
    let Ok(document) = fsl_syntax::parse_surface_document(source) else {
        return Vec::new();
    };
    match document {
        fsl_syntax::SurfaceDocument::Domain(domain) => {
            domain_implicit_warnings(source_file, &domain)
        }
        fsl_syntax::SurfaceDocument::Requirements(requirements) => {
            requirements_implicit_warnings(source_file, &requirements)
        }
        _ => Vec::new(),
    }
}

fn domain_implicit_warnings(path: &str, domain: &fsl_syntax::DomainSpec) -> Vec<Value> {
    let declared = domain
        .types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect::<std::collections::BTreeMap<_, _>>();
    domain
        .aggregates
        .iter()
        .flat_map(|aggregate| {
            aggregate.state.iter().filter_map(|field| {
                let type_name = field.type_name.render_source();
                let selected = omitted_domain_value(&type_name, field.default.as_ref(), &declared)?;
                Some(implicit_initial_value_warning(
                    path,
                    &format!("{}.{}", aggregate.name, field.name.text),
                    field.span,
                    field.type_name.span.end.offset,
                    &selected.0,
                    &selected.1,
                ))
            })
        })
        .collect()
}

fn omitted_domain_value(
    type_name: &str,
    default: Option<&fsl_syntax::SyntaxExpr>,
    declared: &std::collections::BTreeMap<&str, &fsl_syntax::DomainType>,
) -> Option<(String, String)> {
    if default.is_some() {
        return None;
    }
    if type_name == "Bool" {
        return Some(("false".to_owned(), "Bool defaults to false".to_owned()));
    }
    if type_name == "Int" {
        return None;
    }
    if let Some(definition) = declared.get(type_name) {
        return match definition.kind.as_str() {
            "enum" => definition.members.first().map(|member| {
                (
                    member.clone(),
                    format!(
                        "the first declared member of enum '{}' is selected",
                        definition.name
                    ),
                )
            }),
            "range" => definition.lo.as_ref().map(|lo| {
                (
                    lo.render_source(),
                    format!("the lower bound of range '{}' is selected", definition.name),
                )
            }),
            _ => None,
        };
    }
    (!type_name.contains('<')).then(|| {
        (
            "0".to_owned(),
            format!("external placeholder type '{type_name}' defaults to 0"),
        )
    })
}

fn requirements_implicit_warnings(
    path: &str,
    requirements: &fsl_syntax::SurfaceRequirements,
) -> Vec<Value> {
    let mut lower_bounds = std::collections::BTreeMap::new();
    for item in &requirements.items {
        if let fsl_syntax::RequirementsItem::Common(fsl_syntax::SpecItem::VerifyBounds {
            items,
            ..
        }) = item
        {
            for bound in items {
                if let fsl_syntax::VerifyItem::Values(name, lo, _, _) = bound {
                    lower_bounds.insert(name.as_str(), crate::expr_text(lo));
                }
            }
        }
    }
    requirements
        .items
        .iter()
        .filter_map(|item| match item {
            fsl_syntax::RequirementsItem::Process(fsl_syntax::BusinessItem::Process {
                name,
                fields: Some(fields),
                ..
            }) => Some((name, fields)),
            _ => None,
        })
        .flat_map(|(process, fields)| {
            let lower_bounds = &lower_bounds;
            fields.fields.iter().filter_map(move |field| {
                if field.initial.is_some() {
                    return None;
                }
                let selected = lower_bounds.get(field.type_name.name.as_str())?;
                Some(implicit_initial_value_warning(
                    path,
                    &format!("{process}.{}", field.name),
                    field.span,
                    field.type_span.end.offset,
                    selected,
                    &format!(
                        "the lower bound of number '{}' is selected",
                        field.type_name.name
                    ),
                ))
            })
        })
        .collect()
}

fn implicit_initial_value_warning(
    path: &str,
    field: &str,
    span: fsl_syntax::Span,
    insertion_offset: usize,
    selected: &str,
    reason: &str,
) -> Value {
    let replacement = format!(" = {selected}");
    json!({
        "kind": "implicit_initial_value",
        "code": "implicit_initial_value",
        "severity": "warning",
        "edition_severity": {"current": "warning", "next": "error"},
        "message": format!("field '{field}' implicitly selects {selected}; add an explicit initializer"),
        "field": field,
        "selected_value": selected,
        "reason": reason,
        "loc": {
            "file": path,
            "line": span.start.line,
            "column": span.start.column,
            "end_line": span.end.line,
            "end_column": span.end.column,
        },
        "canonical_replacement": replacement,
        "suggestion": {
            "kind": "insert",
            "replacement": replacement,
            "span": {"start": insertion_offset, "end": insertion_offset},
            "machine_applicable": true,
        },
    })
}

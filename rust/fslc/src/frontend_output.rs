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

/// The `ai_project` name project commands report -- the source's file stem,
/// mirroring the frozen reference's `parse_ai_project(src, name=Path(path).stem)`.
#[must_use]
pub fn ai_project_name(source_file: &str) -> &str {
    std::path::Path::new(source_file)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("AiProject")
}

/// Parse an fsl-ai project the way the check stage must: a source whose
/// `require` clauses no evidence command can execute is a spec error, not an
/// analyzed project (issue #542).
///
/// The clause classification is the parser's own
/// (`AiProject::unparsed_clauses`), never a second grammar -- reimplementing
/// it here is exactly how `check` and `eval` drifted apart.
///
/// # Errors
///
/// Returns the parse or unexecutable-clause message, carrying the position of
/// the first offending clause when there is one, for `kind: "parse"`.
pub fn parse_checked_ai_project(
    source: &str,
    name: &str,
) -> Result<fsl_syntax::AiProject, AiProjectParseError> {
    let project =
        fsl_syntax::parse_ai_project(source, name).map_err(|message| AiProjectParseError {
            message,
            position: None,
        })?;
    let unparsed = project.unparsed_clauses();
    if unparsed.is_empty() {
        return Ok(project);
    }
    let detail = unparsed
        .iter()
        .map(|clause| {
            format!(
                "{} '{}' (slice '{}'): require {}",
                clause.declaration_kind, clause.declaration, clause.slice, clause.source
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    // `unparsed_clauses` walks declarations in source order, so the first entry
    // is the first offending clause; report its own line, not its block's.
    let first = &unparsed[0];
    Err(AiProjectParseError {
        message: format!(
            "require clause matches no known fsl-ai evidence clause grammar \
             (min_samples, ci_lower, ci_upper, a point estimate, observed, or drift): {detail}"
        ),
        position: Some((first.line, first.column)),
    })
}

/// An fsl-ai project check failure, carrying the offending clause's position
/// when the parser resolved one. `docs/DESIGN-v1.md` §7.2 guarantees every
/// `parse` error carries a `loc` (#562).
pub struct AiProjectParseError {
    pub message: String,
    pub position: Option<(u32, u32)>,
}

/// Render the multi-declaration AI project check result when applicable,
/// paired with its process exit code.
#[must_use]
pub fn ai_project_check_output(
    source: &str,
    source_file: &str,
    mut output: Map<String, Value>,
) -> Option<(Value, i32)> {
    if !is_ai_project(source) {
        return None;
    }
    let spec = ai_project_name(source_file);
    if let Err(error) = parse_checked_ai_project(source, spec) {
        output.insert("result".to_owned(), json!("error"));
        output.insert("kind".to_owned(), json!("parse"));
        output.insert("message".to_owned(), json!(error.message));
        if let Some((line, column)) = error.position {
            output.insert("loc".to_owned(), json!({"line": line, "column": column}));
        }
        return Some((Value::Object(output), 2));
    }
    output.insert("result".to_owned(), json!("ok"));
    output.insert("spec".to_owned(), json!(spec));
    output.insert("dialect".to_owned(), json!("fsl-ai-project.v0"));
    output.insert("warnings".to_owned(), json!([]));
    output.insert(
        "ai_analysis_result".to_owned(),
        json!("ai_project_analyzed"),
    );
    Some((Value::Object(output), 0))
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
    domain
        .aggregates
        .iter()
        .flat_map(|aggregate| {
            aggregate.state.iter().filter_map(|field| {
                let selected = omitted_domain_value(domain, field)?;
                Some(implicit_initial_value_warning(
                    path,
                    &format!("{}.{}", aggregate.name, field.name.text),
                    field.span,
                    field.type_name.span.end.offset,
                    &selected.value,
                    &selected.reason,
                    selected.insertable,
                ))
            })
        })
        .collect()
}

/// A field's implicit default, as chosen by [`fsl_core::domain_type_default`]
/// -- the same renderer dispatch `domain_kernel_source` uses -- plus whether
/// an explicit `= value` initializer can be safely offered as a
/// machine-applicable edit for this field's shape.
struct SelectedDefault {
    value: String,
    reason: String,
    /// `false` in two distinct cases, both explained in `reason`:
    ///
    /// - A top-level `Map<K, V>` field: an explicit whole-`Map` default is
    ///   unconditionally rejected ("whole-Map domain defaults are not
    ///   supported"), so no single-expression insertion exists at all.
    /// - A brace-literal value (`Set { ... }`, a `value_object` struct
    ///   literal): the syntax itself is valid and `fslc check` accepts it,
    ///   but `fslc fmt`/`migrate --write`'s reformat-and-reparse step cannot
    ///   currently round-trip an `Ident { ... }` literal after a domain
    ///   field declaration (issue #770). Offering a machine-applicable
    ///   insertion here would make `migrate --write` corrupt the file.
    ///
    /// `check` still reports the renderer's implicit choice in both cases;
    /// `migrate --edition next` cannot yet demand an initializer it cannot
    /// safely insert (see `docs/LANGUAGE.md`).
    insertable: bool,
}

/// The value [`omitted_domain_value`]'s caller must warn about for `field`,
/// or `None` when `field` already has an explicit default (nothing implicit
/// to report) or the renderer itself cannot choose a default (an unknown
/// type, an empty enum, etc. -- the full `check` pipeline reports that
/// failure separately; a syntax-only warning pass degrades to silence rather
/// than duplicating that diagnosis).
///
/// The selected *value* always comes from [`fsl_core::domain_type_default`],
/// the renderer's own total dispatch (issue #731) -- this function never
/// re-derives what a type's default value is, only classifies *why* for the
/// warning's `reason` text and whether that value is safe to offer as a
/// machine-applicable `= value` insertion.
fn omitted_domain_value(
    domain: &fsl_syntax::DomainSpec,
    field: &fsl_syntax::DomainField,
) -> Option<SelectedDefault> {
    if field.default.is_some() {
        return None;
    }
    let type_name = field.type_name.render_source();
    if type_name == "Int" {
        return None;
    }
    if let Some((_key, value_type)) = fsl_core::map_key_value(&field.type_name) {
        // A top-level Map<K, V> has no defaultable value of its own: the
        // renderer's only supported default is the dense per-key
        // `forall k: K { field[k] = <V's default> }` init
        // `domain_kernel_source`'s state-field loop builds directly, using
        // this exact same `domain_type_default` call on `V`. Report that
        // per-key value for informational parity with the renderer, but
        // without a suggestion: no `= value` syntax for a whole Map field is
        // ever accepted, so no insertion could be machine-applicable.
        let value_default = fsl_core::domain_type_default(domain, value_type, field.span).ok()?;
        return Some(SelectedDefault {
            reason: format!(
                "'{type_name}' has no whole-field default; each key implicitly defaults to '{value_default}' through the per-key forall init the renderer builds"
            ),
            value: value_default,
            insertable: false,
        });
    }
    let value = match domain.types.iter().find(|ty| ty.name == type_name) {
        // `Context::default_for_type` selects this same first-declared
        // member, but renders it as `domain_kernel_source`'s mangled
        // `Enum_Member` kernel identifier -- correct for the generated
        // kernel text, not parseable as a domain-source field initializer.
        // Read the member domain source itself would accept from the same
        // `DomainSpec.types` the renderer's dispatch reads, rather than
        // asking that dispatch for a kernel-scoped answer to a
        // domain-source question.
        Some(ty) if ty.kind == "enum" => ty.members.first()?.clone(),
        _ => fsl_core::domain_type_default(domain, &field.type_name, field.span).ok()?,
    };
    let reason = domain_default_reason(domain, &field.type_name, &type_name, &value);
    // A brace-literal value (`Set {}`, a value_object struct literal) is
    // valid FSL that `fslc check` accepts, but issue #770 tracks that
    // `fslc fmt` cannot yet reparse its own reformatting of an
    // `Ident { ... }` literal placed after a domain field declaration.
    // Withhold the insertion rather than emit an edit `migrate --write`
    // would use to produce an unparseable file.
    let insertable = !value.contains('{');
    Some(SelectedDefault {
        value,
        reason,
        insertable,
    })
}

/// Explanatory text for why [`fsl_core::domain_type_default`] selected
/// `value` for `type_name` -- classification only, kept separate from value
/// selection so a wording gap here can never make the *reported* value
/// disagree with the renderer's.
fn domain_default_reason(
    domain: &fsl_syntax::DomainSpec,
    type_name: &fsl_syntax::SyntaxTypeExpr,
    type_name_text: &str,
    value: &str,
) -> String {
    match &type_name.kind {
        fsl_syntax::SyntaxTypeExprKind::Apply { constructor, .. } => {
            match constructor.text.as_str() {
                "Option" => format!("'{type_name_text}' defaults to none"),
                "Set" => format!("'{type_name_text}' defaults to an empty set"),
                _ => format!("'{type_name_text}' defaults to '{value}'"),
            }
        }
        fsl_syntax::SyntaxTypeExprKind::Name(_) if type_name_text == "Bool" => {
            "Bool defaults to false".to_owned()
        }
        fsl_syntax::SyntaxTypeExprKind::Name(_) => {
            match domain.types.iter().find(|ty| ty.name == type_name_text) {
                Some(ty) if ty.kind == "enum" => {
                    format!(
                        "the first declared member of enum '{}' is selected",
                        ty.name
                    )
                }
                Some(ty) if ty.kind == "range" => {
                    format!("the lower bound of range '{}' is selected", ty.name)
                }
                Some(ty) if ty.kind == "value_object" => format!(
                    "the default value_object literal for '{}' is selected",
                    ty.name
                ),
                _ => format!("external placeholder type '{type_name_text}' defaults to '{value}'"),
            }
        }
    }
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
                    true,
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
    insertable: bool,
) -> Value {
    let next_severity = if insertable { "error" } else { "warning" };
    let message = if insertable {
        format!("field '{field}' implicitly selects {selected}; add an explicit initializer")
    } else {
        format!(
            "field '{field}' implicitly selects {selected}; \
             no machine-applicable initializer edit is offered for this field yet"
        )
    };
    let mut warning = json!({
        "kind": "implicit_initial_value",
        "code": "implicit_initial_value",
        "severity": "warning",
        "edition_severity": {"current": "warning", "next": next_severity},
        "message": message,
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
    });
    if insertable {
        let replacement = format!(" = {selected}");
        let object = warning.as_object_mut().expect("warning is a JSON object");
        object.insert("canonical_replacement".to_owned(), json!(replacement));
        object.insert(
            "suggestion".to_owned(),
            json!({
                "kind": "insert",
                "replacement": replacement,
                "span": {"start": insertion_offset, "end": insertion_offset},
                "machine_applicable": true,
            }),
        );
    }
    warning
}

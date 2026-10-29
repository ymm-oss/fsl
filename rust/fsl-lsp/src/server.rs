// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crossbeam_channel::Sender;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CompletionItem, CompletionItemKind, CompletionOptions,
    CompletionParams, CompletionResponse, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, Location, MarkupContent, MarkupKind, NumberOrString,
    OneOf, Position, PositionEncodingKind, PublishDiagnosticsParams, ReferenceParams, RenameParams,
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, SymbolKind,
    TextDocumentContentChangeEvent, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextEdit, Url, WorkDoneProgressOptions, WorkspaceEdit,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::index::span_range;
use crate::{DocumentIndex, SourceDiagnostic, SymbolRole};

const KEYWORDS: &[&str] = &[
    "spec",
    "compose",
    "use",
    "requirements",
    "business",
    "governance",
    "refinement",
    "domain",
    "dbsystem",
    "ai_component",
    "agent",
    "const",
    "type",
    "enum",
    "struct",
    "entity",
    "number",
    "state",
    "init",
    "action",
    "requires",
    "ensures",
    "invariant",
    "trans",
    "reachable",
    "terminal",
    "until",
    "unless",
    "leadsTo",
    "forall",
    "exists",
    "if",
    "then",
    "else",
    "true",
    "false",
    "none",
];

#[derive(Clone)]
struct OpenDocument {
    text: String,
    version: i32,
}

#[derive(Default)]
struct ServerState {
    documents: HashMap<Url, OpenDocument>,
    roots: Vec<PathBuf>,
    /// Read once when the server starts (`docs/design/parse-entry-inventory.md`).
    parse_options: fsl_syntax::ParseOptions,
}

struct StoreResolver<'a> {
    state: &'a ServerState,
    base: PathBuf,
}

impl fsl_core::FileResolver for StoreResolver<'_> {
    fn read(&self, path: &str) -> Result<String, fsl_core::CoreError> {
        let path = self.base.join(path);
        let uri = Url::from_file_path(&path).ok();
        if let Some(document) = uri.and_then(|uri| self.state.documents.get(&uri)) {
            return Ok(document.text.clone());
        }
        std::fs::read_to_string(&path).map_err(|error| fsl_core::CoreError {
            message: error.to_string(),
            line: 1,
            column: 1,
            origin: None,
            kind: fsl_core::CoreErrorKind::Semantics,
            diagnostic_code: None,
        })
    }
}

/// Run the synchronous stdio LSP transport.
///
/// # Errors
///
/// Returns protocol, serialization, or stdio failures.
pub fn run_stdio() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let (connection, threads) = Connection::stdio();
    let initialize = connection.initialize(serde_json::to_value(server_capabilities())?)?;
    let params: InitializeParams = serde_json::from_value(initialize)?;
    let mut state = ServerState {
        roots: workspace_roots(&params),
        parse_options: fslc_rust::unstable_gate::from_environment(),
        ..ServerState::default()
    };

    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                handle_request(&connection.sender, &state, &request);
            }
            Message::Notification(notification) => {
                handle_notification(&connection.sender, &mut state, notification)?;
            }
            Message::Response(_) => {}
        }
    }
    drop(connection);
    threads.join()?;
    Ok(())
}

#[must_use]
pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(PositionEncodingKind::UTF16),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                save: Some(lsp_types::TextDocumentSyncSaveOptions::Supported(true)),
                ..TextDocumentSyncOptions::default()
            },
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_owned()]),
            ..CompletionOptions::default()
        }),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
                legend: SemanticTokensLegend {
                    token_types: vec![
                        SemanticTokenType::NAMESPACE,
                        SemanticTokenType::TYPE,
                        SemanticTokenType::FUNCTION,
                        SemanticTokenType::VARIABLE,
                        SemanticTokenType::PARAMETER,
                        SemanticTokenType::PROPERTY,
                    ],
                    token_modifiers: vec![SemanticTokenModifier::DECLARATION],
                },
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        ..ServerCapabilities::default()
    }
}

fn handle_notification(
    sender: &Sender<Message>,
    state: &mut ServerState,
    notification: Notification,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    match notification.method.as_str() {
        "textDocument/didOpen" => {
            let params: DidOpenTextDocumentParams = serde_json::from_value(notification.params)?;
            let uri = params.text_document.uri;
            state.documents.insert(
                uri.clone(),
                OpenDocument {
                    text: params.text_document.text,
                    version: params.text_document.version,
                },
            );
            publish_diagnostics(sender, state, &uri)?;
        }
        "textDocument/didChange" => {
            let params: DidChangeTextDocumentParams = serde_json::from_value(notification.params)?;
            let uri = params.text_document.uri;
            if let Some(document) = state.documents.get_mut(&uri) {
                apply_content_changes(&mut document.text, &params.content_changes);
                document.version = params.text_document.version;
            }
            publish_diagnostics(sender, state, &uri)?;
        }
        "textDocument/didSave" => {
            let params: DidSaveTextDocumentParams = serde_json::from_value(notification.params)?;
            if let Some(text) = params.text
                && let Some(document) = state.documents.get_mut(&params.text_document.uri)
            {
                document.text = text;
            }
            publish_diagnostics(sender, state, &params.text_document.uri)?;
        }
        "textDocument/didClose" => {
            let params: DidCloseTextDocumentParams = serde_json::from_value(notification.params)?;
            state.documents.remove(&params.text_document.uri);
            sender.send(
                Notification::new(
                    "textDocument/publishDiagnostics".to_owned(),
                    PublishDiagnosticsParams::new(params.text_document.uri, Vec::new(), None),
                )
                .into(),
            )?;
        }
        _ => {}
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn handle_request(sender: &Sender<Message>, state: &ServerState, request: &Request) {
    let response = match request.method.as_str() {
        "textDocument/documentSymbol" => {
            parse_request::<DocumentSymbolParams>(request).map(|params| {
                let result = index_for_uri(state, &params.text_document.uri).map(|index| {
                    DocumentSymbolResponse::Nested(
                        index.symbols.iter().map(document_symbol).collect(),
                    )
                });
                ok(request, result)
            })
        }
        "textDocument/definition" => parse_request::<GotoDefinitionParams>(request).map(|params| {
            let result = definition_location(
                state,
                &params.text_document_position_params.text_document.uri,
                params.text_document_position_params.position,
            )
            .map(GotoDefinitionResponse::Scalar);
            ok(request, result)
        }),
        "textDocument/hover" => parse_request::<HoverParams>(request).map(|params| {
            let result = hover(
                state,
                &params.text_document_position_params.text_document.uri,
                params.text_document_position_params.position,
            );
            ok(request, result)
        }),
        "textDocument/references" => parse_request::<ReferenceParams>(request).map(|params| {
            let uri = &params.text_document_position.text_document.uri;
            let result = workspace_references(
                state,
                uri,
                params.text_document_position.position,
                params.context.include_declaration,
            );
            ok(request, result)
        }),
        "textDocument/completion" => parse_request::<CompletionParams>(request).map(|params| {
            let uri = &params.text_document_position.text_document.uri;
            let result = index_for_uri(state, uri).map(|index| {
                let source = index.source();
                let completion_index =
                    alias_before_cursor(source, params.text_document_position.position)
                        .and_then(|alias| index.import_for_alias(alias))
                        .and_then(|binding| resolve_import_uri(uri, &binding.path))
                        .and_then(|target| index_for_uri(state, &target));
                let selected = completion_index.as_ref().unwrap_or(&index);
                let mut items = selected
                    .completion_names()
                    .into_iter()
                    .map(|(name, role)| completion_item(name, role))
                    .collect::<Vec<_>>();
                if completion_index.is_none() {
                    items.extend(alias_completion_items(&index));
                }
                items.extend(keyword_completion_items(index.lexicon()));
                CompletionResponse::Array(items)
            });
            ok(request, result)
        }),
        "textDocument/semanticTokens/full" => {
            parse_request::<SemanticTokensParams>(request).map(|params| {
                let result = index_for_uri(state, &params.text_document.uri).map(|index| {
                    SemanticTokensResult::Tokens(SemanticTokens {
                        result_id: None,
                        data: semantic_tokens(state, &params.text_document.uri, &index),
                    })
                });
                ok(request, result)
            })
        }
        "textDocument/rename" => parse_request::<RenameParams>(request).map(|params| {
            let result = rename(
                state,
                &params.text_document_position.text_document.uri,
                params.text_document_position.position,
                &params.new_name,
            );
            ok(request, result)
        }),
        "textDocument/codeAction" => parse_request::<CodeActionParams>(request).map(|params| {
            let result = code_actions(state, &params.text_document.uri);
            ok(request, result)
        }),
        _ => Err(Box::new(Response::new_err(
            request.id.clone(),
            ErrorCode::MethodNotFound as i32,
            format!("unsupported method {}", request.method),
        ))),
    }
    .unwrap_or_else(|response| *response);
    let _ = sender.send(response.into());
}

fn parse_request<T: DeserializeOwned>(request: &Request) -> Result<T, Box<Response>> {
    serde_json::from_value(request.params.clone()).map_err(|error| {
        Box::new(Response::new_err(
            request.id.clone(),
            ErrorCode::InvalidParams as i32,
            error.to_string(),
        ))
    })
}

fn ok<T: Serialize>(request: &Request, result: T) -> Response {
    Response::new_ok(request.id.clone(), result)
}

fn publish_diagnostics(
    sender: &Sender<Message>,
    state: &ServerState,
    uri: &Url,
) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let Some(source) = source_for_uri(state, uri) else {
        return Ok(());
    };
    let path = uri.to_file_path().ok();
    let base = path
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    let resolver = StoreResolver {
        state,
        base: base.to_path_buf(),
    };
    let source_file = path
        .as_deref()
        .and_then(Path::to_str)
        .unwrap_or(uri.as_str());
    let version = state.documents.get(uri).map(|document| document.version);
    let (source_diagnostics, model) = fslc_rust::source_diagnostic::diagnostics_with_model(
        &source,
        source_file,
        &resolver,
        &state.parse_options,
    );
    let edition = match fsl_syntax::edition_header(&source, &state.parse_options) {
        Ok(Some(header)) => header.edition,
        _ => fsl_syntax::Edition::Fsl4,
    };
    // `check` refuses a spec whose acceptance / forbidden traces the gate
    // rejects, so the editor shows that error instead of accepting it (#1243).
    let trace_gate = model.as_ref().map_or_else(Vec::new, |model| {
        fslc_rust::source_diagnostic::trace_gate_diagnostics(&source, model, &state.parse_options)
    });
    // `check`'s last stage: the requirements checks the enumeration deferred,
    // asked once the trace gate passes (decisions/048 J3).
    let requirements = match &model {
        Some(model)
            if trace_gate.is_empty()
                && state
                    .parse_options
                    .unstable()
                    .contains(fsl_syntax::UnstableFeature::Fsl5) =>
        {
            requirements_diagnostics(&source, model, state.parse_options)
        }
        _ => Vec::new(),
    };
    let mut items = source_diagnostics
        .into_iter()
        .chain(trace_gate)
        .chain(requirements)
        .map(|item| to_lsp_diagnostic(&source, item, edition))
        .chain(
            fsl_syntax::lexical_warnings(&source, &state.parse_options)
                .into_iter()
                .map(|warning| lexical_warning_diagnostic(&source, warning)),
        )
        .collect::<Vec<_>>();
    if analysis_diagnostics_enabled()
        && let Some(model) = model
    {
        let index = DocumentIndex::build(&source, Some(source_file), &state.parse_options).ok();
        items.extend(
            fsl_tools::structural_review_findings(&fsl_tools::build_tsg(&model))
                .into_iter()
                .map(|finding| analysis_diagnostic(&source, index.as_ref(), &finding)),
        );
    }
    sender.send(
        Notification::new(
            "textDocument/publishDiagnostics".to_owned(),
            PublishDiagnosticsParams::new(uri.clone(), items, version),
        )
        .into(),
    )?;
    Ok(())
}

/// The requirements checks of `source` asked of native Z3, on a thread of
/// their own: Z3 keeps one context per thread, and the model it returns (the
/// counterexample a message quotes) depends on what that context was asked
/// before, so a fresh thread answers each publish as a fresh `fslc check`
/// process does, with the same message.
fn requirements_diagnostics(
    source: &str,
    model: &fsl_core::KernelModel,
    options: fsl_syntax::ParseOptions,
) -> Vec<SourceDiagnostic> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            // The main thread's default, which the lowering stages run on.
            .stack_size(8 * 1024 * 1024)
            .spawn_scoped(scope, || {
                block_on_native(fslc_rust::source_diagnostic::solver_diagnostics(
                    source,
                    model,
                    &options,
                    &mut || fsl_solver_z3::Z3Solver::new().map_err(|error| error.to_string()),
                ))
            })
            .expect("spawn the requirements solver thread")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// Run a future of the native solver, which never yields `Pending`.
fn block_on_native<F: std::future::Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("native Z3 backend unexpectedly yielded Pending"),
    }
}

fn analysis_diagnostics_enabled() -> bool {
    std::env::var("FSLC_LSP_ANALYSIS_DIAGNOSTICS").is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn analysis_diagnostic(
    source: &str,
    index: Option<&DocumentIndex>,
    finding: &serde_json::Value,
) -> Diagnostic {
    let range = finding["involved_nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .filter_map(|node| node.rsplit(':').next())
        .find_map(|name| {
            index?
                .symbols
                .iter()
                .find(|symbol| symbol.name == name)
                .map(|symbol| symbol.selection_range)
        })
        .or_else(|| {
            let line = usize::try_from(finding["loc"]["line"].as_u64()?.saturating_sub(1)).ok()?;
            let column =
                usize::try_from(finding["loc"]["column"].as_u64()?.saturating_sub(1)).ok()?;
            // `loc.column` counts characters; an LSP position counts UTF-16
            // code units, which differ past an astral character.
            let character = source.split('\n').nth(line).map_or(column, |text| {
                text.chars()
                    .take(column)
                    .map(char::len_utf16)
                    .sum::<usize>()
            });
            let start = Position::new(u32::try_from(line).ok()?, u32::try_from(character).ok()?);
            Some(lsp_types::Range::new(
                start,
                Position::new(start.line, start.character.saturating_add(1)),
            ))
        })
        .unwrap_or_else(|| lsp_types::Range::new(Position::new(0, 0), Position::new(0, 1)));
    let finding_type = finding["finding_type"].as_str().unwrap_or("finding");
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::INFORMATION),
        code: Some(NumberOrString::String(finding_type.to_owned())),
        source: Some("fslc analyze".to_owned()),
        message: format!(
            "Structural review ({finding_type}): {}",
            finding["why_it_matters"]
                .as_str()
                .unwrap_or("review the structural finding")
        ),
        data: Some(serde_json::json!({
            "finding_id": finding["finding_id"],
            "formal_status": finding["formal_status"],
            "candidate_repairs": finding["candidate_repairs"],
            "do_not_assume": finding["do_not_assume"],
        })),
        ..Diagnostic::default()
    }
}

/// A migration finding is a warning, except in the next edition, which no
/// longer accepts the legacy forms it reports.
fn to_lsp_diagnostic(
    source: &str,
    item: SourceDiagnostic,
    edition: fsl_syntax::Edition,
) -> Diagnostic {
    let severity = if item.kind == "migration" && edition != fsl_syntax::Edition::Fsl5 {
        DiagnosticSeverity::WARNING
    } else {
        DiagnosticSeverity::ERROR
    };
    let mut diagnostic = Diagnostic::new(
        span_range(source, item.span),
        Some(severity),
        Some(NumberOrString::String(item.code)),
        Some("fslc".to_owned()),
        item.message,
        None,
        None,
    );
    diagnostic.data = Some(serde_json::json!({"kind": item.kind}));
    if let Some(related) = item.related {
        diagnostic.severity = Some(if related.error {
            DiagnosticSeverity::ERROR
        } else {
            DiagnosticSeverity::WARNING
        });
        diagnostic.related_information = Some(vec![lsp_types::DiagnosticRelatedInformation {
            location: Location::new(
                Url::from_file_path(related.file).expect("dependency source path"),
                span_range(&related.source, related.span),
            ),
            message: diagnostic.message.clone(),
        }]);
    }
    diagnostic
}

fn lexical_warning_diagnostic(source: &str, warning: fsl_syntax::LexWarning) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(
        span_range(source, warning.span),
        Some(DiagnosticSeverity::WARNING),
        Some(NumberOrString::String(warning.code.to_owned())),
        Some("fslc".to_owned()),
        warning.message,
        None,
        None,
    );
    diagnostic.data = Some(serde_json::json!({
        "kind": "lexical",
        "replacement": warning.replacement,
    }));
    diagnostic
}

fn index_for_uri(state: &ServerState, uri: &Url) -> Option<DocumentIndex> {
    let source = source_for_uri(state, uri)?;
    let path = uri.to_file_path().ok();
    DocumentIndex::build(
        &source,
        path.as_deref().and_then(Path::to_str),
        &state.parse_options,
    )
    .ok()
}

fn source_for_uri(state: &ServerState, uri: &Url) -> Option<String> {
    state.documents.get(uri).map_or_else(
        || {
            uri.to_file_path()
                .ok()
                .and_then(|path| std::fs::read_to_string(path).ok())
        },
        |document| Some(document.text.clone()),
    )
}

fn definition_location(state: &ServerState, uri: &Url, position: Position) -> Option<Location> {
    let index = index_for_uri(state, uri)?;
    if let Some(reference) = index.reference_at(position)
        && let Some(target_spec) = &reference.target_spec
    {
        let target_uri = workspace_uris(state, uri)
            .into_iter()
            .find(|candidate_uri| {
                index_for_uri(state, candidate_uri).is_some_and(|candidate| {
                    candidate.symbols.iter().any(|symbol| {
                        symbol.role == SymbolRole::Namespace && symbol.name == *target_spec
                    })
                })
            })?;
        let target = index_for_uri(state, &target_uri)?;
        let symbol = target
            .symbols
            .iter()
            .find(|symbol| symbol.name == reference.name && symbol.owner.is_none())?;
        return Some(Location::new(target_uri, symbol.selection_range));
    }
    if let Some(reference) = index.reference_at(position)
        && let Some(qualifier) = &reference.qualifier
        && let Some(binding) = index.import_for_alias(qualifier)
    {
        let target_uri = resolve_import_uri(uri, &binding.path)?;
        let target = index_for_uri(state, &target_uri)?;
        let symbol = target
            .symbols
            .iter()
            .find(|symbol| symbol.name == reference.name)?;
        return Some(Location::new(target_uri, symbol.selection_range));
    }
    if let Some(symbol) = index.definition_at(position) {
        return Some(Location::new(uri.clone(), symbol.selection_range));
    }
    let reference = index.reference_at(position)?;
    workspace_uris(state, uri)
        .into_iter()
        .find_map(|candidate_uri| {
            let candidate = index_for_uri(state, &candidate_uri)?;
            let symbol = candidate
                .symbols
                .iter()
                .find(|symbol| symbol.name == reference.name)?;
            Some(Location::new(candidate_uri, symbol.selection_range))
        })
}

fn workspace_references(
    state: &ServerState,
    uri: &Url,
    position: Position,
    include_declaration: bool,
) -> Vec<Location> {
    let Some(target) = definition_location(state, uri, position) else {
        return Vec::new();
    };
    let Some(target_index) = index_for_uri(state, &target.uri) else {
        return Vec::new();
    };
    let Some(target_symbol) = target_index.symbol_at(target.range.start) else {
        return Vec::new();
    };
    let target_name = target_symbol.name.clone();
    let mut locations = Vec::new();
    if include_declaration {
        locations.push(target.clone());
    }
    for candidate_uri in workspace_uris(state, uri) {
        let Some(index) = index_for_uri(state, &candidate_uri) else {
            continue;
        };
        for reference in index
            .references
            .iter()
            .filter(|reference| reference.name == target_name)
        {
            if definition_location(state, &candidate_uri, reference.range.start)
                .is_some_and(|location| location == target)
            {
                locations.push(Location::new(candidate_uri.clone(), reference.range));
            }
        }
    }
    locations.sort_by(|left, right| {
        left.uri.as_str().cmp(right.uri.as_str()).then_with(|| {
            (left.range.start.line, left.range.start.character)
                .cmp(&(right.range.start.line, right.range.start.character))
        })
    });
    locations.dedup();
    locations
}

fn hover(state: &ServerState, uri: &Url, position: Position) -> Option<Hover> {
    let index = index_for_uri(state, uri)?;
    if let Some(keyword) = index.keyword_at(position) {
        return Some(markdown_hover(keyword_hover(&keyword), keyword.range));
    }
    let written = index
        .written_alias_at(position)
        .map(|(alias, canonical)| (alias.to_owned(), canonical.to_owned()));
    let declared = definition_location(state, uri, position).and_then(|location| {
        let target = index_for_uri(state, &location.uri)?;
        let symbol = target.symbol_at(location.range.start)?.clone();
        Some((target, symbol))
    });
    if let Some((target, symbol)) = declared {
        let snippet = source_line(target.source(), symbol.selection_range.start.line);
        let mut value = format!(
            "```fsl\n{snippet}\n```\n**{}** `{}`",
            symbol.role.detail(),
            symbol.name
        );
        value.push_str(&glossary_hover(&index, &target, &symbol.name));
        return Some(markdown_hover(value, symbol.selection_range));
    }
    // A name the glossary aliases but nothing declares (a verb).
    let reference = index.reference_at(position)?;
    let mut value = match written {
        Some((alias, canonical)) => format!("**alias** `{alias}` of `{canonical}`"),
        None if index.glossary_rows_for(&reference.name).next().is_some() => {
            format!("**name** `{}`", reference.name)
        }
        None => return None,
    };
    value.push_str(&glossary_hover(&index, &index, &reference.name));
    Some(markdown_hover(value, reference.range))
}

fn markdown_hover(value: String, range: lsp_types::Range) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(range),
    }
}

/// The glossary rows of `name`: the current document's, or else the
/// declaring document's.
fn glossary_hover(current: &DocumentIndex, target: &DocumentIndex, name: &str) -> String {
    let mut rows = current.glossary_rows_for(name).collect::<Vec<_>>();
    if rows.is_empty() {
        rows = target.glossary_rows_for(name).collect();
    }
    rows.iter().fold(String::new(), |mut value, row| {
        let _ = write!(
            value,
            "\n\nalias `{}` · glossary `{} = {}`",
            row.alias, row.canonical, row.alias
        );
        value
    })
}

/// A keyword's spellings in both languages, one line per row of the keyword
/// table it spells, and a particle's role.
fn keyword_hover(keyword: &crate::KeywordUse) -> String {
    let mut value = String::new();
    if !keyword.rows.is_empty() {
        let _ = writeln!(value, "**keyword** `{}`", keyword.word);
        for row in &keyword.rows {
            let _ = write!(
                value,
                "\n- `{}` · `{}` ({:?})",
                row.english, row.japanese, row.context
            );
        }
    }
    if let Some(particle) = keyword.particle {
        if !value.is_empty() {
            value.push_str("\n\n");
        }
        let _ = write!(
            value,
            "**particle** `{}` marks the {}",
            particle.spelling(),
            particle.role()
        );
    }
    value
}

/// The keywords completion offers: the published starters, and in a
/// next-edition requirements document every active row of the keyword table
/// in English and in Japanese, each with its other spelling as the detail.
fn keyword_completion_items(lexicon: crate::Lexicon) -> Vec<CompletionItem> {
    let mut labels = KEYWORDS
        .iter()
        .map(|keyword| ((*keyword).to_owned(), Vec::<&str>::new()))
        .collect::<Vec<_>>();
    if lexicon == crate::Lexicon::Next {
        for row in fsl_syntax::keywords::KEYWORDS
            .iter()
            .filter(|row| row.status == fsl_syntax::keywords::Status::Active)
        {
            for (label, other) in [(row.english, row.japanese), (row.japanese, row.english)] {
                match labels.iter_mut().find(|(known, _)| known == label) {
                    Some((_, details)) => {
                        if !details.contains(&other) {
                            details.push(other);
                        }
                    }
                    None => labels.push((label.to_owned(), vec![other])),
                }
            }
        }
    }
    labels
        .into_iter()
        .map(|(label, details)| CompletionItem {
            label,
            kind: Some(CompletionItemKind::KEYWORD),
            detail: (!details.is_empty()).then(|| details.join(" / ")),
            ..CompletionItem::default()
        })
        .collect()
}

/// The glossary's aliases, each with its canonical name as the detail.
fn alias_completion_items(index: &DocumentIndex) -> Vec<CompletionItem> {
    index
        .glossary()
        .iter()
        .map(|row| {
            let role = index
                .symbols
                .iter()
                .find(|symbol| symbol.name == row.name)
                .map(|symbol| symbol.role);
            CompletionItem {
                detail: Some(row.canonical.clone()),
                ..role.map_or_else(
                    || CompletionItem {
                        label: row.alias.clone(),
                        kind: Some(CompletionItemKind::VARIABLE),
                        ..CompletionItem::default()
                    },
                    |role| completion_item(&row.alias, role),
                )
            }
        })
        .collect()
}

/// Rename the name at `position`. Written as a glossary alias, the alias is
/// renamed: the glossary's right side and every place the document writes
/// it. Written as its canonical name, the canonical name is renamed: every
/// place any document of the workspace writes it — the glossary's left side
/// and other files' mapping tables included — but no alias.
fn rename(
    state: &ServerState,
    uri: &Url,
    position: Position,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    let index = index_for_uri(state, uri)?;
    if let Some((alias, _)) = index.written_alias_at(position) {
        let source = source_for_uri(state, uri)?;
        let rule = fsl_syntax::IdentifierRule::for_source(&source, &state.parse_options);
        if !valid_alias(&index, alias, new_name, rule) {
            return None;
        }
        let edits = index
            .alias_ranges(alias)
            .into_iter()
            .map(|range| TextEdit::new(range, new_name.to_owned()))
            .collect();
        return Some(WorkspaceEdit::new(HashMap::from([(uri.clone(), edits)])));
    }
    if !valid_identifier_in(state, uri, new_name) {
        return None;
    }
    let mut written = HashMap::<Url, Option<DocumentIndex>>::new();
    let mut locations = workspace_references(state, uri, position, true)
        .into_iter()
        .filter(|location| {
            written
                .entry(location.uri.clone())
                .or_insert_with(|| index_for_uri(state, &location.uri))
                .as_ref()
                .is_none_or(|edited| edited.written_alias_at(location.range.start).is_none())
        })
        .collect::<Vec<_>>();
    if locations.is_empty() {
        locations = undeclared_glossary_name(&index, uri, position);
    }
    if locations.is_empty()
        || !locations
            .iter()
            .map(|location| &location.uri)
            .collect::<HashSet<_>>()
            .into_iter()
            .all(|edited| valid_identifier_in(state, edited, new_name))
    {
        return None;
    }
    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
    for location in locations {
        changes
            .entry(location.uri)
            .or_default()
            .push(TextEdit::new(location.range, new_name.to_owned()));
    }
    Some(WorkspaceEdit::new(changes))
}

/// A canonical name the glossary aliases but nothing declares (a verb): its
/// canonical spellings in the document.
fn undeclared_glossary_name(index: &DocumentIndex, uri: &Url, position: Position) -> Vec<Location> {
    let Some(name) = index
        .reference_at(position)
        .filter(|reference| index.definition_at(reference.range.start).is_none())
        .map(|reference| reference.name.clone())
    else {
        return Vec::new();
    };
    if index.glossary_rows_for(&name).next().is_none() {
        return Vec::new();
    }
    index
        .references
        .iter()
        .filter(|reference| {
            reference.name == name
                && reference.alias.is_none()
                && index.definition_at(reference.range.start).is_none()
        })
        .map(|reference| Location::new(uri.clone(), reference.range))
        .collect()
}

/// Whether `value` may replace the alias `old`: an identifier under the
/// document's rule that is spelled like no keyword (English or Japanese, any
/// context, reserved rows included, or a particle), no canonical or declared
/// name, and no other alias (`alias_shadows_keyword`, `alias_shadows_name`,
/// `alias_collision`).
fn valid_alias(
    index: &DocumentIndex,
    old: &str,
    value: &str,
    rule: fsl_syntax::IdentifierRule,
) -> bool {
    rule.accepts(value)
        && !fsl_syntax::is_keyword_spelling(value)
        && !index.is_keyword(value)
        && !index.glossary().iter().any(|row| {
            (row.alias == value && row.alias != old)
                || row.canonical.split('.').any(|name| name == value)
        })
        && !index.symbols.iter().any(|symbol| symbol.name == value)
}

fn workspace_uris(state: &ServerState, current: &Url) -> Vec<Url> {
    let mut uris = state.documents.keys().cloned().collect::<HashSet<_>>();
    for root in &state.roots {
        collect_fsl_files(root, &mut uris);
    }

    if let Ok(path) = current.to_file_path()
        && let Some(parent) = path.parent()
        && !state.roots.iter().any(|root| parent.starts_with(root))
    {
        // An explicitly opened file outside every workspace root may still
        // refer to an unimported sibling, but it must not turn its arbitrary
        // parent (for example `/tmp`) into a recursive workspace scan.
        collect_direct_fsl_files(parent, &mut uris);
    }
    let mut uris = uris.into_iter().collect::<Vec<_>>();
    uris.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    uris
}

fn collect_direct_fsl_files(path: &Path, uris: &mut HashSet<Url>) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("fsl")
            && let Ok(uri) = Url::from_file_path(path)
        {
            uris.insert(uri);
        }
    }
}

fn collect_fsl_files(path: &Path, uris: &mut HashSet<Url>) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if !name.starts_with('.') && !matches!(name, "target" | "node_modules") {
                collect_fsl_files(&path, uris);
            }
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("fsl")
            && let Ok(uri) = Url::from_file_path(path)
        {
            uris.insert(uri);
        }
    }
}

fn resolve_import_uri(owner: &Url, relative: &str) -> Option<Url> {
    let owner = owner.to_file_path().ok()?;
    Url::from_file_path(owner.parent()?.join(relative)).ok()
}

fn document_symbol(symbol: &crate::Symbol) -> DocumentSymbol {
    #[allow(deprecated)]
    DocumentSymbol {
        name: symbol.name.clone(),
        detail: Some(symbol.role.detail().to_owned()),
        kind: symbol_kind(symbol.role),
        tags: None,
        deprecated: None,
        range: symbol.range,
        selection_range: symbol.selection_range,
        children: None,
    }
}

fn symbol_kind(role: SymbolRole) -> SymbolKind {
    match role {
        SymbolRole::Namespace => SymbolKind::NAMESPACE,
        SymbolRole::Type => SymbolKind::CLASS,
        SymbolRole::Function => SymbolKind::FUNCTION,
        SymbolRole::Variable | SymbolRole::Parameter => SymbolKind::VARIABLE,
        SymbolRole::Property => SymbolKind::PROPERTY,
    }
}

fn completion_item(name: &str, role: SymbolRole) -> CompletionItem {
    CompletionItem {
        label: name.to_owned(),
        kind: Some(match role {
            SymbolRole::Namespace => CompletionItemKind::MODULE,
            SymbolRole::Type => CompletionItemKind::CLASS,
            SymbolRole::Function => CompletionItemKind::FUNCTION,
            SymbolRole::Variable | SymbolRole::Parameter => CompletionItemKind::VARIABLE,
            SymbolRole::Property => CompletionItemKind::PROPERTY,
        }),
        detail: Some(role.detail().to_owned()),
        ..CompletionItem::default()
    }
}

fn semantic_tokens(state: &ServerState, uri: &Url, index: &DocumentIndex) -> Vec<SemanticToken> {
    let mut tokens = index
        .symbols
        .iter()
        .map(|symbol| (symbol.selection_range, token_type(symbol.role), 1_u32))
        .chain(index.references.iter().map(|reference| {
            let role = definition_location(state, uri, reference.range.start)
                .and_then(|location| {
                    index_for_uri(state, &location.uri)
                        .and_then(|target| target.symbol_at(location.range.start).cloned())
                })
                .map_or(3_u32, |symbol| token_type(symbol.role));
            (reference.range, role, 0_u32)
        }))
        .collect::<Vec<_>>();
    tokens.sort_by_key(|(range, _, _)| (range.start.line, range.start.character));
    tokens.dedup_by_key(|(range, _, _)| (range.start.line, range.start.character));
    let mut previous_line = 0_u32;
    let mut previous_character = 0_u32;
    tokens
        .into_iter()
        .map(|(range, token_type, modifiers)| {
            let delta_line = range.start.line - previous_line;
            let delta_start = if delta_line == 0 {
                range.start.character - previous_character
            } else {
                range.start.character
            };
            previous_line = range.start.line;
            previous_character = range.start.character;
            SemanticToken {
                delta_line,
                delta_start,
                length: range.end.character - range.start.character,
                token_type,
                token_modifiers_bitset: modifiers,
            }
        })
        .collect()
}

const fn token_type(role: SymbolRole) -> u32 {
    match role {
        SymbolRole::Namespace => 0,
        SymbolRole::Type => 1,
        SymbolRole::Function => 2,
        SymbolRole::Variable => 3,
        SymbolRole::Parameter => 4,
        SymbolRole::Property => 5,
    }
}

fn code_actions(state: &ServerState, uri: &Url) -> Option<Vec<CodeActionOrCommand>> {
    let source = source_for_uri(state, uri)?;
    let path = uri.to_file_path().ok();
    let base = path
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    let resolver = StoreResolver {
        state,
        base: base.to_path_buf(),
    };
    let source_file = path
        .as_deref()
        .and_then(Path::to_str)
        .unwrap_or(uri.as_str());
    let (diagnostics, _) = fslc_rust::source_diagnostic::diagnostics_with_model(
        &source,
        source_file,
        &resolver,
        &state.parse_options,
    );
    if diagnostics.iter().any(|diagnostic| {
        diagnostic
            .related
            .as_ref()
            .is_some_and(|related| related.error)
    }) {
        return None;
    }
    let semantic_fixes = diagnostics
        .into_iter()
        .filter_map(|diagnostic| {
            diagnostic.quick_fix.map(|edit| {
                CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Apply write-index distinctness repair".to_owned(),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: None,
                    edit: Some(WorkspaceEdit::new(HashMap::from([(
                        uri.clone(),
                        vec![TextEdit::new(
                            span_range(&source, edit.span),
                            edit.replacement,
                        )],
                    )]))),
                    command: None,
                    is_preferred: Some(true),
                    disabled: None,
                    data: None,
                })
            })
        })
        .collect::<Vec<_>>();
    let rewrites = fsl_syntax::canonical_rewrites_with(&source, &state.parse_options).ok()?;
    let canonical_fixes = rewrites
        .into_iter()
        .map(|rewrite| {
            let edits = rewrite
                .edits
                .into_iter()
                .map(|edit| TextEdit::new(span_range(&source, edit.span), edit.replacement))
                .collect::<Vec<_>>();
            CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Use canonical FSL: {}", rewrite.canonical_replacement),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: None,
                edit: Some(WorkspaceEdit::new(HashMap::from([(uri.clone(), edits)]))),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
            })
        })
        .collect::<Vec<_>>();
    let actions = semantic_fixes
        .into_iter()
        .chain(canonical_fixes)
        .collect::<Vec<_>>();
    if actions.is_empty() {
        None
    } else {
        Some(actions)
    }
}

fn workspace_roots(params: &InitializeParams) -> Vec<PathBuf> {
    let mut roots = params
        .workspace_folders
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|folder| folder.uri.to_file_path().ok())
        .collect::<Vec<_>>();
    #[allow(deprecated)]
    if let Some(root) = params
        .root_uri
        .as_ref()
        .and_then(|uri| uri.to_file_path().ok())
    {
        roots.push(root);
    }
    roots.sort();
    roots.dedup();
    roots
}

fn source_line(source: &str, line: u32) -> &str {
    source
        .lines()
        .nth(usize::try_from(line).expect("line fits usize"))
        .unwrap_or_default()
        .trim()
}

/// Whether `value` reads as one identifier in the document at `uri`, whose
/// edition and the server's options decide the identifier rule.
///
/// A canonical name of a next-edition requirements document is ASCII
/// (`canonical_name_not_ascii`), spelled like none of its keywords in either
/// language, and like none of its aliases.
fn valid_identifier_in(state: &ServerState, uri: &Url, value: &str) -> bool {
    let Some(source) = source_for_uri(state, uri) else {
        return valid_identifier(value, fsl_syntax::IdentifierRule::Ascii);
    };
    match DocumentIndex::build(&source, None, &state.parse_options) {
        Ok(index) if index.lexicon() == crate::Lexicon::Next => {
            fsl_syntax::IdentifierRule::Ascii.accepts(value)
                && !index.is_keyword(value)
                && !index.glossary().iter().any(|row| row.alias == value)
        }
        _ => valid_identifier(
            value,
            fsl_syntax::IdentifierRule::for_source(&source, &state.parse_options),
        ),
    }
}

fn valid_identifier(value: &str, rule: fsl_syntax::IdentifierRule) -> bool {
    rule.accepts(value) && !crate::index::is_keyword(value)
}

fn alias_before_cursor(source: &str, position: Position) -> Option<&str> {
    let offset = offset_at_position(source, position);
    let prefix = &source[..offset];
    let dot = prefix.rfind('.')?;
    let alias = prefix[..dot]
        .rsplit(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .next()?;
    (!alias.is_empty()).then_some(alias)
}

fn apply_content_changes(source: &mut String, changes: &[TextDocumentContentChangeEvent]) {
    for change in changes {
        if let Some(range) = change.range {
            let start = offset_at_position(source, range.start);
            let end = offset_at_position(source, range.end);
            if start <= end && end <= source.len() {
                source.replace_range(start..end, &change.text);
            }
        } else {
            source.clone_from(&change.text);
        }
    }
}

fn offset_at_position(source: &str, position: Position) -> usize {
    let mut line = 0_u32;
    let mut character = 0_u32;
    for (offset, ch) in source.char_indices() {
        if (line, character) == (position.line, position.character) {
            return offset;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += u32::try_from(ch.len_utf16()).expect("UTF-16 width fits u32");
        }
    }
    source.len()
}

#[cfg(test)]
mod tests {
    mod dependency_edition {
        include!("../tests/support/edition_dependency_adapter.rs");
    }
    use super::*;
    use lsp_server::RequestId;

    const KEYWORDS_STARTERS: &[&str] = KEYWORDS;
    use lsp_types::Range;

    fn fixture_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "fsl-lsp-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create fixture directory");
        root
    }

    fn fixture_uri(root: &Path, name: &str, source: &str) -> Url {
        let path = root.join(name);
        std::fs::write(&path, source).expect("write fixture");
        Url::from_file_path(path).expect("fixture URI")
    }

    fn fixture_pair(name: &str, first: &str, second: &str) -> (PathBuf, Url, Url) {
        let root = fixture_root(name);
        let first_uri = fixture_uri(&root, "lib.fsl", first);
        let second_uri = fixture_uri(&root, "main.fsl", second);
        (root, first_uri, second_uri)
    }

    struct CompletionKeywordExclusion {
        keywords: &'static [&'static str],
        reason: &'static str,
    }

    /// Keywords rejected by rename validation but intentionally absent from
    /// generic completion. Each group records the context needed to decide
    /// whether a future keyword belongs in completion instead.
    const COMPLETION_KEYWORD_EXCLUSIONS: &[CompletionKeywordExclusion] = &[
        CompletionKeywordExclusion {
            keywords: &["causal"],
            reason: "the review-only causal profile has its own source grammar and is not a generic completion starter",
        },
        CompletionKeywordExclusion {
            keywords: &[
                "table",
                "transition",
                "tool",
                "command",
                "effect",
                "migration",
                "decide",
                "evolve",
                "def",
                "property",
                "requirement",
                "acceptance",
                "forbidden",
                "control",
                "policy",
                "goal",
                "claim",
                "expectation",
                "actor",
                "process",
                "kpi",
                "authority",
                "aggregate",
                "projection",
                "environment",
                "artifact",
                "column",
                "variable",
                "preservation",
                "verify",
            ],
            reason: "these declaration forms require a selected dialect or declaration context, so generic completion does not offer them",
        },
        CompletionKeywordExclusion {
            keywords: &["as", "from", "internal", "symmetric", "fair"],
            reason: "these compose modifiers complete only after their owning declaration has established context",
        },
        CompletionKeywordExclusion {
            keywords: &[
                "let",
                "in",
                "where",
                "decreases",
                "within",
                "helpful",
                "relation",
                "acyclic",
                "functional",
                "injective",
                "map",
                "maps",
                "auto",
                "impl",
                "abs",
                "preserve",
                "progress",
                "respond",
                "by",
                "implements",
                "expect",
                "rejected",
            ],
            reason: "these binders and clause modifiers are meaningful only within their owning grammar production",
        },
        CompletionKeywordExclusion {
            keywords: &[
                "time",
                "urgent",
                "age",
                "while",
                "deadline",
                "with",
                "stages",
                "initial",
                "when",
                "set",
                "covers",
                "count",
                "owner",
                "severity",
                "applies_to",
                "satisfies",
                "responds",
                "every",
                "reaching",
                "must",
                "have",
                "passed",
                "through",
                "eventually",
                "be",
                "some",
                "can",
                "reach",
                "all",
                "owns",
                "delegates",
                "require",
                "satisfied_by",
                "before",
                "after",
                "checked_by",
            ],
            reason: "these requirements, business, and temporal clauses need a surrounding declaration to be useful",
        },
        CompletionKeywordExclusion {
            keywords: &[
                "Int",
                "Bool",
                "Map",
                "Set",
                "Seq",
                "Option",
                "is",
                "and",
                "or",
                "not",
                "sum",
                "min",
                "max",
                "old",
                "unique",
                "exactlyOne",
                "add",
                "remove",
                "push",
                "pop",
                "head",
                "at",
                "size",
                "contains",
            ],
            reason: "these types, operators, and member names require expression or type context rather than generic completion",
        },
        CompletionKeywordExclusion {
            keywords: &[
                "timebase",
                "horizon",
                "scope",
                "clock",
                "feedback",
                "evidence",
                "polarity",
                "lag",
                "persists",
                "basis",
                "status",
                "version",
                "binds",
                "observes",
                "latent",
                "proxy",
                "cadence",
                "trigger",
                "response",
                "derived_from_claim",
                "uses",
            ],
            reason: "these causal-profile declarations and clauses require causal grammar context rather than generic completion",
        },
    ];

    fn request(state: &ServerState, method: &str, params: serde_json::Value) -> serde_json::Value {
        let (sender, receiver) = crossbeam_channel::unbounded();
        handle_request(
            &sender,
            state,
            &Request::new(RequestId::from(1), method.to_owned(), params),
        );
        let Message::Response(response) = receiver.recv().expect("response") else {
            panic!("expected response");
        };
        response.result.expect("successful result")
    }

    fn has_semantic_token(
        value: &serde_json::Value,
        target_line: u64,
        target_character: u64,
        target_type: u64,
    ) -> bool {
        let mut line = 0_u64;
        let mut character = 0_u64;
        value
            .as_array()
            .expect("semantic token data")
            .as_chunks::<5>()
            .0
            .iter()
            .any(|token| {
                let delta_line = token[0].as_u64().expect("delta line");
                line += delta_line;
                character = if delta_line == 0 {
                    character + token[1].as_u64().expect("delta start")
                } else {
                    token[1].as_u64().expect("line start")
                };
                line == target_line && character == target_character && token[3] == target_type
            })
    }

    #[test]
    fn advertises_the_issue_contract_features() {
        let value = serde_json::to_value(server_capabilities()).expect("serialize capabilities");
        for key in [
            "hoverProvider",
            "completionProvider",
            "definitionProvider",
            "referencesProvider",
            "documentSymbolProvider",
            "renameProvider",
            "semanticTokensProvider",
            "codeActionProvider",
        ] {
            assert!(value.get(key).is_some(), "missing capability {key}");
        }
    }

    #[test]
    fn incremental_changes_use_utf16_positions() {
        let mut source =
            "// 😀\nspec Old { state { ready: Bool } init { ready = false } }".to_owned();
        apply_content_changes(
            &mut source,
            &[TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(1, 5), Position::new(1, 8))),
                range_length: Some(3),
                text: "New".to_owned(),
            }],
        );
        assert!(source.contains("spec New"));
    }

    #[test]
    fn did_open_unknown_authority_tool_publishes_an_error_and_keeps_serving() {
        let source = include_str!("../../fslc/tests/fixtures/error_envelope_ai_unknown_tool.fsl");
        let uri = Url::parse("file:///memory/unknown-authority.fsl").expect("in-memory URI");
        let mut state = ServerState::default();
        let (sender, receiver) = crossbeam_channel::unbounded();
        // No fixture file or stdio process is needed: this is the same
        // notification handler run_stdio calls synchronously on its main loop.
        handle_notification(
            &sender,
            &mut state,
            Notification::new(
                "textDocument/didOpen".to_owned(),
                serde_json::json!({"textDocument": {
                    "uri": uri,
                    "languageId": "fsl",
                    "version": 1,
                    "text": source,
                }}),
            ),
        )
        .expect("invalid source must publish a diagnostic, not unwind the server");
        let Message::Notification(notification) =
            receiver.try_recv().expect("diagnostics published")
        else {
            panic!("didOpen must publish a notification");
        };
        assert_eq!(notification.method, "textDocument/publishDiagnostics");
        let published: PublishDiagnosticsParams =
            serde_json::from_value(notification.params).expect("diagnostic payload");
        assert_eq!(published.uri, uri);
        assert_eq!(published.version, Some(1));
        assert_eq!(published.diagnostics.len(), 1, "{published:?}");
        let diagnostic = &published.diagnostics[0];
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(diagnostic.source.as_deref(), Some("fslc"));
        assert_eq!(
            diagnostic.data,
            Some(serde_json::json!({"kind": "semantics"}))
        );
        assert!(
            diagnostic
                .message
                .contains("unknown tool 'MissingTool' in authority block"),
            "{diagnostic:?}"
        );
        assert_eq!(diagnostic.range.start, Position::new(13, 4));

        // A subsequent, well-formed request on the same server state succeeds.
        // Keep both documents in memory; never fall back to a file on disk.
        let healthy_uri = Url::parse("file:///memory/healthy.fsl").expect("in-memory URI");
        state.documents.insert(
            healthy_uri.clone(),
            OpenDocument {
                text: "spec Healthy { state { ready: Bool } init { ready = false } }".to_owned(),
                version: 1,
            },
        );
        let response = request(
            &state,
            "textDocument/completion",
            serde_json::json!({
                "textDocument": {"uri": healthy_uri},
                "position": {"line": 0, "character": 20},
            }),
        );
        assert!(
            response
                .as_array()
                .expect("completion response")
                .iter()
                .any(|item| item["label"] == "ready"),
            "the server must answer after the rejected document: {response}"
        );
    }

    #[test]
    fn rename_identifier_validation_is_closed() {
        for rule in [
            fsl_syntax::IdentifierRule::Ascii,
            fsl_syntax::IdentifierRule::Unicode,
        ] {
            assert!(valid_identifier("new_name", rule));
            assert!(!valid_identifier("9name", rule));
            assert!(!valid_identifier("state", rule));
            assert!(!valid_identifier("use", rule));
            assert!(!valid_identifier("two names", rule));
        }
        assert!(!valid_identifier("経費", fsl_syntax::IdentifierRule::Ascii));
        assert!(valid_identifier(
            "経費",
            fsl_syntax::IdentifierRule::Unicode
        ));
        for look_alike in ["Ｃｌａｉｍ", "申請・承認", "Сlaim", "\u{304b}\u{3099}"] {
            assert!(
                !valid_identifier(look_alike, fsl_syntax::IdentifierRule::Unicode),
                "{look_alike}"
            );
        }
    }

    /// A rename reaches every file that names the symbol, so the new name must
    /// be an identifier under each file's own rule: a `fsl 5` document behind
    /// the gate accepts `経費`, a published-language document never does.
    #[test]
    fn rename_target_rule_follows_each_documents_edition_and_the_gate() {
        let fsl5 = Url::parse("file:///tmp/fsl-lsp-rename/fsl5.fsl").unwrap();
        let published = Url::parse("file:///tmp/fsl-lsp-rename/published.fsl").unwrap();
        let mut state = ServerState {
            parse_options: fsl_syntax::ParseOptions::with_unstable(
                fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
            ),
            ..ServerState::default()
        };
        for (uri, text) in [
            (&fsl5, "fsl 5\nspec S { state { x: Int } }\n"),
            (&published, "spec S { state { x: Int } }\n"),
        ] {
            state.documents.insert(
                uri.clone(),
                OpenDocument {
                    text: text.to_owned(),
                    version: 1,
                },
            );
        }
        assert!(valid_identifier_in(&state, &fsl5, "経費"));
        assert!(!valid_identifier_in(&state, &published, "経費"));
        assert!(valid_identifier_in(&state, &published, "cost"));
        state.parse_options = fsl_syntax::ParseOptions::stable();
        assert!(!valid_identifier_in(&state, &fsl5, "経費"));
    }

    #[test]
    fn completion_and_rename_keyword_populations_are_accounted_for() {
        let completion = KEYWORDS.iter().copied().collect::<HashSet<_>>();
        let mut exclusions = HashSet::new();

        for keyword in KEYWORDS {
            assert!(
                crate::index::is_keyword(keyword),
                "completion offers {keyword:?}, but rename validation does not recognize it"
            );
        }
        for exclusion in COMPLETION_KEYWORD_EXCLUSIONS {
            assert!(
                !exclusion.reason.trim().is_empty(),
                "completion exclusion {:?} needs a reason",
                exclusion.keywords
            );
            for keyword in exclusion.keywords {
                assert!(
                    crate::index::is_keyword(keyword),
                    "completion exclusion {keyword:?} is stale: rename validation no longer recognizes it"
                );
                assert!(
                    !completion.contains(keyword),
                    "completion exclusion {keyword:?} is now offered; remove the stale exclusion"
                );
                assert!(
                    exclusions.insert(*keyword),
                    "completion exclusion {keyword:?} is listed more than once"
                );
            }
        }
        for keyword in crate::index::recognized_keywords() {
            assert!(
                completion.contains(keyword) || exclusions.contains(keyword),
                "rename validation recognizes {keyword:?}, but completion neither offers it nor records a reasoned exclusion"
            );
        }
    }

    /// A next-edition requirements document with a glossary (Japanese
    /// aliases), a Japanese keyword, particles, an `implements` mapping to
    /// `b.fsl`, and a refinement file mapping its event; `b.fsl` is in the
    /// published language.
    const FSL5_A: &str = "fsl 5
requirements ExpenseRequirements {
  glossary {
    Claim = 申請
    Claim.amount = 申請金額
    Submitted = 提出済み
    submits = 提出する
  }
  implements ExpenseToBe from \"b.fsl\" {
    action submit(c, amount) -> submit(c)
  }
  number Amount
  actor Employee
  エンティティ 申請 {
    申請金額: Amount = 0
    lifecycle Draft -> 提出済み
  }
  concept Large = 申請 where 申請金額 > 0
  event submit = Employee が Claim を 提出する amount で Draft -> Submitted
  scenario AC-1 \"t\" {
    Employee submits claim1 with amount 1
    then claim1 is 提出済み
  }
}
verify {
  instances Claim = 1
  values Amount = 0..1
}
";
    const FSL5_B: &str = "business ExpenseToBe {
  actor Employee
  entity Claim
  process Claim {
    stages Draft, Submitted
    initial Draft
    transition submit Draft -> Submitted by Employee
  }
}
verify { instances Claim = 1 }
";
    const FSL5_R: &str = "refinement Mapping {
  impl ExpenseRequirements
  abs ExpenseToBe
  action submit(c, amount) -> submit(c)
}
";

    fn gated() -> fsl_syntax::ParseOptions {
        fsl_syntax::ParseOptions::with_unstable(
            fsl_syntax::UnstableFeatures::NONE.with(fsl_syntax::UnstableFeature::Fsl5),
        )
    }

    /// The UTF-16 position of the `nth` (0-based) `needle` on `line`.
    fn at(source: &str, line: u32, needle: &str, nth: usize) -> Position {
        let text = source.lines().nth(line as usize).expect("line");
        let byte = text
            .match_indices(needle)
            .nth(nth)
            .unwrap_or_else(|| panic!("{needle} on line {line}"))
            .0;
        Position::new(
            line,
            u32::try_from(text[..byte].encode_utf16().count()).expect("fits"),
        )
    }

    struct Fsl5Workspace {
        root: PathBuf,
        state: ServerState,
        a: Url,
        b: Url,
    }

    impl Fsl5Workspace {
        fn new(name: &str) -> Self {
            let root = fixture_root(name);
            let a = fixture_uri(&root, "a.fsl", FSL5_A);
            let b = fixture_uri(&root, "b.fsl", FSL5_B);
            let r = fixture_uri(&root, "r.fsl", FSL5_R);
            let documents = [(&a, FSL5_A), (&b, FSL5_B), (&r, FSL5_R)]
                .into_iter()
                .map(|(uri, text)| {
                    (
                        uri.clone(),
                        OpenDocument {
                            text: text.to_owned(),
                            version: 1,
                        },
                    )
                })
                .collect();
            Self {
                root,
                state: ServerState {
                    documents,
                    roots: Vec::new(),
                    parse_options: gated(),
                },
                a,
                b,
            }
        }

        fn at(uri: &Url, position: Position) -> serde_json::Value {
            serde_json::json!({"textDocument":{"uri":uri},"position":position})
        }

        fn hover(&self, position: Position) -> String {
            request(
                &self.state,
                "textDocument/hover",
                Self::at(&self.a, position),
            )["contents"]["value"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        }

        /// The rename's edits as `(file name, start)`, sorted.
        fn rename(&self, position: Position, new_name: &str) -> Option<Vec<(String, Position)>> {
            let mut params = Self::at(&self.a, position);
            params["newName"] = serde_json::json!(new_name);
            let edit = request(&self.state, "textDocument/rename", params);
            if edit.is_null() {
                return None;
            }
            let edit: WorkspaceEdit = serde_json::from_value(edit).expect("workspace edit");
            let mut edits = edit
                .changes
                .unwrap_or_default()
                .into_iter()
                .flat_map(|(uri, edits)| {
                    let file = uri.path().rsplit('/').next().unwrap_or_default().to_owned();
                    edits.into_iter().map(move |edit| {
                        assert_eq!(edit.new_text, new_name);
                        (file.clone(), edit.range.start)
                    })
                })
                .collect::<Vec<_>>();
            edits.sort_by_key(|(file, start)| (file.clone(), start.line, start.character));
            Some(edits)
        }
    }

    impl Drop for Fsl5Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// fsl 5 plan S11: an alias navigates to its canonical declaration and
    /// hovers with its glossary row; a keyword hovers with its other
    /// spelling and a particle with its role.
    #[test]
    fn fsl5_aliases_and_keywords_navigate_and_hover() {
        let workspace = Fsl5Workspace::new("fsl5-hover");
        let a = FSL5_A;
        let definition = request(
            &workspace.state,
            "textDocument/definition",
            Fsl5Workspace::at(&workspace.a, at(a, 17, "申請", 0)),
        );
        assert_eq!(definition["uri"], workspace.a.as_str());
        assert_eq!(
            definition["range"]["start"],
            serde_json::to_value(at(a, 13, "申請", 0)).expect("position")
        );
        let alias = workspace.hover(at(a, 17, "申請", 0));
        assert!(
            alias.contains("**type** `Claim`") && alias.contains("glossary `Claim = 申請`"),
            "{alias}"
        );
        let field = workspace.hover(at(a, 17, "申請金額", 0));
        assert!(
            field.contains("**property** `amount`")
                && field.contains("glossary `Claim.amount = 申請金額`"),
            "{field}"
        );
        let japanese = workspace.hover(at(a, 13, "エンティティ", 0));
        assert!(
            japanese.contains("**keyword** `エンティティ`")
                && japanese.contains("`entity` · `エンティティ`"),
            "{japanese}"
        );
        let english = workspace.hover(at(a, 15, "lifecycle", 0));
        assert!(
            english.contains("`lifecycle` · `ライフサイクル`"),
            "{english}"
        );
        let particle = workspace.hover(at(a, 18, "が", 0));
        assert!(
            particle.contains("**particle** `が` marks the actor"),
            "{particle}"
        );
        let with = workspace.hover(at(a, 18, "で", 0));
        assert!(
            with.contains("`with` · `で`") && with.contains("**particle** `で` marks the input"),
            "{with}"
        );
        let verb = workspace.hover(at(a, 18, "提出する", 0));
        assert!(
            verb.starts_with("**alias** `提出する` of `submits`")
                && verb.contains("glossary `submits = 提出する`"),
            "{verb}"
        );
        let canonical_verb = workspace.hover(at(a, 20, "submits", 0));
        assert!(
            canonical_verb.starts_with("**name** `submits`")
                && canonical_verb.contains("glossary `submits = 提出する`"),
            "{canonical_verb}"
        );
        assert_eq!(workspace.hover(at(a, 20, "claim1", 0)), "");
    }

    /// fsl 5 plan S11: the references of a name are its spellings as the
    /// canonical name and as the alias; completion offers both languages'
    /// keywords and the aliases, each with its counterpart as the detail, in
    /// a next-edition requirements document only.
    #[test]
    fn fsl5_references_and_completion_cover_both_spellings() {
        let workspace = Fsl5Workspace::new("fsl5-completion");
        let a = FSL5_A;
        let references = request(
            &workspace.state,
            "textDocument/references",
            serde_json::json!({
                "textDocument":{"uri":workspace.a},
                "position":at(a, 18, "Claim", 0),
                "context":{"includeDeclaration":true}
            }),
        );
        let mut starts = references
            .as_array()
            .expect("references")
            .iter()
            .map(|location| {
                assert_eq!(location["uri"], workspace.a.as_str());
                serde_json::from_value::<Position>(location["range"]["start"].clone())
                    .expect("position")
            })
            .collect::<Vec<_>>();
        starts.sort_by_key(|position| (position.line, position.character));
        assert_eq!(
            starts,
            vec![
                at(a, 3, "Claim", 0),
                at(a, 3, "申請", 0),
                at(a, 4, "Claim", 0),
                at(a, 13, "申請", 0),
                at(a, 17, "申請", 0),
                at(a, 18, "Claim", 0),
                at(a, 25, "Claim", 0),
            ]
        );

        let completion = |uri: &Url| {
            request(
                &workspace.state,
                "textDocument/completion",
                Fsl5Workspace::at(uri, Position::new(1, 0)),
            )
            .as_array()
            .expect("completion items")
            .iter()
            .map(|item| {
                (
                    item["label"].as_str().expect("label").to_owned(),
                    item["detail"].as_str().map(str::to_owned),
                )
            })
            .collect::<Vec<_>>()
        };
        let offered = completion(&workspace.a);
        for (label, detail) in [
            ("エンティティ", "entity"),
            ("rule", "規則"),
            ("条件", "only when / when"),
            ("申請", "Claim"),
            ("申請金額", "Claim.amount"),
            ("提出する", "submits"),
        ] {
            assert!(
                offered.contains(&(label.to_owned(), Some(detail.to_owned()))),
                "{label}: {offered:?}"
            );
        }
        let published = completion(&workspace.b);
        for label in ["エンティティ", "rule", "lifecycle", "申請"] {
            assert!(
                published.iter().all(|(offered, _)| offered != label),
                "{label}: {published:?}"
            );
        }
    }

    /// fsl 5 plan S11: renaming an alias rewrites the glossary's right side
    /// and the alias's occurrences; renaming a canonical name rewrites its
    /// canonical occurrences, the glossary's left side and other files'
    /// mapping tables, never an alias. A new alias is spelled like no keyword
    /// and no name; a new canonical name of the document is ASCII.
    #[test]
    fn fsl5_rename_keeps_aliases_and_canonical_names_apart() {
        let workspace = Fsl5Workspace::new("fsl5-rename");
        let a = FSL5_A;
        let file = |name: &str, position: Position| (name.to_owned(), position);
        assert_eq!(
            workspace.rename(at(a, 17, "申請", 0), "請求"),
            Some(vec![
                file("a.fsl", at(a, 3, "申請", 0)),
                file("a.fsl", at(a, 13, "申請", 0)),
                file("a.fsl", at(a, 17, "申請", 0)),
            ])
        );
        assert_eq!(
            workspace.rename(at(a, 21, "提出済み", 0), "提出後"),
            Some(vec![
                file("a.fsl", at(a, 5, "提出済み", 0)),
                file("a.fsl", at(a, 15, "提出済み", 0)),
                file("a.fsl", at(a, 21, "提出済み", 0)),
            ])
        );
        for taken in [
            "エンティティ",
            "資源",
            "が",
            "entity",
            "Claim",
            "申請金額",
            "Amount",
        ] {
            assert_eq!(
                workspace.rename(at(a, 17, "申請", 0), taken),
                None,
                "{taken}"
            );
        }

        assert_eq!(
            workspace.rename(at(a, 18, "Claim", 0), "Expense"),
            Some(vec![
                file("a.fsl", at(a, 3, "Claim", 0)),
                file("a.fsl", at(a, 4, "Claim", 0)),
                file("a.fsl", at(a, 18, "Claim", 0)),
                file("a.fsl", at(a, 25, "Claim", 0)),
            ])
        );
        for invalid in ["請求", "エンティティ", "lifecycle", "申請"] {
            assert_eq!(
                workspace.rename(at(a, 18, "Claim", 0), invalid),
                None,
                "{invalid}"
            );
        }
        // The event: its declaration, the left side of this document's
        // mapping, and the implementation side of the refinement file — not
        // the right side of either mapping, which names b.fsl's action.
        assert_eq!(
            workspace.rename(at(a, 18, "submit", 0), "file"),
            Some(vec![
                file("a.fsl", at(a, 9, "submit", 0)),
                file("a.fsl", at(a, 18, "submit", 0)),
                file("r.fsl", at(FSL5_R, 3, "submit", 0)),
            ])
        );
        // A verb is declared nowhere: its canonical spellings in the document.
        assert_eq!(
            workspace.rename(at(a, 20, "submits", 0), "files"),
            Some(vec![
                file("a.fsl", at(a, 6, "submits", 0)),
                file("a.fsl", at(a, 20, "submits", 0)),
            ])
        );
    }

    /// The keyword population of a next-edition requirements document is
    /// the keyword table's: every spelling of an active row, in English (each
    /// word of it) and in Japanese, is a keyword that completion offers and a
    /// rename may not take, and so is every particle except that completion
    /// does not offer a particle on its own. A reserved row is not read yet,
    /// so its spellings are names unless an active row or the published
    /// language spells them. A published document reads none of the table's
    /// Japanese spellings and completes exactly the published starters.
    #[test]
    fn next_edition_keyword_population_is_the_keyword_tables() {
        use crate::Lexicon;
        use crate::index::{is_keyword, is_keyword_in};
        use fsl_syntax::keywords::{KEYWORDS, PARTICLES, Status};

        let offered = |lexicon| {
            keyword_completion_items(lexicon)
                .into_iter()
                .map(|item| item.label)
                .collect::<HashSet<_>>()
        };
        let next = offered(Lexicon::Next);
        let active = |word: &str| {
            is_keyword(word)
                || KEYWORDS.iter().any(|row| {
                    row.status == Status::Active
                        && (row.japanese == word || row.english.split(' ').any(|part| part == word))
                })
        };
        let mut reserved_names = 0;
        for row in KEYWORDS {
            let spellings = row
                .english
                .split(' ')
                .chain([row.japanese])
                .collect::<Vec<_>>();
            for word in spellings {
                assert_eq!(
                    is_keyword_in(Lexicon::Next, word),
                    active(word),
                    "{:?} `{word}` ({:?})",
                    row.kw,
                    row.status
                );
                if row.status == Status::Reserved && !active(word) {
                    reserved_names += 1;
                }
            }
            let spelled_by_an_active_row = |label: &str| {
                KEYWORDS.iter().any(|other| {
                    other.status == Status::Active
                        && (other.english == label || other.japanese == label)
                })
            };
            for label in [row.english, row.japanese] {
                assert_eq!(
                    next.contains(label),
                    spelled_by_an_active_row(label) || KEYWORDS_STARTERS.contains(&label),
                    "completion of `{label}` ({:?})",
                    row.status
                );
            }
            assert!(
                !is_keyword_in(Lexicon::Published, row.japanese),
                "{}",
                row.japanese
            );
        }
        // Not vacuous: some reserved spelling is a name today.
        assert!(reserved_names > 0);
        for (_, particle) in PARTICLES {
            assert!(is_keyword_in(Lexicon::Next, particle), "{particle}");
            assert_eq!(
                next.contains(*particle),
                KEYWORDS
                    .iter()
                    .any(|row| row.status == Status::Active && row.japanese == *particle),
                "{particle}"
            );
            assert!(!is_keyword_in(Lexicon::Published, particle), "{particle}");
        }
        let published = offered(Lexicon::Published);
        assert_eq!(
            published,
            KEYWORDS_STARTERS
                .iter()
                .map(|keyword| (*keyword).to_owned())
                .collect::<HashSet<_>>()
        );
    }

    #[test]
    fn imported_definition_references_and_completion_cross_files() {
        let lib = "spec Lib { state { n: Int } init { n = 0 } action bump() { n = n + 1 } }";
        let main = r#"compose Main {
  use Lib as lib from "lib.fsl"
  state { n: Int }
  init { n = 0 }
  action run() = lib.bump() { n = n + 1 }
  internal lib.bump
}"#;
        let (root, lib_uri, main_uri) = fixture_pair("cross-file-import", lib, main);
        let state = ServerState {
            documents: HashMap::from([
                (
                    lib_uri.clone(),
                    OpenDocument {
                        text: lib.to_owned(),
                        version: 1,
                    },
                ),
                (
                    main_uri.clone(),
                    OpenDocument {
                        text: main.to_owned(),
                        version: 1,
                    },
                ),
            ]),
            roots: Vec::new(),
            ..ServerState::default()
        };
        let main_index = index_for_uri(&state, &main_uri).expect("main index");
        let bump = main_index
            .references
            .iter()
            .find(|reference| {
                reference.name == "bump" && reference.qualifier.as_deref() == Some("lib")
            })
            .expect("qualified bump");
        let definition =
            definition_location(&state, &main_uri, bump.range.start).expect("definition");
        assert_eq!(definition.uri, lib_uri);
        let references = workspace_references(&state, &main_uri, bump.range.start, true);
        assert!(references.iter().any(|location| location.uri == lib_uri));
        assert!(
            references
                .iter()
                .filter(|location| location.uri == main_uri)
                .count()
                >= 2
        );

        let cursor = Position::new(4, 25);
        assert_eq!(alias_before_cursor(main, cursor), Some("lib"));
        let target = main_index
            .import_for_alias("lib")
            .and_then(|binding| resolve_import_uri(&main_uri, &binding.path))
            .and_then(|uri| index_for_uri(&state, &uri))
            .expect("import completion index");
        assert!(
            target
                .completion_names()
                .iter()
                .any(|(name, _)| *name == "bump")
        );

        std::fs::remove_dir_all(root).expect("remove cross-file fixture");
    }

    #[test]
    fn external_document_scans_siblings_but_not_the_parent_tree() {
        let root = std::env::temp_dir().join(format!(
            "fsl-lsp-bounded-workspace-scan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        let nested = root.join("unrelated");
        std::fs::create_dir_all(&nested).expect("create scan fixture");
        let current_path = root.join("current.fsl");
        let sibling_path = root.join("sibling.fsl");
        let nested_path = nested.join("must-not-index.fsl");
        std::fs::write(&current_path, "spec Current {}").expect("write current fixture");
        std::fs::write(&sibling_path, "spec Sibling {}").expect("write sibling fixture");
        std::fs::write(&nested_path, "spec Nested {}").expect("write nested fixture");

        let current = Url::from_file_path(&current_path).expect("current URI");
        let sibling = Url::from_file_path(&sibling_path).expect("sibling URI");
        let nested_uri = Url::from_file_path(&nested_path).expect("nested URI");
        let state = ServerState {
            documents: HashMap::from([(
                current.clone(),
                OpenDocument {
                    text: "spec Current {}".to_owned(),
                    version: 1,
                },
            )]),
            roots: Vec::new(),
            ..ServerState::default()
        };

        let external_uris = workspace_uris(&state, &current);
        assert!(external_uris.contains(&current));
        assert!(external_uris.contains(&sibling));
        assert!(!external_uris.contains(&nested_uri));

        let workspace_state = ServerState {
            roots: vec![root.clone()],
            ..state
        };
        assert!(workspace_uris(&workspace_state, &current).contains(&nested_uri));

        std::fs::remove_dir_all(root).expect("remove scan fixture");
    }

    #[test]
    fn protocol_queries_project_one_authoritative_index() {
        let source = "spec Shop {\n  enum Status { Open, Closed }\n  state { ready: Bool }\n  init { ready = false }\n  action flip() { ready = true }\n  invariant safe { ready or not ready }\n}";
        let legacy = "domain Orders { type Status = Pending | Approved aggregate Order { state { status: Status = Pending; } } }";
        let (root, uri, legacy_uri) = fixture_pair("queries", source, legacy);
        let state = ServerState {
            documents: HashMap::from([
                (
                    uri.clone(),
                    OpenDocument {
                        text: source.to_owned(),
                        version: 1,
                    },
                ),
                (
                    legacy_uri.clone(),
                    OpenDocument {
                        text: legacy.to_owned(),
                        version: 1,
                    },
                ),
            ]),
            roots: Vec::new(),
            ..ServerState::default()
        };
        DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("query source must parse");
        let document = serde_json::json!({"textDocument":{"uri":uri}});
        let symbols = request(&state, "textDocument/documentSymbol", document.clone());
        assert!(
            symbols.as_array().is_some_and(|items| !items.is_empty()),
            "symbols response: {symbols}"
        );

        let position = serde_json::json!({
            "textDocument":{"uri":uri},
            "position":{"line":4,"character":18}
        });
        let hover = request(&state, "textDocument/hover", position.clone());
        assert!(
            hover["contents"]["value"]
                .as_str()
                .is_some_and(|value| value.contains("ready"))
        );
        let completion = request(&state, "textDocument/completion", position.clone());
        assert!(
            completion
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["label"] == "flip"))
        );
        let definition = request(&state, "textDocument/definition", position.clone());
        assert_eq!(definition["uri"], uri.as_str());
        assert_eq!(
            definition["range"]["start"],
            serde_json::json!({"line":2,"character":10})
        );
        let references = request(
            &state,
            "textDocument/references",
            serde_json::json!({
                "textDocument":{"uri":uri},
                "position":{"line":4,"character":18},
                "context":{"includeDeclaration":true}
            }),
        );
        assert!(references.as_array().is_some_and(
            |items| items.len() >= 3 && items.iter().all(|item| item["uri"] == uri.as_str())
        ));
        let rename = request(
            &state,
            "textDocument/rename",
            serde_json::json!({
                "textDocument":{"uri":uri},
                "position":{"line":4,"character":18},
                "newName":"enabled"
            }),
        );
        assert!(
            rename["changes"][uri.as_str()]
                .as_array()
                .is_some_and(|edits| edits.len() >= 3)
        );

        let tokens = request(&state, "textDocument/semanticTokens/full", document.clone());
        assert!(
            tokens["data"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
        );
        assert!(
            has_semantic_token(&tokens["data"], 4, 18, 3),
            "ready reference must retain its variable role"
        );
        let actions = request(
            &state,
            "textDocument/codeAction",
            serde_json::json!({
                "textDocument":{"uri":legacy_uri},
                "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":106}},
                "context":{"diagnostics":[]}
            }),
        );
        assert!(actions.as_array().is_some_and(|items| !items.is_empty()));
        std::fs::remove_dir_all(root).expect("remove query fixture");
    }

    #[test]
    fn structural_finding_maps_to_information_diagnostic() {
        let index = DocumentIndex::build(
            "spec Review { state { ready: Bool } init { ready = false } }",
            None,
            &fsl_syntax::ParseOptions::stable(),
        )
        .expect("index");
        let finding = serde_json::json!({
            "finding_id":"finding-1",
            "finding_type":"unwritten_state",
            "formal_status":"not_a_violation",
            "involved_nodes":["state:ready"],
            "why_it_matters":"No action writes this state.",
            "candidate_repairs":[],
            "do_not_assume":[]
        });
        let diagnostic = analysis_diagnostic(
            "spec Review { state { ready: Bool } init { ready = false } }",
            Some(&index),
            &finding,
        );
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(diagnostic.source.as_deref(), Some("fslc analyze"));
        assert_eq!(diagnostic.range.start, Position::new(0, 22));
    }

    /// The `loc` fallback converts the finding's character column to UTF-16
    /// code units: the emoji before the token counts twice.
    #[test]
    fn structural_finding_loc_fallback_counts_utf16_units() {
        let source = "spec Review {\n  invariant I \"😀\" { true }\n}";
        let finding = serde_json::json!({
            "finding_type":"unwritten_state",
            "involved_nodes":["state:absent"],
            "loc":{"line":2,"column":21},
            "why_it_matters":"No action writes this state."
        });
        let diagnostic = analysis_diagnostic(source, None, &finding);
        assert_eq!(diagnostic.range.start, Position::new(1, 21));
    }

    #[test]
    fn unresolved_rename_returns_null() {
        let root = fixture_root("unresolved");
        let uri = fixture_uri(
            &root,
            "unresolved.fsl",
            "spec Empty { state { ready: Bool } init { ready = false } }",
        );
        let state = ServerState {
            documents: HashMap::from([(
                uri.clone(),
                OpenDocument {
                    text: "spec Empty { state { ready: Bool } init { ready = false } }".to_owned(),
                    version: 1,
                },
            )]),
            roots: Vec::new(),
            ..ServerState::default()
        };
        let result = request(
            &state,
            "textDocument/rename",
            serde_json::json!({
                "textDocument":{"uri":uri},
                "position":{"line":0,"character":0},
                "newName":"renamed"
            }),
        );
        assert!(result.is_null());
        std::fs::remove_dir_all(root).expect("remove unresolved fixture");
    }

    #[test]
    fn refinement_targets_and_binders_resolve_without_workspace_guessing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repository root")
            .join("specs");
        let uri = Url::from_file_path(root.join("seat_refines.fsl")).expect("refinement URI");
        let state = ServerState {
            documents: HashMap::new(),
            roots: vec![root],
            ..ServerState::default()
        };
        let index = index_for_uri(&state, &uri).expect("refinement index");

        for (name, target_file) in [
            ("confirm", "seat_booking_impl.fsl"),
            ("book", "seat_booking.fsl"),
            ("seats", "seat_booking.fsl"),
            ("slots", "seat_booking_impl.fsl"),
        ] {
            let reference = index
                .references
                .iter()
                .find(|reference| reference.name == name)
                .unwrap_or_else(|| panic!("missing {name} reference"));
            let location = definition_location(&state, &uri, reference.range.start)
                .unwrap_or_else(|| panic!("unresolved {name}"));
            assert!(
                location.uri.path().ends_with(target_file),
                "{name}: {}",
                location.uri
            );
        }

        let local = index
            .references
            .iter()
            .find(|reference| reference.name == "u")
            .expect("local action parameter use");
        let location = definition_location(&state, &uri, local.range.start).expect("local binder");
        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, local.range.start.line);
    }

    #[test]
    fn refinement_progress_targets_resolve_without_workspace_guessing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repository root")
            .join("examples/refinement_liveness");
        let uri = Url::from_file_path(root.join("design_keeps_liveness_progress_refines.fsl"))
            .expect("refinement URI");
        let state = ServerState {
            documents: HashMap::new(),
            roots: vec![root],
            ..ServerState::default()
        };
        let index = index_for_uri(&state, &uri).expect("refinement index");

        for (name, target_file) in [
            ("EveryClaimDecided", "policy.fsl"),
            ("approve", "design_keeps_liveness.fsl"),
            ("reject", "design_keeps_liveness.fsl"),
        ] {
            let reference = index
                .references
                .iter()
                .find(|reference| reference.name == name && reference.range.start.line == 20)
                .unwrap_or_else(|| panic!("missing progress {name} reference"));
            let location = definition_location(&state, &uri, reference.range.start)
                .unwrap_or_else(|| panic!("unresolved progress {name}"));
            assert!(
                location.uri.path().ends_with(target_file),
                "{name}: {}",
                location.uri
            );
        }
    }
}

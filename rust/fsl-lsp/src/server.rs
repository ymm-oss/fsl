// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};
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
            let uri = &params.text_document_position_params.text_document.uri;
            let result =
                definition_location(state, uri, params.text_document_position_params.position)
                    .and_then(|location| {
                        let index = index_for_uri(state, &location.uri)?;
                        let symbol = index.symbol_at(location.range.start)?;
                        let snippet =
                            source_line(index.source(), symbol.selection_range.start.line);
                        Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!(
                                    "```fsl\n{snippet}\n```\n**{}** `{}`",
                                    symbol.role.detail(),
                                    symbol.name
                                ),
                            }),
                            range: Some(symbol.selection_range),
                        })
                    });
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
                items.extend(KEYWORDS.iter().map(|keyword| CompletionItem {
                    label: (*keyword).to_owned(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    ..CompletionItem::default()
                }));
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
            let uri = &params.text_document_position.text_document.uri;
            let result = valid_identifier(&params.new_name)
                .then(|| {
                    let locations = workspace_references(
                        state,
                        uri,
                        params.text_document_position.position,
                        true,
                    );
                    if locations.is_empty() {
                        return None;
                    }
                    let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
                    for location in locations {
                        changes
                            .entry(location.uri)
                            .or_default()
                            .push(TextEdit::new(location.range, params.new_name.clone()));
                    }
                    Some(WorkspaceEdit::new(changes))
                })
                .flatten();
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
    let (source_diagnostics, model) =
        fslc_rust::source_diagnostic::diagnostics_with_model(&source, source_file, &resolver);
    let mut items = source_diagnostics
        .into_iter()
        .map(|item| to_lsp_diagnostic(&source, item))
        .collect::<Vec<_>>();
    if analysis_diagnostics_enabled()
        && let Some(model) = model
    {
        let index = DocumentIndex::build(&source, Some(source_file)).ok();
        items.extend(
            fsl_tools::structural_review_findings(&fsl_tools::build_tsg(&model))
                .into_iter()
                .map(|finding| analysis_diagnostic(index.as_ref(), &finding)),
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

fn analysis_diagnostics_enabled() -> bool {
    std::env::var("FSLC_LSP_ANALYSIS_DIAGNOSTICS").is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn analysis_diagnostic(index: Option<&DocumentIndex>, finding: &serde_json::Value) -> Diagnostic {
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
            let line = finding["loc"]["line"].as_u64()?.saturating_sub(1);
            let column = finding["loc"]["column"].as_u64()?.saturating_sub(1);
            let start = Position::new(u32::try_from(line).ok()?, u32::try_from(column).ok()?);
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

fn to_lsp_diagnostic(source: &str, item: SourceDiagnostic) -> Diagnostic {
    let severity = if item.kind == "migration" {
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
    diagnostic
}

fn index_for_uri(state: &ServerState, uri: &Url) -> Option<DocumentIndex> {
    let source = source_for_uri(state, uri)?;
    let path = uri.to_file_path().ok();
    DocumentIndex::build(&source, path.as_deref().and_then(Path::to_str)).ok()
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

fn workspace_uris(state: &ServerState, current: &Url) -> Vec<Url> {
    let mut uris = state.documents.keys().cloned().collect::<HashSet<_>>();
    let mut roots = state.roots.clone();
    if let Ok(path) = current.to_file_path()
        && let Some(parent) = path.parent()
    {
        roots.push(parent.to_path_buf());
    }
    for root in roots {
        collect_fsl_files(&root, &mut uris);
    }
    let mut uris = uris.into_iter().collect::<Vec<_>>();
    uris.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    uris
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
    let rewrites = fsl_syntax::canonical_rewrites(&source).ok()?;
    Some(
        rewrites
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
            .collect(),
    )
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

fn valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && !crate::index::is_keyword(value)
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
    use super::*;
    use lsp_server::RequestId;
    use lsp_types::Range;

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
            .chunks_exact(5)
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
    fn rename_identifier_validation_is_closed() {
        assert!(valid_identifier("new_name"));
        assert!(!valid_identifier("9name"));
        assert!(!valid_identifier("state"));
        assert!(!valid_identifier("use"));
        assert!(!valid_identifier("two names"));
    }

    #[test]
    fn imported_definition_references_and_completion_cross_files() {
        let lib_uri = Url::parse("file:///tmp/fsl-lsp-workspace/lib.fsl").expect("lib URI");
        let main_uri = Url::parse("file:///tmp/fsl-lsp-workspace/main.fsl").expect("main URI");
        let lib = "spec Lib { state { n: Int } init { n = 0 } action bump() { n = n + 1 } }";
        let main = r#"compose Main {
  use Lib as lib from "lib.fsl"
  state { n: Int }
  init { n = 0 }
  action run() = lib.bump() { n = n + 1 }
  internal lib.bump
}"#;
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
    }

    #[test]
    fn protocol_queries_project_one_authoritative_index() {
        let uri = Url::parse("file:///tmp/fsl-lsp-queries.fsl").expect("URI");
        let legacy_uri = Url::parse("file:///tmp/fsl-lsp-legacy.fsl").expect("legacy URI");
        let source = "spec Shop {\n  enum Status { Open, Closed }\n  state { ready: Bool }\n  init { ready = false }\n  action flip() { ready = true }\n  invariant safe { ready or not ready }\n}";
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
                        text: "domain Orders { type Status = Pending | Approved aggregate Order { state { status: Status = Pending; } } }".to_owned(),
                        version: 1,
                    },
                ),
            ]),
            roots: Vec::new(),
        };
        DocumentIndex::build(source, None).expect("query source must parse");
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
    }

    #[test]
    fn structural_finding_maps_to_information_diagnostic() {
        let index = DocumentIndex::build(
            "spec Review { state { ready: Bool } init { ready = false } }",
            None,
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
        let diagnostic = analysis_diagnostic(Some(&index), &finding);
        assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(diagnostic.source.as_deref(), Some("fslc analyze"));
        assert_eq!(diagnostic.range.start, Position::new(0, 22));
    }

    #[test]
    fn unresolved_rename_returns_null() {
        let uri = Url::parse("file:///tmp/fsl-lsp-unresolved.fsl").expect("URI");
        let state = ServerState {
            documents: HashMap::from([(
                uri.clone(),
                OpenDocument {
                    text: "spec Empty { state { ready: Bool } init { ready = false } }".to_owned(),
                    version: 1,
                },
            )]),
            roots: Vec::new(),
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

// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeSet, HashSet};
use std::fmt;

use fsl_syntax::{Span, Token, TokenKind};
use lsp_types::{Position, Range};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SymbolRole {
    Namespace,
    Type,
    Function,
    Variable,
    Parameter,
    Property,
}

impl SymbolRole {
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Type => "type",
            Self::Function => "function",
            Self::Variable => "variable",
            Self::Parameter => "parameter",
            Self::Property => "property",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub role: SymbolRole,
    pub range: Range,
    pub selection_range: Range,
    pub(crate) owner: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reference {
    pub name: String,
    pub range: Range,
    pub qualifier: Option<String>,
    pub(crate) owner: Option<String>,
    pub(crate) target_spec: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportBinding {
    pub spec_name: String,
    pub alias: String,
    pub path: String,
    pub alias_range: Range,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentIndex {
    source: String,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub imports: Vec<ImportBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexError(pub String);

impl fmt::Display for IndexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for IndexError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Context {
    Top,
    Action,
    State,
    Struct,
    Enum,
    Other,
}

impl DocumentIndex {
    /// Build an editor projection only after the authoritative Rust frontend accepts the source.
    ///
    /// # Errors
    ///
    /// Returns the compiler parse diagnostic for invalid source.
    #[allow(clippy::too_many_lines)]
    pub fn build(source: &str, _path: Option<&str>) -> Result<Self, IndexError> {
        let refinement = if fslc_rust::frontend_output::is_ai_project(source)
            || fsl_syntax::is_causal_source(source)
        {
            false
        } else {
            matches!(
                fsl_syntax::parse_document(fsl_syntax::SourceFile::new(source))
                    .map_err(|error| IndexError(error.to_string()))?
                    .surface,
                fsl_syntax::SurfaceDocument::Refinement(_)
            )
        };
        let tokens = fsl_syntax::lex(source).map_err(|error| IndexError(error.to_string()))?;
        let mut symbols = Vec::new();
        let mut references = Vec::new();
        let mut contexts: Vec<(Context, Option<String>)> = Vec::new();
        let mut expected: Option<(SymbolRole, Option<Context>)> = None;
        let mut awaiting_block: Option<(Context, Option<String>)> = None;
        let mut declaration_offsets = BTreeSet::new();

        for (index, token) in tokens.iter().enumerate() {
            match &token.kind {
                TokenKind::Ident(name) => {
                    if let Some((role, context)) = expected.take() {
                        add_symbol(source, token, name, role, None, &mut symbols);
                        declaration_offsets.insert(token.span.start.offset);
                        awaiting_block = context.map(|context| {
                            let owner = matches!(context, Context::Action | Context::Other)
                                .then(|| name.clone());
                            (context, owner)
                        });
                        continue;
                    }
                    if let Some((role, context)) = declaration_keyword(name) {
                        expected = role.map(|role| (role, context));
                        if role.is_none() {
                            awaiting_block = context.map(|context| (context, None));
                        }
                        continue;
                    }
                    if is_keyword(name) {
                        continue;
                    }

                    let next_is_colon = token_symbol(tokens.get(index + 1)) == Some(":");
                    let previous = token_ident(index.checked_sub(1).and_then(|i| tokens.get(i)));
                    let context = awaiting_block
                        .as_ref()
                        .map(|(context, _)| *context)
                        .or_else(|| contexts.last().map(|(context, _)| *context));
                    let owner = awaiting_block
                        .as_ref()
                        .and_then(|(_, owner)| owner.clone())
                        .or_else(|| contexts.last().and_then(|(_, owner)| owner.clone()));
                    let enum_member = matches!(context, Some(Context::Enum))
                        && (token_symbol(index.checked_sub(1).and_then(|i| tokens.get(i)))
                            == Some("{")
                            || token_symbol(index.checked_sub(1).and_then(|i| tokens.get(i)))
                                == Some(","));
                    let role = if next_is_colon {
                        match context {
                            Some(Context::Action) => Some(SymbolRole::Parameter),
                            Some(Context::State) => Some(SymbolRole::Variable),
                            Some(Context::Struct) => Some(SymbolRole::Property),
                            _ => None,
                        }
                    } else if enum_member
                        || matches!(previous, Some("as" | "let" | "forall" | "exists"))
                    {
                        Some(SymbolRole::Variable)
                    } else {
                        None
                    };
                    if let Some(role) = role {
                        let scoped = matches!(role, SymbolRole::Parameter)
                            || matches!(previous, Some("as" | "let" | "forall" | "exists"));
                        add_symbol(
                            source,
                            token,
                            name,
                            role,
                            scoped.then_some(owner).flatten(),
                            &mut symbols,
                        );
                        declaration_offsets.insert(token.span.start.offset);
                    } else {
                        references.push(Reference {
                            name: name.clone(),
                            range: span_range(source, token.span),
                            qualifier: qualifier_at(&tokens, index),
                            owner,
                            target_spec: None,
                        });
                    }
                }
                TokenKind::Symbol(symbol) if symbol == "{" => {
                    let inherited = contexts.last().and_then(|(_, owner)| owner.clone());
                    contexts.push(awaiting_block.take().unwrap_or((Context::Other, inherited)));
                }
                TokenKind::Symbol(symbol) if symbol == "}" => {
                    contexts.pop();
                    awaiting_block = None;
                    expected = None;
                }
                TokenKind::Symbol(symbol)
                    if symbol == ";"
                        && !matches!(awaiting_block, Some((Context::Action | Context::Top, _))) =>
                {
                    awaiting_block = None;
                }
                _ => {}
            }
        }

        references.retain(|reference| {
            !declaration_offsets.contains(&offset_at_position(source, reference.range.start))
        });
        if refinement {
            apply_refinement_hints(source, &tokens, &mut symbols, &mut references);
        }
        let imports = import_bindings(source, &tokens);
        Ok(Self {
            source: source.to_owned(),
            symbols,
            references,
            imports,
        })
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn symbol_at(&self, position: Position) -> Option<&Symbol> {
        self.symbols
            .iter()
            .find(|symbol| contains(symbol.selection_range, position))
    }

    #[must_use]
    pub fn reference_at(&self, position: Position) -> Option<&Reference> {
        self.references
            .iter()
            .find(|reference| contains(reference.range, position))
    }

    #[must_use]
    pub fn definition_at(&self, position: Position) -> Option<&Symbol> {
        if let Some(symbol) = self.symbol_at(position) {
            return Some(symbol);
        }
        let reference = self.reference_at(position)?;
        self.symbols
            .iter()
            .filter(|symbol| symbol.name == reference.name)
            .max_by_key(|symbol| {
                usize::from(symbol.owner == reference.owner) * 2
                    + usize::from(symbol.owner.is_none())
            })
    }

    #[must_use]
    pub fn references_at(&self, position: Position, include_declaration: bool) -> Vec<Range> {
        let Some(target) = self.definition_at(position) else {
            return Vec::new();
        };
        let mut ranges = self
            .references
            .iter()
            .filter(|reference| self.definition_at(reference.range.start) == Some(target))
            .map(|reference| reference.range)
            .collect::<Vec<_>>();
        if include_declaration {
            ranges.push(target.selection_range);
        }
        ranges.sort_by_key(|range| (range.start.line, range.start.character));
        ranges.dedup();
        ranges
    }

    #[must_use]
    pub fn completion_names(&self) -> Vec<(&str, SymbolRole)> {
        let mut values = self
            .symbols
            .iter()
            .map(|symbol| (symbol.name.as_str(), symbol.role))
            .collect::<Vec<_>>();
        values.sort_unstable();
        values.dedup();
        values
    }

    #[must_use]
    pub fn import_for_alias(&self, alias: &str) -> Option<&ImportBinding> {
        self.imports.iter().find(|binding| binding.alias == alias)
    }

    /// Return non-keyword identifiers that have neither declaration nor reference coverage.
    #[must_use]
    pub fn unindexed_identifiers(&self) -> Vec<String> {
        let covered = self
            .symbols
            .iter()
            .map(|symbol| {
                (
                    symbol.selection_range.start.line,
                    symbol.selection_range.start.character,
                )
            })
            .chain(
                self.references
                    .iter()
                    .map(|reference| (reference.range.start.line, reference.range.start.character)),
            )
            .collect::<HashSet<_>>();
        fsl_syntax::lex(&self.source).map_or_else(
            |_| Vec::new(),
            |tokens| {
                tokens
                    .into_iter()
                    .filter_map(|token| {
                        let TokenKind::Ident(name) = &token.kind else {
                            return None;
                        };
                        let position = span_range(&self.source, token.span).start;
                        (!is_keyword(name)
                            && !covered.contains(&(position.line, position.character)))
                        .then(|| format!("{}:{}:{name}", position.line + 1, position.character + 1))
                    })
                    .collect()
            },
        )
    }
}

fn apply_refinement_hints(
    source: &str,
    tokens: &[Token],
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<Reference>,
) {
    let spec_name = |keyword: &str| {
        tokens.windows(2).find_map(|pair| {
            (token_ident(pair.first()) == Some(keyword))
                .then(|| token_ident(pair.get(1)))
                .flatten()
                .map(str::to_owned)
        })
    };
    let implementation = spec_name("impl");
    let abstraction = spec_name("abs");
    let specs = (implementation.as_ref(), abstraction.as_ref());
    for (index, token) in tokens.iter().enumerate() {
        match token_ident(Some(token)) {
            Some(k @ ("impl" | "abs")) => set_spec_ref(source, tokens, index, k, specs, references),
            Some("map") => {
                let end = refinement_item_end(tokens, index + 1);
                let owner = format!("map:{}", token.span.start.offset);
                if let Some(target) = tokens.get(index + 1) {
                    update_reference(source, target, None, abstraction.clone(), references);
                }
                let equals = (index + 1..end)
                    .find(|position| token_symbol(tokens.get(*position)) == Some("="));
                let binder = (index + 1..equals.unwrap_or(end)).find(|position| {
                    token_symbol(tokens.get(position + 1)) == Some(":")
                        && token_ident(tokens.get(*position)).is_some()
                });
                if let Some(position) = binder {
                    promote_local_symbol(source, &tokens[position], &owner, symbols, references);
                }
                if let Some(equals) = equals {
                    for item in tokens.iter().take(end).skip(equals + 1) {
                        let Some(name) = token_ident(Some(item)) else {
                            continue;
                        };
                        let local = binder.and_then(|position| token_ident(tokens.get(position)))
                            == Some(name);
                        update_reference(
                            source,
                            item,
                            local.then(|| owner.clone()),
                            (!local).then(|| implementation.clone()).flatten(),
                            references,
                        );
                    }
                }
            }
            Some("action") => {
                let end = refinement_item_end(tokens, index + 1);
                let owner = format!("action:{}", token.span.start.offset);
                if let Some(name) = tokens.get(index + 1) {
                    demote_to_reference(
                        source,
                        name,
                        None,
                        implementation.clone(),
                        symbols,
                        references,
                    );
                }
                let arrow = (index + 1..end)
                    .find(|position| token_symbol(tokens.get(*position)) == Some("->"));
                let open = (index + 1..arrow.unwrap_or(end))
                    .find(|position| token_symbol(tokens.get(*position)) == Some("("));
                let close = open.and_then(|open| {
                    (open + 1..arrow.unwrap_or(end))
                        .find(|position| token_symbol(tokens.get(*position)) == Some(")"))
                });
                if let (Some(open), Some(close)) = (open, close) {
                    for item in tokens.iter().take(close).skip(open + 1) {
                        if token_ident(Some(item)).is_some() {
                            promote_local_symbol(source, item, &owner, symbols, references);
                        }
                    }
                }
                if let Some(arrow) = arrow {
                    if let Some(target) = tokens.get(arrow + 1)
                        && token_ident(Some(target)) != Some("stutter")
                    {
                        update_reference(source, target, None, abstraction.clone(), references);
                    }
                    for item in tokens.iter().take(end).skip(arrow + 2) {
                        if token_ident(Some(item)).is_some() {
                            update_reference(source, item, Some(owner.clone()), None, references);
                        }
                    }
                }
            }
            Some("respond") => {
                apply_progress_response_hint(
                    source,
                    tokens,
                    index,
                    abstraction.clone(),
                    implementation.as_ref(),
                    references,
                );
            }
            _ => {}
        }
    }
}

fn set_spec_ref(
    source: &str,
    tokens: &[Token],
    index: usize,
    keyword: &str,
    specs: (Option<&String>, Option<&String>),
    references: &mut [Reference],
) {
    if let Some(token) = tokens.get(index + 1) {
        let target_spec = if keyword == "impl" { specs.0 } else { specs.1 };
        update_reference(source, token, None, target_spec.cloned(), references);
    }
}

fn apply_progress_response_hint(
    source: &str,
    tokens: &[Token],
    index: usize,
    abstraction: Option<String>,
    implementation: Option<&String>,
    references: &mut [Reference],
) {
    let end = progress_response_end(tokens, index + 1);
    if let Some(property) = tokens.get(index + 1) {
        update_reference(source, property, None, abstraction, references);
    }
    let Some(by) =
        (index + 2..end).find(|position| token_ident(tokens.get(*position)) == Some("by"))
    else {
        return;
    };
    for action in tokens.iter().take(end).skip(by + 1) {
        if token_ident(Some(action)).is_some() {
            update_reference(source, action, None, implementation.cloned(), references);
        }
    }
}

fn progress_response_end(tokens: &[Token], start: usize) -> usize {
    (start..tokens.len())
        .find(|index| {
            token_ident(tokens.get(*index)) == Some("respond")
                || token_symbol(tokens.get(*index)) == Some("}")
        })
        .unwrap_or(tokens.len())
}

fn refinement_item_end(tokens: &[Token], start: usize) -> usize {
    (start..tokens.len())
        .find(|index| {
            *index > start
                && matches!(
                    token_ident(tokens.get(*index)),
                    Some("impl" | "abs" | "map" | "action" | "preserve" | "progress")
                )
        })
        .unwrap_or(tokens.len())
}

fn update_reference(
    source: &str,
    token: &Token,
    owner: Option<String>,
    target_spec: Option<String>,
    references: &mut [Reference],
) {
    let range = span_range(source, token.span);
    if let Some(reference) = references
        .iter_mut()
        .find(|reference| reference.range == range)
    {
        reference.owner = owner;
        reference.target_spec = target_spec;
    }
}

fn promote_local_symbol(
    source: &str,
    token: &Token,
    owner: &str,
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<Reference>,
) {
    let range = span_range(source, token.span);
    references.retain(|reference| reference.range != range);
    if !symbols.iter().any(|symbol| symbol.selection_range == range)
        && let Some(name) = token_ident(Some(token))
    {
        add_symbol(
            source,
            token,
            name,
            SymbolRole::Parameter,
            Some(owner.to_owned()),
            symbols,
        );
    }
}

fn demote_to_reference(
    source: &str,
    token: &Token,
    owner: Option<String>,
    target_spec: Option<String>,
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<Reference>,
) {
    let range = span_range(source, token.span);
    symbols.retain(|symbol| symbol.selection_range != range);
    if let Some(name) = token_ident(Some(token))
        && !references.iter().any(|reference| reference.range == range)
    {
        references.push(Reference {
            name: name.to_owned(),
            range,
            qualifier: None,
            owner,
            target_spec,
        });
    }
}

fn qualifier_at(tokens: &[Token], index: usize) -> Option<String> {
    if token_symbol(index.checked_sub(1).and_then(|i| tokens.get(i))) != Some(".") {
        return None;
    }
    token_ident(index.checked_sub(2).and_then(|i| tokens.get(i))).map(str::to_owned)
}

fn import_bindings(source: &str, tokens: &[Token]) -> Vec<ImportBinding> {
    let mut bindings = Vec::new();
    for index in 0..tokens.len().saturating_sub(5) {
        if token_ident(tokens.get(index)) != Some("use") {
            continue;
        }
        let Some(spec_name) = token_ident(tokens.get(index + 1)) else {
            continue;
        };
        if token_ident(tokens.get(index + 2)) != Some("as") {
            continue;
        }
        let Some(alias) = token_ident(tokens.get(index + 3)) else {
            continue;
        };
        if token_ident(tokens.get(index + 4)) != Some("from") {
            continue;
        }
        let Some(Token {
            kind: TokenKind::String(path),
            ..
        }) = tokens.get(index + 5)
        else {
            continue;
        };
        bindings.push(ImportBinding {
            spec_name: spec_name.to_owned(),
            alias: alias.to_owned(),
            path: path.to_owned(),
            alias_range: span_range(source, tokens[index + 3].span),
        });
    }
    bindings
}

fn declaration_keyword(value: &str) -> Option<(Option<SymbolRole>, Option<Context>)> {
    let declaration = match value {
        "spec" | "compose" | "requirements" | "business" | "governance" | "refinement"
        | "domain" | "dbsystem" | "ai_component" | "agent" | "causal" => {
            (Some(SymbolRole::Namespace), Some(Context::Top))
        }
        "type" | "number" | "entity" => (Some(SymbolRole::Type), None),
        "enum" => (Some(SymbolRole::Type), Some(Context::Enum)),
        "struct" | "table" => (Some(SymbolRole::Type), Some(Context::Struct)),
        "action" | "transition" | "tool" | "command" | "effect" | "migration" | "decide"
        | "evolve" => (Some(SymbolRole::Function), Some(Context::Action)),
        "invariant" | "trans" | "reachable" | "until" | "unless" | "leadsTo" | "property"
        | "requirement" | "acceptance" | "forbidden" | "control" | "policy" | "goal" | "claim"
        | "expectation" => (Some(SymbolRole::Property), Some(Context::Other)),
        "const" | "actor" | "process" | "kpi" | "authority" | "aggregate" | "projection"
        | "environment" | "artifact" | "column" | "variable" => {
            (Some(SymbolRole::Variable), Some(Context::Other))
        }
        "state" => (None, Some(Context::State)),
        "init" | "verify" => (None, Some(Context::Other)),
        _ => return None,
    };
    Some(declaration)
}

const INDEX_KEYWORDS: &[&str] = &[
    "use",
    "as",
    "from",
    "internal",
    "symmetric",
    "fair",
    "requires",
    "ensures",
    "let",
    "if",
    "then",
    "else",
    "forall",
    "exists",
    "in",
    "terminal",
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
    "preservation",
    "before",
    "after",
    "checked_by",
    "Int",
    "Bool",
    "Map",
    "Set",
    "Seq",
    "Option",
    "true",
    "false",
    "none",
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
];

pub(crate) fn is_keyword(value: &str) -> bool {
    declaration_keyword(value).is_some() || INDEX_KEYWORDS.contains(&value)
}

fn add_symbol(
    source: &str,
    token: &Token,
    name: &str,
    role: SymbolRole,
    owner: Option<String>,
    symbols: &mut Vec<Symbol>,
) {
    let range = span_range(source, token.span);
    symbols.push(Symbol {
        name: name.to_owned(),
        role,
        range,
        selection_range: range,
        owner,
    });
}

fn token_ident(token: Option<&Token>) -> Option<&str> {
    match token.map(|token| &token.kind) {
        Some(TokenKind::Ident(value)) => Some(value),
        _ => None,
    }
}

fn token_symbol(token: Option<&Token>) -> Option<&str> {
    match token.map(|token| &token.kind) {
        Some(TokenKind::Symbol(value)) => Some(value),
        _ => None,
    }
}

#[must_use]
pub fn span_range(source: &str, span: Span) -> Range {
    Range::new(
        position_at_offset(source, span.start.offset),
        position_at_offset(source, span.end.offset),
    )
}

fn position_at_offset(source: &str, offset: usize) -> Position {
    let mut line = 0_u32;
    let mut character = 0_u32;
    for ch in source[..offset.min(source.len())].chars() {
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += u32::try_from(ch.len_utf16()).expect("UTF-16 width fits u32");
        }
    }
    Position::new(line, character)
}

fn offset_at_position(source: &str, position: Position) -> usize {
    let mut line = 0_u32;
    let mut character = 0_u32;
    for (offset, ch) in source.char_indices() {
        if line == position.line && character == position.character {
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

fn contains(range: Range, position: Position) -> bool {
    (position.line, position.character) >= (range.start.line, range.start.character)
        && (position.line, position.character) < (range.end.line, range.end.character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_authoritatively_parsed_declarations_and_references() {
        let source = r"spec Cart {
  type Item = 0..1
  state { stock: Int }
  init { stock = 0 }
  action add(item: Item) { stock = stock + 1 }
  invariant NonNegative { stock >= 0 }
}";
        let index = DocumentIndex::build(source, Some("cart.fsl")).expect("valid index");
        assert!(index.symbols.iter().any(|symbol| symbol.name == "Cart"));
        assert!(index.symbols.iter().any(|symbol| symbol.name == "stock"));
        assert!(
            index
                .references
                .iter()
                .filter(|reference| reference.name == "stock")
                .count()
                >= 3
        );
    }

    #[test]
    fn rejects_source_rejected_by_the_authoritative_parser() {
        let error = DocumentIndex::build("spec Broken { state {", Some("broken.fsl"))
            .expect_err("invalid syntax must not be indexed");
        assert!(!error.0.is_empty());
    }

    #[test]
    fn converts_source_offsets_to_utf16_ranges() {
        let ascii_after_non_bmp = "// 😀\nspec Cafe { state { value: Int } }";
        let value = fsl_syntax::lex(ascii_after_non_bmp)
            .expect("lex")
            .into_iter()
            .find(|token| token_ident(Some(token)) == Some("value"))
            .expect("value token");
        assert_eq!(
            span_range(ascii_after_non_bmp, value.span).start,
            Position::new(1, 20)
        );
    }

    #[test]
    fn resolves_same_named_parameters_inside_their_own_action() {
        let source = r"spec Scoped {
  state { value: Int }
  init { value = 0 }
  action first(value: Int) { value = value }
  action second(value: Int) { value = value }
}";
        let index = DocumentIndex::build(source, None).expect("valid source");
        let second_reference = index
            .references
            .iter()
            .rfind(|reference| reference.name == "value" && reference.range.start.line == 4)
            .expect("second action reference");
        let definition = index
            .definition_at(second_reference.range.start)
            .expect("scoped definition");
        assert_eq!(definition.role, SymbolRole::Parameter);
        assert_eq!(definition.selection_range.start.line, 4);
        assert!(
            index
                .references_at(second_reference.range.start, true)
                .iter()
                .all(|range| range.start.line == 4)
        );
    }
}

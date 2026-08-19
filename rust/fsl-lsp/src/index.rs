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
    refinement: bool,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeclarationKeyword {
    value: &'static str,
    role: Option<SymbolRole>,
    context: Option<Context>,
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
        let mut list_role: Option<(SymbolRole, Option<Context>)> = None;
        let mut in_annotation = false;
        let mut declaration_offsets = BTreeSet::new();

        for (index, token) in tokens.iter().enumerate() {
            match &token.kind {
                TokenKind::Ident(name) => {
                    // `@name(args)` is an annotation, never a declaration. Its
                    // name path collides head-on with `declaration_keyword`
                    // (`@requirement("R", "t")` against the real
                    // `requirement NAME { ... }` form), and its symbol-path
                    // arguments sit exactly where the binder and enum-member
                    // heuristics fire, so no identifier from `@` through the
                    // closing `)` may start a declaration or consume a pending
                    // one. Names that are not keywords stay references, which
                    // is what `unindexed_identifiers` requires of them.
                    if !in_annotation {
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
                        // `reachable` and `domain` each name both a top-level
                        // declaration keyword (`reachable NAME { expr }`,
                        // `domain SpecName { ... }`) and a relation builtin
                        // call (`reachable(r, a, b)`, `domain(r)`). Only the
                        // declaration form starts a new declaration; a
                        // following `(` is always the builtin call, which is a
                        // keyword like every other builtin and owns no local
                        // name.
                        let builtin_call = matches!(name.as_str(), "reachable" | "domain")
                            && token_symbol(tokens.get(index + 1)) == Some("(");
                        if !builtin_call && let Some((role, context)) = declaration_keyword(name) {
                            expected = role.map(|role| (role, context));
                            list_role = (name == "actor").then_some(expected).flatten();
                            if role.is_none() {
                                awaiting_block = context.map(|context| (context, None));
                            }
                            continue;
                        }
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
                    // A binder introduced by `forall`/`exists`, or the first
                    // parameter of an aggregate call (`count(c: T where ...)`,
                    // `sum(...)`, `unique(...)`, `exactlyOne(...)`), declares a
                    // local name regardless of the enclosing declaration
                    // context and regardless of whether it uses the `name: T`
                    // or `name in ...` binder form.
                    let binder_after_aggregate_paren =
                        token_symbol(index.checked_sub(1).and_then(|i| tokens.get(i))) == Some("(")
                            && matches!(
                                token_ident(index.checked_sub(2).and_then(|i| tokens.get(i))),
                                Some("count" | "sum" | "unique" | "exactlyOne")
                            );
                    let quantifier_binder = matches!(previous, Some("forall" | "exists"))
                        || binder_after_aggregate_paren;
                    // `x is some(v)` binds `v` for the rest of the guarded
                    // expression, the same as a `let`.
                    let pattern_binder =
                        token_symbol(index.checked_sub(1).and_then(|i| tokens.get(i))) == Some("(")
                            && token_ident(index.checked_sub(2).and_then(|i| tokens.get(i)))
                                == Some("some")
                            && token_ident(index.checked_sub(3).and_then(|i| tokens.get(i)))
                                == Some("is");
                    let role = if in_annotation {
                        None
                    } else if quantifier_binder || pattern_binder {
                        Some(SymbolRole::Variable)
                    } else if next_is_colon {
                        match context {
                            Some(Context::Action) => Some(SymbolRole::Parameter),
                            Some(Context::State) => Some(SymbolRole::Variable),
                            Some(Context::Struct) => Some(SymbolRole::Property),
                            _ => None,
                        }
                    } else if enum_member || matches!(previous, Some("as" | "let")) {
                        Some(SymbolRole::Variable)
                    } else {
                        None
                    };
                    if let Some(role) = role {
                        let scoped = matches!(role, SymbolRole::Parameter)
                            || quantifier_binder
                            || pattern_binder
                            || matches!(previous, Some("as" | "let"));
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
                // `@` opens an annotation and its closing `)` ends it.
                // `annotation_parse::annotation` is the one grammar every
                // dialect uses (`parser.rs`, `domain.rs`, `db.rs`, `ai.rs`):
                // `@` path `(` args `)`, where `(` is mandatory and an
                // argument is a string, integer, Boolean, or dotted symbol
                // path — never a parenthesized expression. With no nesting
                // possible the first `)` always closes the annotation.
                TokenKind::Symbol(symbol) if symbol == "@" => {
                    in_annotation = true;
                }
                TokenKind::Symbol(symbol) if symbol == ")" => {
                    in_annotation = false;
                }
                TokenKind::Symbol(symbol) if symbol == "," => {
                    // `actor A, B` continues one declaration list, but the
                    // parser also ends the list on a trailing comma followed
                    // by the next item keyword (`actor A, entity Case`), so
                    // only a non-keyword name re-arms the pending role. A
                    // comma between annotation arguments separates no list.
                    let continues_list = !in_annotation
                        && token_ident(tokens.get(index + 1)).is_some_and(|next| !is_keyword(next));
                    if continues_list && let Some(pending) = list_role {
                        expected = Some(pending);
                    }
                }
                TokenKind::Symbol(symbol) if symbol == "{" => {
                    let inherited = contexts.last().and_then(|(_, owner)| owner.clone());
                    contexts.push(awaiting_block.take().unwrap_or((Context::Other, inherited)));
                    list_role = None;
                }
                TokenKind::Symbol(symbol) if symbol == "}" => {
                    contexts.pop();
                    awaiting_block = None;
                    expected = None;
                    list_role = None;
                }
                TokenKind::Symbol(symbol)
                    if symbol == ";"
                        && !matches!(awaiting_block, Some((Context::Action | Context::Top, _))) =>
                {
                    awaiting_block = None;
                    list_role = None;
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
            refinement,
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

    /// Return declaration-name positions the index failed to declare.
    ///
    /// A declaration keyword that owns a name (`action`, `invariant`, `entity`,
    /// `command`, ...) is followed in the grammar by that name, so whenever the
    /// very next token is an identifier the index must have declared a symbol
    /// there. `unindexed_identifiers` cannot see this: a swallowed name is
    /// still indexed, just as a reference to nothing, and a keyword registered
    /// as a symbol in its place is still an entry. Positions where the next
    /// token is not an identifier are skipped, because the keyword is then not
    /// introducing a name — `reachable(r, a, b)` and `domain(r)` are relation
    /// builtin calls, `@requirement("R", "t")` is an annotation, and
    /// `until`/`leadsTo` open a block.
    ///
    /// `action` is skipped in a `refinement` document for the same reason:
    /// there `action impl_name(args) -> abs_name` maps an implementation
    /// action onto an abstract one, so the name is a cross-spec reference and
    /// `apply_refinement_hints` demotes it on purpose.
    #[must_use]
    pub fn misprojected_declarations(&self) -> Vec<String> {
        let declared = self
            .symbols
            .iter()
            .map(|symbol| {
                (
                    symbol.selection_range.start.line,
                    symbol.selection_range.start.character,
                )
            })
            .collect::<HashSet<_>>();
        fsl_syntax::lex(&self.source).map_or_else(
            |_| Vec::new(),
            |tokens| {
                tokens
                    .iter()
                    .enumerate()
                    .filter_map(|(index, token)| {
                        let TokenKind::Ident(keyword) = &token.kind else {
                            return None;
                        };
                        if self.refinement && keyword == "action" {
                            return None;
                        }
                        declaration_keyword(keyword)?.0?;
                        let name = tokens.get(index + 1)?;
                        let TokenKind::Ident(name_text) = &name.kind else {
                            return None;
                        };
                        let position = span_range(&self.source, name.span).start;
                        (!declared.contains(&(position.line, position.character))).then(|| {
                            format!(
                                "{}:{}: `{keyword} {name_text}` declares nothing",
                                position.line + 1,
                                position.character + 1
                            )
                        })
                    })
                    .collect()
            },
        )
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

const DECLARATION_KEYWORDS: &[DeclarationKeyword] = &[
    // Top-level declarations.
    DeclarationKeyword {
        value: "spec",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "compose",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "requirements",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "business",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "governance",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "refinement",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "domain",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "dbsystem",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "ai_component",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "agent",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    DeclarationKeyword {
        value: "causal",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Top),
    },
    // Type declarations.
    DeclarationKeyword {
        value: "type",
        role: Some(SymbolRole::Type),
        context: None,
    },
    DeclarationKeyword {
        value: "number",
        role: Some(SymbolRole::Type),
        context: None,
    },
    DeclarationKeyword {
        value: "entity",
        role: Some(SymbolRole::Type),
        context: None,
    },
    DeclarationKeyword {
        value: "enum",
        role: Some(SymbolRole::Type),
        context: Some(Context::Enum),
    },
    DeclarationKeyword {
        value: "struct",
        role: Some(SymbolRole::Type),
        context: Some(Context::Struct),
    },
    DeclarationKeyword {
        value: "table",
        role: Some(SymbolRole::Type),
        context: Some(Context::Struct),
    },
    // Action declarations.
    DeclarationKeyword {
        value: "action",
        role: Some(SymbolRole::Function),
        context: Some(Context::Action),
    },
    DeclarationKeyword {
        value: "transition",
        role: Some(SymbolRole::Function),
        context: Some(Context::Action),
    },
    DeclarationKeyword {
        value: "tool",
        role: Some(SymbolRole::Function),
        context: Some(Context::Action),
    },
    DeclarationKeyword {
        value: "command",
        role: Some(SymbolRole::Function),
        context: Some(Context::Action),
    },
    DeclarationKeyword {
        value: "effect",
        role: Some(SymbolRole::Function),
        context: Some(Context::Action),
    },
    DeclarationKeyword {
        value: "migration",
        role: Some(SymbolRole::Function),
        context: Some(Context::Action),
    },
    DeclarationKeyword {
        value: "decide",
        role: Some(SymbolRole::Function),
        context: Some(Context::Action),
    },
    DeclarationKeyword {
        value: "evolve",
        role: Some(SymbolRole::Function),
        context: Some(Context::Action),
    },
    DeclarationKeyword {
        value: "def",
        role: Some(SymbolRole::Function),
        context: Some(Context::Action),
    },
    // Property declarations.
    DeclarationKeyword {
        value: "invariant",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "trans",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "reachable",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "until",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "unless",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "leadsTo",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "property",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "requirement",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "acceptance",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "forbidden",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "control",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "policy",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "goal",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "claim",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "expectation",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    // Variable declarations.
    DeclarationKeyword {
        value: "const",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "actor",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "process",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "kpi",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "authority",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "aggregate",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "projection",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "environment",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "artifact",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "column",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "variable",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
    // Context-only declarations.
    DeclarationKeyword {
        value: "preservation",
        role: Some(SymbolRole::Namespace),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "state",
        role: None,
        context: Some(Context::State),
    },
    DeclarationKeyword {
        value: "init",
        role: None,
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "verify",
        role: None,
        context: Some(Context::Other),
    },
];

fn declaration_keyword(value: &str) -> Option<(Option<SymbolRole>, Option<Context>)> {
    DECLARATION_KEYWORDS
        .iter()
        .find(|keyword| keyword.value == value)
        .map(|keyword| (keyword.role, keyword.context))
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
    "where",
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

#[cfg(test)]
pub(crate) fn recognized_keywords() -> impl Iterator<Item = &'static str> {
    DECLARATION_KEYWORDS
        .iter()
        .map(|keyword| keyword.value)
        .chain(INDEX_KEYWORDS.iter().copied())
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
    fn declaration_keyword_registry_preserves_the_pre_refactor_mapping() {
        // These expected role/context pairs are transcribed from
        // origin/main's declaration_keyword match, not derived from the table.
        let expected = [
            (
                &[
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
                    "causal",
                ][..],
                Some(SymbolRole::Namespace),
                Some(Context::Top),
            ),
            (
                &["type", "number", "entity"][..],
                Some(SymbolRole::Type),
                None,
            ),
            (&["enum"][..], Some(SymbolRole::Type), Some(Context::Enum)),
            (
                &["struct", "table"][..],
                Some(SymbolRole::Type),
                Some(Context::Struct),
            ),
            (
                &[
                    "action",
                    "transition",
                    "tool",
                    "command",
                    "effect",
                    "migration",
                    "decide",
                    "evolve",
                    "def",
                ][..],
                Some(SymbolRole::Function),
                Some(Context::Action),
            ),
            (
                &[
                    "invariant",
                    "trans",
                    "reachable",
                    "until",
                    "unless",
                    "leadsTo",
                    "property",
                    "requirement",
                    "acceptance",
                    "forbidden",
                    "control",
                    "policy",
                    "goal",
                    "claim",
                    "expectation",
                ][..],
                Some(SymbolRole::Property),
                Some(Context::Other),
            ),
            (
                &[
                    "const",
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
                ][..],
                Some(SymbolRole::Variable),
                Some(Context::Other),
            ),
            (
                &["preservation"][..],
                Some(SymbolRole::Namespace),
                Some(Context::Other),
            ),
            (&["state"][..], None, Some(Context::State)),
            (&["init", "verify"][..], None, Some(Context::Other)),
        ];
        let expected_count = expected
            .iter()
            .map(|(keywords, _, _)| keywords.len())
            .sum::<usize>();
        assert_eq!(DECLARATION_KEYWORDS.len(), expected_count);

        for (keywords, role, context) in expected {
            for keyword in keywords {
                assert_eq!(
                    declaration_keyword(keyword),
                    Some((role, context)),
                    "{keyword} changed from its origin/main declaration mapping"
                );
            }
        }
        assert!(recognized_keywords().all(is_keyword));
    }

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

    /// Issue #504 evidence: the minimal accepted `spec` probing `def`, an
    /// aggregate binder, an `is some(v)` pattern binder, and the
    /// `reachable`/`domain` relation builtins.
    const ISSUE_504_PROBE: &str = r"spec Probe {
  type Id = 0..1
  def eligible(x: Id) = x == 0
  state { maybe: Option<Id>, edge: relation Id -> Id }
  invariant UsesDef { eligible(0) }
  invariant Agg { count(c: Id where c == 0) >= 0 }
  invariant Pattern { maybe is some(v) and v == 0 }
  invariant Relation { reachable(edge, 0, 1) and domain(edge).contains(0) }
}";

    /// `def eligible(x: Id) = x == 0` must declare `eligible` as a navigable
    /// `Function` symbol and `x` as a `Parameter` scoped to it, and the call
    /// `eligible(0)` and the body's `x == 0` must resolve back to them.
    /// Before this fix, `def` was entirely absent from `declaration_keyword`,
    /// so `eligible`/`x` were indexed as ordinary references with no
    /// declaration to resolve to (`definition_at` returned `None`).
    #[test]
    fn def_declares_a_function_symbol_with_a_scoped_parameter() {
        let index = DocumentIndex::build(ISSUE_504_PROBE, Some("probe.fsl")).expect("valid index");
        let call_site = Position::new(4, 22); // `eligible(0)` inside UsesDef
        let definition = index
            .definition_at(call_site)
            .expect("eligible(0) must resolve");
        assert_eq!(definition.name, "eligible");
        assert_eq!(definition.role, SymbolRole::Function);

        let param_use = Position::new(2, 24); // the `x` inside `x == 0`
        let param_definition = index
            .definition_at(param_use)
            .expect("the parameter use of x must resolve");
        assert_eq!(param_definition.role, SymbolRole::Parameter);
        assert_eq!(param_definition.owner.as_deref(), Some("eligible"));
        let param_references = index.references_at(Position::new(2, 15), true);
        assert_eq!(param_references.len(), 2, "{param_references:?}");
    }

    /// `count(c: Id where c == 0)` must declare `c` as a `Variable` binder
    /// scoped to the enclosing invariant, resolvable from both `where c ==
    /// 0`. Before this fix, a `name: Type` binder only got a role inside
    /// `Context::Action`/`Context::State`/`Context::Struct`; inside an
    /// invariant's `Context::Other` it fell through to `None` and was
    /// indexed as a plain reference with no declaration.
    #[test]
    fn aggregate_binder_declares_a_scoped_variable() {
        let index = DocumentIndex::build(ISSUE_504_PROBE, Some("probe.fsl")).expect("valid index");
        let binder_use = Position::new(5, 36); // `c` inside `where c == 0`
        let definition = index
            .definition_at(binder_use)
            .expect("the aggregate binder use of c must resolve");
        assert_eq!(definition.name, "c");
        assert_eq!(definition.role, SymbolRole::Variable);
        assert_eq!(definition.owner.as_deref(), Some("Agg"));
    }

    /// A `forall`/`exists` binder must declare its name as a `Variable`
    /// scoped to the enclosing declaration in the `name: Type` form too, not
    /// only the `name in ...` form. Before this fix the typed form was
    /// matched first by `next_is_colon`, which outside
    /// `Context::Action`/`State`/`Struct` yields no role at all, so an
    /// invariant's quantifier binder was indexed as a plain reference.
    #[test]
    fn forall_and_exists_typed_binders_declare_scoped_variables() {
        let source = r"spec Quant {
  type Id = 0..1
  state { seen: Set<Id> }
  invariant Every { forall i: Id { seen.contains(i) } }
  invariant Any { exists j: Id { seen.contains(j) } }
}";
        let index = DocumentIndex::build(source, None).expect("valid index");
        for (name, owner) in [("i", "Every"), ("j", "Any")] {
            let binder = index
                .symbols
                .iter()
                .find(|symbol| symbol.name == name)
                .unwrap_or_else(|| panic!("{name} must be declared, got {:?}", index.symbols));
            assert_eq!(binder.role, SymbolRole::Variable);
            assert_eq!(binder.owner.as_deref(), Some(owner));
            let use_site = index
                .references
                .iter()
                .find(|reference| reference.name == name)
                .unwrap_or_else(|| panic!("{name} must be used in the body"));
            let definition = index
                .definition_at(use_site.range.start)
                .expect("the binder use must resolve");
            assert_eq!(definition.selection_range, binder.selection_range);
        }
    }

    /// `maybe is some(v) and v == 0` must declare `v` as a `Variable`
    /// pattern binder scoped to the enclosing invariant, resolvable from the
    /// guarded use `v == 0`. Before this fix, `is some(name)` was not
    /// recognized as a binder position at all.
    #[test]
    fn is_some_pattern_binder_declares_a_scoped_variable() {
        let index = DocumentIndex::build(ISSUE_504_PROBE, Some("probe.fsl")).expect("valid index");
        let binder_use = Position::new(6, 43); // `v` inside `v == 0`
        let definition = index
            .definition_at(binder_use)
            .expect("the pattern binder use of v must resolve");
        assert_eq!(definition.name, "v");
        assert_eq!(definition.role, SymbolRole::Variable);
        assert_eq!(definition.owner.as_deref(), Some("Pattern"));
    }

    /// `reachable` and `domain` each name both a top-level declaration
    /// keyword (`reachable NAME { expr }`, `domain SpecName { ... }`) and a
    /// relation builtin call. `reachable(edge, 0, 1) and
    /// domain(edge).contains(0)` must not spuriously declare a second `edge`
    /// symbol from either builtin call; the state declaration must stay the
    /// only `edge` symbol, and every use of `edge` must resolve to it.
    /// Before this fix, the unconditional `declaration_keyword` match
    /// consumed the identifier immediately following `reachable`/`domain` as
    /// a brand-new declaration, corrupting `edge`'s definition/reference set
    /// with a self-defining duplicate. The builtin call names themselves stay
    /// keywords and own no index entry, like every other builtin.
    #[test]
    fn reachable_and_domain_builtin_calls_do_not_shadow_the_relation_declaration() {
        let index = DocumentIndex::build(ISSUE_504_PROBE, Some("probe.fsl")).expect("valid index");
        let edge_symbols = index
            .symbols
            .iter()
            .filter(|symbol| symbol.name == "edge")
            .count();
        assert_eq!(edge_symbols, 1, "{:?}", index.symbols);
        assert!(
            !index
                .references
                .iter()
                .any(|reference| matches!(reference.name.as_str(), "reachable" | "domain")),
            "{:?}",
            index.references
        );

        let state_declaration = Position::new(3, 29);
        for use_position in [
            Position::new(7, 33), // reachable(edge, ...)
            Position::new(7, 56), // domain(edge)
        ] {
            let definition = index
                .definition_at(use_position)
                .expect("edge use must resolve");
            assert_eq!(definition.selection_range.start, state_declaration);
        }
    }

    /// A named `preservation NAME { ... }` governance block must declare
    /// `NAME` as a navigable symbol. Before this fix, `preservation` was
    /// absent from `declaration_keyword`, so the block name was indexed as
    /// an ordinary reference with no declaration.
    #[test]
    fn named_preservation_block_declares_a_symbol() {
        let source = r#"governance Controls { control CTRL "control" preservation Reform { preserve CTRL } }"#;
        let index = DocumentIndex::build(source, None).expect("valid governance");
        let reform = index
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Reform")
            .expect("preservation block name must be declared");
        assert_eq!(reform.role, SymbolRole::Namespace);
    }

    /// `actor Customer, Manager` must declare every comma-separated name,
    /// not only the first. Before this fix, `expected` (the pending
    /// declaration role/context) was consumed by the first identifier and
    /// never re-armed at the following `,`, so `Manager` fell through to an
    /// ordinary reference with no declaration.
    #[test]
    fn comma_separated_actor_list_declares_every_name() {
        let source = "business Actors { actor Customer, Manager entity Case }\n\
             verify { instances Case = 1 }";
        let index = DocumentIndex::build(source, None).expect("valid business");
        for name in ["Customer", "Manager"] {
            let symbol = index
                .symbols
                .iter()
                .find(|symbol| symbol.name == name)
                .unwrap_or_else(|| panic!("{name} must be declared"));
            assert_eq!(symbol.role, SymbolRole::Variable);
        }
    }

    /// `rust/fsl-syntax/src/parser.rs`'s `open_ident_list` also ends an
    /// `actor` list on a trailing comma followed by the next item keyword, so
    /// `actor Customer, entity Case` declares one actor and one entity. The
    /// pending-role re-arm must not consume that keyword: a naive re-arm at
    /// every `,` declared a symbol literally named `entity` and left `Case`
    /// an unresolved reference.
    #[test]
    fn actor_list_terminated_by_a_trailing_comma_does_not_consume_the_next_keyword() {
        let source = "business Actors { actor Customer, entity Case }\n\
             verify { instances Case = 1 }";
        let index = DocumentIndex::build(source, None).expect("valid business");
        assert!(
            !index.symbols.iter().any(|symbol| symbol.name == "entity"),
            "{:?}",
            index.symbols
        );
        let case = index
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Case")
            .expect("Case must be declared by `entity Case`");
        assert_eq!(case.role, SymbolRole::Type);
    }

    /// Issue #551 evidence, reduced from `examples/annotations/annotated_domain.fsl`.
    const ISSUE_551_PROBE: &str = r#"domain AnnotatedOrders {
  implementation_profile functional_ddd

  aggregate Order {
    id OrderId

    state {
      placed: Bool = false;
    }

    @requirement("REQ-COMMAND", "placing an order is traceable")
    command Place {
      input order_id: OrderId
    }

    event Placed { order_id: OrderId }

    decide Place {
      emits Placed
    }

    evolve Placed {
      placed = true
    }
  }
}"#;

    /// `@requirement(...)` is an annotation; `requirement NAME { ... }` is a
    /// declaration. Before this fix the shared name made the annotation arm a
    /// pending `(Property, Other)` declaration that nothing between there and
    /// the next identifier disarmed, so the *next line's* construct keyword —
    /// `command` — was consumed as the declaration name and the real name
    /// `Place` was left with no declaration at all.
    #[test]
    fn annotation_named_like_a_declaration_keyword_does_not_swallow_the_declaration() {
        let index = DocumentIndex::build(ISSUE_551_PROBE, None).expect("valid domain");
        assert!(
            !index.symbols.iter().any(|symbol| symbol.name == "command"),
            "{:?}",
            index.symbols
        );
        let place = index
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Place")
            .expect("`command Place` must declare Place");
        assert_eq!(place.role, SymbolRole::Function);
        assert!(index.misprojected_declarations().is_empty());
    }

    /// The positive control for the fix above: resolving the collision by
    /// dropping `requirement` from `declaration_keyword` would also pass that
    /// negative control, and would break this — `requirement NAME "text"
    /// { ... }` is a real declaration form.
    #[test]
    fn a_real_requirement_declaration_still_declares_its_name() {
        let source = r#"requirements Support {
  type CaseId = 0..1
  state { done: Bool }
  init { done = false }
  requirement REQ-1 "a case can finish" {
    action finish() {
      done = true
    }
  }
}"#;
        let index = DocumentIndex::build(source, None).expect("valid requirements");
        let requirement = index
            .symbols
            .iter()
            .find(|symbol| symbol.name == "REQ")
            .expect("`requirement REQ-1` must declare its ID");
        assert_eq!(requirement.role, SymbolRole::Property);
        assert!(index.misprojected_declarations().is_empty());
    }

    /// `annotation_parse::annotation` makes `(` mandatory and accepts a dotted
    /// path with an empty argument list, so `@doc.control()` — not a bare
    /// `@undecided` — is the no-argument form. Every segment of the path is a
    /// name position, so a trailing segment that collides with
    /// `declaration_keyword` (`control`) swallows the next declaration exactly
    /// like a single-segment `@requirement` does, and an empty argument list
    /// leaves no token between the name and the declaration to recover on.
    #[test]
    fn empty_argument_and_dotted_annotations_do_not_swallow_the_declaration() {
        let source = ISSUE_551_PROBE.replace(
            r#"@requirement("REQ-COMMAND", "placing an order is traceable")"#,
            "@doc.control()",
        );
        let index = DocumentIndex::build(&source, None).expect("valid domain");
        let place = index
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Place")
            .expect("`command Place` must declare Place");
        assert_eq!(place.role, SymbolRole::Function);
        assert!(index.misprojected_declarations().is_empty());
    }

    /// An annotation's symbol-path arguments sit exactly where the aggregate
    /// binder heuristic fires (`count(c: T ...)`), so `@count(binder)` would
    /// otherwise declare its arguments as scoped Variables. Nothing between
    /// `@` and the closing `)` may declare anything, and non-keyword argument
    /// names stay references, which `unindexed_identifiers` requires.
    #[test]
    fn annotation_arguments_declare_nothing_and_stay_references() {
        let source = ISSUE_551_PROBE.replace(
            r#"@requirement("REQ-COMMAND", "placing an order is traceable")"#,
            "@count(swallowed, alsoSwallowed)",
        );
        let index = DocumentIndex::build(&source, None).expect("valid domain");
        for argument in ["swallowed", "alsoSwallowed"] {
            assert!(
                !index.symbols.iter().any(|symbol| symbol.name == argument),
                "{argument} must not be declared: {:?}",
                index.symbols
            );
            assert!(
                index
                    .references
                    .iter()
                    .any(|reference| reference.name == argument),
                "{argument} must stay a reference: {:?}",
                index.references
            );
        }
        assert!(index.misprojected_declarations().is_empty());
    }
}

// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use fsl_syntax::keywords::{KEYWORDS, Keyword, Particle, Status};
use fsl_syntax::{Span, Token, TokenKind};
use lsp_types::{Position, Range};

use crate::ast_projection::DeclarationAuthority;

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
    /// The glossary alias the declaration is written as; `None` when it is
    /// written as its canonical name, [`Symbol::name`].
    pub alias: Option<String>,
    pub(crate) owner: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reference {
    /// The canonical name the use reads, also where it is written as an alias.
    pub name: String,
    pub range: Range,
    pub qualifier: Option<String>,
    /// The glossary alias the use is written as; `None` when it is written as
    /// its canonical name.
    pub alias: Option<String>,
    pub(crate) owner: Option<String>,
    pub(crate) target_spec: Option<String>,
}

impl Reference {
    /// The specification a mapping's name belongs to (`impl` / `abs` of a
    /// refinement, the implemented layer of `implements`), when it is not
    /// this document.
    #[must_use]
    pub fn target_spec(&self) -> Option<&str> {
        self.target_spec.as_deref()
    }
}

/// One `Canonical = alias` line of a next-edition document's `glossary`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlossaryRow {
    /// The left side as written: `Claim` or `Claim.amount`.
    pub canonical: String,
    /// The name the alias reads as (the field name of a qualified row).
    pub name: String,
    pub alias: String,
    /// The left side's range (`Claim.amount` whole).
    pub canonical_range: Range,
    pub alias_range: Range,
}

/// Which words a document's index reads as keywords.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Lexicon {
    /// The published language: the declaration keywords and the index's
    /// keyword list.
    #[default]
    Published,
    /// A next-edition (`fsl 5`) requirements document: the published words,
    /// and every active row of the keyword table in English and in Japanese,
    /// and the particles. Its glossary aliases read as their canonical names.
    Next,
}

/// A keyword or particle at a position of a next-edition document, with the
/// rows of the keyword table it spells.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeywordUse {
    pub word: String,
    pub range: Range,
    /// The active rows spelled by the word, in English or in Japanese.
    pub rows: Vec<&'static Keyword>,
    pub particle: Option<Particle>,
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
    parse_options: fsl_syntax::ParseOptions,
    refinement: bool,
    lexicon: Lexicon,
    glossary: Vec<GlossaryRow>,
    /// The action names an `implements` mapping of a next-edition document
    /// maps (`action impl(args) -> abs(args)`): uses, not declarations.
    mapped: HashSet<(u32, u32)>,
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

/// A block on the walk's stack, or the declaration whose block is about to
/// open.
#[derive(Clone, Debug)]
struct Scope {
    context: Context,
    owner: Option<String>,
    /// Whether the AST projection covers the declarations written here. It
    /// does not inside a declaration it leaves out — a process `transition`,
    /// `process`, `acceptance`, ... — nor in the body of
    /// `implements NAME from "path" { ... }`, whose `action impl(args) ->
    /// abs(args)` maps an action instead of declaring one; there the walk
    /// alone decides, as it does in a document without a projection.
    projected: bool,
}

impl Scope {
    const fn new(context: Context, owner: Option<String>, projected: bool) -> Self {
        Self {
            context,
            owner,
            projected,
        }
    }
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
    /// For a `spec`, `requirements` or `compose` document read in the
    /// published edition the declarations come from the parsed AST
    /// (`crate::ast_projection`): the token walk proposes each declaration
    /// site, the AST decides whether a name is declared there and with which
    /// role and owner, and a word spelled like an index keyword is a name
    /// wherever the AST declares it (`reads_as_declared_name`). Every other
    /// document is indexed by the token walk alone; a next-edition
    /// requirements document ([`Lexicon::Next`]) through `read_tokens`, which
    /// reads each glossary alias as its canonical name and each Japanese
    /// keyword as its English spelling, so an alias is a use of the canonical
    /// declaration and a keyword or particle is never a name.
    ///
    /// # Errors
    ///
    /// Returns the compiler parse diagnostic for invalid source.
    #[allow(clippy::too_many_lines)]
    pub fn build(
        source: &str,
        _path: Option<&str>,
        parse_options: &fsl_syntax::ParseOptions,
    ) -> Result<Self, IndexError> {
        let parsed = if fslc_rust::frontend_output::is_ai_project(source)
            || fsl_syntax::is_causal_source(source)
        {
            None
        } else {
            Some(
                fsl_syntax::parse_document_with(fsl_syntax::SourceFile::new(source), parse_options)
                    .map_err(|error| IndexError(error.to_string()))?,
            )
        };
        let refinement = parsed.as_ref().is_some_and(|parsed| {
            matches!(parsed.surface, fsl_syntax::SurfaceDocument::Refinement(_))
        });
        // An `fsl 5` document's surface already reads glossary aliases as
        // their canonical names and desugars new forms, so its declarations
        // need not be spelled where its tokens are; it keeps the token walk,
        // which reads a next-edition requirements document through
        // `read_tokens` (aliases as canonical names, Japanese keywords as
        // English ones).
        let mut authority = parsed
            .as_ref()
            .filter(|parsed| parsed.edition == fsl_syntax::Edition::Fsl4)
            .and_then(|parsed| crate::ast_projection::ast_declarations(&parsed.surface))
            .map(DeclarationAuthority::new);
        let (lexicon, glossary) = match parsed
            .as_ref()
            .map(|parsed| (parsed.edition, &parsed.surface))
        {
            Some((
                fsl_syntax::Edition::Fsl5,
                fsl_syntax::SurfaceDocument::Requirements(requirements),
            )) => (Lexicon::Next, glossary_rows(source, &requirements.items)),
            _ => (Lexicon::Published, Vec::new()),
        };
        drop(parsed);
        let reading = read_tokens(source, *parse_options, lexicon, &glossary)
            .map_err(|error| IndexError(error.to_string()))?;
        let tokens = &reading.tokens;
        let mut symbols = Vec::new();
        let mut references = Vec::new();
        let document_projected = authority.is_some();
        let mut contexts: Vec<Scope> = Vec::new();
        let mut expected: Option<(SymbolRole, Option<Context>)> = None;
        // The declaration keyword that armed `expected`.
        let mut armed_by: Option<&str> = None;
        let mut awaiting_block: Option<Scope> = None;
        let mut list_role: Option<(SymbolRole, Option<Context>)> = None;
        let mut in_annotation = false;
        let mut declaration_offsets = BTreeSet::new();
        // A next-edition entity's `lifecycle A -> B`: whether a stage name
        // comes next, and the stages of the entity declared so far.
        let mut stage_expected = false;
        let mut entity_stages = BTreeSet::new();

        for (index, token) in tokens.iter().enumerate() {
            match &token.kind {
                TokenKind::Ident(name) => {
                    // A name written as an alias is a name, whatever its
                    // canonical name is spelled like.
                    let aliased = reading.aliased.contains_key(&token.span.start.offset);
                    if lexicon == Lexicon::Next && !in_annotation {
                        if stage_expected {
                            let owner = contexts.last().and_then(|scope| scope.owner.clone());
                            if entity_stages.insert(name.clone()) {
                                add_symbol(
                                    source,
                                    token,
                                    name,
                                    SymbolRole::Variable,
                                    None,
                                    &mut symbols,
                                );
                                declaration_offsets.insert(token.span.start.offset);
                            } else {
                                references.push(Reference {
                                    name: name.clone(),
                                    range: span_range(source, token.span),
                                    qualifier: None,
                                    alias: None,
                                    owner,
                                    target_spec: None,
                                });
                            }
                            stage_expected = token_symbol(tokens.get(index + 1)) == Some("->");
                            continue;
                        }
                        if !aliased
                            && name == "lifecycle"
                            && token_symbol(tokens.get(index + 1)) != Some(":")
                            && contexts
                                .last()
                                .is_some_and(|scope| scope.context == Context::Struct)
                        {
                            stage_expected = true;
                            continue;
                        }
                    }
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
                            let enclosing = contexts
                                .last()
                                .map_or(document_projected, |scope| scope.projected);
                            let projected = enclosing
                                && armed_by.is_some_and(
                                    crate::ast_projection::projects_declaration_keyword,
                                );
                            let declared = !projected
                                || authority
                                    .as_mut()
                                    .is_none_or(|authority| authority.locate(name, role, None));
                            if declared {
                                add_symbol(source, token, name, role, None, &mut symbols);
                                declaration_offsets.insert(token.span.start.offset);
                            } else {
                                references.push(Reference {
                                    name: name.clone(),
                                    range: span_range(source, token.span),
                                    qualifier: None,
                                    alias: None,
                                    owner: contexts.last().and_then(|scope| scope.owner.clone()),
                                    target_spec: None,
                                });
                            }
                            let opens_projected = enclosing
                                && armed_by
                                    .is_some_and(crate::ast_projection::opens_projected_block);
                            awaiting_block = context.map(|context| {
                                let owner = matches!(context, Context::Action | Context::Other)
                                    .then(|| name.clone());
                                Scope::new(context, owner, opens_projected)
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
                        if !aliased
                            && name == "implements"
                            && token_symbol(tokens.get(index + 4)) == Some("{")
                        {
                            let inherited = contexts.last().and_then(|scope| scope.owner.clone());
                            awaiting_block = Some(Scope::new(Context::Other, inherited, false));
                        }
                        if !aliased
                            && !builtin_call
                            && let Some((role, context)) =
                                walk_declaration_keyword(lexicon, tokens, index, name)
                        {
                            if lexicon == Lexicon::Next && name == "entity" {
                                entity_stages.clear();
                            }
                            expected = role.map(|role| (role, context));
                            armed_by = Some(name);
                            list_role = (name == "actor").then_some(expected).flatten();
                            if role.is_none() {
                                let enclosing = contexts
                                    .last()
                                    .map_or(document_projected, |scope| scope.projected);
                                awaiting_block =
                                    context.map(|context| Scope::new(context, None, enclosing));
                            }
                            continue;
                        }
                    }
                    if !aliased
                        && is_keyword_in(lexicon, name)
                        && !authority.as_ref().is_some_and(|authority| {
                            reads_as_declared_name(authority, tokens, index, name)
                        })
                    {
                        continue;
                    }

                    let next_is_colon = token_symbol(tokens.get(index + 1)) == Some(":");
                    let previous = token_ident(index.checked_sub(1).and_then(|i| tokens.get(i)));
                    let scope = awaiting_block.as_ref().or_else(|| contexts.last());
                    let context = scope.map(|scope| scope.context);
                    let owner = awaiting_block
                        .as_ref()
                        .and_then(|scope| scope.owner.clone())
                        .or_else(|| contexts.last().and_then(|scope| scope.owner.clone()));
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
                    // `action count(x: T)` declares an action named like the
                    // aggregate; its parameter list is not an aggregate.
                    let binder_after_aggregate_paren =
                        token_symbol(index.checked_sub(1).and_then(|i| tokens.get(i))) == Some("(")
                            && matches!(
                                token_ident(index.checked_sub(2).and_then(|i| tokens.get(i))),
                                Some("count" | "sum" | "unique" | "exactlyOne")
                            )
                            && token_ident(index.checked_sub(3).and_then(|i| tokens.get(i)))
                                .is_none_or(|word| declaration_keyword_in(lexicon, word).is_none());
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
                    // Parameters, state variables, struct fields, enum members
                    // and `let` bindings are projected from the AST; expression
                    // binders and `as` names are not.
                    let mut projected_site =
                        scope.map_or(document_projected, |scope| scope.projected);
                    let role = if in_annotation {
                        None
                    } else if quantifier_binder || pattern_binder {
                        projected_site = false;
                        Some(SymbolRole::Variable)
                    } else if next_is_colon {
                        match context {
                            Some(Context::Action) => Some(SymbolRole::Parameter),
                            Some(Context::State) => Some(SymbolRole::Variable),
                            Some(Context::Struct) => Some(SymbolRole::Property),
                            _ => None,
                        }
                    } else if lexicon == Lexicon::Next && relation_role(tokens, index) {
                        projected_site = false;
                        Some(SymbolRole::Property)
                    } else if enum_member || matches!(previous, Some("as" | "let")) {
                        projected_site = enum_member || previous == Some("let");
                        Some(SymbolRole::Variable)
                    } else {
                        None
                    };
                    let scoped = matches!(role, Some(SymbolRole::Parameter))
                        || quantifier_binder
                        || pattern_binder
                        || (matches!(previous, Some("as" | "let"))
                            && role != Some(SymbolRole::Property));
                    let symbol_owner = scoped.then(|| owner.clone()).flatten();
                    if let Some(role) = role
                        && (!projected_site
                            || authority.as_mut().is_none_or(|authority| {
                                authority.locate(name, role, symbol_owner.as_deref())
                            }))
                    {
                        add_symbol(source, token, name, role, symbol_owner, &mut symbols);
                        declaration_offsets.insert(token.span.start.offset);
                    } else {
                        references.push(Reference {
                            name: name.clone(),
                            range: span_range(source, token.span),
                            qualifier: qualifier_at(tokens, index),
                            alias: None,
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
                        && token_ident(tokens.get(index + 1))
                            .is_some_and(|next| !is_keyword_in(lexicon, next));
                    if continues_list && let Some(pending) = list_role {
                        expected = Some(pending);
                    }
                }
                TokenKind::Symbol(symbol) if symbol == "{" => {
                    let inherited = contexts.last().map_or_else(
                        || Scope::new(Context::Other, None, document_projected),
                        |scope| Scope::new(Context::Other, scope.owner.clone(), scope.projected),
                    );
                    contexts.push(awaiting_block.take().unwrap_or(inherited));
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
                        && !matches!(
                            awaiting_block.as_ref().map(|scope| scope.context),
                            Some(Context::Action | Context::Top)
                        ) =>
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
            apply_refinement_hints(source, tokens, &mut symbols, &mut references);
        }
        let mut mapped = HashSet::new();
        if lexicon == Lexicon::Next {
            apply_implements_hints(source, tokens, &mut symbols, &mut references, &mut mapped);
            let aliased = reading
                .aliased
                .iter()
                .map(|(offset, alias)| {
                    let start = position_at_offset(source, *offset);
                    ((start.line, start.character), alias)
                })
                .collect::<HashMap<_, _>>();
            let written = |range: Range| {
                aliased
                    .get(&(range.start.line, range.start.character))
                    .map(|alias| (*alias).clone())
            };
            for symbol in &mut symbols {
                symbol.alias = written(symbol.selection_range);
            }
            for reference in &mut references {
                reference.alias = written(reference.range);
            }
        }
        let imports = import_bindings(source, tokens);
        Ok(Self {
            source: source.to_owned(),
            parse_options: *parse_options,
            refinement,
            lexicon,
            glossary,
            mapped,
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

    /// Which words the document reads as keywords.
    #[must_use]
    pub const fn lexicon(&self) -> Lexicon {
        self.lexicon
    }

    /// The rows of the document's `glossary`, in source order; empty without
    /// one.
    #[must_use]
    pub fn glossary(&self) -> &[GlossaryRow] {
        &self.glossary
    }

    /// The glossary rows whose alias reads as `name`.
    pub fn glossary_rows_for<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a GlossaryRow> {
        self.glossary.iter().filter(move |row| row.name == name)
    }

    /// Whether `value` is a keyword of this document (in a next-edition
    /// requirements document, in English or Japanese, or a particle).
    #[must_use]
    pub fn is_keyword(&self, value: &str) -> bool {
        is_keyword_in(self.lexicon, value)
    }

    /// `(alias, canonical name)` when the name at `position` is written as a
    /// glossary alias.
    #[must_use]
    pub fn written_alias_at(&self, position: Position) -> Option<(&str, &str)> {
        self.symbol_at(position)
            .and_then(|symbol| Some((symbol.alias.as_deref()?, symbol.name.as_str())))
            .or_else(|| {
                let reference = self.reference_at(position)?;
                Some((reference.alias.as_deref()?, reference.name.as_str()))
            })
    }

    /// The ranges where the document writes `alias`: the glossary's right
    /// side and every name written so.
    #[must_use]
    pub fn alias_ranges(&self, alias: &str) -> Vec<Range> {
        let mut ranges = self
            .symbols
            .iter()
            .filter(|symbol| symbol.alias.as_deref() == Some(alias))
            .map(|symbol| symbol.selection_range)
            .chain(
                self.references
                    .iter()
                    .filter(|reference| reference.alias.as_deref() == Some(alias))
                    .map(|reference| reference.range),
            )
            .collect::<Vec<_>>();
        ranges.sort_by_key(|range| (range.start.line, range.start.character));
        ranges.dedup();
        ranges
    }

    /// The keyword or particle at `position` of a next-edition requirements
    /// document, with the keyword-table rows it spells.
    #[must_use]
    pub fn keyword_at(&self, position: Position) -> Option<KeywordUse> {
        if self.lexicon != Lexicon::Next {
            return None;
        }
        let tokens = fsl_syntax::lex_with(&self.source, &self.parse_options).ok()?;
        let token = tokens.iter().find(|token| {
            matches!(token.kind, TokenKind::Ident(_))
                && contains(span_range(&self.source, token.span), position)
        })?;
        let TokenKind::Ident(word) = &token.kind else {
            return None;
        };
        if self.symbol_at(position).is_some() || self.reference_at(position).is_some() {
            return None;
        }
        let rows = active_rows()
            .filter(|keyword| {
                keyword.japanese == word
                    || keyword.english == word
                    || keyword.english.split(' ').any(|part| part == word)
            })
            .collect::<Vec<_>>();
        let particle = fsl_syntax::keywords::particle(word);
        (!rows.is_empty() || particle.is_some()).then(|| KeywordUse {
            word: word.clone(),
            range: span_range(&self.source, token.span),
            rows,
            particle,
        })
    }

    fn reading(&self) -> Result<Reading, fsl_syntax::LexError> {
        read_tokens(
            &self.source,
            self.parse_options,
            self.lexicon,
            &self.glossary,
        )
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
        self.reading().map_or_else(
            |_| Vec::new(),
            |reading| {
                let tokens = &reading.tokens;
                tokens
                    .iter()
                    .enumerate()
                    .filter_map(|(index, token)| {
                        let TokenKind::Ident(keyword) = &token.kind else {
                            return None;
                        };
                        if (self.refinement && keyword == "action")
                            || reading.aliased.contains_key(&token.span.start.offset)
                        {
                            return None;
                        }
                        walk_declaration_keyword(self.lexicon, tokens, index, keyword)?.0?;
                        let name = tokens.get(index + 1)?;
                        let TokenKind::Ident(name_text) = &name.kind else {
                            return None;
                        };
                        let position = span_range(&self.source, name.span).start;
                        let key = (position.line, position.character);
                        (!declared.contains(&key) && !self.mapped.contains(&key)).then(|| {
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
        self.reading().map_or_else(
            |_| Vec::new(),
            |reading| {
                reading
                    .tokens
                    .iter()
                    .filter_map(|token| {
                        let TokenKind::Ident(name) = &token.kind else {
                            return None;
                        };
                        let position = span_range(&self.source, token.span).start;
                        let aliased = reading.aliased.contains_key(&token.span.start.offset);
                        ((aliased || !is_keyword_in(self.lexicon, name))
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
            Some("map" | "action") => {
                apply_mapping_item(
                    source,
                    tokens,
                    index,
                    (implementation.as_ref(), abstraction.as_ref()),
                    symbols,
                    references,
                );
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

/// A next-edition document's `implements NAME from "path" { … }` is a
/// mapping like a refinement file's, from this document (the
/// implementation) to the specification `NAME` (the abstraction): its
/// `action impl(args) -> abs(args)` uses this document's event `impl` and
/// `NAME`'s action `abs`, and binds its own parameters. The mapped action
/// names are recorded in `mapped`.
fn apply_implements_hints(
    source: &str,
    tokens: &[Token],
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<Reference>,
    mapped: &mut HashSet<(u32, u32)>,
) {
    for (index, token) in tokens.iter().enumerate() {
        if token_ident(Some(token)) != Some("implements")
            || token_ident(tokens.get(index + 2)) != Some("from")
            || token_symbol(tokens.get(index + 4)) != Some("{")
        {
            continue;
        }
        let abstraction = token_ident(tokens.get(index + 1)).map(str::to_owned);
        let open = index + 4;
        let mut depth = 0_usize;
        let close = (open..tokens.len())
            .find(|position| {
                match token_symbol(tokens.get(*position)) {
                    Some("{") => depth += 1,
                    Some("}") => depth -= 1,
                    _ => {}
                }
                depth == 0
            })
            .unwrap_or(tokens.len());
        let body = &tokens[open + 1..close];
        for (item, word) in body.iter().enumerate() {
            if let Some(keyword @ ("map" | "action")) = token_ident(Some(word)) {
                apply_mapping_item(
                    source,
                    body,
                    item,
                    (None, abstraction.as_ref()),
                    symbols,
                    references,
                );
                if keyword == "action"
                    && let Some(name) = body.get(item + 1)
                {
                    let start = span_range(source, name.span).start;
                    mapped.insert((start.line, start.character));
                }
            }
        }
    }
}

/// One `map` or `action` item of a mapping between an implementation and an
/// abstraction, at `tokens[index]`; `specs` names the two (`None` is this
/// document).
fn apply_mapping_item(
    source: &str,
    tokens: &[Token],
    index: usize,
    specs: (Option<&String>, Option<&String>),
    symbols: &mut Vec<Symbol>,
    references: &mut Vec<Reference>,
) {
    let (implementation, abstraction) = specs;
    let token = &tokens[index];
    match token_ident(Some(token)) {
        Some("map") => {
            let end = refinement_item_end(tokens, index + 1);
            let owner = format!("map:{}", token.span.start.offset);
            if let Some(target) = tokens.get(index + 1) {
                update_reference(source, target, None, abstraction.cloned(), references);
            }
            let equals =
                (index + 1..end).find(|position| token_symbol(tokens.get(*position)) == Some("="));
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
                    let local =
                        binder.and_then(|position| token_ident(tokens.get(position))) == Some(name);
                    update_reference(
                        source,
                        item,
                        local.then(|| owner.clone()),
                        (!local).then(|| implementation.cloned()).flatten(),
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
                    implementation.cloned(),
                    symbols,
                    references,
                );
            }
            let arrow =
                (index + 1..end).find(|position| token_symbol(tokens.get(*position)) == Some("->"));
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
                    update_reference(source, target, None, abstraction.cloned(), references);
                }
                for item in tokens.iter().take(end).skip(arrow + 2) {
                    if token_ident(Some(item)).is_some() {
                        update_reference(source, item, Some(owner.clone()), None, references);
                    }
                }
            }
        }
        _ => {}
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
            alias: None,
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

/// The published language's words the walk reads as keywords. A
/// next-edition requirements document also reads the keyword table's
/// ([`is_keyword_in`]); they are read from the table, not copied here, so a
/// row a later stage activates is a keyword as soon as the parser reads it.
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

/// The declaration keywords a next-edition requirements document adds or
/// reads differently. `decide` opens a decision table whose `REQ-…` label is
/// a requirement's, as `rule` and `scenario` do; `resource` declares a state
/// of no entity and `measure` a measure as `kpi` does; `entity` with a body
/// is read by [`walk_declaration_keyword`], and the role a relation names
/// (`E belongs to E2 as role via event`) by [`relation_role`]. `goal` and
/// `policy` declare as in the published language. Each is the English spelling of an active
/// declaration row of the keyword table (`next_declaration_keywords_are_the_tables`).
const NEXT_DECLARATION_KEYWORDS: &[DeclarationKeyword] = &[
    DeclarationKeyword {
        value: "concept",
        role: Some(SymbolRole::Type),
        context: None,
    },
    DeclarationKeyword {
        value: "event",
        role: Some(SymbolRole::Function),
        context: None,
    },
    DeclarationKeyword {
        value: "rule",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "decide",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "scenario",
        role: Some(SymbolRole::Property),
        context: Some(Context::Other),
    },
    DeclarationKeyword {
        value: "resource",
        role: Some(SymbolRole::Variable),
        context: None,
    },
    DeclarationKeyword {
        value: "measure",
        role: Some(SymbolRole::Variable),
        context: Some(Context::Other),
    },
];

fn declaration_keyword_in(
    lexicon: Lexicon,
    value: &str,
) -> Option<(Option<SymbolRole>, Option<Context>)> {
    (lexicon == Lexicon::Next)
        .then(|| {
            NEXT_DECLARATION_KEYWORDS
                .iter()
                .find(|keyword| keyword.value == value)
                .map(|keyword| (keyword.role, keyword.context))
        })
        .flatten()
        .or_else(|| declaration_keyword(value))
}

/// The declaration keyword `tokens[index]` (spelled `value`) starts, read in
/// `lexicon`. A next-edition `entity Name { … }` declares its fields like a
/// struct's and its lifecycle's stages.
fn walk_declaration_keyword(
    lexicon: Lexicon,
    tokens: &[Token],
    index: usize,
    value: &str,
) -> Option<(Option<SymbolRole>, Option<Context>)> {
    if lexicon == Lexicon::Next
        && value == "entity"
        && token_symbol(tokens.get(index + 2)) == Some("{")
    {
        return Some((Some(SymbolRole::Type), Some(Context::Struct)));
    }
    declaration_keyword_in(lexicon, value)
}

/// Whether `tokens[index]` is the role a next-edition relation names:
/// `E belongs to E2 as ROLE`, where the reading spells `belongs to` as two
/// words in English and as one (`所属先`) in Japanese.
fn relation_role(tokens: &[Token], index: usize) -> bool {
    let before = |back: usize| index.checked_sub(back).and_then(|i| tokens.get(i));
    token_ident(before(1)) == Some("as")
        && token_ident(before(2)).is_some()
        && (token_ident(before(3)) == Some("belongs to")
            || (token_ident(before(3)) == Some("to") && token_ident(before(4)) == Some("belongs")))
}

/// The rows of the keyword table the parser reads. A reserved row is not a
/// keyword yet, so a word spelled like it is a name.
fn active_rows() -> impl Iterator<Item = &'static Keyword> {
    KEYWORDS
        .iter()
        .filter(|keyword| keyword.status == Status::Active)
}

/// Whether `value` is a keyword of a document read in `lexicon`: in a
/// next-edition requirements document also each word of an active row's
/// English spelling, the row's Japanese spelling, and a particle.
pub(crate) fn is_keyword_in(lexicon: Lexicon, value: &str) -> bool {
    is_keyword(value)
        || (lexicon == Lexicon::Next
            && (fsl_syntax::keywords::particle(value).is_some()
                || active_rows().any(|keyword| {
                    keyword.japanese == value
                        || keyword.english == value
                        || keyword.english.split(' ').any(|word| word == value)
                })))
}

/// The English spelling the walk reads a Japanese keyword as. A Japanese
/// word shared by rows of different contexts (`条件`) is read as the first
/// row's English; every row it spells is a keyword and none declares.
fn english_spelling(japanese: &str) -> Option<&'static str> {
    active_rows()
        .find(|keyword| keyword.japanese == japanese)
        .map(|keyword| keyword.english)
}

/// The tokens the walk reads, and the alias each name was written as, by
/// byte offset.
struct Reading {
    tokens: Vec<Token>,
    aliased: BTreeMap<usize, String>,
}

/// Lex `source` for the walk. In a next-edition requirements document each
/// glossary alias reads as its canonical name and each Japanese keyword as
/// its English spelling, the way the parser reads them, so the walk reads
/// one language. A particle stays as written: it is a keyword there.
fn read_tokens(
    source: &str,
    options: fsl_syntax::ParseOptions,
    lexicon: Lexicon,
    glossary: &[GlossaryRow],
) -> Result<Reading, fsl_syntax::LexError> {
    let mut tokens = fsl_syntax::lex_with(source, &options)?;
    let mut aliased = BTreeMap::new();
    if lexicon == Lexicon::Next {
        let canonical_of = glossary
            .iter()
            .map(|row| (row.alias.as_str(), row.name.as_str()))
            .collect::<HashMap<_, _>>();
        for token in &mut tokens {
            let TokenKind::Ident(word) = &mut token.kind else {
                continue;
            };
            if let Some(canonical) = canonical_of.get(word.as_str()) {
                let written = std::mem::replace(word, (*canonical).to_owned());
                aliased.insert(token.span.start.offset, written);
            } else if let Some(english) = english_spelling(word) {
                english.clone_into(word);
            }
        }
    }
    Ok(Reading { tokens, aliased })
}

fn glossary_rows(source: &str, items: &[fsl_syntax::RequirementsItem]) -> Vec<GlossaryRow> {
    items
        .iter()
        .find_map(|item| match item {
            fsl_syntax::RequirementsItem::Glossary(glossary) => Some(glossary),
            _ => None,
        })
        .map_or_else(Vec::new, |glossary| {
            glossary
                .entries
                .iter()
                .map(|entry| GlossaryRow {
                    canonical: entry.canonical.written(),
                    name: entry.canonical.name.clone(),
                    alias: entry.alias.clone(),
                    canonical_range: span_range(source, entry.canonical_span),
                    alias_range: span_range(source, entry.alias_span),
                })
                .collect()
        })
}

/// Literals in every expression; the checker rejects a declaration so named
/// (issue #570), so the index never reads one as a name.
const LITERAL_WORDS: &[&str] = &["true", "false", "none"];

/// Whether the identifier at `tokens[index]`, spelled like an index keyword,
/// is a name in a document whose declarations the AST decides (issue 1362).
///
/// FSL reserves none of these words, so a word the AST declares is a name —
/// its declaration in a state, field, member, parameter or `let` position,
/// and its uses. The exceptions are the positions where the grammar reads the
/// word as a keyword whatever the document declares:
///
/// - a call `word(...)` when the document declares no action or `def` so
///   named (the aggregate `count(...)`, `sum(...)`, `old(...)`, ...), and a
///   collection method `.word(...)` always;
/// - `expect rejected }`, which closes a `forbidden` block;
/// - a clause keyword followed on its own line by the name, number or string
///   it introduces (`age NAME[...]` and `urgent NAME` in a `time` block,
///   `kpi K = count NAME in ...`). A name used in an expression is followed
///   by an operator, a bracket, a separator or a keyword instead.
fn reads_as_declared_name(
    authority: &DeclarationAuthority,
    tokens: &[Token],
    index: usize,
    name: &str,
) -> bool {
    if LITERAL_WORDS.contains(&name) || !authority.declares(name) {
        return false;
    }
    let token = &tokens[index];
    let previous = index.checked_sub(1).and_then(|i| tokens.get(i));
    let next = tokens.get(index + 1);
    if token_symbol(next) == Some("(") {
        return token_symbol(previous) != Some(".") && authority.declares_function(name);
    }
    if token_ident(previous) == Some("expect") && token_symbol(next) == Some("}") {
        return false;
    }
    !next.is_some_and(|next| {
        next.span.start.line == token.span.start.line
            && match &next.kind {
                TokenKind::Ident(word) => !is_keyword(word),
                TokenKind::Int(_) | TokenKind::String(_) => true,
                _ => false,
            }
    })
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
        alias: None,
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

    // These expected role/context pairs are transcribed from origin/main's
    // declaration_keyword match, not derived from the table.
    const PRE_REFACTOR_DECLARATION_KEYWORDS: &[(&[&str], Option<SymbolRole>, Option<Context>)] = &[
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
            ],
            Some(SymbolRole::Namespace),
            Some(Context::Top),
        ),
        (&["type", "number", "entity"], Some(SymbolRole::Type), None),
        (&["enum"], Some(SymbolRole::Type), Some(Context::Enum)),
        (
            &["struct", "table"],
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
            ],
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
            ],
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
            ],
            Some(SymbolRole::Variable),
            Some(Context::Other),
        ),
        (
            &["preservation"],
            Some(SymbolRole::Namespace),
            Some(Context::Other),
        ),
        (&["state"], None, Some(Context::State)),
        (&["init", "verify"], None, Some(Context::Other)),
    ];

    #[test]
    fn declaration_keyword_registry_preserves_the_pre_refactor_mapping() {
        let expected_count = PRE_REFACTOR_DECLARATION_KEYWORDS
            .iter()
            .map(|(keywords, _, _)| keywords.len())
            .sum::<usize>();
        assert_eq!(DECLARATION_KEYWORDS.len(), expected_count);

        for &(keywords, role, context) in PRE_REFACTOR_DECLARATION_KEYWORDS {
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

    /// The declaration keywords a next-edition requirements document adds
    /// are the keyword table's: each is the English spelling of an active
    /// declaration row, every active declaration row declares a name in
    /// either language, and the walk reads its Japanese spelling as its
    /// English one. A row a later stage activates (`resource`, `measure`)
    /// fails here until the walk declares its name.
    #[test]
    fn next_declaration_keywords_are_the_tables() {
        use fsl_syntax::keywords::KeywordContext;
        let declaration_rows =
            || active_rows().filter(|keyword| keyword.context == KeywordContext::Declaration);
        for keyword in NEXT_DECLARATION_KEYWORDS {
            assert!(
                declaration_rows().any(|row| row.english == keyword.value),
                "{} is no active declaration row",
                keyword.value
            );
        }
        for row in declaration_rows() {
            assert!(
                declaration_keyword_in(Lexicon::Next, row.english)
                    .is_some_and(|(role, _)| role.is_some()),
                "{:?} `{}` declares no name",
                row.kw,
                row.english
            );
            assert_eq!(
                english_spelling(row.japanese),
                Some(row.english),
                "{:?}",
                row.kw
            );
        }
        // The published language reads none of them differently.
        for keyword in NEXT_DECLARATION_KEYWORDS {
            assert_ne!(
                declaration_keyword(keyword.value),
                Some((keyword.role, keyword.context)),
                "{} is the published mapping",
                keyword.value
            );
        }
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
        let index = DocumentIndex::build(
            source,
            Some("cart.fsl"),
            &fsl_syntax::ParseOptions::stable(),
        )
        .expect("valid index");
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
        let error = DocumentIndex::build(
            "spec Broken { state {",
            Some("broken.fsl"),
            &fsl_syntax::ParseOptions::stable(),
        )
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
        let index = DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid source");
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
        let index = DocumentIndex::build(
            ISSUE_504_PROBE,
            Some("probe.fsl"),
            &fsl_syntax::ParseOptions::stable(),
        )
        .expect("valid index");
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
        let index = DocumentIndex::build(
            ISSUE_504_PROBE,
            Some("probe.fsl"),
            &fsl_syntax::ParseOptions::stable(),
        )
        .expect("valid index");
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
        let index = DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid index");
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
        let index = DocumentIndex::build(
            ISSUE_504_PROBE,
            Some("probe.fsl"),
            &fsl_syntax::ParseOptions::stable(),
        )
        .expect("valid index");
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
        let index = DocumentIndex::build(
            ISSUE_504_PROBE,
            Some("probe.fsl"),
            &fsl_syntax::ParseOptions::stable(),
        )
        .expect("valid index");
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
        let index = DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid governance");
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
        let index = DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid business");
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
        let index = DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid business");
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

    /// Issue #1362's other half: a name being declared does not turn every
    /// token so spelled into a use of it. Where the grammar reads the word
    /// as a keyword whatever the document declares — an aggregate
    /// `count(...)`, a collection method `.add(...)`, the `age NAME` clause
    /// of a `time` block, the `expect rejected` that closes a `forbidden`
    /// block — it stays a keyword, so rename never rewrites syntax. The same
    /// words as plain expressions, calls of the declared action and the
    /// acceptance's `expect rejected == 1` are uses.
    #[test]
    fn a_declared_keyword_spelled_name_leaves_the_grammars_keyword_uses_alone() {
        let source = r#"requirements Clash {
  type Id = 0..1
  state { count: Int, rejected: Int, age: Int, cases: Set<Id> }
  init { count = 0  rejected = 0  age = 0  cases = Set {} }
  action add(c: Id) {
    requires count(i: Id where cases.contains(i)) == count
    cases = cases.add(c)
    rejected = rejected + age
  }
  time {
    urgent add
    age waited[c: Id] while cases.contains(c)
  }
  forbidden FB-1 "nothing is rejected twice" {
    add(0)
    expect rejected
  }
  acceptance AC-1 "one add is one rejection" {
    add(0)
    expect rejected == 1
  }
}"#;
        let index = DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid requirements");
        let referenced = |name: &str| {
            let mut lines = index
                .references
                .iter()
                .filter(|reference| reference.name == name)
                .map(|reference| reference.range.start.line + 1)
                .collect::<Vec<_>>();
            lines.dedup();
            lines
        };
        // Line 6's first `count` is the aggregate, line 7's `add` the method,
        // line 12's `age` the clause and line 16's `rejected` the keyword.
        assert_eq!(referenced("count"), vec![4, 6]);
        assert_eq!(referenced("add"), vec![11, 15, 19]);
        assert_eq!(referenced("age"), vec![4, 8]);
        assert_eq!(referenced("rejected"), vec![4, 8, 20]);
        let count_uses = index
            .references
            .iter()
            .filter(|reference| reference.name == "count")
            .count();
        assert_eq!(count_uses, 2, "{:?}", index.references);
        assert!(index.unindexed_identifiers().is_empty());
        assert!(index.misprojected_declarations().is_empty());
    }

    /// The AST decides only the declarations it projects. A process
    /// transition's input (`with a: Amount`) and the mapped action of an
    /// `implements` block are left out of the projection, so the walk still
    /// declares them and their uses still resolve.
    #[test]
    fn declarations_the_projection_leaves_out_keep_the_walks_index() {
        let source = r#"requirements Expense {
  implements ExpenseToBe from "1_business.fsl" {
    action submit(c, a) -> submit(c)
  }
  number Amount
  process Claim with amount: Amount = 0 {
    stages Draft, Submitted
    initial Draft
    transition submit Draft -> Submitted by Employee
      with a: Amount
      when a > 0
      set amount = a
  }
}"#;
        let index = DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid requirements");
        let input = index
            .symbols
            .iter()
            .find(|symbol| symbol.name == "a" && symbol.selection_range.start.line == 9)
            .unwrap_or_else(|| panic!("the transition input is declared: {:?}", index.symbols));
        assert_eq!(input.role, SymbolRole::Parameter);
        assert_eq!(input.owner.as_deref(), Some("submit"));
        for (line, character) in [(10, 11), (11, 19)] {
            assert_eq!(
                index.definition_at(Position::new(line, character)),
                Some(input),
                "the use at {line}:{character}"
            );
        }
        assert!(
            index
                .symbols
                .iter()
                .any(|symbol| symbol.name == "submit" && symbol.selection_range.start.line == 2),
            "{:?}",
            index.symbols
        );
        assert!(index.misprojected_declarations().is_empty());
    }

    /// Inside `until NAME { P until Q }` the second `until` is the operator,
    /// not a declaration. The walk read it as one and declared the first name
    /// of `Q` — a use of the state variable `req` — as a property, and every
    /// `req` in the document then resolved to that phantom. The AST declares
    /// no such property, so the name stays a use of the state variable, also
    /// inside a `requirement` block, whose properties the AST projects.
    #[test]
    fn the_until_operator_declares_nothing() {
        let body = "  type Id = 0..1
  state { req: Map<Id, Bool> }
  init { forall i: Id { req[i] = false } }
  action on(i: Id) { req[i] = true }";
        let property = "until Stays { (req[0] == false) until (req[0] == true) }";
        for source in [
            format!("spec Hold {{\n{body}\n  {property}\n}}"),
            format!("requirements Hold {{\n{body}\n  requirement REQ-1 \"t\" {{ {property} }}\n}}"),
        ] {
            let index = DocumentIndex::build(&source, None, &fsl_syntax::ParseOptions::stable())
                .expect("valid source");
            let declarations = index
                .symbols
                .iter()
                .filter(|symbol| symbol.name == "req")
                .map(|symbol| (symbol.role, symbol.selection_range.start.line))
                .collect::<Vec<_>>();
            assert_eq!(declarations, vec![(SymbolRole::Variable, 2)], "{source}");
            let state = index.symbols.iter().find(|symbol| symbol.name == "req");
            let uses = index
                .references
                .iter()
                .filter(|reference| reference.name == "req")
                .collect::<Vec<_>>();
            assert_eq!(uses.len(), 4, "{source}: {uses:?}");
            assert!(
                uses.iter()
                    .all(|reference| index.definition_at(reference.range.start) == state)
            );
        }
    }

    /// `true`, `false` and `none` are literals in every expression, so the
    /// checker rejects a declaration so named (issue #570). The index never
    /// treats them as names, even where the parser accepted the declaration.
    #[test]
    fn literal_words_are_never_names() {
        let source = "spec Literal {
  state { true: Bool }
  init { true = false }
  invariant Holds { true }
}";
        let index = DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("parsed");
        assert!(
            !index.symbols.iter().any(|symbol| symbol.name == "true")
                && !index
                    .references
                    .iter()
                    .any(|reference| reference.name == "true"),
            "{index:?}"
        );
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
        let index =
            DocumentIndex::build(ISSUE_551_PROBE, None, &fsl_syntax::ParseOptions::stable())
                .expect("valid domain");
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
        let index = DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid requirements");
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
        let index = DocumentIndex::build(&source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid domain");
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
        let index = DocumentIndex::build(&source, None, &fsl_syntax::ParseOptions::stable())
            .expect("valid domain");
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

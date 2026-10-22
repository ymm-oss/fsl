// SPDX-License-Identifier: Apache-2.0

//! Declarations projected from the authoritative surface AST (issue 552).
//!
//! This module reads the declarations the parser accepted — name, role and
//! owning scope — straight from `fsl_syntax::SurfaceDocument`, without
//! looking at a token. It is the half of the structure
//! `docs/design/DESIGN-rust-lsp.md` describes that decides *what* is
//! declared; the token walk in `DocumentIndex::build` only finds *where*.
//!
//! For a covered document `DocumentIndex::build` hands the projection to the
//! walk as a [`DeclarationAuthority`]: a name in a position the projection
//! covers becomes a symbol only when the AST declares that name with that
//! role and owner, and a word spelled like an index keyword (`status`,
//! `count`, ...) is read as a name wherever the AST declares it (issue 1362).
//! `DocumentIndex::role_scope_mismatches` then checks the other direction —
//! that every projected declaration was located — so the corpus test fails
//! when the walk cannot find a declaration the parser accepted.
//!
//! Coverage is the spec family — `spec`, `requirements` and `compose` — whose
//! declarations share `SpecItem`. Other dialects return `None` and are
//! indexed by the token walk alone.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fsl_syntax::{
    ActionItem, ComposeItem, Param, RequirementAction, RequirementActionItem, RequirementBlockItem,
    RequirementsItem, SpecItem, SurfaceDocument,
};

use crate::{DocumentIndex, SymbolRole};

/// One declaration the authoritative parser accepted.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AstDeclaration {
    pub name: String,
    pub role: SymbolRole,
    /// The declaration whose body scopes this name (an action or `def` for its
    /// parameters and `let` bindings); `None` for a document-level name.
    pub owner: Option<String>,
    /// Whether the grammar puts this name directly after its own declaration
    /// keyword (`action NAME`, `const NAME`, `enum NAME`). State variables,
    /// struct fields, enum members, parameters and `let` bindings are not:
    /// they sit in a list or binder position.
    pub introduced_by_keyword: bool,
}

impl AstDeclaration {
    fn keyword(name: &str, role: SymbolRole) -> Self {
        Self {
            name: name.to_owned(),
            role,
            owner: None,
            introduced_by_keyword: true,
        }
    }

    fn positional(name: &str, role: SymbolRole, owner: Option<&str>) -> Self {
        Self {
            name: name.to_owned(),
            role,
            owner: owner.map(str::to_owned),
            introduced_by_keyword: false,
        }
    }
}

/// Project the declarations of a parsed document.
///
/// Returns `None` for a dialect the projection does not cover yet, so callers
/// can tell "covered and empty" from "not covered".
#[must_use]
pub fn ast_declarations(document: &SurfaceDocument) -> Option<Vec<AstDeclaration>> {
    let mut out = Vec::new();
    match document {
        SurfaceDocument::Spec(spec) => {
            out.push(AstDeclaration::keyword(&spec.name, SymbolRole::Namespace));
            for item in &spec.items {
                spec_item(item, &mut out);
            }
        }
        SurfaceDocument::Requirements(requirements) => {
            out.push(AstDeclaration::keyword(
                &requirements.name,
                SymbolRole::Namespace,
            ));
            for item in &requirements.items {
                requirements_item(item, &mut out);
            }
        }
        SurfaceDocument::Compose(compose) => {
            out.push(AstDeclaration::keyword(
                &compose.name,
                SymbolRole::Namespace,
            ));
            for item in &compose.items {
                match item {
                    ComposeItem::SyncAction(action) => {
                        out.push(AstDeclaration::keyword(&action.name, SymbolRole::Function));
                        params(&action.params, &action.name, &mut out);
                        action_items(&action.items, &action.name, &mut out);
                    }
                    ComposeItem::Common(item) => spec_item(item, &mut out),
                    ComposeItem::Use { .. } | ComposeItem::Internal { .. } => {}
                }
            }
        }
        _ => return None,
    }
    Some(out)
}

/// The declaration keywords whose name [`ast_declarations`] projects. In a
/// covered document the name after one of these is declared only when the
/// AST declares it; the name after any other declaration keyword
/// (`requirement REQ-1`, `acceptance`, `process`, `actor`, ...) is a label or
/// belongs to a projection that does not exist yet, so the walk alone
/// decides it.
const PROJECTED_DECLARATION_KEYWORDS: &[&str] = &[
    "spec",
    "requirements",
    "compose",
    "const",
    "def",
    "type",
    "number",
    "entity",
    "enum",
    "struct",
    "action",
    "invariant",
    "trans",
    "reachable",
    "until",
    "unless",
    "leadsTo",
];

pub(crate) fn projects_declaration_keyword(keyword: &str) -> bool {
    PROJECTED_DECLARATION_KEYWORDS.contains(&keyword)
}

/// Whether the block a declaration keyword opens holds projected
/// declarations: those of a projected declaration, and a `requirement`
/// block, whose label is not projected but whose actions and properties are.
pub(crate) fn opens_projected_block(keyword: &str) -> bool {
    projects_declaration_keyword(keyword) || keyword == "requirement"
}

/// The projected declarations of one document, consumed as the token walk
/// locates them.
#[derive(Debug)]
pub(crate) struct DeclarationAuthority {
    /// How many declarations of each (name, role, owner) are still unlocated.
    remaining: BTreeMap<(String, SymbolRole, Option<String>), usize>,
    names: BTreeSet<String>,
    functions: BTreeSet<String>,
}

impl DeclarationAuthority {
    pub(crate) fn new(declarations: Vec<AstDeclaration>) -> Self {
        let mut authority = Self {
            remaining: BTreeMap::new(),
            names: BTreeSet::new(),
            functions: BTreeSet::new(),
        };
        for declaration in declarations {
            if declaration.role == SymbolRole::Function {
                authority.functions.insert(declaration.name.clone());
            }
            authority.names.insert(declaration.name.clone());
            *authority
                .remaining
                .entry((declaration.name, declaration.role, declaration.owner))
                .or_default() += 1;
        }
        authority
    }

    /// Whether the document declares `name` anywhere.
    pub(crate) fn declares(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Whether the document declares an action or `def` named `name`.
    pub(crate) fn declares_function(&self, name: &str) -> bool {
        self.functions.contains(name)
    }

    /// Locate one unlocated declaration of `name` with `role` and `owner`;
    /// `false` when the AST declares no such name there.
    pub(crate) fn locate(&mut self, name: &str, role: SymbolRole, owner: Option<&str>) -> bool {
        let key = (name.to_owned(), role, owner.map(str::to_owned));
        match self.remaining.get_mut(&key) {
            Some(count) if *count > 0 => {
                *count -= 1;
                true
            }
            _ => false,
        }
    }
}

fn spec_item(item: &SpecItem, out: &mut Vec<AstDeclaration>) {
    match item {
        SpecItem::Const { name, .. } => {
            out.push(AstDeclaration::keyword(name, SymbolRole::Variable));
        }
        SpecItem::Def { name, params, .. } => {
            out.push(AstDeclaration::keyword(name, SymbolRole::Function));
            for (param, _) in params {
                out.push(AstDeclaration::positional(
                    param,
                    SymbolRole::Parameter,
                    Some(name),
                ));
            }
        }
        SpecItem::Type { name, .. } | SpecItem::Entity(name, _) | SpecItem::Number(name, _) => {
            out.push(AstDeclaration::keyword(name, SymbolRole::Type));
        }
        SpecItem::Enum { name, members, .. } => {
            out.push(AstDeclaration::keyword(name, SymbolRole::Type));
            for member in members {
                out.push(AstDeclaration::positional(
                    member,
                    SymbolRole::Variable,
                    None,
                ));
            }
        }
        SpecItem::Struct { name, fields, .. } => {
            out.push(AstDeclaration::keyword(name, SymbolRole::Type));
            for (field, _) in fields {
                out.push(AstDeclaration::positional(
                    field,
                    SymbolRole::Property,
                    None,
                ));
            }
        }
        SpecItem::State(fields) => {
            for field in fields {
                out.push(AstDeclaration::positional(
                    &field.name,
                    SymbolRole::Variable,
                    None,
                ));
            }
        }
        SpecItem::Action {
            name,
            params: action_params,
            items,
            ..
        } => {
            out.push(AstDeclaration::keyword(name, SymbolRole::Function));
            params(action_params, name, out);
            action_items(items, name, out);
        }
        SpecItem::Invariant { name, .. }
        | SpecItem::Trans { name, .. }
        | SpecItem::Reachable { name, .. }
        | SpecItem::Until { name, .. }
        | SpecItem::Unless { name, .. }
        | SpecItem::LeadsTo { name, .. } => {
            out.push(AstDeclaration::keyword(name, SymbolRole::Property));
        }
        SpecItem::Init { .. } | SpecItem::Terminal { .. } | SpecItem::VerifyBounds { .. } => {}
    }
}

fn requirements_item(item: &RequirementsItem, out: &mut Vec<AstDeclaration>) {
    match item {
        RequirementsItem::Requirement { items, .. } => {
            for item in items {
                match item {
                    RequirementBlockItem::Action(action) => requirement_action(action, out),
                    RequirementBlockItem::Property(item) => spec_item(item, out),
                    RequirementBlockItem::Deadline { .. } => {}
                }
            }
        }
        RequirementsItem::Action(action) => requirement_action(action, out),
        RequirementsItem::Common(item) => spec_item(item, out),
        // Requirement, acceptance and forbidden IDs (`REQ-1`) are labels, not
        // identifiers; `process`/`kpi` and `time` belong to the business
        // projection, which is not covered yet. A glossary declares no
        // name: its aliases resolve to names declared elsewhere (the
        // language server's alias support is a later stage).
        RequirementsItem::Glossary(_)
        | RequirementsItem::Implements { .. }
        | RequirementsItem::Acceptance { .. }
        | RequirementsItem::Forbidden { .. }
        | RequirementsItem::Process(_)
        | RequirementsItem::Kpi(_)
        | RequirementsItem::Time { .. } => {}
    }
}

fn requirement_action(action: &RequirementAction, out: &mut Vec<AstDeclaration>) {
    out.push(AstDeclaration::keyword(&action.name, SymbolRole::Function));
    params(&action.params, &action.name, out);
    for item in &action.items {
        if let RequirementActionItem::Action(item) = item {
            action_items(std::slice::from_ref(item.as_ref()), &action.name, out);
        }
    }
}

fn params(params: &[Param], owner: &str, out: &mut Vec<AstDeclaration>) {
    for param in params {
        let (Param::Typed(name, _) | Param::Range(name, _, _)) = param;
        out.push(AstDeclaration::positional(
            name,
            SymbolRole::Parameter,
            Some(owner),
        ));
    }
}

fn action_items(items: &[ActionItem], owner: &str, out: &mut Vec<AstDeclaration>) {
    for item in items {
        if let ActionItem::Let(name, _, _) = item {
            out.push(AstDeclaration::positional(
                name,
                SymbolRole::Variable,
                Some(owner),
            ));
        }
    }
}

/// An AST declaration the index does not declare with the same name, role and
/// owning scope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleScopeMismatch {
    pub expected: AstDeclaration,
    /// The role and owner of every index symbol with the same name.
    pub found: Vec<(SymbolRole, Option<String>)>,
}

impl fmt::Display for RoleScopeMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scoped = |role: SymbolRole, owner: Option<&str>| {
            owner.map_or_else(
                || role.detail().to_owned(),
                |owner| format!("{} in {owner}", role.detail()),
            )
        };
        let found = if self.found.is_empty() {
            "no declaration".to_owned()
        } else {
            let mut found = self
                .found
                .iter()
                .map(|(role, owner)| scoped(*role, owner.as_deref()))
                .collect::<Vec<_>>();
            found.sort();
            found.dedup();
            found.join(", ")
        };
        write!(
            formatter,
            "`{}` should be a {} but the index has {found}",
            self.expected.name,
            scoped(self.expected.role, self.expected.owner.as_deref())
        )
    }
}

impl DocumentIndex {
    /// Compare the index's declarations with the authoritative AST's.
    ///
    /// `misprojected_declarations` only asks whether a name after a
    /// declaration keyword is declared at all, and `unindexed_identifiers`
    /// only whether an identifier has some entry. Neither notices a name
    /// declared with the wrong role or attached to the wrong action, which is
    /// what hover, completion and same-name resolution read. The expectation
    /// here comes from the parser, not from the index's keyword table, so the
    /// check does not share the heuristic's blind spots.
    ///
    /// Each expected declaration consumes one matching symbol, so a name the
    /// AST declares twice must be declared twice. The index declares a
    /// projected name only where the walk locates it with the AST's role and
    /// owner, so for a covered document a mismatch is a declaration the walk
    /// could not locate — and therefore one hover, definition and rename
    /// cannot reach.
    #[must_use]
    pub fn role_scope_mismatches(&self, expected: &[AstDeclaration]) -> Vec<RoleScopeMismatch> {
        let mut available = self
            .symbols
            .iter()
            .map(|symbol| Some((symbol.name.as_str(), symbol.role, symbol.owner.as_deref())))
            .collect::<Vec<_>>();
        expected
            .iter()
            .filter_map(|declaration| {
                let wanted = Some((
                    declaration.name.as_str(),
                    declaration.role,
                    declaration.owner.as_deref(),
                ));
                if let Some(slot) = available.iter_mut().find(|slot| **slot == wanted) {
                    *slot = None;
                    return None;
                }
                Some(RoleScopeMismatch {
                    expected: declaration.clone(),
                    found: self
                        .symbols
                        .iter()
                        .filter(|symbol| symbol.name == declaration.name)
                        .map(|symbol| (symbol.role, symbol.owner.clone()))
                        .collect(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> (Vec<AstDeclaration>, DocumentIndex) {
        let parsed =
            fsl_syntax::parse_document(fsl_syntax::SourceFile::new(source)).expect("valid source");
        (
            ast_declarations(&parsed.surface).expect("spec family"),
            DocumentIndex::build(source, None, &fsl_syntax::ParseOptions::stable())
                .expect("valid index"),
        )
    }

    #[test]
    fn projects_roles_and_owners_from_the_surface_ast() {
        let source = "spec Cart {
  const Max = 2
  type Item = 0..1
  enum Mode { Open, Closed }
  struct Line { qty: Int }
  state { stock: Int, mode: Mode }
  init { stock = 0  mode = Open }
  def bump(n: Int) = n + 1
  action add(item: Item) {
    let next = stock + 1
    stock = next
  }
  invariant NonNegative { stock >= 0 }
}";
        let (expected, index) = parse(source);
        let owned = |name: &str| Some(name.to_owned());
        assert_eq!(
            expected
                .iter()
                .map(|declaration| (
                    declaration.name.as_str(),
                    declaration.role,
                    declaration.owner.clone(),
                    declaration.introduced_by_keyword,
                ))
                .collect::<Vec<_>>(),
            vec![
                ("Cart", SymbolRole::Namespace, None, true),
                ("Max", SymbolRole::Variable, None, true),
                ("Item", SymbolRole::Type, None, true),
                ("Mode", SymbolRole::Type, None, true),
                ("Open", SymbolRole::Variable, None, false),
                ("Closed", SymbolRole::Variable, None, false),
                ("Line", SymbolRole::Type, None, true),
                ("qty", SymbolRole::Property, None, false),
                ("stock", SymbolRole::Variable, None, false),
                ("mode", SymbolRole::Variable, None, false),
                ("bump", SymbolRole::Function, None, true),
                ("n", SymbolRole::Parameter, owned("bump"), false),
                ("add", SymbolRole::Function, None, true),
                ("item", SymbolRole::Parameter, owned("add"), false),
                ("next", SymbolRole::Variable, owned("add"), false),
                ("NonNegative", SymbolRole::Property, None, true),
            ]
        );
        assert_eq!(index.role_scope_mismatches(&expected), Vec::new());
    }

    #[test]
    fn a_wrong_owner_or_role_is_a_mismatch_even_when_the_name_is_declared() {
        let source = "spec S {
  state { x: Int }
  init { x = 0 }
  action a(p: Int) { x = p }
}";
        let (mut expected, index) = parse(source);
        assert_eq!(index.role_scope_mismatches(&expected), Vec::new());

        for declaration in &mut expected {
            match declaration.name.as_str() {
                "p" => declaration.owner = Some("b".to_owned()),
                "x" => declaration.role = SymbolRole::Property,
                _ => {}
            }
        }
        assert_eq!(
            index
                .role_scope_mismatches(&expected)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec![
                "`x` should be a property but the index has variable",
                "`p` should be a parameter in b but the index has parameter in a",
            ]
        );
    }

    /// Issue 1362: FSL reserves none of the index's keywords, so a name
    /// spelled like one (`status`, `owner`, `severity`, `evidence`,
    /// `rejected`, `time`, `age`, `count`) in a state, field, member,
    /// parameter or `let` position is declared like any other name, and each
    /// of its uses — a field access, a struct literal field, a call of a
    /// keyword-spelled `def` — resolves to that declaration.
    #[test]
    fn keyword_spelled_names_in_every_list_and_binder_position_are_names() {
        let source = "spec Contextual {
  type Id = 0..1
  enum Level { low, severity }
  struct Case { owner: Id }
  state { status: Level, cases: Map<Id, Case> }
  init {
    status = low
    forall i: Id { cases[i] = Case { owner: 0 } }
  }
  def age(evidence: Id) = evidence + 1
  action count(rejected: Id) {
    let time = rejected
    requires cases[time].owner == 0
    status = severity
  }
  invariant Kept { age(0) >= 0 and (status == low or status == severity) }
}";
        let (expected, index) = parse(source);
        assert_eq!(index.role_scope_mismatches(&expected), Vec::new());

        let spelled = [
            "status", "owner", "severity", "evidence", "rejected", "time", "age", "count",
        ];
        let mut uses = 0;
        for token in fsl_syntax::lex(source).expect("lex") {
            let fsl_syntax::TokenKind::Ident(name) = &token.kind else {
                continue;
            };
            if !spelled.contains(&name.as_str()) {
                continue;
            }
            let position = crate::index::span_range(source, token.span).start;
            let definition = index
                .definition_at(position)
                .unwrap_or_else(|| panic!("`{name}` at {position:?} resolves to nothing"));
            assert_eq!(&definition.name, name, "at {position:?}");
            uses += 1;
        }
        assert_eq!(uses, 20);
        let status = index
            .symbols
            .iter()
            .find(|symbol| symbol.name == "status")
            .expect("status declared");
        assert_eq!(
            index
                .references_at(status.selection_range.start, true)
                .len(),
            5
        );
    }
}

// SPDX-License-Identifier: Apache-2.0

//! Declarations projected from the authoritative surface AST (issue 552, B).
//!
//! `DocumentIndex::build` derives its symbols by walking tokens. This module
//! is the other half of the structure `docs/design/DESIGN-rust-lsp.md` describes: it
//! reads the declarations the parser accepted — name, role and owning scope —
//! straight from `fsl_syntax::SurfaceDocument`, without looking at a token.
//! `DocumentIndex::role_scope_mismatches` compares the two, so a token
//! heuristic that declares a name with the wrong role or in the wrong scope
//! fails the corpus test instead of silently degrading hover, completion and
//! rename. The index itself does not read this projection yet.
//!
//! Coverage is the spec family — `spec`, `requirements` and `compose` — whose
//! declarations share `SpecItem`. Other dialects return `None`. Extending the
//! projection dialect by dialect is the migration path to issue 552's option
//! C, where the index takes its declarations from here and keeps tokens only
//! for spans the AST does not retain.

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
        // projection, which is not covered yet.
        RequirementsItem::Implements { .. }
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
    /// The name is spelled like a word the index treats as a keyword.
    pub keyword_spelled: bool,
}

impl RoleScopeMismatch {
    /// FSL has no reserved words, but the index's token walk skips every
    /// identifier spelled like one of its keywords (`status`, `count`, `age`,
    /// ...) except the name right after a declaration keyword. A state
    /// variable, struct field, enum member, parameter or `let` binding so
    /// spelled therefore gets no declaration at all.
    ///
    /// This is the one structural limitation of the token walk that the LSP
    /// still ships; repairing it changes index output and belongs to issue
    /// 552's option C. Anything else — a wrong role, a wrong owner, or a
    /// missing name that is not keyword-spelled or that follows its own
    /// declaration keyword — is drift the corpus test rejects.
    #[must_use]
    pub fn is_keyword_spelled_positional_name(&self) -> bool {
        self.keyword_spelled && !self.expected.introduced_by_keyword && self.found.is_empty()
    }
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
    /// AST declares twice must be declared twice.
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
                    keyword_spelled: crate::index::is_keyword(&declaration.name),
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
            DocumentIndex::build(source, None).expect("valid index"),
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

    #[test]
    fn only_a_keyword_spelled_name_in_list_position_is_the_known_limitation() {
        // `status` is in the index keyword set and `push` is a sequence
        // builtin; FSL reserves neither. `push` follows its own declaration
        // keyword and is declared; `status` sits in the state list and is not.
        let source = "spec S {
  state { status: Int }
  init { status = 0 }
  action push() { status = 1 }
}";
        let (expected, index) = parse(source);
        let found = index.role_scope_mismatches(&expected);
        assert_eq!(
            found
                .iter()
                .map(|mismatch| (
                    mismatch.expected.name.as_str(),
                    mismatch.is_keyword_spelled_positional_name()
                ))
                .collect::<Vec<_>>(),
            vec![("status", true)]
        );
    }
}

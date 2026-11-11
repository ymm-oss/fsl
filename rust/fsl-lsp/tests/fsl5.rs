// SPDX-License-Identifier: Apache-2.0

//! The index of a next-edition (`fsl 5`) requirements document (fsl 5 plan
//! §3 S11): glossary aliases are read as their canonical names, Japanese
//! keywords as keywords and particles as particles, so the English, the
//! alias-and-particle and the all-Japanese spelling of one document declare
//! the same names.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fsl_lsp::{DocumentIndex, SymbolRole};
use fsl_syntax::keywords::{KEYWORDS, PARTICLES, Status};
use fsl_syntax::{ParseOptions, TokenKind};
use lsp_types::Position;

fn e2e() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust directory")
        .join("fslc/tests/fixtures/fsl5/e2e")
}

const SPELLINGS: [&str; 3] = ["fsl5_en.fsl", "fsl5_ja_particles.fsl", "fsl5_ja_full.fsl"];

fn index(file: &str) -> (String, DocumentIndex) {
    let source = std::fs::read_to_string(e2e().join(file)).expect("read fixture");
    let index = DocumentIndex::build(&source, Some(file), &ParseOptions::stable())
        .unwrap_or_else(|error| panic!("{file}: {error}"));
    (source, index)
}

/// Every word of the document is a keyword, a particle, a declaration or a
/// use; no Japanese keyword and no particle is indexed as a name; and the
/// three spellings declare the same canonical names with the same roles.
#[test]
fn the_three_spellings_of_one_document_declare_the_same_canonical_names() {
    let japanese = KEYWORDS
        .iter()
        .filter(|keyword| keyword.status == Status::Active)
        .map(|keyword| keyword.japanese)
        .chain(PARTICLES.iter().map(|(_, spelling)| *spelling))
        .collect::<BTreeSet<_>>();
    let mut declared = Vec::new();
    for file in SPELLINGS {
        let (source, index) = index(file);
        assert_eq!(
            index.unindexed_identifiers(),
            Vec::<String>::new(),
            "{file}"
        );
        assert_eq!(
            index.misprojected_declarations(),
            Vec::<String>::new(),
            "{file}"
        );
        let named = index
            .symbols
            .iter()
            .map(|symbol| symbol.selection_range.start)
            .chain(
                index
                    .references
                    .iter()
                    .map(|reference| reference.range.start),
            )
            .collect::<BTreeSet<_>>();
        let mut keywords = 0;
        for token in fsl_syntax::lex_with(&source, &ParseOptions::stable()).expect("lex") {
            let TokenKind::Ident(word) = &token.kind else {
                continue;
            };
            if japanese.contains(word.as_str()) {
                keywords += 1;
                let position = fsl_lsp::span_range(&source, token.span).start;
                assert!(
                    !named.contains(&position),
                    "{file}: the keyword `{word}` at {position:?} is indexed as a name"
                );
            }
        }
        if file != "fsl5_en.fsl" {
            assert!(keywords > 20, "{file}: only {keywords} Japanese words");
        }
        declared.push((
            file,
            index
                .symbols
                .iter()
                .map(|symbol| (symbol.name.clone(), symbol.role))
                .collect::<BTreeSet<_>>(),
        ));
    }
    let (_, english) = &declared[0];
    for expected in [
        ("ExpenseRequirements", SymbolRole::Namespace),
        ("Amount", SymbolRole::Type),
        ("AUTO_LIMIT", SymbolRole::Variable),
        ("Employee", SymbolRole::Variable),
        ("Claim", SymbolRole::Type),
        ("amount", SymbolRole::Property),
        ("Draft", SymbolRole::Variable),
        ("Paid", SymbolRole::Variable),
        ("SmallClaim", SymbolRole::Type),
        ("submit", SymbolRole::Function),
        ("paid_claims", SymbolRole::Variable),
    ] {
        assert!(
            english.contains(&(expected.0.to_owned(), expected.1)),
            "{expected:?} in {english:?}"
        );
    }
    for (file, names) in &declared[1..] {
        assert_eq!(names, english, "{file}");
    }
}

/// Every next-edition fixture is indexed in full: each word is a keyword, a
/// particle, a declaration or a use, and each name after a declaration
/// keyword is declared.
#[test]
fn every_next_edition_fixture_is_indexed_in_full() {
    let root = e2e().parent().expect("fsl5 fixtures").to_path_buf();
    let mut files = Vec::new();
    for directory in std::fs::read_dir(&root).expect("read fixtures") {
        for entry in std::fs::read_dir(directory.expect("entry").path()).expect("read fixture") {
            let path = entry.expect("entry").path();
            let source = std::fs::read_to_string(&path).expect("read fixture");
            if path.extension().and_then(|extension| extension.to_str()) == Some("fsl")
                && source.starts_with("fsl 5")
            {
                files.push((path, source));
            }
        }
    }
    assert!(files.len() >= 12, "{files:?}");
    for (path, source) in files {
        let index = DocumentIndex::build(&source, path.to_str(), &ParseOptions::stable())
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            index.lexicon(),
            fsl_lsp::Lexicon::Next,
            "{}",
            path.display()
        );
        assert_eq!(
            index.unindexed_identifiers(),
            Vec::<String>::new(),
            "{}",
            path.display()
        );
        assert_eq!(
            index.misprojected_declarations(),
            Vec::<String>::new(),
            "{}",
            path.display()
        );
    }
}

/// #1815: a decision table `for Claim in Submitted` / `対象 申請 段階 提出済み`
/// indexes like `for Submitted Claim`: the same declarations, its entity and
/// stage are uses of theirs, the name has as many references, and `in` /
/// `段階` is a keyword, not a name.
#[test]
fn for_entity_in_stage_uses_the_entity_and_the_stage() {
    let declared = |index: &DocumentIndex| {
        index
            .symbols
            .iter()
            .map(|symbol| (symbol.name.clone(), symbol.role))
            .collect::<BTreeSet<_>>()
    };
    for (file, stage_first, entity_first, words) in [
        (
            "fsl5_en.fsl",
            "for Submitted Claim {",
            "for Claim in Submitted {",
            [("Claim", "Claim"), ("in", ""), ("Submitted", "Submitted")],
        ),
        (
            "fsl5_ja_full.fsl",
            "対象 提出済み 申請 {",
            "対象 申請 段階 提出済み {",
            [("申請", "Claim"), ("段階", ""), ("提出済み", "Submitted")],
        ),
    ] {
        let (source, original) = index(file);
        assert_eq!(source.matches(stage_first).count(), 1, "{file}");
        let rewritten = source.replacen(stage_first, entity_first, 1);
        let index = DocumentIndex::build(&rewritten, Some(file), &ParseOptions::stable())
            .unwrap_or_else(|error| panic!("{file}: {error}"));
        assert_eq!(
            index.unindexed_identifiers(),
            Vec::<String>::new(),
            "{file}"
        );
        assert_eq!(declared(&index), declared(&original), "{file}");
        let line = rewritten
            .lines()
            .position(|line| line.contains(entity_first))
            .expect("the table's header");
        let header = rewritten.lines().nth(line).expect("header line");
        let from = header.find(entity_first).expect("on its line");
        for (word, canonical) in words {
            let byte = from + entity_first.find(word).expect("the word");
            let position = Position::new(
                u32::try_from(line).expect("fits"),
                u32::try_from(header[..byte].encode_utf16().count()).expect("fits"),
            );
            let used = index
                .references
                .iter()
                .find(|reference| reference.range.start == position);
            if canonical.is_empty() {
                assert!(used.is_none(), "{file}: `{word}` is indexed as {used:?}");
                assert!(
                    index.definition_at(position).is_none(),
                    "{file}: `{word}` names something"
                );
                continue;
            }
            assert_eq!(
                used.map(|reference| reference.name.as_str()),
                Some(canonical),
                "{file}: `{word}`"
            );
            let declaration = index
                .definition_at(position)
                .unwrap_or_else(|| panic!("{file}: `{word}` has a declaration"));
            assert_eq!(declaration.name, canonical, "{file}");
            let before = original
                .symbols
                .iter()
                .find(|symbol| symbol.name == canonical)
                .expect("declared before");
            assert_eq!(
                index
                    .references_at(declaration.selection_range.start, true)
                    .len(),
                original
                    .references_at(before.selection_range.start, true)
                    .len(),
                "{file}: references of {canonical}"
            );
        }
    }
}

/// #1815: a rule's `never … for Claim in Approved` indexes as
/// `for Approved Claim` does: the same declarations and the same uses, and
/// `in` is not a name.
#[test]
fn a_rules_for_entity_in_stage_indexes_as_stage_entity() {
    let (source, _) = index("fsl5_en.fsl");
    let anchor = "    allow Employee submits Claim only when input amount > 0\n";
    let build = |condition: &str| {
        assert_eq!(source.matches(anchor).count(), 1);
        let rewritten = source.replacen(
            anchor,
            &format!("{anchor}    never Finance pays Claim for {condition}\n"),
            1,
        );
        let index = DocumentIndex::build(&rewritten, Some("fsl5_en.fsl"), &ParseOptions::stable())
            .unwrap_or_else(|error| panic!("{condition}: {error}"));
        assert_eq!(
            index.unindexed_identifiers(),
            Vec::<String>::new(),
            "{condition}"
        );
        let symbols = index
            .symbols
            .iter()
            .map(|symbol| (symbol.name.clone(), symbol.role))
            .collect::<BTreeSet<_>>();
        let mut uses = index
            .references
            .iter()
            .map(|reference| reference.name.clone())
            .collect::<Vec<_>>();
        uses.sort();
        (symbols, uses)
    };
    let (stage_first, entity_first) = (build("Approved Claim"), build("Claim in Approved"));
    assert!(!entity_first.1.contains(&"in".to_owned()));
    assert_eq!(entity_first, stage_first);
}

/// The relations stage's declarations: a relation's role is a property of
/// the entity, `resource` and `measure` declare variables, `goal` and
/// `policy` properties; and the English and all-Japanese spellings of the
/// multi-entity example declare the same canonical names.
#[test]
fn relations_resources_measures_goals_and_policies_declare_their_names() {
    let fixtures = e2e().parent().expect("fsl5 fixtures").to_path_buf();
    let read = |file: &str| {
        let source = std::fs::read_to_string(fixtures.join(file)).expect("read fixture");
        DocumentIndex::build(&source, Some(file), &ParseOptions::stable())
            .unwrap_or_else(|error| panic!("{file}: {error}"))
    };
    let declared = |index: &DocumentIndex| {
        index
            .symbols
            .iter()
            .map(|symbol| (symbol.name.clone(), symbol.role))
            .collect::<BTreeSet<_>>()
    };
    let relations = read("relations/fsl5_en.fsl");
    for expected in [
        ("assignee", SymbolRole::Property),
        ("desk_open", SymbolRole::Variable),
        ("Resolved", SymbolRole::Property),
        ("Closable", SymbolRole::Property),
    ] {
        assert!(
            declared(&relations).contains(&(expected.0.to_owned(), expected.1)),
            "{expected:?}"
        );
    }
    // `has assignee` and the steps' `with assignee 1` use the role.
    let role = relations
        .symbols
        .iter()
        .find(|symbol| symbol.name == "assignee")
        .expect("the role");
    assert_eq!(
        relations
            .references_at(role.selection_range.start, false)
            .len(),
        4
    );
    let english = read("multi_entity/fsl5_en.fsl");
    let japanese = read("multi_entity/fsl5_ja_full.fsl");
    for expected in [
        ("buyer", SymbolRole::Property),
        ("stock", SymbolRole::Variable),
        ("paid_count", SymbolRole::Variable),
        ("RefundedOne", SymbolRole::Property),
    ] {
        assert!(
            declared(&english).contains(&(expected.0.to_owned(), expected.1)),
            "{expected:?}"
        );
    }
    assert_eq!(declared(&japanese), declared(&english));
}

/// An alias is a use of its canonical name: definition goes from the alias
/// to the declaration, and the references of the declaration are every
/// spelling of the name — the canonical one in the glossary's left side and
/// in `implements`, and the alias everywhere else.
#[test]
fn an_alias_resolves_to_its_canonical_declaration() {
    let (source, index) = index("fsl5_ja_full.fsl");
    let lines = source.lines().collect::<Vec<_>>();
    let written = |position: Position, length: usize| {
        lines[position.line as usize]
            .encode_utf16()
            .skip(position.character as usize)
            .take(length)
            .collect::<Vec<_>>()
    };
    let claim = index
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Claim")
        .expect("Claim is declared");
    assert_eq!(claim.role, SymbolRole::Type);
    assert_eq!(claim.alias.as_deref(), Some("申請"));
    let alias_uses = index
        .references
        .iter()
        .filter(|reference| reference.alias.as_deref() == Some("申請"))
        .collect::<Vec<_>>();
    // The glossary's right side, two concepts, five events, the rule, the
    // decision table, the kpi and `個数 申請` in verify.
    assert_eq!(alias_uses.len(), 12, "{alias_uses:#?}");
    for reference in &alias_uses {
        assert_eq!(reference.name, "Claim");
        assert_eq!(index.definition_at(reference.range.start), Some(claim));
    }
    let occurrences = index.references_at(claim.selection_range.start, true);
    let canonical = occurrences
        .iter()
        .filter(|range| written(range.start, 5) == "Claim".encode_utf16().collect::<Vec<_>>())
        .count();
    let aliased = occurrences
        .iter()
        .filter(|range| written(range.start, 2) == "申請".encode_utf16().collect::<Vec<_>>())
        .count();
    // `Claim = 申請` and `Claim.amount = 申請金額` spell it canonically.
    assert_eq!((canonical, aliased), (2, 13), "{occurrences:#?}");

    // `Claim.amount = 申請金額` names the field, and its alias resolves to it.
    let amount = index
        .symbols
        .iter()
        .find(|symbol| symbol.name == "amount")
        .expect("the field is declared");
    assert_eq!(amount.alias.as_deref(), Some("申請金額"));
    let qualified = index
        .references
        .iter()
        .find(|reference| reference.name == "amount" && reference.alias.is_none())
        .expect("the glossary's left side");
    assert_eq!(qualified.qualifier.as_deref(), Some("Claim"));
    assert_eq!(index.definition_at(qualified.range.start), Some(amount));
}

/// The left side of an `implements` mapping names this document's event and
/// the right side the implemented layer's action, as in a refinement file:
/// the mapping's parameters are its own, not the entity's field `amount`.
#[test]
fn an_implements_mapping_names_the_event_and_the_other_layers_action() {
    let (_, index) = index("fsl5_en.fsl");
    let event = index
        .symbols
        .iter()
        .find(|symbol| symbol.name == "submit")
        .expect("event submit");
    assert_eq!(event.role, SymbolRole::Function);
    let mut uses = index
        .references
        .iter()
        .filter(|reference| reference.name == "submit")
        .collect::<Vec<_>>();
    uses.sort_by_key(|reference| (reference.range.start.line, reference.range.start.character));
    assert_eq!(uses.len(), 2, "{uses:#?}");
    assert_eq!(index.definition_at(uses[0].range.start), Some(event));
    assert_eq!(uses[1].target_spec(), Some("ExpenseToBe"));
    let field = index
        .symbols
        .iter()
        .find(|symbol| symbol.name == "amount" && symbol.role == SymbolRole::Property)
        .expect("field amount");
    let field_uses = index.references_at(field.selection_range.start, false);
    assert!(
        field_uses.iter().all(|range| range.start.line != 7),
        "the mapping's parameter is not the field: {field_uses:#?}"
    );
}

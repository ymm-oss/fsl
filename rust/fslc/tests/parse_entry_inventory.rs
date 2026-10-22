// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Parse-entry inventory (decisions/049 D2, `docs/design/parse-entry-inventory.md`).
//!
//! Every document parse must receive the process's `ParseOptions`; a parse
//! that silently uses the published language while the rest of the command
//! runs behind the unstable gate would read one file two ways. The `_with`
//! entries (`parse_document_with`, `parse_surface_document_with`,
//! `dialect_keyword_with`, `lex_with`) take the options explicitly. Every
//! other fsl-syntax entry that reads source text is *stable-pinned*: it
//! parses the published language only, which is fail-closed (a document that
//! needs the gate is rejected there, never read differently), but it is still
//! a place a later stage must convert. fsl-core's published-language wrappers
//! (each beside a `_with_options` entry) are counted the same way.
//!
//! This test reads every non-test Rust source of the workspace as a syntax
//! tree and counts, per file, each reference to a stable-pinned entry and each
//! explicit construction of stable options. The counts must equal
//! [`INVENTORY`] exactly: a new reference, a removed one, or a reference
//! through an alias or a glob import fails, and the failure names the file so
//! the inventory row (with its reason and owning stage) is updated in the
//! same change as the code.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use syn::visit::Visit;

/// fsl-syntax entries that read source text without taking `ParseOptions`.
///
/// `parse_expr` is deliberately absent: it parses one expression fragment
/// that has no document header, so no gate decision can depend on it.
const STABLE_ENTRIES: &[&str] = &[
    "parse_document",
    "parse_surface_document",
    "parse_surface_spec",
    "dialect_keyword",
    "declaration_keyword",
    "lex",
    "lossless_document",
    "format_source",
    "canonical_rewrites",
    "parse_causal",
    "is_causal_source",
    "parse_ai_project",
    "parse_ai_component",
    "parse_domain",
    "parse_db_system",
];

/// fsl-core entries that read source text under the published language: the
/// wrappers kept beside each `_with_options` entry, which takes the options.
/// Counted like the fsl-syntax entries, so a product caller cannot reach a
/// stable parse through fsl-core either.
const CORE_STABLE_ENTRIES: &[&str] = &[
    "parse_kernel_source",
    "parse_kernel_source_with_file",
    "parse_kernel_source_with_bounds",
    "parse_abstraction_kernel_source_with_bounds",
    "filter_scope_overrides_for_abstraction",
    "retain_dialect_scope_overrides",
    "lower_compose",
    "parse_direct_kernel_spec",
    "parse_refinement",
    "requirements_implements",
    "requirements_implements_with_bounds",
    "requirements_trace_contract",
    "verify_scope_type_names",
    "requirements_declared_ids",
    "requirements_has_implements",
    "governance_contract",
];

/// The crates whose stable entries are counted: `(crate name, workspace
/// directory, entries)`.
const OWNERS: &[(&str, &str, &[&str])] = &[
    ("fsl_syntax", "fsl-syntax/", STABLE_ENTRIES),
    ("fsl_core", "fsl-core/", CORE_STABLE_ENTRIES),
];

/// Explicit constructions of the published-language options. Counted like an
/// entry so `parse_document_with(source, &ParseOptions::stable())` cannot
/// pass for a routed parse.
const STABLE_CONSTRUCTIONS: &[&str] = &["ParseOptions::stable", "ParseOptions::default"];

/// Owning stage for converting a stable-pinned reference.
#[derive(Clone, Copy, Debug)]
enum Owner {
    /// The published-language wrapper itself, a fixed published-language
    /// input, or a parity tool of the published language; stays.
    Definition,
    /// decisions/049 D8: this dialect rejects an edition header, so it stays
    /// on the published language.
    D8,
}

const CAUSAL_AI: &str = "causal or ai dialect: decisions/049 D8 makes an edition header an error \
     there, so the published language is the intended reading";
const STANDALONE_FRONTEND: &str = "standalone public parser of one dialect with no product \
     caller (the registry frontend reuses the dispatcher's routed tokens instead); it never sees \
     the options, so a document needing the wider identifier rule is rejected here (fail closed)";
const FRAGMENT: &str = "expression / spec fragment parser with no document header; the wider \
     identifier rule is selected by the header, so a fragment always reads ASCII identifiers";
const PARITY_TOOL: &str = "Python-parity debug binary: it compares against the frozen reference, \
     which reads only the published language, so it never takes the gate";
const REGISTRY: &str = "the private `domain` frontend wrapper shares the public entry's name; \
     it is reached only through parse_document_with";
const STABLE_WRAPPER: &str = "the published-language entry itself: the options-taking entry \
     under ParseOptions::stable";
const DIRECT_KERNEL: &str = "parse_direct_kernel_spec keeps the published language: its only \
     callers are tests";
const FIXTURE: &str = "reads a fixed in-repository fixture written in the published language, \
     never a user document";

/// `(workspace-relative file, entry, count, owner, reason)`.
const INVENTORY: &[(&str, &str, usize, Owner, &str)] = &[
    (
        "fsl-core/src/bin/fsl-parse-kernel.rs",
        "parse_kernel_source",
        1,
        Owner::Definition,
        PARITY_TOOL,
    ),
    (
        "fsl-core/src/compose.rs",
        "ParseOptions::stable",
        3,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-core/src/dialect.rs",
        "ParseOptions::stable",
        5,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-core/src/lib.rs",
        "ParseOptions::stable",
        4,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-core/src/lib.rs",
        "parse_document",
        1,
        Owner::Definition,
        DIRECT_KERNEL,
    ),
    (
        "fsl-core/src/refinement.rs",
        "ParseOptions::stable",
        2,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-core/src/refinement.rs",
        "requirements_implements_with_bounds",
        1,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-lsp/src/index.rs",
        "is_causal_source",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fsl-runtime/src/bin/fsl-bfs.rs",
        "parse_kernel_source",
        1,
        Owner::Definition,
        PARITY_TOOL,
    ),
    (
        "fsl-runtime/src/bin/fsl-explicit.rs",
        "parse_kernel_source",
        1,
        Owner::Definition,
        PARITY_TOOL,
    ),
    (
        "fsl-runtime/src/bin/fsl-refine.rs",
        "parse_kernel_source",
        1,
        Owner::Definition,
        PARITY_TOOL,
    ),
    (
        "fsl-runtime/src/bin/fsl-refine.rs",
        "parse_refinement",
        1,
        Owner::Definition,
        PARITY_TOOL,
    ),
    ("fsl-syntax/src/ai.rs", "lex", 1, Owner::D8, CAUSAL_AI),
    (
        "fsl-syntax/src/ai_project.rs",
        "parse_ai_component",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fsl-syntax/src/bin/fsl-parse-surface.rs",
        "parse_surface_document",
        1,
        Owner::Definition,
        PARITY_TOOL,
    ),
    (
        "fsl-syntax/src/causal.rs",
        "declaration_keyword",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    ("fsl-syntax/src/causal.rs", "lex", 1, Owner::D8, CAUSAL_AI),
    (
        "fsl-syntax/src/db.rs",
        "lex",
        1,
        Owner::Definition,
        STANDALONE_FRONTEND,
    ),
    (
        "fsl-syntax/src/dispatch.rs",
        "ParseOptions::stable",
        2,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-syntax/src/dispatch.rs",
        "parse_domain",
        1,
        Owner::Definition,
        REGISTRY,
    ),
    (
        "fsl-syntax/src/domain.rs",
        "lex",
        1,
        Owner::Definition,
        STANDALONE_FRONTEND,
    ),
    (
        "fsl-syntax/src/lexer.rs",
        "ParseOptions::stable",
        1,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-syntax/src/lossless.rs",
        "ParseOptions::stable",
        3,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-syntax/src/parser.rs",
        "ParseOptions::stable",
        1,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-syntax/src/parser.rs",
        "lex",
        2,
        Owner::Definition,
        FRAGMENT,
    ),
    (
        "fsl-tools/src/causal.rs",
        "ParseOptions::stable",
        1,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-tools/src/causal.rs",
        "parse_causal",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fsl-tools/src/document_project.rs",
        "ParseOptions::stable",
        2,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fsl-tools/src/document_render.rs",
        "ParseOptions::stable",
        1,
        Owner::Definition,
        STABLE_WRAPPER,
    ),
    (
        "fslc/src/bin/fsl-bmc.rs",
        "parse_kernel_source",
        1,
        Owner::Definition,
        PARITY_TOOL,
    ),
    (
        "fslc/src/bin/fsl-replay-actions.rs",
        "parse_kernel_source",
        1,
        Owner::Definition,
        PARITY_TOOL,
    ),
    (
        "fslc/src/causal.rs",
        "parse_causal",
        2,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fslc/src/coverage.rs",
        "parse_kernel_source",
        1,
        Owner::Definition,
        FIXTURE,
    ),
    (
        "fslc/src/frontend_output.rs",
        "declaration_keyword",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fslc/src/frontend_output.rs",
        "parse_ai_project",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fslc/src/main.rs",
        "is_causal_source",
        2,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fslc/src/main.rs",
        "parse_ai_project",
        3,
        Owner::D8,
        CAUSAL_AI,
    ),
    ("fslc/src/main.rs", "parse_causal", 1, Owner::D8, CAUSAL_AI),
    (
        "fslc/src/origin_coverage.rs",
        "parse_kernel_source_with_file",
        1,
        Owner::Definition,
        FIXTURE,
    ),
    (
        "fslc/src/source_diagnostic.rs",
        "is_causal_source",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fslc/src/source_diagnostic.rs",
        "parse_causal",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fslc/src/verification.rs",
        "is_causal_source",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fslc/src/verification.rs",
        "parse_causal",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
];

#[test]
fn every_parse_entry_is_routed_or_inventoried() {
    let observed = observe_workspace();
    let mut expected: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut rows = BTreeSet::new();
    for &(file, entry, count, owner, reason) in INVENTORY {
        assert!(
            rows.insert((file, entry)),
            "duplicate inventory row {file} / {entry}"
        );
        assert!(
            OWNERS
                .iter()
                .any(|(_, _, entries)| entries.contains(&entry))
                || STABLE_CONSTRUCTIONS.contains(&entry),
            "inventory row {file} / {entry} names no stable entry"
        );
        assert!(
            reason.len() >= 20,
            "inventory row {file} / {entry} ({owner:?}) needs a reason"
        );
        expected.insert((file.to_owned(), entry.to_owned()), count);
    }

    let mut failures = Vec::new();
    for ((file, entry), sites) in &observed {
        match expected.get(&(file.clone(), entry.clone())) {
            None => failures.push(format!(
                "{file}: {} reference(s) to stable-pinned `{entry}` in {} are not in the \
                 inventory. Route the parse through the `_with` entry and the process's \
                 ParseOptions, or add a reasoned row to INVENTORY and to \
                 docs/design/parse-entry-inventory.md",
                sites.len(),
                sites.join(", ")
            )),
            Some(&count) if count != sites.len() => failures.push(format!(
                "{file}: `{entry}` is referenced {} time(s) (in {}), the inventory says {count}",
                sites.len(),
                sites.join(", ")
            )),
            Some(_) => {}
        }
    }
    for (file, entry) in expected.keys() {
        if !observed.contains_key(&(file.clone(), entry.clone())) {
            failures.push(format!(
                "{file}: inventory row for `{entry}` matches no reference; remove the stale row"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The scanner must see what a text search would miss and must not count a
/// same-named local function. Without this, an empty observation would make
/// the inventory test pass vacuously.
#[test]
fn scanner_resolves_paths_aliases_macros_and_shadowing() {
    let source = r#"
        use fsl_syntax::parse_surface_document as reparse;
        use fsl_syntax::{lex, ParseOptions};
        fn routed(source: &str) {
            let _ = fsl_syntax::parse_document_with(fsl_syntax::SourceFile::new(source), &OPTIONS);
            let _ = fsl_syntax::parse_document(fsl_syntax::SourceFile::new(source));
            let _ = reparse(source);
            let _ = Some(source).map(fsl_syntax::dialect_keyword);
            assert!(lex(source).is_ok());
            let _ = ParseOptions::stable();
            let _ = fsl_syntax::ParseOptions::default();
        }
        fn parse_causal(_: &str) {}
        fn local() { parse_causal(""); }
        #[cfg(test)]
        mod tests { fn t() { let _ = fsl_syntax::lex(""); } }
    "#;
    let observed = scan_source("fixture.rs", source, None);
    let counts = observed
        .iter()
        .map(|((_, entry), sites)| (entry.as_str(), sites.len()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        counts,
        BTreeMap::from([
            ("ParseOptions::default", 1),
            ("ParseOptions::stable", 1),
            ("dialect_keyword", 1),
            ("lex", 1),
            ("parse_document", 1),
            ("parse_surface_document", 1),
        ])
    );

    let glob = scan_source("glob.rs", "use fsl_syntax::*; fn f() {}", None);
    assert!(
        glob.contains_key(&("glob.rs".to_owned(), "fsl_syntax::*".to_owned())),
        "a glob import must be reported because it hides entries: {glob:?}"
    );
}

/// fsl-core's published-language wrappers are counted like fsl-syntax's (by
/// path, by alias, and inside fsl-core by a bare call to a wrapper the file
/// defines); its options-taking entries are not.
#[test]
fn scanner_counts_fsl_core_wrappers_but_not_their_options_entries() {
    let product = r"
        use fsl_core::parse_refinement as refine;
        fn product(source: &str) {
            let _ = fsl_core::parse_kernel_source(source, &resolver);
            let _ = fsl_core::parse_kernel_source_with_options(source, &resolver, &options);
            let _ = refine(source, &implementation, &abstraction);
            let _ = fsl_core::requirements_trace_contract_with_options(source, &options);
        }
    ";
    let counts = |observed: &BTreeMap<(String, String), Vec<String>>| {
        observed
            .iter()
            .map(|((_, entry), sites)| (entry.clone(), sites.len()))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(
        counts(&scan_source("product.rs", product, None)),
        BTreeMap::from([
            ("parse_kernel_source".to_owned(), 1),
            ("parse_refinement".to_owned(), 1),
        ])
    );

    let core = OWNERS
        .iter()
        .position(|(name, _, _)| *name == "fsl_core")
        .expect("fsl_core owner");
    let wrapper = r"
        use fsl_syntax::ParseOptions;
        pub fn parse_kernel_source(source: &str) {
            parse_kernel_source_with_options(source, &ParseOptions::stable())
        }
        pub fn caller(source: &str) {
            let _ = parse_kernel_source(source);
        }
    ";
    assert_eq!(
        counts(&scan_source("fsl-core/src/compose.rs", wrapper, Some(core))),
        BTreeMap::from([
            ("ParseOptions::stable".to_owned(), 1),
            ("parse_kernel_source".to_owned(), 1),
        ])
    );
}

/// `(file, entry)` -> the functions referencing it. Signatures spell the map
/// out instead of returning this alias, so the #868 census reads them as plain
/// maps rather than unresolved aliases.
type Observed = BTreeMap<(String, String), Vec<String>>;

fn workspace_rust_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust workspace")
        .to_owned()
}

fn observe_workspace() -> BTreeMap<(String, String), Vec<String>> {
    let root = workspace_rust_root();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&root).expect("read rust workspace") {
        let src = entry.expect("workspace entry").path().join("src");
        if src.is_dir() {
            collect_rust_files(&src, &mut files);
        }
    }
    files.sort();
    assert!(
        files.len() > 100,
        "scan floor: found only {} Rust sources",
        files.len()
    );
    let mut observed = Observed::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .expect("under workspace")
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(&path).expect("read Rust source");
        let home = OWNERS
            .iter()
            .position(|(_, directory, _)| relative.starts_with(directory));
        for (key, sites) in scan_source(&relative, &source, home) {
            observed.entry(key).or_default().extend(sites);
        }
    }
    observed
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// Scan one file. In an owning crate (`home`, an index into [`OWNERS`]), a
/// bare name also counts when the file defines it (the entry's own module) or
/// imports it through `crate`, `super`, or `self`; elsewhere a bare name
/// counts only when imported from the owning crate by name.
fn scan_source(
    relative: &str,
    source: &str,
    home: Option<usize>,
) -> BTreeMap<(String, String), Vec<String>> {
    let file = syn::parse_file(source)
        .unwrap_or_else(|error| panic!("{relative}: not parseable as Rust: {error}"));
    let mut imports = Imports::default();
    for item in &file.items {
        if let syn::Item::Use(item_use) = item {
            if is_test_only(&item_use.attrs) {
                continue;
            }
            imports.collect(&item_use.tree, &mut Vec::new(), home);
        }
        if let Some(home) = home
            && let syn::Item::Fn(function) = item
        {
            let name = function.sig.ident.to_string();
            if OWNERS[home].2.contains(&name.as_str()) {
                imports.bare.insert(name.clone(), name);
            }
        }
    }
    let mut scanner = Scanner {
        relative,
        home,
        imports,
        functions: Vec::new(),
        observed: Observed::new(),
    };
    for glob in &scanner.imports.globs {
        scanner
            .observed
            .entry((relative.to_owned(), glob.clone()))
            .or_default()
            .push("use".to_owned());
    }
    scanner.visit_file(&file);
    scanner.observed
}

#[derive(Default)]
struct Imports {
    /// Local name -> entry name (`use fsl_syntax::lex as tokens` maps
    /// `tokens` -> `lex`).
    bare: BTreeMap<String, String>,
    /// `ParseOptions` imported by name, so `ParseOptions::stable()` resolves.
    parse_options: bool,
    globs: Vec<String>,
}

impl Imports {
    fn collect(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>, home: Option<usize>) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.collect(&path.tree, prefix, home);
                prefix.pop();
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.collect(item, prefix, home);
                }
            }
            syn::UseTree::Name(name) => {
                self.import(prefix, &name.ident.to_string(), None, home);
            }
            syn::UseTree::Rename(rename) => {
                self.import(
                    prefix,
                    &rename.ident.to_string(),
                    Some(rename.rename.to_string()),
                    home,
                );
            }
            syn::UseTree::Glob(_) => {
                if owner_of(prefix, home).is_some() {
                    self.globs.push(format!("{}::*", prefix.join("::")));
                }
            }
        }
    }

    fn import(
        &mut self,
        prefix: &[String],
        name: &str,
        rename: Option<String>,
        home: Option<usize>,
    ) {
        let Some(owner) = owner_of(prefix, home) else {
            return;
        };
        let local = rename.unwrap_or_else(|| name.to_owned());
        if OWNERS[owner].2.contains(&name) {
            self.bare.insert(local, name.to_owned());
        } else if owner == SYNTAX && name == "ParseOptions" {
            self.parse_options = true;
        }
    }
}

/// Index of fsl-syntax in [`OWNERS`], the crate that defines `ParseOptions`.
const SYNTAX: usize = 0;

/// The owning crate a path prefix names: by crate name, or the file's own
/// crate through `crate` / `super` / `self`.
fn owner_of(prefix: &[String], home: Option<usize>) -> Option<usize> {
    let first = prefix.first()?;
    if matches!(first.as_str(), "crate" | "super" | "self") {
        return home;
    }
    OWNERS.iter().position(|(name, _, _)| first == name)
}

fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        let path = attribute.path();
        if path.is_ident("test") {
            return true;
        }
        if !path.is_ident("cfg") {
            return false;
        }
        attribute
            .parse_args::<syn::Ident>()
            .is_ok_and(|ident| ident == "test")
    })
}

struct Scanner<'a> {
    relative: &'a str,
    home: Option<usize>,
    imports: Imports,
    functions: Vec<String>,
    observed: Observed,
}

impl Scanner<'_> {
    fn record(&mut self, entry: &str) {
        let site = self
            .functions
            .last()
            .cloned()
            .unwrap_or_else(|| "<module>".to_owned());
        self.observed
            .entry((self.relative.to_owned(), entry.to_owned()))
            .or_default()
            .push(site);
    }

    /// Resolve one path (as identifier segments) to an entry, if it is one.
    fn resolve(&self, segments: &[String]) -> Option<String> {
        let (last, head) = segments.split_last()?;
        if let [qualifier] = head
            && qualifier == "ParseOptions"
            && self.imports.parse_options
        {
            let construction = format!("ParseOptions::{last}");
            return STABLE_CONSTRUCTIONS
                .contains(&construction.as_str())
                .then_some(construction);
        }
        if let [.., qualifier] = head
            && qualifier == "ParseOptions"
            && owner_of(head, self.home) == Some(SYNTAX)
        {
            let construction = format!("ParseOptions::{last}");
            return STABLE_CONSTRUCTIONS
                .contains(&construction.as_str())
                .then_some(construction);
        }
        if head.is_empty() {
            return self.imports.bare.get(last).cloned();
        }
        let owner = owner_of(head, self.home)?;
        OWNERS[owner]
            .2
            .contains(&last.as_str())
            .then(|| last.clone())
    }

    /// Macro bodies are token trees, not syntax; rebuild `a::b::c` paths from
    /// identifiers joined by a joint `::` and resolve each like a parsed path.
    fn scan_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        let flush = |scanner: &mut Self, path: &mut Vec<String>| {
            if let Some(entry) = scanner.resolve(path) {
                scanner.record(&entry);
            }
            path.clear();
        };
        let mut path: Vec<String> = Vec::new();
        // `None` = no colon seen; `Some(false)` = first `:` of a joint pair;
        // `Some(true)` = a complete `::`, so the next identifier extends `path`.
        let mut colons: Option<bool> = None;
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Ident(ident) => {
                    if colons != Some(true) {
                        flush(self, &mut path);
                    }
                    path.push(ident.to_string());
                    colons = None;
                }
                proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ':' => {
                    colons = match colons {
                        None if punct.spacing() == proc_macro2::Spacing::Joint => Some(false),
                        Some(false) => Some(true),
                        _ => {
                            flush(self, &mut path);
                            None
                        }
                    };
                }
                proc_macro2::TokenTree::Group(group) => {
                    flush(self, &mut path);
                    colons = None;
                    self.scan_tokens(group.stream());
                }
                proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {
                    flush(self, &mut path);
                    colons = None;
                }
            }
        }
        flush(self, &mut path);
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if is_test_only(&node.attrs) {
            return;
        }
        self.functions.push(node.sig.ident.to_string());
        syn::visit::visit_item_fn(self, node);
        self.functions.pop();
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if is_test_only(&node.attrs) {
            return;
        }
        self.functions.push(node.sig.ident.to_string());
        syn::visit::visit_impl_item_fn(self, node);
        self.functions.pop();
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !is_test_only(&node.attrs) {
            syn::visit::visit_item_mod(self, node);
        }
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if !is_test_only(&node.attrs) {
            syn::visit::visit_item_impl(self, node);
        }
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        // An import alone is not a reference. Module-level imports were
        // resolved up front; a block-level import applies from here on.
        if !is_test_only(&node.attrs) {
            let mut globs = std::mem::take(&mut self.imports.globs);
            self.imports.collect(&node.tree, &mut Vec::new(), self.home);
            for glob in self.imports.globs.drain(..) {
                if !globs.contains(&glob) {
                    self.observed
                        .entry((self.relative.to_owned(), glob.clone()))
                        .or_default()
                        .push("use".to_owned());
                    globs.push(glob);
                }
            }
            self.imports.globs = globs;
        }
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        let segments = node
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        if let Some(entry) = self.resolve(&segments) {
            self.record(&entry);
        }
        syn::visit::visit_path(self, node);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        self.scan_tokens(node.tokens.clone());
    }
}

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
//! a place a later stage must convert.
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
    "extract_literate_fsl",
];

/// Explicit constructions of the published-language options. Counted like an
/// entry so `parse_document_with(source, &ParseOptions::stable())` cannot
/// pass for a routed parse.
const STABLE_CONSTRUCTIONS: &[&str] = &["ParseOptions::stable", "ParseOptions::default"];

/// Owning stage for converting a stable-pinned reference.
#[derive(Clone, Copy, Debug)]
enum Owner {
    /// The published-language wrapper itself, or the option source; stays.
    Definition,
    /// S1 threads the options through this library API (edition wiring).
    S1,
    /// decisions/049 D8: this dialect rejects an edition header, so it stays
    /// on the published language.
    D8,
}

/// fsl-core / fsl-tools library function taking source text: S1 threads the
/// options through its signature. Until then a gated document is rejected
/// here (fail closed), never read differently.
const LIBRARY_API: &str = "library API taking source text; S1 threads ParseOptions through it, \
     until then a gated document is rejected here (fail closed)";
/// fslc library module compiled into fsl-wasm / fsl-lsp too: the CLI's
/// process value would be wrong for a Worker request, so S1 passes the
/// options as a parameter.
const SHARED_MODULE: &str = "fslc library module shared with fsl-wasm and fsl-lsp, so it cannot \
     use the CLI's process options; S1 passes them as a parameter";
const FORMAT_MIGRATE: &str = "fmt / migrate / migration diagnostics on the lossless tree; S1 owns \
     the edition wiring of formatting and header insertion";
const LITERATE: &str = "literate Markdown extraction; S1 reads the edition header in the first \
     fsl fence";
const CAUSAL_AI: &str = "causal or ai dialect: decisions/049 D8 makes an edition header an error \
     there, so the published language is the intended reading";
const STANDALONE_FRONTEND: &str = "standalone public parser of one dialect with no product \
     caller (the registry frontend reuses the dispatcher's routed tokens instead); it never sees \
     the options, so a document needing the wider identifier rule is rejected here (fail closed)";
const FRAGMENT: &str = "expression / spec fragment parser with no document header; the wider \
     identifier rule is selected by the header, so a fragment always reads ASCII identifiers";
const EDITOR_LEX: &str = "re-lexes source to place a diagnostic of the parse in the same module; \
     it takes that parse's options when S1 passes them to this shared module, and until then that \
     parse already rejects a gated document";
const PARITY_TOOL: &str = "Python-parity debug binary reading the published language; S1 \
     decides whether it takes the gate";
const REGISTRY: &str = "the private `domain` frontend wrapper shares the public entry's name; \
     it is reached only through parse_document_with";
const STABLE_WRAPPER: &str = "the published-language entry itself: the `_with` entry under \
     ParseOptions::stable";

/// `(workspace-relative file, entry, count, owner, reason)`.
const INVENTORY: &[(&str, &str, usize, Owner, &str)] = &[
    (
        "fsl-core/src/compose.rs",
        "parse_document",
        2,
        Owner::S1,
        LIBRARY_API,
    ),
    (
        "fsl-core/src/dialect.rs",
        "parse_surface_document",
        7,
        Owner::S1,
        LIBRARY_API,
    ),
    (
        "fsl-core/src/lib.rs",
        "parse_document",
        4,
        Owner::S1,
        LIBRARY_API,
    ),
    (
        "fsl-core/src/refinement.rs",
        "parse_surface_document",
        2,
        Owner::S1,
        LIBRARY_API,
    ),
    (
        "fsl-lsp/src/index.rs",
        "is_causal_source",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fsl-lsp/src/server.rs",
        "canonical_rewrites",
        1,
        Owner::S1,
        FORMAT_MIGRATE,
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
        Owner::S1,
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
        "parse_domain",
        1,
        Owner::Definition,
        REGISTRY,
    ),
    (
        "fsl-syntax/src/dispatch.rs",
        "ParseOptions::stable",
        2,
        Owner::Definition,
        STABLE_WRAPPER,
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
        "canonical_rewrites",
        1,
        Owner::S1,
        FORMAT_MIGRATE,
    ),
    (
        "fsl-syntax/src/lossless.rs",
        "lex",
        1,
        Owner::S1,
        FORMAT_MIGRATE,
    ),
    (
        "fsl-syntax/src/lossless.rs",
        "lossless_document",
        2,
        Owner::S1,
        FORMAT_MIGRATE,
    ),
    (
        "fsl-syntax/src/lossless.rs",
        "parse_document",
        3,
        Owner::S1,
        FORMAT_MIGRATE,
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
        "parse_causal",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fsl-tools/src/causal_expectation.rs",
        "parse_document",
        1,
        Owner::S1,
        LIBRARY_API,
    ),
    (
        "fsl-tools/src/document_project.rs",
        "parse_document",
        2,
        Owner::S1,
        LIBRARY_API,
    ),
    (
        "fslc/src/causal.rs",
        "parse_causal",
        2,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fslc/src/check_stages.rs",
        "parse_document",
        1,
        Owner::S1,
        SHARED_MODULE,
    ),
    (
        "fslc/src/check_stages.rs",
        "parse_surface_document",
        1,
        Owner::S1,
        SHARED_MODULE,
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
        "fslc/src/frontend_output.rs",
        "parse_surface_document",
        1,
        Owner::S1,
        SHARED_MODULE,
    ),
    (
        "fslc/src/literate_access.rs",
        "extract_literate_fsl",
        1,
        Owner::S1,
        LITERATE,
    ),
    (
        "fslc/src/main.rs",
        "extract_literate_fsl",
        1,
        Owner::S1,
        LITERATE,
    ),
    (
        "fslc/src/main.rs",
        "format_source",
        1,
        Owner::S1,
        FORMAT_MIGRATE,
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
        "fslc/src/migration.rs",
        "canonical_rewrites",
        1,
        Owner::S1,
        FORMAT_MIGRATE,
    ),
    (
        "fslc/src/migration.rs",
        "dialect_keyword",
        1,
        Owner::S1,
        FORMAT_MIGRATE,
    ),
    (
        "fslc/src/migration.rs",
        "format_source",
        1,
        Owner::S1,
        FORMAT_MIGRATE,
    ),
    ("fslc/src/migration.rs", "lex", 1, Owner::S1, FORMAT_MIGRATE),
    (
        "fslc/src/migration.rs",
        "lossless_document",
        6,
        Owner::S1,
        FORMAT_MIGRATE,
    ),
    (
        "fslc/src/migration.rs",
        "parse_surface_document",
        1,
        Owner::S1,
        FORMAT_MIGRATE,
    ),
    (
        "fslc/src/reproducer.rs",
        "parse_document",
        1,
        Owner::S1,
        SHARED_MODULE,
    ),
    (
        "fslc/src/source_diagnostic.rs",
        "canonical_rewrites",
        1,
        Owner::S1,
        FORMAT_MIGRATE,
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
        "lex",
        1,
        Owner::S1,
        EDITOR_LEX,
    ),
    (
        "fslc/src/source_diagnostic.rs",
        "parse_causal",
        1,
        Owner::D8,
        CAUSAL_AI,
    ),
    (
        "fslc/src/source_diagnostic.rs",
        "parse_document",
        1,
        Owner::S1,
        SHARED_MODULE,
    ),
    (
        "fslc/src/spec_load.rs",
        "parse_document",
        1,
        Owner::S1,
        SHARED_MODULE,
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
    (
        "fslc/src/verification_output.rs",
        "parse_surface_document",
        1,
        Owner::S1,
        SHARED_MODULE,
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
            STABLE_ENTRIES.contains(&entry) || STABLE_CONSTRUCTIONS.contains(&entry),
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
    let observed = scan_source("fixture.rs", source, false);
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

    let glob = scan_source("glob.rs", "use fsl_syntax::*; fn f() {}", false);
    assert!(
        glob.contains_key(&("glob.rs".to_owned(), "fsl_syntax::*".to_owned())),
        "a glob import must be reported because it hides entries: {glob:?}"
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
        let in_syntax_crate = relative.starts_with("fsl-syntax/");
        for (key, sites) in scan_source(&relative, &source, in_syntax_crate) {
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

/// Scan one file. In fsl-syntax, a bare name also counts when the file
/// defines it (the entry's own module) or imports it through `crate`,
/// `super`, or `self`; elsewhere a bare name counts only when imported from
/// `fsl_syntax`.
fn scan_source(
    relative: &str,
    source: &str,
    in_syntax_crate: bool,
) -> BTreeMap<(String, String), Vec<String>> {
    let file = syn::parse_file(source)
        .unwrap_or_else(|error| panic!("{relative}: not parseable as Rust: {error}"));
    let mut imports = Imports::default();
    for item in &file.items {
        if let syn::Item::Use(item_use) = item {
            if is_test_only(&item_use.attrs) {
                continue;
            }
            imports.collect(&item_use.tree, &mut Vec::new(), in_syntax_crate);
        }
        if in_syntax_crate && let syn::Item::Fn(function) = item {
            let name = function.sig.ident.to_string();
            if STABLE_ENTRIES.contains(&name.as_str()) {
                imports.bare.insert(name.clone(), name);
            }
        }
    }
    let mut scanner = Scanner {
        relative,
        in_syntax_crate,
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
    fn collect(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>, in_syntax_crate: bool) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.collect(&path.tree, prefix, in_syntax_crate);
                prefix.pop();
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.collect(item, prefix, in_syntax_crate);
                }
            }
            syn::UseTree::Name(name) => {
                self.import(prefix, &name.ident.to_string(), None, in_syntax_crate);
            }
            syn::UseTree::Rename(rename) => {
                self.import(
                    prefix,
                    &rename.ident.to_string(),
                    Some(rename.rename.to_string()),
                    in_syntax_crate,
                );
            }
            syn::UseTree::Glob(_) => {
                if from_syntax(prefix, in_syntax_crate) {
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
        in_syntax_crate: bool,
    ) {
        if !from_syntax(prefix, in_syntax_crate) {
            return;
        }
        let local = rename.unwrap_or_else(|| name.to_owned());
        if STABLE_ENTRIES.contains(&name) {
            self.bare.insert(local, name.to_owned());
        } else if name == "ParseOptions" {
            self.parse_options = true;
        }
    }
}

fn from_syntax(prefix: &[String], in_syntax_crate: bool) -> bool {
    prefix.first().is_some_and(|first| {
        first == "fsl_syntax"
            || (in_syntax_crate && matches!(first.as_str(), "crate" | "super" | "self"))
    })
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
    in_syntax_crate: bool,
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
            && from_syntax(head, self.in_syntax_crate)
        {
            let construction = format!("ParseOptions::{last}");
            return STABLE_CONSTRUCTIONS
                .contains(&construction.as_str())
                .then_some(construction);
        }
        if head.is_empty() {
            return self.imports.bare.get(last).cloned();
        }
        (from_syntax(head, self.in_syntax_crate) && STABLE_ENTRIES.contains(&last.as_str()))
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
            self.imports
                .collect(&node.tree, &mut Vec::new(), self.in_syntax_crate);
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

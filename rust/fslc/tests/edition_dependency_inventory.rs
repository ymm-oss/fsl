// SPDX-License-Identifier: Apache-2.0

#[path = "support/edition_dependency_cases.rs"]
mod cases;
#[path = "support/edition_cases.rs"]
mod common;
#[path = "support/product_source.rs"]
mod product;
use std::collections::{BTreeMap, BTreeSet};
use syn::spanned::Spanned;
use syn::visit::Visit;

fn adapter_hook(items: &[syn::Item]) -> Option<proc_macro2::Span> {
    items.iter().find_map(|item| {
        if let syn::Item::Mod(module) = item {
            if module.ident == "dependency_edition" {
                Some(module.span())
            } else {
                module
                    .content
                    .as_ref()
                    .and_then(|(_, items)| adapter_hook(items))
            }
        } else {
            None
        }
    })
}

fn production_files(rust: &std::path::Path) -> BTreeMap<String, String> {
    fn modules(items: &[syn::Item], base: &std::path::Path, queue: &mut Vec<std::path::PathBuf>) {
        for item in items {
            if let syn::Item::Mod(module) = item {
                if let Some((_, items)) = &module.content {
                    modules(items, &base.join(module.ident.to_string()), queue);
                } else {
                    let file = base.join(format!("{}.rs", module.ident));
                    let directory = base.join(module.ident.to_string()).join("mod.rs");
                    queue.push(match (file.exists(), directory.exists()) {
                        (true, false) => file,
                        (false, true) => directory,
                        _ => panic!("unresolved/ambiguous production module {}", module.ident),
                    });
                }
            }
        }
    }
    let mut files = BTreeMap::new();
    let mut queue = [
        "fsl-core/src/lib.rs",
        "fsl-tools/src/lib.rs",
        "fslc/src/lib.rs",
        "fslc/src/main.rs",
        "fsl-wasm/src/lib.rs",
        "fsl-lsp/src/lib.rs",
    ]
    .into_iter()
    .map(|p| rust.join(p))
    .collect::<Vec<_>>();
    while let Some(path) = queue.pop() {
        let relative = path
            .strip_prefix(rust)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if files.contains_key(&relative) {
            continue;
        }
        let source = product::product_source(&std::fs::read_to_string(&path).unwrap());
        let parsed = syn::parse_file(&source).unwrap();
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let parent = path.parent().unwrap();
        let base = if matches!(stem, "lib" | "main" | "mod") {
            parent.to_owned()
        } else {
            parent.join(stem)
        };
        modules(&parsed.items, &base, &mut queue);
        files.insert(relative, source);
    }
    files
}

#[derive(Default)]
struct Reads {
    function: String,
    endpoints: Vec<String>,
    spawns: BTreeSet<String>,
    constructors: BTreeMap<String, usize>,
}
impl<'ast> Visit<'ast> for Reads {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let saved = std::mem::replace(&mut self.function, node.sig.ident.to_string());
        syn::visit::visit_item_fn(self, node);
        self.function = saved;
    }
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "read"
            && matches!(node.receiver.as_ref(), syn::Expr::Path(path) if path.path.is_ident("resolver"))
        {
            self.endpoints.push(self.function.clone());
        }
        if node.method == "spawn" || node.method == "spawn_scoped" {
            self.spawns.insert(self.function.clone());
        }
        syn::visit::visit_expr_method_call(self, node);
    }
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            let names = path
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>();
            if names.ends_with(&["FsResolver".into(), "new".into()])
                || names.ends_with(&["RecordingResolver".into(), "new".into()])
            {
                *self.constructors.entry(self.function.clone()).or_default() += 1;
            }
        }
        syn::visit::visit_expr_call(self, node);
    }
    fn visit_expr_struct(&mut self, node: &'ast syn::ExprStruct) {
        if node
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "MemoryResolver" || s.ident == "StoreResolver")
        {
            *self.constructors.entry(self.function.clone()).or_default() += 1;
        }
        syn::visit::visit_expr_struct(self, node);
    }
}

#[test]
#[allow(clippy::too_many_lines)] // One discovery/completeness detector with live exclusions.
fn dependency_edition_inventory_discovers_read_sites_and_endpoint_incidence() {
    let rust = common::root().join("rust");
    let mut discovered = BTreeSet::new();
    let mut spawn_sites = BTreeSet::new();
    let mut providers = BTreeMap::new();
    for (path, text) in production_files(&rust) {
        let mut visitor = Reads::default();
        visitor.visit_file(&syn::parse_file(&text).unwrap());
        let mut indices = BTreeMap::<String, usize>::new();
        for function in visitor.endpoints {
            let endpoint = if path == "fsl-wasm/src/lib.rs" {
                let index = indices.entry(function.clone()).or_default();
                let name = format!("{path}:{function}:{index}");
                *index += 1;
                name
            } else {
                format!("{path}:{function}")
            };
            assert!(
                discovered.insert(endpoint),
                "unclassified repeated endpoint"
            );
        }
        spawn_sites.extend(visitor.spawns.into_iter().map(|f| format!("{path}:{f}")));
        for (function, count) in visitor.constructors {
            providers.insert(format!("{path}:{function}"), count);
        }
    }
    let expected = cases::EDGES
        .iter()
        .map(|e| e.2.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(expected.len(), 11);
    assert_eq!(
        discovered, expected,
        "a dependency read has no acceptance incidence"
    );
    assert_eq!(
        cases::EDGES
            .iter()
            .map(|e| e.1)
            .collect::<BTreeSet<_>>()
            .len(),
        7
    );
    assert_eq!(cases::all_rows().len(), 128);
    assert_eq!(
        cases::all_rows()
            .iter()
            .map(|r| &r.id)
            .collect::<BTreeSet<_>>()
            .len(),
        128
    );
    let assigned = ["cli", "routes", "worker", "lsp", "private", "inventory"]
        .into_iter()
        .flat_map(cases::assigned)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        assigned,
        cases::all_rows().iter().map(|r| r.id.clone()).collect(),
        "adapter assignment gap"
    );
    for endpoint in [
        "fsl-lsp/src/server.rs:publish_diagnostics",
        "fsl-lsp/src/server.rs:code_actions",
    ] {
        assert_eq!(providers[endpoint], 1);
    }
    assert_eq!(
        providers
            .iter()
            .filter(|(path, _)| path.starts_with("fsl-wasm/"))
            .map(|(_, n)| n)
            .sum::<usize>(),
        4
    );
    assert_eq!(
        providers
            .iter()
            .filter(|(path, _)| path.starts_with("fslc/")
                && !path.starts_with("fslc/src/coverage.rs:")
                && !path.starts_with("fslc/src/origin_coverage.rs:"))
            .map(|(_, n)| n)
            .sum::<usize>(),
        26
    );
    let expected_spawns = cases::additional_rows()
        .iter()
        .filter(|r| r.id.starts_with('T') && r.endpoint.contains("/src/"))
        .map(|r| r.endpoint.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        spawn_sites, expected_spawns,
        "production spawn lacks a table policy"
    );
    // Named exclusions remain live: these are internal comparison tools and
    // fixture producers, not product consumers. Document projection only accepts
    // Spec/Requirements and does not resolve declared implements dependencies.
    for path in [
        "fsl-core/src/bin/fsl-parse-kernel.rs",
        "fsl-runtime/src/bin/fsl-bfs.rs",
        "fsl-runtime/src/bin/fsl-explicit.rs",
        "fsl-runtime/src/bin/fsl-refine.rs",
        "fslc/src/bin/fsl-bmc.rs",
        "fslc/src/bin/fsl-replay-actions.rs",
        "fslc/src/coverage.rs",
        "fslc/src/origin_coverage.rs",
        "fsl-tools/src/document_project.rs",
    ] {
        assert!(
            std::fs::read_to_string(rust.join(path))
                .unwrap()
                .contains("FsResolver"),
            "stale exclusion {path}"
        );
    }
    for path in [
        "fslc/src/main.rs",
        "fsl-wasm/src/lib.rs",
        "fsl-lsp/src/server.rs",
    ] {
        let source = std::fs::read_to_string(rust.join(path)).unwrap();
        let parsed = syn::parse_file(&source).unwrap();
        let hooks = parsed
            .items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Mod(outer) if product::is_test_only(&outer.attrs) => outer
                    .content
                    .as_ref()
                    .and_then(|(_, items)| adapter_hook(items).map(|hook| (outer.span(), hook))),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            hooks.len(),
            1,
            "test adapter hook is not inside one cfg(test) module: {path}"
        );
        let (outer, hook) = hooks[0];
        println!(
            "HOOK {path} cfg_module={}..{} added_module={}..{}",
            outer.start().line,
            outer.end().line,
            hook.start().line,
            hook.end().line
        );
    }
    println!("DISCOVERED reads={discovered:?} spawns={spawn_sites:?}");
    for (id, reasons) in cases::deferred_rows() {
        println!("DEFERRED_SUBCONTRACT {id} {reasons:?}");
    }
    println!(
        "DEPENDENCY I01 agreement=true produced=discovered read/provider/spawn sets expected=registry incidence and assignments"
    );
}

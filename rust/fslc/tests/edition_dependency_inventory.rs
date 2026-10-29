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
        "fsl-runtime/src/lib.rs",
        "fsl-verifier/src/lib.rs",
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
            .replace('\\', "/");
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
    constructors: BTreeMap<String, Vec<String>>,
    admitted: BTreeSet<String>,
    reader_types: BTreeMap<String, bool>,
}
impl Reads {
    fn admitting(path: &syn::Path) -> bool {
        path.segments
            .iter()
            .any(|s| s.ident == "DependencyResolver")
            || path.segments.last().is_some_and(|s| {
                matches!(
                    s.ident.to_string().as_str(),
                    "dependency_resolver" | "admitted_resolver"
                )
            })
    }
    fn provider(&self, expr: &syn::Expr) -> bool {
        match expr {
            syn::Expr::Reference(reference) => self.provider(&reference.expr),
            syn::Expr::Path(path) => path
                .path
                .get_ident()
                .is_some_and(|name| self.admitted.contains(&name.to_string())),
            syn::Expr::Call(call) => {
                matches!(call.func.as_ref(), syn::Expr::Path(path) if Self::admitting(&path.path))
            }
            _ => false,
        }
    }
}
impl<'ast> Visit<'ast> for Reads {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if node.sig.ident == "dependency_resolver" || node.sig.ident == "admitted_resolver" {
            assert!(
                matches!(&node.sig.output, syn::ReturnType::Type(_, ty) if matches!(ty.as_ref(), syn::Type::Path(path) if path.path.segments.last().unwrap().ident == "DependencyResolver")),
                "native factory provider type"
            );
        }
        let saved = std::mem::replace(&mut self.function, node.sig.ident.to_string());
        let locals = std::mem::take(&mut self.admitted);
        syn::visit::visit_item_fn(self, node);
        self.admitted = locals;
        self.function = saved;
    }
    fn visit_local(&mut self, node: &'ast syn::Local) {
        if let (syn::Pat::Ident(name), Some(init)) = (&node.pat, &node.init) {
            if self.provider(&init.expr) {
                self.admitted.insert(name.ident.to_string());
            } else {
                self.admitted.remove(&name.ident.to_string());
            }
        }
        syn::visit::visit_local(self, node);
    }
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "read" {
            let mut endpoint = self.function.clone();
            if self.function == "governance_output" {
                let syn::Expr::Reference(argument) = &node.args[0] else {
                    panic!("unbound governance read")
                };
                let syn::Expr::Field(field) = argument.expr.as_ref() else {
                    panic!("unnamed governance operand")
                };
                let syn::Member::Named(name) = &field.member else {
                    panic!("positional governance operand")
                };
                endpoint = format!("{endpoint}:{name}");
            }
            self.reader_types
                .insert(endpoint.clone(), self.provider(&node.receiver));
            self.endpoints.push(endpoint);
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
            if names.last().is_some_and(|name| name == "spawn") {
                self.spawns.insert(self.function.clone());
            }
            if names
                .last()
                .is_some_and(|name| name == "compile_expectations")
            {
                self.reader_types.insert(
                    format!("{}:compile_expectations", self.function),
                    self.provider(&node.args[2]),
                );
            }
            if names.ends_with(&["FsResolver".into(), "new".into()])
                || names.ends_with(&["RecordingResolver".into(), "new".into()])
                || Self::admitting(&path.path)
            {
                let kind = if Self::admitting(&path.path) {
                    "DependencyResolver"
                } else {
                    names[names.len() - 2].as_str()
                };
                self.constructors
                    .entry(self.function.clone())
                    .or_default()
                    .push(kind.into());
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
            self.constructors
                .entry(self.function.clone())
                .or_default()
                .push(node.path.segments.last().unwrap().ident.to_string());
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
    let mut library_counts = BTreeMap::<String, (usize, usize, usize)>::new();
    let mut reader_types = BTreeMap::new();
    for (path, text) in production_files(&rust) {
        let mut visitor = Reads::default();
        visitor.visit_file(&syn::parse_file(&text).unwrap());
        if matches!(
            path.as_str(),
            "fslc/src/migration.rs" | "fslc/src/source_diagnostic.rs"
        ) {
            // The one trait-provider forwarding read is not a language edge.
            assert_eq!(
                visitor
                    .endpoints
                    .iter()
                    .filter(|name| name.is_empty())
                    .count(),
                1
            );
            visitor.endpoints.retain(|name| !name.is_empty());
        }
        reader_types.extend(
            visitor
                .reader_types
                .into_iter()
                .map(|(name, ty)| (format!("{path}:{name}"), ty)),
        );
        let crate_name = path.split('/').next().unwrap();
        if matches!(crate_name, "fsl-runtime" | "fsl-verifier") {
            let counts = library_counts.entry(crate_name.into()).or_default();
            counts.0 += 1;
            counts.1 += visitor.endpoints.len();
            counts.2 += visitor.spawns.len();
        }
        for function in visitor.endpoints {
            assert!(
                discovered.insert(format!("{path}:{function}")),
                "unclassified repeated endpoint"
            );
        }
        spawn_sites.extend(visitor.spawns.into_iter().map(|f| format!("{path}:{f}")));
        for (function, count) in visitor.constructors {
            providers.insert(format!("{path}:{function}"), count);
        }
    }
    for endpoint in [
        "fslc/src/causal.rs:run_causal_observe_expectations",
        "fslc/src/causal.rs:run_causal_verify_expectations:compile_expectations",
        "fslc/src/causal.rs:run_causal_observe_expectations:compile_expectations",
        "fsl-wasm/src/lib.rs:governance_output:after_path",
        "fsl-wasm/src/lib.rs:governance_output:before_path",
        "fsl-wasm/src/lib.rs:governance_output:refinement_path",
    ] {
        assert_eq!(
            reader_types.get(endpoint),
            Some(&true),
            "late read provider type: {endpoint}"
        );
    }
    for name in ["fsl-runtime", "fsl-verifier"] {
        let (files, reads, spawns) = library_counts[name];
        assert!(files > 0);
        assert_eq!((reads, spawns), (0, 0), "unclassified library consumer");
    }
    let expected = cases::EDGES
        .iter()
        .map(|e| e.2.to_owned())
        .collect::<BTreeSet<_>>();
    let kinds = cases::EDGES.iter().map(|e| e.1).collect::<BTreeSet<_>>();
    assert_eq!((expected.len(), kinds.len()), (11, 7));
    assert_eq!(
        discovered, expected,
        "a dependency read has no acceptance incidence"
    );
    let rows = cases::all_rows();
    assert_eq!(rows.len(), 128);
    let ids = rows.into_iter().map(|r| r.id).collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), 128);
    let assigned = ["cli", "routes", "worker", "lsp", "private", "inventory"]
        .into_iter()
        .flat_map(cases::assigned)
        .collect::<BTreeSet<_>>();
    assert_eq!(assigned, ids, "adapter assignment gap");
    providers.retain(|path, _| {
        ![
            "fslc/src/coverage.rs:",
            "fslc/src/origin_coverage.rs:",
            "fsl-tools/src/document_project.rs:",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix))
    });
    let expected = "\
fsl-lsp/src/server.rs|code_actions publish_diagnostics|StoreResolver
fsl-wasm/src/lib.rs|admitted_resolver|DependencyResolver,MemoryResolver
fsl-wasm/src/lib.rs|build check_kernel governance_output implements_output|DependencyResolver
fslc/src/approval.rs|spec_digest|DependencyResolver
fslc/src/causal.rs|load_causal_model_from_source run_causal_observe_expectations run_causal_verify_expectations|DependencyResolver
fslc/src/main.rs|mutation_implements_contract approval_spec_digest document_model_failure document_trace_gate external_mutation_model invariant_counterfactuals_from_source reachable_counterfactuals_from_source render_document_for_approval run_check_from_source run_document_check run_document_generate run_kernel_contract_from_source run_mutate_from_source run_typestate_from_source validate_fmt_semantics validate_migration_input_semantics|DependencyResolver
fslc/src/migration.rs|kernel_contract|FsResolver
fslc/src/refinement.rs|governance_result_from_source implements_result_from_source_with_bounds|DependencyResolver
fslc/src/source_diagnostic.rs|diagnostics_with_model|DependencyResolver
fslc/src/spec_loading.rs|dependency_resolver|DependencyResolver,FsResolver
fslc/src/spec_loading.rs|load_declared_model_beside_scope load_kernel_model_from_source|DependencyResolver
fslc/src/verification.rs|run_verify_cli_kernel_from_source|DependencyResolver,RecordingResolver"
    .lines().flat_map(|line| {
        let fields = line.split('|').collect::<Vec<_>>();
        fields[1].split_whitespace().map(move |function| (format!("{}:{function}", fields[0]), fields[2].split(',').map(str::to_owned).collect::<Vec<_>>()))
    }).collect::<BTreeMap<_, _>>();
    assert_eq!(providers, expected, "per-site provider types");
    let expected_spawns = cases::additional_rows()
        .iter()
        .filter(|r| r.id.starts_with('T') && r.endpoint.contains("/src/"))
        .map(|r| r.endpoint.to_owned())
        .chain(["fslc/src/main.rs:native_mutation_bmc".into()]) // read-free main.rs:10613,10632
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
    }
    assert_eq!(cases::deferred_rows().len(), 36);
    println!(
        "DEPENDENCY I01 agreement=true produced=discovered read/provider/spawn sets expected=registry incidence and assignments"
    );
}

#[test]
fn inventory_recognizes_receiver_aliases_and_free_thread_spawns() {
    let source = "fn consumer() { r.read(path); self.resolver.read(path); std::thread::spawn(|| {}); } fn governance_output() { r.read(&p.refinement_path); r.read(&p.after_path); r.read(&p.before_path); }";
    let mut visitor = Reads::default();
    visitor.visit_file(&syn::parse_file(source).unwrap());
    assert_eq!(
        visitor.endpoints,
        [
            "consumer",
            "consumer",
            "governance_output:refinement_path",
            "governance_output:after_path",
            "governance_output:before_path"
        ]
    );
    assert_eq!(visitor.spawns, BTreeSet::from(["consumer".into()]));
}

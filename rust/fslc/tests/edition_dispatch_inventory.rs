// SPDX-License-Identifier: Apache-2.0

// Reuse standalone adapters verbatim, each with its own observation collector.
#![allow(clippy::duplicate_mod)]

#[path = "support/edition_cases.rs"]
mod cases;

#[path = "edition_all_commands.rs"]
mod all_commands;
#[path = "edition_dialect_controls.rs"]
mod dialect_controls;
#[path = "edition_ownership.rs"]
mod ownership;
#[path = "edition_root_slots.rs"]
mod root_slots;

use std::collections::BTreeSet;
use syn::visit::Visit;

fn literal(expression: &syn::Expr) -> Option<String> {
    match expression {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => Some(s.value()),
        syn::Expr::Call(call) if call.args.len() == 1 => literal(&call.args[0]),
        _ => None,
    }
}

fn patterns(pattern: &syn::Pat, output: &mut BTreeSet<String>) {
    match pattern {
        syn::Pat::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => {
            output.insert(s.value());
        }
        syn::Pat::Or(or) => {
            for p in &or.cases {
                patterns(p, output);
            }
        }
        syn::Pat::Wild(_) | syn::Pat::Ident(_) => {} // Explicit unknown-command arm.
        _ => panic!("unrecognized dispatch pattern; inventory must be taught its syntax"),
    }
}

struct Selectors {
    selector: &'static str,
    leaves: BTreeSet<String>,
    matches: usize,
}

fn contains_selector(expression: &syn::Expr, selector: &str) -> bool {
    struct Uses<'a>(&'a str, bool);
    impl<'ast> Visit<'ast> for Uses<'_> {
        fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
            self.1 |= path.path.is_ident(self.0);
        }
        fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
            self.1 |= matches!(&field.member, syn::Member::Named(name) if name == self.0);
            syn::visit::visit_expr_field(self, field);
        }
    }
    let mut uses = Uses(selector, false);
    uses.visit_expr(expression);
    uses.1
}

fn frontend_guard(expression: &syn::Expr) -> bool {
    matches!(expression, syn::Expr::Call(call) if matches!(call.func.as_ref(), syn::Expr::Path(p) if p.path.is_ident("literate_access")))
}

impl<'ast> Visit<'ast> for Selectors {
    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        let before = self.matches;
        if let syn::Expr::MethodCall(method) = node.expr.as_ref()
            && method.method == "as_str"
        {
            let selected = match method.receiver.as_ref() {
                syn::Expr::Path(path) => path.path.is_ident(self.selector),
                syn::Expr::Field(field) => {
                    matches!(&field.member, syn::Member::Named(name) if name == self.selector)
                }
                _ => false,
            };
            if selected {
                self.matches += 1;
                for arm in &node.arms {
                    assert!(
                        arm.guard.is_none(),
                        "new guarded dispatch needs explicit inventory handling"
                    );
                    patterns(&arm.pat, &mut self.leaves);
                }
            }
        }
        assert!(
            self.matches > before
                || frontend_guard(&node.expr)
                || !contains_selector(&node.expr, self.selector),
            "unrecognized selector match syntax"
        );
        syn::visit::visit_expr_match(self, node);
    }
    fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
        if contains_selector(&node.cond, self.selector) {
            // This uses the already-selected command as a diagnostic label;
            // it is a frontend guard, not another command selector.
            let is_frontend_guard =
                matches!(node.cond.as_ref(), syn::Expr::Let(e) if frontend_guard(&e.expr));
            assert!(
                is_frontend_guard
                    || matches!(node.cond.as_ref(), syn::Expr::Binary(e) if matches!(e.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) && matches!(e.left.as_ref(), syn::Expr::Path(p) if p.path.is_ident(self.selector)) && literal(&e.right).is_some()),
                "unrecognized selector condition"
            );
        }
        syn::visit::visit_expr_if(self, node);
    }
    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        fn has_name(tokens: proc_macro2::TokenStream, name: &str) -> bool {
            tokens.into_iter().any(|token| match token {
                proc_macro2::TokenTree::Ident(ident) => ident == name,
                proc_macro2::TokenTree::Group(group) => has_name(group.stream(), name),
                _ => false,
            })
        }
        assert!(
            !has_name(node.tokens.clone(), self.selector),
            "unrecognized macro-based dispatch selector"
        );
        syn::visit::visit_macro(self, node);
    }
    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        // ai compare is dispatched by an early if, not the later match.
        if let syn::Expr::Path(path) = node.left.as_ref()
            && path.path.is_ident(self.selector)
            && let Some(value) = literal(&node.right)
        {
            self.leaves.insert(value);
        }
        syn::visit::visit_expr_binary(self, node);
    }
}

fn function<'a>(file: &'a syn::File, name: &str) -> &'a syn::ItemFn {
    file.items
        .iter()
        .find_map(|item| match item {
            syn::Item::Fn(f) if f.sig.ident == name => Some(f),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing dispatcher {name}"))
}

fn selectors(file: &syn::File, name: &str, selector: &'static str) -> BTreeSet<String> {
    let mut visitor = Selectors {
        selector,
        leaves: BTreeSet::new(),
        matches: 0,
    };
    visitor.visit_block(&function(file, name).block);
    assert!(visitor.matches > 0, "missing selector match in {name}");
    visitor.leaves
}

// fmt and compat check use a next().as_deref() != Some(literal) guard.
struct Guard(Vec<String>);
impl<'a> Visit<'a> for Guard {
    fn visit_expr_binary(&mut self, e: &'a syn::ExprBinary) {
        if matches!(e.op, syn::BinOp::Ne(_))
            && let Some(s) = literal(&e.right)
        {
            self.0.push(s);
        }
        syn::visit::visit_expr_binary(self, e);
    }
}

fn dispatch(main: &str) -> BTreeSet<String> {
    let main = syn::parse_file(main).expect("parse main dispatch");
    let mut leaves = selectors(&main, "command", "command");
    // Aliases are separately exercised as metadata, not extra leaves.
    for alias in ["-V", "--version"] {
        assert!(leaves.remove(alias));
    }
    for group in [
        "approval",
        "counterexample",
        "document",
        "db",
        "ai",
        "domain",
    ] {
        assert!(leaves.remove(group), "missing group {group}");
        for leaf in selectors(&main, &format!("{group}_command"), "subcommand") {
            leaves.insert(format!("{group} {leaf}"));
        }
    }
    assert!(leaves.remove("causal"));
    let causal = syn::parse_file(include_str!("../src/causal.rs")).unwrap();
    for leaf in selectors(&causal, "causal_command", "subcommand") {
        leaves.insert(format!("causal {leaf}"));
    }
    let mut fmt = Guard(Vec::new());
    fmt.visit_block(&function(&main, "fmt_command").block);
    assert_eq!(fmt.0.len(), 1, "fmt guard changed");
    leaves.insert(fmt.0.pop().unwrap());
    assert!(leaves.remove("compat"));
    let options = syn::parse_file(include_str!("../src/cli_options.rs")).unwrap();
    let implementation = options.items.iter().find_map(|item| match item {
        syn::Item::Impl(i) if matches!(i.self_ty.as_ref(), syn::Type::Path(p) if p.path.is_ident("CompatCheckOptions")) => Some(i), _ => None,
    }).expect("compat options dispatcher");
    let mut compat = Guard(Vec::new());
    compat.visit_item_impl(implementation);
    assert_eq!(compat.0.len(), 1, "compat selector changed");
    leaves.insert(format!("compat {}", compat.0[0]));
    let worker = syn::parse_file(include_str!("../../fsl-wasm/src/lib.rs")).unwrap();
    for leaf in selectors(&worker, "run", "cmd") {
        leaves.insert(format!("worker {leaf}"));
    }
    leaves
}

fn advertised() -> BTreeSet<String> {
    fn walk(node: &serde_json::Value, leaves: &mut BTreeSet<String>) {
        let children = node["commands"].as_array().unwrap();
        if children.is_empty() {
            leaves.insert(
                node["path"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        } else {
            for child in children {
                walk(child, leaves);
            }
        }
    }
    let contract: serde_json::Value =
        serde_json::from_str(include_str!("../cli-contract.json")).unwrap();
    let mut leaves = BTreeSet::new();
    walk(&contract["root"], &mut leaves);
    leaves
}

#[test]
fn metadata_and_dispatch_population_is_exact() {
    let rows = cases::rows();
    let all: BTreeSet<_> = rows.iter().map(|r| r.key.to_owned()).collect();
    assert_eq!(all.len(), rows.len(), "duplicate edition row");
    // LSP is a library API, not a CLI/Worker dispatch leaf. Its real execution
    // and assignment are checked by the native adapter, not inserted here.
    let table = rows
        .iter()
        .filter(|r| r.role != "lsp")
        .map(|r| r.key.to_owned())
        .collect::<BTreeSet<_>>();
    let cli: BTreeSet<_> = rows
        .iter()
        .filter(|r| !r.key.starts_with("worker ") && r.role != "lsp")
        .map(|r| r.key.to_owned())
        .collect();
    assert_eq!(cli, advertised(), "metadata leaf absent from edition table");
    assert_eq!(
        table,
        dispatch(include_str!("../src/main.rs")),
        "actual dispatch differs from edition table"
    );
    assert_eq!(rows.iter().filter(|r| r.role == "nonreader").count(), 3);
    let deferred: BTreeSet<_> = cases::DEFERRED_TO_SECOND_PR
        .iter()
        .map(|(leaf, mode, reason)| {
            assert!(!reason.is_empty());
            assert!(
                all.contains(*leaf),
                "deferred leaf absent from edition table: {leaf}"
            );
            (*leaf, *mode)
        })
        .collect();
    assert_eq!(
        deferred.len(),
        cases::DEFERRED_TO_SECOND_PR.len(),
        "duplicate deferred mode"
    );
}

#[test]
fn deferred_modes_have_live_disagreements_across_the_complete_native_adapters() {
    let mut cells = all_commands::cases::capture(|| {
        all_commands::every_registered_root_has_the_same_admission_before_dialect_or_sidecars();
        all_commands::four_warning_classes_and_refusals_finish_once_on_all_eight_verification_routes();
        all_commands::argv_validation_remains_ahead_of_source_admission();
        all_commands::report_baselines_receive_one_finished_warning_before_rendering();
    });
    cells.extend(root_slots::cases::capture(|| {
        root_slots::explicit_refine_diff_replay_and_batch_slots_are_admitted_individually();
        root_slots::manifest_layers_and_mapping_refuse_before_the_implementation_marker();
        root_slots::causal_explicit_mapping_is_a_root_and_missing_sidecars_lose_to_refusal();
        root_slots::closed_schemas_and_raw_output_send_the_exact_warning_to_stderr_once();
        root_slots::header_flag_conflict_and_gate_off_modes_are_preserved();
        root_slots::checked_repairs_and_editionless_sources_are_preserved();
    }));
    cells.extend(ownership::cases::capture(|| {
        ownership::domain_check_finishes_once_for_definitive_and_non_definitive_verdicts();
        ownership::repeated_file_slots_each_contribute_one_warning();
        ownership::chain_owns_warnings_in_layer_detail_and_refuses_before_effects();
        ownership::causal_root_keeps_one_owned_warning_and_refuses_before_output();
        ownership::unreadable_roots_keep_the_command_specific_error_kind();
        ownership::domain_check_forwards_requested_edition_without_changing_conflicts();
        ownership::counterexample_refuses_before_model_and_source_preflight();
        ownership::mutate_success_keeps_one_complete_legacy_warning();
        #[cfg(unix)]
        {
            ownership::counterexample_refuses_before_digest_rereads_the_root();
            ownership::sweep_reuses_one_captured_root_across_all_depth_cells();
        }
    }));
    cells.extend(dialect_controls::cases::capture(|| {
        dialect_controls::actual_dialects_and_valid_sidecars_are_preserved_twice();
        dialect_controls::approval_records_and_git_roots_refuse_before_output_writes();
    }));
    cases::assert_deferred_evidence(&cells);
}

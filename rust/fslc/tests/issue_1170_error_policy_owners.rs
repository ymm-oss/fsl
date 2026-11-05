// SPDX-License-Identifier: Apache-2.0

//! Issue #1170: the binary's file-IO classification has one owner, and new
//! hand-written IO, error-envelope and error-exit sites are counted.
//!
//! - IO classification: `spec_loading.rs` (`read_spec_source`, `read_source`
//!   with its `IoMessage` spelling, `read_unprefixed_spec_source`,
//!   `io_error_output`, `io_error_detail`, and the named
//!   `read_source_classified_semantics` exception), rendered through
//!   `SpecLoadError::Io` in the library's `spec_load.rs`.
//! - Error envelope: `cli_output.rs` (`envelope`, `error_output`) and the
//!   library renderers it delegates to.
//! - Error exit code: `fslc_rust::outcome` (the `Verdict` table and
//!   `exit_status`). Routing the literal exits through it is #1170's
//!   follow-up; this file only stops their number from growing.
//!
//! Each rule counts the sites outside its owners in product source: comments,
//! doc attributes and `#[cfg(test)]`/`#[test]` items are blanked first by
//! `support/product_source.rs`. A count may only go down. A count above its
//! pin is a new hand-written site and fails; a count below its pin also
//! fails until the pin is lowered, so a removed site cannot silently come
//! back.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

#[path = "support/product_source.rs"]
mod product;

/// A hand-written policy site, matched in whitespace-free product source.
struct Rule {
    name: &'static str,
    /// Files (relative to `src/`) that own the policy and are not counted.
    owners: &'static [&'static str],
    needles: &'static [&'static str],
    /// Count `(expr, 2)` / `(expr, 3)` tuples outside macros as well.
    exit_tuples: bool,
}

const RULES: &[Rule] = &[
    Rule {
        name: "io-classification",
        owners: &["spec_loading.rs", "spec_load.rs"],
        needles: &[
            "error_output(\"io\"",
            "error_detail(\"io\"",
            "\"kind\":\"io\"",
            "json!(\"io\")",
            "SpecLoadError::Io(",
        ],
        exit_tuples: false,
    },
    Rule {
        name: "error-envelope",
        owners: &["cli_output.rs"],
        needles: &[
            "\"result\":\"error\"",
            "\"result\".to_owned(),json!(\"error\")",
        ],
        exit_tuples: false,
    },
    Rule {
        name: "error-exit-literal",
        owners: &["outcome.rs"],
        needles: &[
            "\"exit_code\":2",
            "\"exit_code\":3",
            "process::exit(2)",
            "process::exit(3)",
        ],
        exit_tuples: true,
    },
];

/// `(rule, file)` counts on the #1170 head. Lower a pin when a site goes
/// away; never raise one. Before #1170 `main.rs` alone had 74 `io` sites
/// and 13 hand-spelled `"result":"error"` objects.
const PINNED: &[(&str, &str, usize)] = &[
    ("error-envelope", "causal.rs", 5),
    ("error-envelope", "check_stages.rs", 1),
    ("error-envelope", "frontend_output.rs", 2),
    ("error-envelope", "literate_access.rs", 3),
    // `chain`'s nested `parse` details and its failed-layer records.
    ("error-envelope", "main.rs", 9),
    ("error-envelope", "migration.rs", 1),
    ("error-envelope", "property_selection.rs", 1),
    ("error-envelope", "spec_load.rs", 1),
    ("error-envelope", "verification.rs", 6),
    ("error-envelope", "verification_output.rs", 6),
    ("error-exit-literal", "bin/fsl-bmc.rs", 1),
    ("error-exit-literal", "bin/fsl-replay-actions.rs", 1),
    ("error-exit-literal", "causal.rs", 54),
    ("error-exit-literal", "check_stages.rs", 8),
    ("error-exit-literal", "cli_options.rs", 1),
    ("error-exit-literal", "coverage.rs", 1),
    ("error-exit-literal", "frontend_output.rs", 2),
    ("error-exit-literal", "literate_access.rs", 3),
    ("error-exit-literal", "main.rs", 332),
    ("error-exit-literal", "migration.rs", 3),
    ("error-exit-literal", "refinement.rs", 9),
    ("error-exit-literal", "spec_loading.rs", 1),
    ("error-exit-literal", "verification.rs", 49),
    ("error-exit-literal", "verification_output.rs", 1),
    // The library's literate capture cannot call the binary's IO owner.
    ("io-classification", "literate_access.rs", 1),
];

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Two-element tuples whose second element is the integer literal 2 or 3:
/// an envelope paired with a hand-picked error exit.
struct ExitTuples(usize);

impl<'ast> Visit<'ast> for ExitTuples {
    fn visit_expr_tuple(&mut self, node: &'ast syn::ExprTuple) {
        if node.elems.len() == 2
            && let Some(syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(literal),
                ..
            })) = node.elems.last()
            && matches!(literal.base10_digits(), "2" | "3")
        {
            self.0 += 1;
        }
        syn::visit::visit_expr_tuple(self, node);
    }
}

fn count(rule: &Rule, source: &str) -> usize {
    let product = product::product_source(source);
    let flat = product
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    let mut total = rule
        .needles
        .iter()
        .map(|needle| flat.matches(needle).count())
        .sum::<usize>();
    if rule.exit_tuples {
        let file = syn::parse_file(&product).expect("parse product source");
        let mut tuples = ExitTuples(0);
        tuples.visit_file(&file);
        total += tuples.0;
    }
    total
}

fn census() -> BTreeMap<(&'static str, String), usize> {
    let root = src();
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    files.sort();
    let mut counts = BTreeMap::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .expect("under src")
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(&path).expect("read source");
        for rule in RULES {
            if rule.owners.contains(&relative.as_str()) {
                continue;
            }
            let found = count(rule, &source);
            if found > 0 {
                counts.insert((rule.name, relative.clone()), found);
            }
        }
    }
    counts
}

#[test]
fn hand_written_error_policy_sites_only_decrease() {
    let actual = census();
    let pinned = PINNED
        .iter()
        .map(|(rule, file, count)| ((*rule, (*file).to_owned()), *count))
        .collect::<BTreeMap<_, _>>();
    let mut problems = Vec::new();
    for (key, found) in &actual {
        let pin = pinned.get(key).copied().unwrap_or(0);
        if *found > pin {
            problems.push(format!(
                "{} in src/{}: {found} sites, pinned {pin}: route the new site through the owner",
                key.0, key.1
            ));
        }
    }
    for (key, pin) in &pinned {
        let found = actual.get(key).copied().unwrap_or(0);
        if found < *pin {
            problems.push(format!(
                "{} in src/{}: {found} sites, pinned {pin}: lower the pin to {found}",
                key.0, key.1
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "#1170 error-policy ratchet:\n{}\ncurrent census: {actual:#?}",
        problems.join("\n")
    );
}

/// The census sees each spelling it is meant to see, and ignores comments,
/// doc text and test-only items.
#[test]
fn census_counts_each_spelling_and_skips_tests_and_comments() {
    let rule = |name| RULES.iter().find(|rule| rule.name == name).expect("rule");
    let io = rule("io-classification");
    for (source, expected) in [
        ("fn f() { error_output(\"io\", \"x\"); }", 1),
        ("fn f() { error_detail( \"io\" , \"x\"); }", 1),
        (
            "fn f() { json!({\"result\": \"error\", \"kind\": \"io\"}); }",
            1,
        ),
        ("fn f() { SpecLoadError::Io(m); }", 1),
        ("fn f() { o.insert(k, json!(\"io\")); }", 1),
        ("// error_output(\"io\", \"x\")\nfn f() {}", 0),
        ("/// error_output(\"io\", \"x\")\nfn f() {}", 0),
        ("#[cfg(test)]\nfn f() { error_output(\"io\", \"x\"); }", 0),
        ("#[test]\nfn f() { error_output(\"io\", \"x\"); }", 0),
        ("fn f() { error_output(\"usage\", \"x\"); }", 0),
    ] {
        assert_eq!(count(io, source), expected, "{source}");
    }
    let envelope = rule("error-envelope");
    for (source, expected) in [
        ("fn f() { json!({\"result\":\n \"error\"}); }", 1),
        (
            "fn f() { o.insert(\"result\".to_owned(), json!(\"error\")); }",
            1,
        ),
        ("fn f() { json!({\"result\": \"violated\"}); }", 0),
    ] {
        assert_eq!(count(envelope, source), expected, "{source}");
    }
    let exit = rule("error-exit-literal");
    for (source, expected) in [
        ("fn f() -> (u8, i32) { return (x, 2); }", 1),
        ("fn f() -> (u8, i32) { (\n x,\n 3,\n) }", 1),
        ("fn f() { json!({\"exit_code\": 2}); }", 1),
        ("fn f() { std::process::exit(2); }", 1),
        ("fn f() -> (u8, i32) { (x, 1) }", 0),
        ("fn f() -> (u8, u8, i32) { (x, y, 2) }", 0),
        ("fn f() -> (u8, i32) { error_exit(x) }", 0),
    ] {
        assert_eq!(count(exit, source), expected, "{source}");
    }
}

// SPDX-License-Identifier: Apache-2.0

//! Issue #1171 (candidate 4): the binary's modules depend on shared helper
//! modules, never back on `main.rs`.
//!
//! `verification.rs`, `causal.rs` and `code_audit.rs` used to pull ~45
//! envelope, error, loader and display-path helpers from the crate root with
//! `use super::{...}` and `crate::<item>`, so every helper had to stay in
//! `main.rs`. Those helpers now live in the modules `main.rs` declares. A
//! `super::` path or a `crate::` path whose first segment is not one of those
//! modules names an item (or an import) of `main.rs` again, and is rejected
//! here. Test modules are skipped: their `super::` is the module under test.

use std::path::Path;

/// `(module file, path)` dependencies on `main.rs` that must stay. Empty: no
/// module needs an item that only `main.rs` can own.
const ALLOWED: &[(&str, &str)] = &[];

fn src() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))
}

/// The modules `main.rs` declares (`mod name;`).
fn declared_modules(main: &str) -> Vec<String> {
    main.lines()
        .filter_map(|line| line.strip_prefix("mod ")?.strip_suffix(';'))
        .map(str::to_owned)
        .collect()
}

/// Source with every `#[cfg(test)]` module removed and line comments blanked.
fn product_lines(source: &str) -> Vec<(usize, String)> {
    let mut lines = Vec::new();
    let mut in_test_module = false;
    let mut previous_cfg_test = false;
    for (index, line) in source.lines().enumerate() {
        if in_test_module {
            in_test_module = line != "}";
            continue;
        }
        if previous_cfg_test && line.starts_with("mod ") && line.ends_with('{') {
            in_test_module = true;
            previous_cfg_test = false;
            continue;
        }
        previous_cfg_test = line == "#[cfg(test)]";
        let code = line.split("//").next().unwrap_or_default();
        lines.push((index + 1, code.to_owned()));
    }
    lines
}

/// Every path in `source` that reaches an item of the crate root.
fn root_dependencies(source: &str, modules: &[String]) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (line_number, line) in product_lines(source) {
        let line = line.replace("pub(super)", "");
        for (offset, _) in line.match_indices("super::") {
            let preceded_by_ident = line[..offset]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if !preceded_by_ident {
                found.push((line_number, line[offset..].trim().to_owned()));
            }
        }
        for (offset, _) in line.match_indices("crate::") {
            let preceded_by_ident = line[..offset]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if preceded_by_ident {
                continue;
            }
            let rest = &line[offset + "crate::".len()..];
            let segment = rest
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap_or_default();
            let names_a_module = modules.iter().any(|module| module == segment)
                && rest[segment.len()..].starts_with("::");
            if !names_a_module {
                found.push((line_number, line[offset..].trim().to_owned()));
            }
        }
    }
    found
}

#[test]
fn binary_modules_do_not_depend_on_main_rs_items() {
    let main = std::fs::read_to_string(src().join("main.rs")).expect("read main.rs");
    let modules = declared_modules(&main);
    for required in [
        "verification",
        "causal",
        "code_audit",
        "cli_output",
        "spec_loading",
    ] {
        assert!(
            modules.iter().any(|module| module == required),
            "main.rs no longer declares `mod {required};`: {modules:?}"
        );
    }
    let mut violations = Vec::new();
    for module in &modules {
        let file = format!("{module}.rs");
        let source = std::fs::read_to_string(src().join(&file)).expect("read module");
        for (line, path) in root_dependencies(&source, &modules) {
            if !ALLOWED
                .iter()
                .any(|(allowed_file, allowed)| *allowed_file == file && path.starts_with(allowed))
            {
                violations.push(format!("src/{file}:{line}: {path}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "modules reach main.rs items; move the helper into a module instead:\n{}",
        violations.join("\n")
    );
}

/// The scan itself rejects each spelling of a crate-root dependency and
/// accepts module paths, visibility, and test modules.
#[test]
fn root_dependency_scan_rejects_each_spelling() {
    let modules = vec!["cli_output".to_owned(), "causal".to_owned()];
    let rejected = [
        "use super::{envelope, error_output};",
        "    let x = super::STACK_SIZE;",
        "    crate::analysis_display_path(path)",
        "use crate::{cli_output::envelope};",
        "    crate::cli_output_extra::f()",
    ];
    for line in rejected {
        assert_eq!(
            root_dependencies(line, &modules).len(),
            1,
            "the scan must reject `{line}`"
        );
    }
    let accepted = "pub(super) fn f() {}\n\
                    use crate::cli_output::{envelope, error_output};\n\
                    // super::comment\n\
                    let y = fslc_rust::f();\n\
                    crate::causal::causal_parse_error_output(&e, false)\n\
                    #[cfg(test)]\n\
                    mod tests {\n\
                    \x20   use super::*;\n\
                    }\n";
    assert_eq!(root_dependencies(accepted, &modules), Vec::new());
}

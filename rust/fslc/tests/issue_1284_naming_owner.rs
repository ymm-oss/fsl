// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! Drift control for #1284: `fsl_core::naming` is the only place that spells
//! an engine-synthesised name (`_partial_<action>`, `_bounds_<state>`, ...),
//! applies the string rule that turns an internal Kernel name into its public
//! spelling, or joins two name parts with the compose separator `__`.
//!
//! The scan covers non-test code under `rust/*/src` (comments and
//! `#[cfg(test)]` items are skipped). Every finding outside `naming.rs` needs
//! an [`ALLOWED`] entry with a reason, and an entry that no longer matches a
//! finding is an error, so the list cannot outlive the code it excuses.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[path = "support/naming_census_rule.rs"]
mod naming_census_rule;
use naming_census_rule::{Finding, Kind, OWNER, line_findings, non_test_lines};

struct Allowed {
    path: &'static str,
    kind: Kind,
    /// A substring of the finding's line.
    needle: &'static str,
    #[allow(dead_code)]
    reason: &'static str,
}

const ALLOWED: &[Allowed] = &[
    Allowed {
        path: "rust/fsl-tools/src/analysis.rs",
        kind: Kind::SeparatorJoin,
        needle: "format!(\"{logical}__",
        reason: "physical column names of the analysis export (`<logical>__<field>`, \
                 `__present`, `__value`, `__data`, `__len`); a storage layout, not a \
                 Kernel name, and never read back through the naming rule",
    },
    Allowed {
        path: "rust/fsl-core/src/dialect.rs",
        kind: Kind::SeparatorJoin,
        needle: "format!(\"ai_",
        reason: "ai dialect invariant names (`ai_forbidden_tool_not_executed__<tool>`, \
                 `ai_approval_before_execute__<tool>`); a declaration name the dialect \
                 lowers, rendered later like any other property",
    },
    Allowed {
        path: "rust/fsl-core/src/dialect.rs",
        kind: Kind::SeparatorJoin,
        needle: "__b",
        reason: "dialect branch actions (`<action>__b<n>`); a declaration name the \
                 dialect lowers, rendered later like any other action",
    },
    Allowed {
        path: "rust/fsl-core/src/refinement.rs",
        kind: Kind::SeparatorJoin,
        needle: "__b{}",
        reason: "refinement branch actions (`<action>__b<n>`), the same shape the \
                 dialect lowers",
    },
    Allowed {
        path: "rust/fslc/src/verification_output.rs",
        kind: Kind::SeparatorJoin,
        needle: "__b\"",
        reason: "reads the branch-action prefix `<action>__b` back to group branches \
                 of one requirements step",
    },
    Allowed {
        path: "rust/fsl-core/src/lib.rs",
        kind: Kind::SeparatorJoin,
        needle: "_substitution_",
        reason: "fresh binder names for capture-avoiding substitution; never published",
    },
    Allowed {
        path: "rust/fsl-verifier/src/value.rs",
        kind: Kind::SeparatorJoin,
        needle: "format!(\"{name}__",
        reason: "solver constant names for the components of a structured value; \
                 they stay inside the solver",
    },
    Allowed {
        path: "rust/fsl-core/src/expr_text.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "expression text: identifiers of every kind (state, binder, struct type, enum member, call), rendered with or without a model",
    },
    Allowed {
        path: "rust/fsl-core/src/trace_json.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "`state_json` publishes a bare state map; it has no model parameter",
    },
    Allowed {
        path: "rust/fsl-tools/src/analysis.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "graph node labels are built from a node name alone",
    },
    Allowed {
        path: "rust/fsl-tools/src/analysis_graph.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "`--focus` matches node IDs read back from the exported graph JSON",
    },
    Allowed {
        path: "rust/fsl-tools/src/document_render.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "`type_ref_text` renders a type with no model parameter",
    },
    Allowed {
        path: "rust/fsl-core/src/typecheck.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "`shown`: type-check diagnostics name identifiers and types read from an \
                 expression; the string rule is the public spelling of every name a \
                 checked document can produce (#1256)",
    },
    Allowed {
        path: "rust/fsl-core/src/model.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "`shown`: model-build diagnostics, raised while the model (and its \
                 `NameMap`) is still being built",
    },
    Allowed {
        path: "rust/fsl-tools/src/testplan.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "testplan matches Public Kernel action names to conformance vectors; it \
                 rejects compose, so the string rule is the public spelling (#1256)",
    },
    Allowed {
        path: "rust/fsl-tools/src/testgen.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "testgen reads names from the Public Kernel JSON",
    },
    Allowed {
        path: "rust/fsl-tools/src/typestate.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "typestate analysis reads the Public Kernel JSON view, not a `KernelModel`",
    },
    Allowed {
        path: "rust/fslc/src/main.rs",
        kind: Kind::UnmappedSpelling,
        needle: "unmapped_public_spelling(",
        reason: "`enum_member`, the statement-root collector, and `tag_lvalue_text` \
                 (the assignment root beside `expr_text`) receive no model",
    },
    Allowed {
        path: "rust/fslc/src/main.rs",
        kind: Kind::SeparatorJoin,
        needle: "__b",
        reason: "requirements branch actions (`<action>__b<n>`): the mutate label table and \
                 the scenario final-step match; the same shape the dialect lowers",
    },
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn sources() -> Vec<(String, String)> {
    fn visit(directory: &Path, paths: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory).expect("read source directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                visit(&path, paths);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                paths.push(path);
            }
        }
    }
    let root = repository_root();
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(root.join("rust")).expect("read rust/") {
        let src = entry.expect("crate entry").path().join("src");
        if src.is_dir() {
            visit(&src, &mut paths);
        }
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&root)
                .expect("source below repository")
                .to_string_lossy()
                .replace('\\', "/");
            let source = std::fs::read_to_string(&path).expect("read Rust source");
            (relative, source)
        })
        .collect()
}

fn findings(sources: &[(String, String)]) -> Vec<Finding> {
    sources
        .iter()
        .flat_map(|(path, source)| naming_census_rule::findings(path, source))
        .collect()
}

fn covers(entry: &Allowed, finding: &Finding) -> bool {
    entry.path == finding.path && entry.kind == finding.kind && finding.text.contains(entry.needle)
}

/// Findings no entry covers, and entries that cover no finding.
fn drift<'a>(
    findings: &'a [Finding],
    allowed: &'a [Allowed],
) -> (Vec<&'a Finding>, Vec<(&'static str, Kind, &'static str)>) {
    let uncovered = findings
        .iter()
        .filter(|finding| !allowed.iter().any(|entry| covers(entry, finding)))
        .collect();
    let stale = allowed
        .iter()
        .filter(|entry| !findings.iter().any(|finding| covers(entry, finding)))
        .map(|entry| (entry.path, entry.kind, entry.needle))
        .collect();
    (uncovered, stale)
}

#[test]
fn naming_is_the_only_owner_of_synthetic_and_public_spellings() {
    let sources = sources();
    let findings = findings(&sources);
    let (uncovered, stale) = drift(&findings, ALLOWED);
    let report = uncovered
        .iter()
        .map(|finding| {
            format!(
                "{}:{} {:?}: {}",
                finding.path, finding.line, finding.kind, finding.text
            )
        })
        .collect::<Vec<_>>();
    assert!(
        report.is_empty() && stale.is_empty(),
        "#1284 naming drift: build synthetic names with `fsl_core::SyntheticName`, read them \
         with `SyntheticName::parse_internal`, and publish names through `fsl_core::NameMap`.\n\
         {} finding(s) outside naming.rs:\n{}\nstale allowlist entries: {stale:?}",
        report.len(),
        report.join("\n")
    );
}

#[test]
fn the_exempt_owner_holds_every_spelling_it_owns() {
    let sources = sources();
    let (_, owner) = sources
        .iter()
        .find(|(path, _)| path == OWNER)
        .expect("the naming owner exists");
    let kinds = non_test_lines(owner)
        .into_iter()
        .enumerate()
        .flat_map(|(index, text)| line_findings("owner", index + 1, &text))
        .map(|finding| finding.kind)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        kinds,
        BTreeSet::from([
            Kind::SyntheticPrefix,
            Kind::DisplayName,
            Kind::SeparatorJoin,
            Kind::UnmappedSpelling,
        ]),
        "the exemption must cover a module that really owns all three spellings"
    );
}

#[test]
fn test_items_comments_and_strings_are_classified() {
    let source = "fn live() { let _ = format!(\"_partial_{}\", a); }\n\
                  // format!(\"_bounds_{x}\")\n\
                  /* display_name(x) */\n\
                  fn rule() -> String { origin_display_name(x); display_name(y) }\n\
                  #[cfg(test)]\n\
                  mod tests {\n\
                      fn t() { let _ = \"_bounds_x\"; let _ = '{'; }\n\
                  }\n\
                  fn after() { let _ = format!(\"{a}__{b}\"); }\n";
    let found = non_test_lines(source)
        .into_iter()
        .enumerate()
        .flat_map(|(index, text)| line_findings("fixture.rs", index + 1, &text))
        .map(|finding| (finding.line, finding.kind))
        .collect::<Vec<_>>();
    assert_eq!(
        found,
        [
            (1, Kind::SyntheticPrefix),
            (4, Kind::DisplayName),
            (9, Kind::SeparatorJoin),
        ]
    );
}

#[test]
fn an_uncovered_finding_and_a_stale_entry_are_both_reported() {
    let findings = vec![Finding {
        path: "fixture.rs".to_owned(),
        line: 1,
        kind: Kind::SyntheticPrefix,
        text: "format!(\"_bounds_{name}\")".to_owned(),
    }];
    let allowed = [Allowed {
        path: "fixture.rs",
        kind: Kind::SeparatorJoin,
        needle: "__b",
        reason: "fixture",
    }];
    let (uncovered, stale) = drift(&findings, &allowed);
    assert_eq!(uncovered, [&findings[0]]);
    assert_eq!(stale, [("fixture.rs", Kind::SeparatorJoin, "__b")]);
}

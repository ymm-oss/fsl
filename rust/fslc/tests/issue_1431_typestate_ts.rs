// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

//! #1431 CLI acceptance. `fslc typestate FILE` includes TypeScript in each
//! entity's `typescript` field; no --ts flag is needed. It emits declarations,
//! not executable function bodies. Runtime-precondition comments quote FSL,
//! rather than containing executable TS parameter references.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const RESERVED: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "implements",
    "interface",
    "let",
    "package",
    "private",
    "protected",
    "public",
    "static",
    "await",
    "type",
];
const ALREADY_ESCAPED: &[&str] = &[
    "void",
    "delete",
    "new",
    "default",
    "function",
    "return",
    "switch",
    "case",
    "class",
    "enum",
    "interface",
    "type",
];
const FSL_REJECTED: &[&str] = &["true", "false"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf()
}

fn source(action: &str, parameters: &str, precondition: &str) -> String {
    format!(
        "spec Keyword {{ type N = 0..3 enum St {{ Open, Closed }} struct Item {{ status: St }}
         state {{ item: Item }} init {{ item = Item {{ status: Open }} }}
         action {action}({parameters}) {{ requires item.status == Open
         {precondition} item.status = Closed }} }}"
    )
}

fn cli(command: &str, source: &str) -> (i32, Value) {
    let directory = root().join(".codex/tmp").join(format!(
        "issue-1431-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("local scratch");
    let path = directory.join("model.fsl");
    std::fs::write(&path, source).expect("write source");
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .arg(command)
        .arg(&path)
        .current_dir(root())
        .output()
        .expect("run fslc");
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "JSON={error}; stdout={:?}; stderr={:?}",
            output.stdout, output.stderr
        )
    });
    (output.status.code().expect("exit code"), value)
}

fn typescript(source: &str) -> String {
    // Check the exact reported input before asking the generator to consume it.
    let (status, checked) = cli("check", source);
    assert_eq!(status, 0, "check produced={checked}; source={source}");
    let (status, report) = cli("typestate", source);
    assert_eq!(status, 0, "typestate produced={report}; source={source}");
    report["entities"]
        .as_array()
        .expect("entities")
        .iter()
        .find(|entity| entity["entity"] == "Item.status")
        .and_then(|entity| entity["typescript"].as_str())
        .unwrap_or_else(|| panic!("missing Item.status TypeScript: {report}"))
        .to_owned()
}

/// Control (passes before/after): inventory each requested word separately as
/// action and parameter declarations using fslc check. Only true/false are
/// rejected as reserved FSL literal names; pin those errors, never skip silently.
#[test]
fn reserved_word_declarations_are_accepted_by_fsl_check() {
    let mut rejected = Vec::new();
    for word in RESERVED {
        for (role, source) in [
            ("action", source(word, "", "")),
            ("parameter", source("advance", &format!("{word}: N"), "")),
        ] {
            let (status, value) = cli("check", &source);
            println!("RESERVED_PROBE role={role} word={word} exit={status} output={value}");
            if FSL_REJECTED.contains(word) {
                let article = if role == "action" {
                    "a action"
                } else {
                    "a parameter"
                };
                assert_eq!(status, 2, "produced {role} {word}: {value}");
                assert_eq!(value["kind"], "name", "produced {role} {word}: {value}");
                assert_eq!(
                    value["message"],
                    format!(
                        "'{word}' is a reserved FSL keyword and cannot be used as {article} name; every occurrence in an expression resolves to the literal, so the declaration would be unreadable"
                    ),
                    "produced {role} {word}: {value}"
                );
            } else if status != 0 {
                rejected.push(format!("{role} {word}: {value}"));
            }
        }
    }
    assert!(
        rejected.is_empty(),
        "produced rejected declarations={rejected:#?}"
    );
}

/// Detector (fails on 9ad07c7a): every accepted reserved action absent from the
/// old short list must have a trailing _. Collect all mismatches to expose the
/// entire accepted set, not only the first missing word.
#[test]
fn every_newly_covered_reserved_action_gets_a_trailing_underscore() {
    let mut mismatches = Vec::new();
    for word in RESERVED
        .iter()
        .filter(|word| !ALREADY_ESCAPED.contains(word) && !FSL_REJECTED.contains(word))
    {
        let ts = typescript(&source(word, "", ""));
        if !ts.contains(&format!("export function {word}_(")) {
            mismatches.push(format!("word={word}; produced TS={ts}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "produced unescaped actions={mismatches:#?}"
    );
}

/// Detector (fails on 9ad07c7a): ALL accepted reserved parameter names, including
/// delete, need escaping in function signatures. There are no executable body
/// references in the current declaration-only generator to test separately.
#[test]
fn every_reserved_parameter_gets_a_trailing_underscore() {
    let mut mismatches = Vec::new();
    for word in RESERVED.iter().filter(|word| !FSL_REJECTED.contains(word)) {
        let ts = typescript(&source("advance", &format!("{word}: N"), ""));
        if !ts.contains(&format!(", {word}_: number)")) {
            mismatches.push(format!("word={word}; produced TS={ts}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "produced unescaped parameters={mismatches:#?}"
    );
}

/// Control (passes before/after): the entire previous action-word list stays
/// escaped, including delete and the additional TypeScript word type.
#[test]
fn already_escaped_action_words_stay_escaped() {
    for word in ALREADY_ESCAPED {
        let ts = typescript(&source(word, "", ""));
        assert!(
            ts.contains(&format!("export function {word}_(")),
            "word={word}; produced TS={ts}"
        );
    }
}

/// Detector (fails on 9ad07c7a): CLI-reachable parameter types come from the
/// model, not blanket number: Bool/enum plus finite ranges and named domains.
#[test]
fn parameter_signatures_use_model_types() {
    let source = "spec Keyword {
        type Domain = 0..3
        enum St { Open, Closed } enum Mode { Fast, Slow }
        struct Item { status: St }
        state { item: Item } init { item = Item { status: Open } }
        action advance(n in 0..3, r in 0..2, d: Domain, flag: Bool, mode: Mode) {
            requires item.status == Open item.status = Closed
        }
    }";
    let ts = typescript(source);
    let expected = "export function advance(self: Item<\"Open\">, n: number, r: number, d: number, flag: boolean, mode: Mode): Item<\"Closed\">;";
    assert!(
        ts.lines().any(|line| line == expected),
        "expected signature={expected}; produced TS={ts}"
    );
}

/// Detector (fails on 9ad07c7a): unbounded Int, struct, Option and Set parameters
/// are not finite scalars accepted by CLI check. Adapt a real CLI kernel's
/// parameters through public `analyze_typestate`, preserving its real entity and
/// transition. Public JSON uses the same type shapes as real state fields.
#[test]
fn public_kernel_parameter_types_cover_non_cli_scalar_shapes() {
    let source = "spec Keyword { type N = 0..3 enum St { Open, Closed }
        enum Mode { Fast, Slow } struct Payload { count: Int }
        struct Item { status: St } state { item: Item }
        init { item = Item { status: Open } }
        action advance(n: N) { requires item.status == Open item.status = Closed } }";
    let (status, mut kernel) = cli("kernel", source);
    assert_eq!(status, 0, "produced kernel={kernel}");
    kernel["actions"][0]["parameters"] = json!([
        {"name":"n", "type":{"kind":"int"}},
        {"name":"r", "type":{"kind":"domain", "lo":0,"hi":3}},
        {"name":"d", "type":{"kind":"named", "name":"N"}},
        {"name":"flag", "type":{"kind":"bool"}},
        {"name":"mode", "type":{"kind":"named", "name":"Mode"}},
        {"name":"data", "type":{"kind":"named", "name":"Payload"}},
        {"name":"maybe", "type":{"kind":"option", "item":{"kind":"int"}}},
        {"name":"modes", "type":{"kind":"set", "item":{"kind":"named", "name":"Mode"}}}
    ]);
    let result = fsl_tools::analyze_typestate(&kernel);
    assert!(
        result.is_ok(),
        "produced result={result:?}; kernel={kernel}"
    );
    let report = result.expect("checked result");
    let ts = report["entities"][0]["typescript"]
        .as_str()
        .unwrap_or_else(|| panic!("produced report={report}"));
    let expected = "export function advance(self: Item<\"Open\">, n: number, r: number, d: number, flag: boolean, mode: Mode, data: Payload, maybe: number | null, modes: Set<Mode>): Item<\"Closed\">;";
    assert!(
        ts.lines().any(|line| line == expected),
        "expected signature={expected}; produced TS={ts}"
    );
}

/// Detector (fails on 9ad07c7a): export is also used by a runtime precondition.
/// Its signature binding is escaped, while the comment keeps the original FSL
/// expression, which is not an executable TS reference to be renamed.
#[test]
fn reserved_parameter_precondition_stays_source_text_not_ts_body() {
    let ts = typescript(&source("advance", "export: N", "requires export > 0"));
    assert!(
        ts.contains("// runtime precondition (not in type): (export > 0)"),
        "produced TS={ts}"
    );
    assert!(ts.contains(", export_: number)"), "produced TS={ts}");
}

/// Control (passes before/after): full all-integer output captured on 9ad07c7a;
/// finite named parameters are necessary because unbounded Int is rejected.
/// ordinary action/parameter names, source precondition comment, and all bytes
/// remain unchanged. Source comments are FSL, not executable TS references.
#[test]
fn nonreserved_all_int_typescript_keeps_exact_bytes() {
    let ts = typescript(&source("advance", "amount: N", "requires amount > 0"));
    let expected = concat!(
        "// Typestate skeleton for `Item` from spec `Keyword`.\n",
        "// FSL holds these in a collection; phantom types track one entity, so each becomes an independently-typed handle.\n",
        "// Only transitions with a LOCAL from-state guard are typed; the rest stay dynamic.\n\n",
        "export type ItemState = \"Open\" | \"Closed\";\n\n",
        "declare const __state: unique symbol;\n",
        "export interface Item<S extends ItemState> {\n",
        "  readonly [__state]: S;\n}\n\n",
        "  // runtime precondition (not in type): (amount > 0)\n",
        "export function advance(self: Item<\"Open\">, amount: number): Item<\"Closed\">;"
    );
    assert_eq!(ts, expected, "produced TS={ts}");
}

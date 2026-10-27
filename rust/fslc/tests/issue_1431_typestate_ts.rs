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

/// Detector on 12d196da: escaping type and delete aliases their already-suffixed
/// parameter names. Allocate in declaration order, appending _ until distinct.
/// FSL rejects the extra type__ chain name (reserved __); explicitly check that
/// rejection, then adapt an accepted real public Kernel for that case only.
#[test]
fn escaped_parameter_pairs_remain_distinct_in_declaration_order() {
    let mut mismatches = Vec::new();
    for (params, expected_params) in [
        ("type: N, type_: N", "type_: number, type__: number"),
        ("delete: N, delete_: N", "delete_: number, delete__: number"),
    ] {
        let ts = typescript(&source("advance", params, ""));
        let expected = format!(
            "export function advance(self: Item<\"Open\">, {expected_params}): Item<\"Closed\">;"
        );
        if !ts.lines().any(|line| line == expected) {
            mismatches.push(format!("expected signature={expected}; produced TS={ts}"));
        }
    }
    let rejected = source("advance", "type: N, type_: N, type__: N", "");
    let (status, checked) = cli("check", &rejected);
    assert_eq!(status, 2, "produced check={checked}");
    assert_eq!(checked["kind"], "name", "produced check={checked}");
    assert!(
        checked["message"]
            .as_str()
            .expect("diagnostic")
            .contains("'type__' contains '__'"),
        "produced check={checked}"
    );
    let (status, mut kernel) = cli("kernel", &source("advance", "type: N, type_: N", ""));
    assert_eq!(status, 0, "produced kernel={kernel}");
    let ty = kernel["actions"][0]["parameters"][0]["type"].clone();
    kernel["actions"][0]["parameters"]
        .as_array_mut()
        .expect("parameters")
        .push(json!({"name":"type__", "type":ty}));
    let result = fsl_tools::analyze_typestate(&kernel);
    assert!(
        result.is_ok(),
        "produced result={result:?}; kernel={kernel}"
    );
    let report = result.expect("checked report");
    let ts = report["entities"][0]["typescript"]
        .as_str()
        .expect("typescript");
    let expected = "export function advance(self: Item<\"Open\">, type_: number, type__: number, type___: number): Item<\"Closed\">;";
    if !ts.lines().any(|line| line == expected) {
        mismatches.push(format!(
            "public Kernel expected signature={expected}; produced TS={ts}"
        ));
    }
    assert!(mismatches.is_empty(), "produced mismatches={mismatches:#?}");
}

/// Control on 12d196da: check self, eval and arguments explicitly in both roles.
/// All are CLI-accepted; none needs the public-Kernel fallback used in round 0.
#[test]
fn generated_and_strict_mode_names_are_accepted_by_fsl_check() {
    for name in ["self", "eval", "arguments"] {
        for source in [
            source(name, "", ""),
            source("advance", &format!("{name}: N"), ""),
        ] {
            let (status, checked) = cli("check", &source);
            assert_eq!(status, 0, "source={source}; produced check={checked}");
        }
    }
}

/// Detector on 12d196da: the fixed self handle must not alias a model parameter;
/// subsequent parameters are allocated in declaration order against that handle.
#[test]
fn self_handle_is_reserved_before_model_parameters() {
    let ts = typescript(&source("advance", "self: N, self_: N", ""));
    let expected = "export function advance(self: Item<\"Open\">, self_: number, self__: number): Item<\"Closed\">;";
    assert!(
        ts.lines().any(|line| line == expected),
        "expected signature={expected}; produced TS={ts}"
    );
}

/// Detector on 12d196da: strict-mode modules forbid eval/arguments bindings even
/// though they are not keywords. Both parameters and action names get one _.
#[test]
fn strict_mode_bindings_are_escaped_in_parameters_and_actions() {
    let mut mismatches = Vec::new();
    for (action, params, expected) in [
        (
            "advance",
            "eval: N, arguments: N",
            "export function advance(self: Item<\"Open\">, eval_: number, arguments_: number): Item<\"Closed\">;",
        ),
        (
            "eval",
            "",
            "export function eval_(self: Item<\"Open\">): Item<\"Closed\">;",
        ),
        (
            "arguments",
            "",
            "export function arguments_(self: Item<\"Open\">): Item<\"Closed\">;",
        ),
    ] {
        let ts = typescript(&source(action, params, ""));
        if !ts.lines().any(|line| line == expected) {
            mismatches.push(format!("expected={expected}; produced TS={ts}"));
        }
    }
    assert!(mismatches.is_empty(), "produced mismatches={mismatches:#?}");
}

/// Detector on 12d196da: distinct actions delete and delete_ currently both emit
/// delete_. Keep the first action's escaped name, then allocate delete__.
#[test]
fn distinct_actions_do_not_alias_after_escaping() {
    let ts = typescript(
        "spec Keyword { enum St { Open, Closed } struct Item { status: St }
         state { item: Item } init { item = Item { status: Open } }
         action delete() { requires item.status == Open item.status = Closed }
         action delete_() { requires item.status == Open item.status = Closed } }",
    );
    let signatures: Vec<_> = ts
        .lines()
        .filter(|line| line.starts_with("export function"))
        .collect();
    assert_eq!(
        signatures,
        [
            "export function delete_(self: Item<\"Open\">): Item<\"Closed\">;",
            "export function delete__(self: Item<\"Open\">): Item<\"Closed\">;",
        ],
        "produced signatures={signatures:?}; TS={ts}"
    );
}

/// Detector on 12d196da: the phantom S generic captures enum S in mode: S.
/// Skip all declared model type names, not merely the first occupied candidate.
#[test]
fn phantom_generic_does_not_capture_model_type_names() {
    let mut mismatches = Vec::new();
    for (extra_types, generic) in [("", "S_"), ("enum S_ { Low, High }", "S__")] {
        let ts = typescript(&format!(
            "spec Capture {{ enum St {{ Open, Closed }} enum S {{ Fast, Slow }}
             {extra_types} struct Item {{ status: St }} state {{ item: Item }}
             init {{ item = Item {{ status: Open }} }}
             action inspect(mode: S) {{ requires item.status == Open }} }}"
        ));
        let expected = format!(
            "export function inspect<{generic} extends \"Open\">(self: Item<{generic}>, mode: S): Item<{generic}>;"
        );
        if !ts.lines().any(|line| line == expected) {
            mismatches.push(format!("expected signature={expected}; produced TS={ts}"));
        }
    }
    assert!(mismatches.is_empty(), "produced mismatches={mismatches:#?}");
}

/// Detector on 12d196da: parameter S must not alias the fixed phantom generic S.
/// Without a model type S the generic stays S and the parameter becomes S_.
#[test]
fn phantom_generic_and_parameter_bindings_are_distinct() {
    let ts = typescript(
        "spec Capture { type N = 0..3 enum St { Open, Closed }
         struct Item { status: St } state { item: Item }
         init { item = Item { status: Open } }
         action inspect(S: N) { requires item.status == Open } }",
    );
    let expected =
        "export function inspect<S extends \"Open\">(self: Item<S>, S_: number): Item<S>;";
    assert!(
        ts.lines().any(|line| line == expected),
        "expected signature={expected}; produced TS={ts}"
    );
}

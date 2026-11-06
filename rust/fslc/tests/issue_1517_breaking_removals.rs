// SPDX-License-Identifier: Apache-2.0

//! #1517 breaking removals, one test per form in
//! DESIGN-sync-argument-root-validation, "Breaking removals". Each form passed
//! `check` on the base before #1517; the complete packet pins kind and location.
//! The `valid_*` forms are the migration targets and must keep passing.

#[path = "support/issue_1517_scratch.rs"]
mod scratch;

use serde_json::{Value, json};
use std::{fs, process::Command};

const PIECE: &str = "spec Piece {\n  type K = 0..1\n  enum Color { Red, Blue }\n  const C = 1\n  def twice(v: Int) = v + v\n  def isz(v: K) = m[v] == 0\n  state { h: relation K -> K, last: K, flag: Bool, col: Color, o: Option<K>, s: Set<K>, q: Seq<K, 2>, m: Map<K, K> }\n  init { h = Set {} last = 0 flag = false col = Red o = none s = Set {} q = Seq {} forall k: K { m[k] = 0 } }\n  action unusedK(a: K) { last = 0 }\n  action unusedBool(a: Bool) { }\n  action unusedColor(a: Color) { }\n}\n";
const OTHER: &str = "spec Other {\n  type K = 0..1\n  enum Tone { Hi, Lo }\n  state { last: K, tone: Tone }\n  init { last = 0 tone = Hi }\n  action useK(a: K) { last = a }\n}\n";
const BOOL_TO_K: &str =
    "invalid model expression: expression of type Bool is not assignable to Named(\"part.K\")";

/// Line 6 holds the reference unless a declaration or a line break moves it.
fn wide(reference: &str, uses: &str, declarations: &str, body: &str) -> String {
    format!(
        "compose Root {{\n  use Piece as part from \"piece.fsl\"\n{uses}\n{declarations}\n  action step() =\n      {reference} {{\n{body}\n  }}\n\n  internal part.unusedK\n  internal part.unusedBool\n  internal part.unusedColor\n}}\n"
    )
}

fn unused(reference: &str) -> String {
    wide(reference, "", "", "")
}

/// Line 5 holds the reference, with one internal action.
fn narrow(actual: &str) -> String {
    format!(
        "compose Root {{\n  use Piece as part from \"piece.fsl\"\n\n  action step() =\n      part.put({actual}) {{\n  }}\n\n  internal part.put\n}}\n"
    )
}

fn single_action(declarations: &str, action: &str) -> String {
    format!(
        "spec Piece {{ {declarations} state {{ last: Int, flag: Bool }} init {{ last = 0 flag = false }} action {action} }}\n"
    )
}

fn component_rejection(reason: &str, column: u32) -> String {
    format!(
        "component \"piece.fsl\" failed to check (part.put: {reason} at piece.fsl:1:{column}) at root.fsl:2:3"
    )
}

/// `(root, piece, Some((complete message, line, column)))`; `None` must pass.
struct Form {
    root: String,
    piece: String,
    expected: Option<(String, u32, u32)>,
}

#[allow(clippy::too_many_lines)] // One literal row per recorded removal form.
fn form(id: &str) -> Form {
    let model = |reason: &str| format!("invalid model expression: {reason}");
    let cannot_type = |name: &str| model(&format!("public Kernel cannot type identifier '{name}'"));
    let wide_piece = PIECE.to_owned();
    let (root, piece, expected): (String, String, Option<(String, u32, u32)>) = match id {
        "internal_int_formal" => (
            narrow("0"),
            single_action("", "put(a: Int) { last = a }"),
            Some((
                component_rejection("action parameter type is not a finite scalar", 78),
                2,
                3,
            )),
        ),
        "internal_empty_range_formal" => (
            narrow("0"),
            single_action("", "put(a in 1..0) { last = a }"),
            Some((
                component_rejection("action parameter range is empty", 78),
                2,
                3,
            )),
        ),
        "internal_unknown_type_formal" => (
            narrow("0"),
            single_action("", "put(a: Nope) { }"),
            Some((component_rejection("unknown type 'Nope'", 78), 2, 3)),
        ),
        "internal_struct_formal" => (
            narrow("0"),
            single_action("struct T { x: Bool }", "put(a: T) { }"),
            Some((
                component_rejection("action parameter type is not a finite scalar", 98),
                2,
                3,
            )),
        ),
        "used_range_formal_with_bool" => (
            narrow("true"),
            single_action("", "put(a in 0..1) { flag = a }"),
            Some((
                model("expression of type Bool is not assignable to Range(0, 1) at 5:16"),
                5,
                16,
            )),
        ),
        "used_named_formal_with_bool" => (
            narrow("true"),
            single_action("type K = 0..1", "put(a: K) { flag = a }"),
            Some((format!("{BOOL_TO_K} at 5:16"), 5, 16)),
        ),
        "used_named_formal_with_enum" => (
            narrow("R"),
            "spec Piece { enum C { R, G } type K = 0..1 state { col: C, m: Map<C, Bool> } init { col = R forall c: C { m[c] = false } } action put(a: K) { m[a] = true } }\n"
                .to_owned(),
            Some((
                model(
                    "expression of type Named(\"part.C\") is not assignable to Named(\"part.K\") at 5:16",
                ),
                5,
                16,
            )),
        ),
        // #1680 refuses the repeated formal itself, and a declaration failure comes
        // before typing the actuals (DESIGN-sync-argument-root-validation, "Error
        // phases"), so the report moved from the erased actual to the formal.
        "duplicate_formal_with_erased_bool" => (
            narrow("true, 0"),
            "spec Piece { type K = 0..1 state { last: K } init { last = 0 } action put(a: K, a: K) { last = a } }\n"
                .to_owned(),
            Some((
                component_rejection("duplicate action parameter 'a'", 81),
                2,
                3,
            )),
        ),
        "unused_bool" => (
            narrow("true"),
            "spec Piece { type K = 0..1 state { last: K } init { last = 0 } action put(a: K) { last = 0 } }\n"
                .to_owned(),
            Some((format!("{BOOL_TO_K} at 5:16"), 5, 16)),
        ),
        "unused_relation_index" => (
            unused("part.unusedK(part.h[0])"),
            wide_piece,
            Some((
                model("public Kernel index requires Map or Seq at 6:20"),
                6,
                20,
            )),
        ),
        "unused_unresolved_name" => (
            unused("part.unusedK(nosuch)"),
            wide_piece,
            Some((format!("{} at 6:20", cannot_type("nosuch")), 6, 20)),
        ),
        "unused_name_bound_only_by_let" => (
            wide("part.unusedK(x)", "", "", "    let x = 0"),
            wide_piece,
            Some((format!("{} at 6:20", cannot_type("x")), 6, 20)),
        ),
        "unused_other_component_enum" => (
            wide(
                "part.unusedColor(other.tone)",
                "  use Other as other from \"other.fsl\"",
                "",
                "",
            ),
            wide_piece,
            Some((
                model(
                    "expression of type Named(\"other.Tone\") is not assignable to Named(\"part.Color\") at 6:24",
                ),
                6,
                24,
            )),
        ),
        "unused_alias_qualified_member" => (
            unused("part.unusedColor(part.Red)"),
            wide_piece,
            Some((format!("{} at 6:24", cannot_type("part.Red")), 6, 24)),
        ),
        "unused_type_qualified_member" => (
            unused("part.unusedColor(Color.Red)"),
            wide_piece,
            Some((format!("{} at 6:24", cannot_type("Color")), 6, 24)),
        ),
        "unused_alias_type_qualified_member" => (
            unused("part.unusedColor(part.Color.Red)"),
            wide_piece,
            Some((format!("{} at 6:24", cannot_type("part.Color")), 6, 24)),
        ),
        "unused_non_bool_condition" => (
            unused("part.unusedK(if 1 then 0 else 0)"),
            wide_piece,
            Some((
                model("expression of type Int is not assignable to Bool at 6:23"),
                6,
                23,
            )),
        ),
        "unused_root_def_bad_argument" => (
            wide(
                "part.unusedBool(rp(true))",
                "",
                "  def rp(v: Int) = v == part.last",
                "",
            ),
            wide_piece,
            Some((
                model(
                    "expression of type Named(\"part.K\") is not assignable to Bool at 6:23",
                ),
                6,
                23,
            )),
        ),
        "unused_root_def_bad_result" => (
            wide(
                "part.unusedK(rp(0))",
                "",
                "  def rp(v: Int) = v == part.last",
                "",
            ),
            wide_piece,
            Some((format!("{BOOL_TO_K} at 6:20"), 6, 20)),
        ),
        "unused_root_def_relation_index" => (
            wide(
                "part.unusedBool(rb(0))",
                "",
                "  def rb(v: Int) = part.h[v] == 0",
                "",
            ),
            wide_piece,
            Some((
                model("public Kernel index requires Map or Seq at 6:23"),
                6,
                23,
            )),
        ),
        "unused_pattern_binder" => (
            unused("part.unusedK(if part.o is some(v) then v else 0)"),
            wide_piece,
            Some((format!("{} at 6:20", cannot_type("v")), 6, 20)),
        ),
        "unused_bare_component_state" => (
            unused("part.unusedK(last)"),
            wide_piece,
            Some((format!("{} at 6:20", cannot_type("last")), 6, 20)),
        ),
        "unused_formal_own_name" => (
            unused("part.unusedK(a)"),
            wide_piece,
            Some((format!("{} at 6:20", cannot_type("a")), 6, 20)),
        ),
        "unused_root_bool_state" => (
            wide(
                "part.unusedK(rb)",
                "",
                "  state { rb: Bool }\n  init { rb = false }",
                "",
            ),
            wide_piece,
            Some((format!("{BOOL_TO_K} at 7:20"), 7, 20)),
        ),
        "unused_int_for_enum" => (
            unused("part.unusedColor(0)"),
            wide_piece,
            Some((
                model(
                    "expression of type Int is not assignable to Named(\"part.Color\") at 6:24",
                ),
                6,
                24,
            )),
        ),
        "unused_parenthesized_bool" => (
            unused("part.unusedK(((true)))"),
            wide_piece,
            Some((format!("{BOOL_TO_K} at 6:20"), 6, 20)),
        ),
        "unused_multiline_bool" => (
            unused("part.unusedK(\n          true\n        )"),
            wide_piece,
            Some((format!("{BOOL_TO_K} at 7:11"), 7, 11)),
        ),
        "valid_bare_enum_member" => (unused("part.unusedColor(Red)"), wide_piece, None),
        "valid_literal" => (unused("part.unusedK(0)"), wide_piece, None),
        "valid_header_parameter" => (
            unused("part.unusedK(x)").replace("step()", "step(x: part.K)"),
            wide_piece,
            None,
        ),
        _ => panic!("unknown finite form {id}"),
    };
    Form {
        root,
        piece,
        expected,
    }
}

fn run(id: &str) {
    let Form {
        root,
        piece,
        expected,
    } = form(id);
    let scratch = scratch::Scratch::new("FSLC_1517_REMOVAL_INPUTS", id, &format!("removal-{id}"));
    for (name, source) in [
        ("root.fsl", root.as_str()),
        ("piece.fsl", piece.as_str()),
        ("other.fsl", OTHER),
    ] {
        fs::write(scratch.path().join(name), source).expect("fixture");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_fslc"))
        .args(["check", "root.fsl"])
        .current_dir(scratch.path())
        .env("FSLC_CACHE", "off")
        .output()
        .expect("CLI");
    let actual: Value = serde_json::from_slice(&output.stdout).expect("JSON packet");
    let versions = fsl_core::version_metadata(
        "fslc-rust",
        env!("CARGO_PKG_VERSION"),
        "native-z3",
        fsl_solver_z3::version(),
    );
    let (want, exit) = expected.map_or_else(
        || (None, 0),
        |(message, line, column)| {
            (
                Some(
                    json!({"fsl":"1.0","result":"error","kind":"semantics","message":message,
                    "loc":{"line":line,"column":column},"versions":versions}),
                ),
                2,
            )
        },
    );
    assert_eq!(output.status.code(), Some(exit), "{id}: {actual}");
    assert!(output.stderr.is_empty(), "{id}");
    match want {
        Some(want) => assert_eq!(actual, want, "{id}"),
        None => assert_eq!(actual["result"], "ok", "{id}: {actual}"),
    }
}

macro_rules! forms {
    ($($name:ident),+ $(,)?) => { $(#[test] fn $name() { run(stringify!($name)); })+ };
}
forms!(
    internal_int_formal,
    internal_empty_range_formal,
    internal_unknown_type_formal,
    internal_struct_formal,
    used_range_formal_with_bool,
    used_named_formal_with_bool,
    used_named_formal_with_enum,
    duplicate_formal_with_erased_bool,
    unused_bool,
    unused_relation_index,
    unused_unresolved_name,
    unused_name_bound_only_by_let,
    unused_other_component_enum,
    unused_alias_qualified_member,
    unused_type_qualified_member,
    unused_alias_type_qualified_member,
    unused_non_bool_condition,
    unused_root_def_bad_argument,
    unused_root_def_bad_result,
    unused_root_def_relation_index,
    unused_pattern_binder,
    unused_bare_component_state,
    unused_formal_own_name,
    unused_root_bool_state,
    unused_int_for_enum,
    unused_parenthesized_bool,
    unused_multiline_bool,
    valid_bare_enum_member,
    valid_literal,
    valid_header_parameter,
);

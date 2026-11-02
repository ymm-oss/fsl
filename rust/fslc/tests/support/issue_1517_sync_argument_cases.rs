// SPDX-License-Identifier: Apache-2.0

//! #1517 Stage A draft. Unused bad actuals are deliberately NOT policy tests.
//! Coordinates below are literal authored positions, never production spans.

pub const PIECE: &str = "spec Piece {\n  type K = 0..1\n  state { h: relation K -> K, last: K }\n  init { h = Set {}  last = 0 }\n  // padding\n  // padding\n  action put(a: K) {\n    last = a\n  }\n}\n";
pub const GOOD: &str =
    "spec Good { state { seen: Bool } init { seen = false } action idle() {} }\n";
const BOOL: &str = "expression of type Bool is not assignable to Named(\"part.K\")";
const INDEX: &str = "public Kernel index requires Map or Seq";

pub struct Case {
    pub id: &'static str,
    pub source: String,
    pub piece: String,
    pub spec: &'static str,
    pub error: Option<(usize, usize, String)>,
}

pub fn root(actual: &str) -> String {
    format!(
        "compose Root {{\n  use Piece as part from \"piece.fsl\"\n\n  action step() =\n      part.put({actual}) {{\n  }}\n\n  internal part.put\n}}\n"
    )
}

fn valid(id: &'static str, source: String, piece: String) -> Case {
    Case {
        id,
        source,
        piece,
        spec: "Root",
        error: None,
    }
}

fn map_piece(body: &str) -> String {
    PIECE
        .replace("h: relation K -> K", "h: Map<K, Bool>")
        .replace("h = Set {}", "forall k: K { h[k] = false }")
        .replace("last = a", body)
}

#[allow(clippy::too_many_lines)] // Finite, literal ownership and preservation matrix.
pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (id, actual, core, layout, line, column) in [
        ("root_bool", "true", BOOL, 0, 5, 16),
        ("root_index", "part.h[0]", INDEX, 0, 5, 16),
        ("root_pad_bool", "true", BOOL, 1, 8, 16),
        ("root_pad_index", "part.h[0]", INDEX, 1, 8, 16),
        ("piece_pad_bool", "true", BOOL, 2, 5, 16),
        ("piece_pad_index", "part.h[0]", INDEX, 2, 5, 16),
        ("second_bool", "true", BOOL, 3, 6, 32),
        ("second_index", "part.h[0]", INDEX, 3, 6, 32),
        ("second_actual_bool", "0, true", BOOL, 4, 5, 19),
        ("second_actual_index", "0, part.h[0]", INDEX, 4, 5, 19),
    ] {
        let mut case = valid(id, root(actual), PIECE.into());
        match layout {
            1 => case.source.insert_str(0, "\n// root padding\n\n"),
            2 => case.piece.insert_str(0, "\n\n\n\n\n\n\n\n"),
            3 => {
                case.source = case
                    .source
                    .replace(
                        "compose Root {\n",
                        "compose Root {\n  use Good as first from \"good.fsl\"\n",
                    )
                    .replace("part.put(", "first.idle() || part.put(");
            }
            4 => {
                case.piece = case
                    .piece
                    .replace("put(a: K)", "put(a: K, b: K)")
                    .replace("last = a", "last = b");
            }
            _ => {}
        }
        case.error = Some((
            line,
            column,
            format!("invalid model expression: {core} at {line}:{column}"),
        ));
        cases.push(case);
    }
    for (id, expression, core) in [
        ("component_bool", "true", BOOL),
        ("component_index", "h[a]", INDEX),
    ] {
        let mut case = valid(
            id,
            root("0"),
            PIECE.replace("last = a", &format!("last = {expression}")),
        );
        case.error = Some((
            2,
            3,
            format!(
                "invalid model expression: component \"piece.fsl\" failed to check (part.put: {core} at piece.fsl:8:5) at 2:3"
            ),
        ));
        cases.push(case);
    }
    let mut solo = valid("component_alone", PIECE.into(), PIECE.into());
    solo.spec = "Piece";
    cases.push(solo);
    for (id, source, piece) in [
        ("valid_zero", root("0"), PIECE.into()),
        // DESIGN-compose: range mismatch remains a verification-time concern.
        ("numeric_out_of_range", root("2"), PIECE.into()),
        (
            "root_parameter",
            root("x").replace("step()", "step(x: part.K)"),
            PIECE.into(),
        ),
        (
            "bounded_parameter",
            root("x").replace("step()", "step(x in 0..2)"),
            PIECE.into(),
        ),
        ("qualified_state", root("part.last"), PIECE.into()),
        (
            "forward_constant",
            root("part.ZERO"),
            PIECE.replace("  }\n}\n", "  }\n  const ZERO = 0\n}\n"),
        ),
        (
            "forward_root_state",
            root("x").replace(
                "  internal",
                "  state { x: Int }\n  init { x = 0 }\n  internal",
            ),
            PIECE.into(),
        ),
        ("nested_actual", root("(0 + 1) - 1"), PIECE.into()),
        (
            "unused_valid",
            root("0"),
            PIECE.replace("last = a", "last = 0"),
        ),
        (
            "forall_capture",
            root("k").replace("step()", "step(k: part.K)"),
            map_piece("forall k: K where k < a { h[k] = true }"),
        ),
        (
            "parameter_range",
            root("lim").replace("step()", "step(lim: part.K)"),
            map_piece("forall k in 0..a { h[k] = true }"),
        ),
        (
            "let_capture",
            root("y").replace("step()", "step(y: part.K)"),
            PIECE.replace("last = a", "let y = 0\n    last = a + y"),
        ),
        (
            "current_key",
            root("0"),
            map_piece("requires a == 0\n    forall a: K { h[a] = true }"),
        ),
    ] {
        cases.push(valid(id, source, piece));
    }
    cases
}

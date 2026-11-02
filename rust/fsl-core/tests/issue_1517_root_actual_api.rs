// SPDX-License-Identifier: Apache-2.0

#[path = "support/issue_1517_root_actual_api.rs"]
mod support;
use fsl_core::{build_model, lower_compose};
use support::*;

#[test]
fn literal_stage_a_full_ranges_and_unused_originals() {
    let mut r = Report::default();
    // Dense independent literal coordinate roster, not production-derived locations.
    #[rustfmt::skip]
    let ranges = [(86,90,5,16,20), (86,95,5,16,25), (104,108,8,16,20),
        (104,113,8,16,25), (86,90,5,16,20), (86,95,5,16,25),
        (138,142,6,32,36), (138,147,6,32,41), (89,93,5,19,23), (89,98,5,19,28)];
    for (case, (s, e, l, c, z)) in stage_a::cases().into_iter().take(10).zip(ranges) {
        let why = if case.id.ends_with("index") {
            INDEX
        } else {
            BOOL
        };
        reject(
            &mut r,
            case.id,
            &case.source,
            &case.piece,
            &expected(why, span(s, e, l, c, z)),
        );
    }
    for (actual, why, at) in [
        ("true", BOOL, span(86, 90, 5, 16, 20)),
        ("part.h[0]", INDEX, span(86, 95, 5, 16, 25)),
        ("(true)", BOOL, span(86, 92, 5, 16, 22)),
    ] {
        reject(
            &mut r,
            "unused",
            &root(actual),
            &PIECE.replace("last = a", "last = 0"),
            &expected(why, at),
        );
    }
    let source = root("x").replace("step()", "step(x: Bool)");
    reject(
        &mut r,
        "Bool header param",
        &source,
        PIECE,
        &expected(BOOL, span(93, 94, 5, 16, 17)),
    );
    r.finish();
}

#[test]
fn parser_cardinality_and_swapped_metadata_calibration() {
    let mut r = Report::default();
    let source = root("(0), (true),")
        .replace("part.put(", "first.idle() || part.put(")
        .replace(
            "compose Root {\n",
            "compose Root {\n  use Good as first from \"good.fsl\"\n",
        );
    let mut parsed = compose(&source);
    let refs = &action(&mut parsed).refs;
    r.eq("empty reference", &refs[0].arg_spans.clone(), &vec![]);
    r.eq(
        "parens comma second ref",
        &refs[1].arg_spans.clone(),
        &vec![span(138, 141, 6, 32, 35), span(143, 149, 6, 37, 43)],
    );
    let piece = PIECE
        .replace("put(a: K)", "put(a: K, b: K)")
        .replace("last = a", "last = 0");
    for count in [0, 1] {
        let mut manual = parsed.clone();
        action(&mut manual).refs[1].arg_spans.truncate(count);
        let got = lower_compose(manual, &Memory(piece.clone()));
        let want = lower_error(
            "sync reference argument/span cardinality mismatch",
            span(91, 97, 5, 3, 9),
        );
        r.eq("cardinality", &got, &Err(want));
    }
    let want = expected(BOOL, span(143, 149, 6, 37, 43));
    let got = build_model(lower_compose(parsed.clone(), &Memory(piece.clone())).unwrap());
    r.eq("literal metadata", &got, &Err(want.clone()));
    action(&mut parsed).refs[1].arg_spans.swap(0, 1);
    let swapped = build_model(lower_compose(parsed, &Memory(piece)).unwrap()).unwrap_err();
    eprintln!("SWAPPED CALIBRATION {swapped:?} LITERAL {want:?}");
    r.eq(
        "swapped breaks literal location oracle",
        &(swapped == want),
        &false,
    );
    r.finish();
}

#[test]
fn clone_private_identity_rebinding_and_public_registry() {
    let mut r = Report::default();
    let value = kernel(&root("true"), &PIECE.replace("last = a", "last = 0"));
    let original = private_actuals(&value);
    r.eq("clone obligations", &value.clone(), &value);
    let stamped = value.clone().with_source_file("root.fsl");
    for (file, stamped) in [
        ("root.fsl", stamped.clone()),
        ("other.fsl", stamped.with_source_file("other.fsl")),
    ] {
        r.eq(
            "stamp/rebind private delta",
            &private_actuals(&stamped),
            &original.replace("source_file: None", &format!("source_file: Some({file:?})")),
        );
        r.eq(
            "wrong identity calibration",
            &(private_actuals(&stamped).replace(file, "wrong.fsl") == private_actuals(&stamped)),
            &false,
        );
        r.eq(
            "public JSON unchanged",
            &stamped.kernel_ast_v1(),
            &value.kernel_ast_v1(),
        );
        let mut registry = fsl_core::OriginRegistry::default();
        for (target, origins) in value.origins().targets() {
            for mut origin in origins.iter().cloned() {
                for site in origin.primary.iter_mut().chain(&mut origin.secondary) {
                    site.source_file = Some(file.into());
                }
                registry.bind(target, origin);
            }
        }
        r.eq(
            "full public registry: stamp only, no fake targets",
            stamped.origins(),
            &registry,
        );
        r.eq(
            "bare root error",
            &build_model(stamped),
            &Err(expected(BOOL, span(86, 90, 5, 16, 20))),
        );
    }
    r.finish();
}

#[test]
fn header_only_inner_and_expanded_definition_locations() {
    let mut r = Report::default();
    for body in ["let x = 0", "let x = 0\n    requires x == 0"] {
        let source = root("x").replace(" {\n  }", &format!(" {{\n    {body}\n  }}"));
        reject(
            &mut r,
            "body cannot bind original",
            &source,
            &PIECE.replace("last = a", "last = 0"),
            &expected(
                "public Kernel cannot type identifier 'x'",
                span(86, 87, 5, 16, 17),
            ),
        );
    }
    reject(
        &mut r,
        "inner condition",
        &root("if 1 then 0 else 1"),
        PIECE,
        &expected(CONDITION, span(89, 90, 5, 19, 20)),
    );
    let source =
        root("bad()").replace("  internal", "  def bad() = if 1 then 0 else 1\n  internal");
    reject(
        &mut r,
        "expanded definition fallback",
        &source,
        PIECE,
        &expected(CONDITION, span(86, 91, 5, 16, 21)),
    );
    // Disposable manual AST: an inner span belonging to a foreign definition must not escape.
    let mut manual = compose(&root("if 1 then 0 else 1"));
    if let fsl_syntax::Expr::Conditional { spans, .. } = &mut action(&mut manual).refs[0].args[0] {
        spans.condition = span(152, 153, 8, 5, 6);
    } else {
        panic!("conditional fixture")
    }
    let value = lower_compose(manual, &Memory(PIECE.into()))
        .unwrap()
        .with_source_file("root.fsl");
    r.eq(
        "private root not foreign identity",
        &private_actuals(&value).contains("source_file: Some(\"root.fsl\")"),
        &true,
    );
    r.eq(
        "foreign/outside fallback",
        &build_model(value),
        &Err(expected(CONDITION, span(86, 104, 5, 16, 34))),
    );
    r.finish();
}

#[test]
fn immediate_assembly_queue_then_init_component_phases() {
    let mut r = Report::default();
    for (reference, message) in [
        ("ghost.put(0)", "unknown alias 'ghost'"),
        (
            "part.put()",
            "sync reference 'part.put' expects 1 argument(s), got 0",
        ),
    ] {
        let source =
            root("true").replace("part.put(true)", &format!("part.put(true) || {reference}"));
        let got = fsl_core::parse_kernel_source(&source, &Memory(PIECE.into()));
        r.eq(
            "immediate before queue",
            &got,
            &Err(lower_error(message, span(55, 61, 4, 3, 9))),
        );
    }
    let source = root("true").replace("  internal", "  state { x: Missing }\n  internal");
    r.eq(
        "assembly before queue",
        &build_model(kernel(&source, PIECE)),
        &Err(plain(
            "unknown type 'Missing'",
            Some(span(109, 119, 8, 11, 21)),
        )),
    );
    // Finite competing-error fixtures; no expected value comes from a validator.
    #[rustfmt::skip]
    let cases = [
        ("before init", root("true").replace("  internal", "  state { x: Int }\n  init { x = true }\n  internal"), PIECE.to_owned()),
        ("before component", root("true"), PIECE.replace("last = a", "last = true")),
        ("reference order", root("true").replace("part.put(true)", "part.put(true) || part.second(part.h[0])"), PIECE.replace("last = a", "last = 0").replace("  }\n}\n", "  }\n  action second(a: K) {}\n}\n")),
        ("action order", root("true").replace("  internal", "  action aardvark() = part.put(part.h[0]) {}\n  internal"), PIECE.replace("last = a", "last = 0")),
        ("argument order", root("true, part.h[0]"), PIECE.replace("put(a: K)", "put(a: K, b: K)").replace("last = a", "last = 0")),
    ];
    for (id, source, piece) in cases {
        reject(
            &mut r,
            id,
            &source,
            &piece,
            &expected(BOOL, span(86, 90, 5, 16, 20)),
        );
    }
    for case in stage_a::cases().into_iter().skip(10).take(2) {
        let got = build_model(kernel(&case.source, &case.piece));
        let why = if case.id.ends_with("index") {
            INDEX
        } else {
            BOOL
        };
        let mut want = expected(
            &format!("component \"piece.fsl\" failed to check (part.put: {why} at piece.fsl:8:5)"),
            span(17, 20, 2, 3, 6),
        );
        want.span = None;
        r.eq(case.id, &got, &Err(want));
        r.eq(
            "frozen component presentation",
            &build_model(kernel(&case.source, &case.piece))
                .unwrap_err()
                .to_string(),
            &case.error.unwrap().2,
        );
    }
    r.finish();
}

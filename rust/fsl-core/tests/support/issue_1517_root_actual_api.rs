// SPDX-License-Identifier: Apache-2.0

#[path = "../../../fslc/tests/support/issue_1517_sync_argument_cases.rs"]
pub mod stage_a;
use fsl_core::{
    CoreError, FileResolver, KernelSpec, ModelError, OriginChain, OriginId, OriginSite,
};
use fsl_syntax::{ComposeItem, SourceFile, SourcePos, Span, SurfaceCompose, SurfaceDocument};
pub use stage_a::{GOOD, PIECE, root};
pub const BOOL: &str = "expression of type Bool is not assignable to Named(\"part.K\")";
pub const INDEX: &str = "public Kernel index requires Map or Seq";
pub const CONDITION: &str = "expression of type Int is not assignable to Bool";

pub struct Memory(pub String);
impl FileResolver for Memory {
    fn read(&self, path: &str) -> Result<String, CoreError> {
        match path {
            "piece.fsl" => Ok(self.0.clone()),
            "good.fsl" => Ok(GOOD.into()),
            _ => Err(CoreError {
                message: format!("missing {path}"),
                line: 1,
                column: 1,
                origin: None,
                kind: fsl_core::CoreErrorKind::Semantics,
                diagnostic_code: None,
            }),
        }
    }
}
pub fn kernel(source: &str, piece: &str) -> KernelSpec {
    eprintln!("API_FIXTURE [{source:?},{piece:?}]");
    fsl_core::parse_kernel_source(source, &Memory(piece.into())).expect("fixture lowers")
}
pub fn compose(source: &str) -> SurfaceCompose {
    let SurfaceDocument::Compose(value) = fsl_syntax::parse_document(SourceFile::new(source))
        .expect("literal fixture parses")
        .surface
    else {
        panic!("expected compose")
    };
    value
}
pub fn action(value: &mut SurfaceCompose) -> &mut fsl_syntax::SyncAction {
    value
        .items
        .iter_mut()
        .find_map(|item| match item {
            ComposeItem::SyncAction(action) => Some(action),
            _ => None,
        })
        .expect("sync action")
}
pub fn span(s: usize, e: usize, line: u32, column: u32, end: u32) -> Span {
    Span {
        start: SourcePos {
            offset: s,
            line,
            column,
        },
        end: SourcePos {
            offset: e,
            line,
            column: end,
        },
    }
}
fn origin(prefix: &str, dialect: &str, at: Span) -> Box<OriginChain> {
    Box::new(OriginChain {
        id: OriginId(format!("{prefix}:{}:{}", at.start.offset, at.end.offset)),
        dialect: dialect.into(),
        primary: Some(OriginSite {
            source_file: None,
            span: Some(at),
            dialect: dialect.into(),
            declaration_path: vec![],
        }),
        secondary: vec![],
        lowering_steps: vec![],
        generated: false,
    })
}
pub fn plain(message: &str, at: Option<Span>) -> ModelError {
    ModelError {
        message: message.into(),
        origin: None,
        span: at,
        name_resolution: false,
        diagnostic_code: None,
        hint: None,
        quick_fix: None,
    }
}
pub fn expected(message: &str, at: Span) -> ModelError {
    let mut error = plain(&format!("invalid model expression: {message}"), Some(at));
    error.origin = Some(origin("kernel:type-diagnostic", "kernel", at));
    error
}
pub fn lower_error(message: &str, at: Span) -> CoreError {
    CoreError {
        message: message.into(),
        line: at.start.line,
        column: at.start.column,
        origin: Some(origin("compose:error", "compose", at)),
        kind: fsl_core::CoreErrorKind::Semantics,
        diagnostic_code: None,
    }
}
#[derive(Default)]
pub struct Report(Vec<String>);
impl Report {
    pub fn eq<T: std::fmt::Debug + PartialEq>(&mut self, id: &str, got: &T, want: &T) {
        eprintln!("{id}\nPRODUCED {got:?}\nEXPECTED {want:?}");
        if got != want {
            self.0.push(id.into());
        }
    }
    pub fn finish(self) {
        assert!(self.0.is_empty(), "{:?}", self.0);
    }
}
pub fn reject(r: &mut Report, id: &str, source: &str, piece: &str, want: &ModelError) {
    let value = kernel(source, piece);
    for (label, error) in [
        ("build", fsl_core::build_model(value.clone()).map(|_| ())),
        (
            "validated",
            value.clone().into_validated_syntax().map(|_| ()),
        ),
        (
            "stamped",
            fsl_core::build_model(value.clone().with_source_file("root.fsl")).map(|_| ()),
        ),
    ] {
        let at = want.span.unwrap().start;
        let presentation = format!("{} at {}:{}", want.message, at.line, at.column);
        r.eq(
            "bare presentation",
            &error.as_ref().err().map(ToString::to_string),
            &Some(presentation),
        );
        r.eq(&format!("{id}/{label}"), &error, &Err(want.clone()));
    }
}
pub fn private_actuals(value: &KernelSpec) -> String {
    // INTERNAL calibrated evidence, not a public Debug formatting contract.
    format!("{value:?}")
        .rsplit_once("root_actuals: ")
        .expect("private queue Debug")
        .1
        .into()
}

#[test]
fn full_valid_projections_and_raw_erasure() {
    use fsl_core::{build_model, build_surface_model};
    let mut r = Report::default();
    let mut cases = stage_a::cases();
    #[rustfmt::skip]
    let extras = [
        ("enum_alias", "Red", PIECE.replace("type K = 0..1", "enum K { Red, Blue }").replace("last = 0", "last = Red")),
        ("unused_numeric", "2", PIECE.replace("last = a", "last = 0")),
    ];
    for (id, actual, piece) in extras {
        cases.push(stage_a::Case {
            id,
            source: root(actual),
            piece,
            spec: "Root",
            error: None,
        });
    }
    for case in cases.into_iter().filter(|case| case.error.is_none()) {
        let value = kernel(&case.source, &case.piece);
        let model = build_model(value.clone());
        eprintln!("VALID {} spec={} MODEL {model:?}", case.id, case.spec);
        r.eq("full model succeeds", &model.is_ok(), &true);
        let raw = value.clone().into_syntax();
        let validated = value.clone().into_validated_syntax();
        r.eq(case.id, &validated.clone(), &Ok(raw.clone()));
        r.eq(case.id, &value.syntax().clone(), &raw.clone());
        if let Ok(checked) = validated {
            r.eq(case.id, &checked.kernel_ast_v1(), &value.kernel_ast_v1());
            r.eq(
                case.id,
                &build_surface_model(checked),
                &build_surface_model(raw),
            );
        }
    }
    let unused = PIECE.replace("last = a", "last = 0");
    let good = kernel(&root("0"), &unused);
    // Independently authored complete manual Kernel oracle; no location/metadata masks.
    #[rustfmt::skip]
    let literal = serde_json::json!(["spec", "Root", [
        ["type", "part__K", ["num", 0], ["num", 1]],
        ["state", [["decl", "part__h", ["relation", ["name", "part__K"], ["name", "part__K"]]], ["decl", "part__last", ["name", "part__K"]]]],
        ["init", [["assign", ["var", "part__h"], ["set_lit", []], {"line":4,"column":10}], ["assign", ["var", "part__last"], ["num",0], {"line":4,"column":22}]]],
        ["action", "step", [], [["assign", ["var", "part__last"], ["num",0], {"line":8,"column":5}]], {"line":4,"column":3}, false, null, true]
    ]]);
    r.eq(
        "literal entire Kernel JSON",
        &good.kernel_ast_v1(),
        &literal.clone(),
    );
    let mut wrong = literal.clone();
    wrong[2][3][4]["line"] = 5.into();
    r.eq(
        "action-location negative calibration",
        &(good.kernel_ast_v1() == wrong),
        &false,
    );
    let good_model = build_surface_model(good.clone().into_syntax());
    for actual in ["true", "part.h[0]"] {
        let bad = kernel(&root(actual), &unused);
        r.eq("erased JSON", &bad.kernel_ast_v1(), &good.kernel_ast_v1());
        r.eq(
            "raw unused invalid builds",
            &build_surface_model(bad.into_syntax()),
            &good_model.clone(),
        );
    }
    // Literal independent supplied surface: build_surface_model must still validate it.
    let bad = kernel(&root("0"), &PIECE.replace("last = a", "last = true"));
    let got = build_surface_model(bad.into_syntax());
    let mut want = expected(BOOL, span(152, 156, 8, 5, 9));
    want.span = None; // Ordinary expression error carries its location in origin, not root metadata.
    r.eq("supplied surface rejects Bool", &got, &Err(want));
    r.finish();
}
